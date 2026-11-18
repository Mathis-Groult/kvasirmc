//! The steps of `World::flush_block_updates`: group the queued changes by chunk section, then
//! send one section's changes as a single update or a multi-block update, and the Bedrock
//! water layer.

use std::collections::HashMap;

use pumpkin_data::{BlockState, BlockStateId};
use pumpkin_protocol::bedrock::client::block_actor_data::CBlockActorData;
use pumpkin_protocol::java::client::play::CBlockUpdate;
use pumpkin_protocol::{
    codec::var_int::VarInt,
    java::client::play::{CBlockEntityData, CMultiBlockUpdate},
};
use pumpkin_util::math::{
    position::{BlockPos, chunk_section_from_pos},
    vector2::Vector2,
    vector3::Vector3,
};
use pumpkin_world::chunk::palette::bedrock_water_state;

use crate::net::ClientPlatform;
use crate::world::World;

impl World {
    /// Empties the queue of unsent block changes, grouped by chunk section.
    pub(super) fn take_block_changes_by_section(
        &self,
    ) -> HashMap<Vector3<i32>, Vec<(BlockPos, BlockStateId)>> {
        let mut block_state_updates_by_chunk_section: HashMap<
            Vector3<i32>,
            Vec<(BlockPos, BlockStateId)>,
        > = HashMap::new();
        let changes = {
            let mut guard = self
                .unsent_block_changes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::take(&mut *guard)
        };
        for (position, block_state_id) in changes {
            let chunk_section = chunk_section_from_pos(&position);
            block_state_updates_by_chunk_section
                .entry(chunk_section)
                .or_default()
                .push((position, block_state_id));
        }
        block_state_updates_by_chunk_section
    }

    /// One changed block: a block update for both editions, its block entity data for Java
    /// and its block actor data for Bedrock.
    pub(super) fn send_single_block_update(
        &self,
        chunk_pos: Vector2<i32>,
        block_pos: BlockPos,
        block_state_id: BlockStateId,
    ) {
        let be_block_id = BlockState::to_be_network_id(block_state_id);
        self.broadcast_to_chunk_editioned(
            chunk_pos,
            &CBlockUpdate::new(block_pos, i32::from(block_state_id.as_u16()).into()),
            &pumpkin_protocol::bedrock::client::CUpdateBlock::new(block_pos, be_block_id),
        );
        if let Some(block_entity) = self.get_block_entity(&block_pos)
            && let Some(nbt) = block_entity.chunk_data_nbt()
        {
            let bytes = pumpkin_nbt::Nbt::from(nbt).write_unnamed();
            self.broadcast_to_chunk(
                chunk_pos,
                &CBlockEntityData::new(
                    block_pos,
                    VarInt(block_entity.get_id() as i32),
                    bytes.as_ref().into(),
                ),
            );
        }
        if let Some(data) = self.bedrock_block_entity_data(block_state_id, block_pos) {
            self.broadcast_to_chunk_bedrock(chunk_pos, &CBlockActorData::new(block_pos, data));
        }
    }

    /// Several changed blocks of one section: Bedrock watchers get one update (and actor data)
    /// per block, Java watchers one multi-block update, then the block entity data of each.
    pub(super) fn send_multi_block_update(
        &self,
        chunk_pos: Vector2<i32>,
        updates: &[(BlockPos, BlockStateId)],
    ) {
        let players = self.players.load();
        let mut java_recipients = Vec::new();

        let recipients = players.iter().filter(|p| {
            p.watched_section
                .load()
                .is_within_distance(chunk_pos.x, chunk_pos.y)
        });

        let mut bedrock_packets = Vec::new();
        for (block_pos, block_state_id) in updates {
            let be_block_id = BlockState::to_be_network_id(*block_state_id);
            let update_packet =
                pumpkin_protocol::bedrock::client::CUpdateBlock::new(*block_pos, be_block_id);
            let actor_packet = self
                .bedrock_block_entity_data(*block_state_id, *block_pos)
                .map(|data| CBlockActorData::new(*block_pos, data));
            bedrock_packets.push((update_packet, actor_packet));
        }

        let mut bedrock_recipients = Vec::new();
        for p in recipients {
            match p.client.as_ref() {
                ClientPlatform::Java(_) => java_recipients.push(p),
                ClientPlatform::Bedrock(be_client) => {
                    bedrock_recipients.push(be_client);
                }
            }
        }

        for be_client in bedrock_recipients {
            for (update_packet, actor_packet) in &bedrock_packets {
                if let Ok(data) = be_client.serialize_packet(update_packet) {
                    be_client.try_enqueue_packet(data);
                }
                if let Some(actor_packet) = actor_packet
                    && let Ok(data) = be_client.serialize_packet(actor_packet)
                {
                    be_client.try_enqueue_packet(data);
                }
            }
        }

        let recipients_by_version =
            Self::collect_java_recipients_by_version(java_recipients.into_iter());
        Self::broadcast_java_grouped(&CMultiBlockUpdate::new(updates), recipients_by_version);

        for (block_pos, _) in updates {
            if let Some(block_entity) = self.get_block_entity(block_pos)
                && let Some(nbt) = block_entity.chunk_data_nbt()
            {
                let bytes = pumpkin_nbt::Nbt::from(nbt).write_unnamed();
                self.broadcast_to_chunk(
                    chunk_pos,
                    &CBlockEntityData::new(
                        *block_pos,
                        VarInt(block_entity.get_id() as i32),
                        bytes.as_ref().into(),
                    ),
                );
            }
        }
    }

    /// The water layer (layer 1) of each changed block, for Bedrock watchers of the chunk.
    pub(super) fn send_bedrock_water_layer(
        &self,
        chunk_pos: Vector2<i32>,
        updates: &[(BlockPos, BlockStateId)],
    ) {
        let mut bedrock_water_packets = Vec::new();
        for (block_pos, block_state_id) in updates {
            let water_state = bedrock_water_state(*block_state_id);
            let packet = pumpkin_protocol::bedrock::client::CUpdateBlock::with_layer(
                *block_pos,
                BlockState::to_be_network_id(water_state),
                1,
            );
            bedrock_water_packets.push(packet);
        }

        if !bedrock_water_packets.is_empty() {
            let players = self.players.load();
            let recipients = players.iter().filter(|player| {
                player
                    .watched_section
                    .load()
                    .is_within_distance(chunk_pos.x, chunk_pos.y)
            });
            for player in recipients {
                if let ClientPlatform::Bedrock(client) = player.client.as_ref() {
                    for packet in &bedrock_water_packets {
                        if let Ok(data) = client.serialize_packet(packet) {
                            client.try_enqueue_packet(data);
                        }
                    }
                }
            }
        }
    }
}
