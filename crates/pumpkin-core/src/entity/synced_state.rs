//! Synced entity data and spawn packets.

use super::player::Player;
use super::{Entity, SpawnState};
use crate::{
    net::{ClientPlatform, java::JavaClient},
    world::World,
};
use bytes::BufMut;
use pumpkin_data::entity::EntityType;
use pumpkin_protocol::{
    bedrock::client::set_actor_data::{CSetActorData, PropertySyncData, SyncedActorDataList},
    codec::var_int::VarInt,
    codec::var_ulong::VarULong,
    java::client::play::{CSetEntityMetadata, CSpawnEntity, Metadata, MetadataSerializer},
};
use pumpkin_util::math::pack_degrees;
use std::sync::{Arc, atomic::Ordering::Relaxed};

impl Entity {
    /// Vanilla `ClientboundAddEntityPacket(entity, serverEntity)`: tracker, not live.
    /// Players live: tracker skips `send_changes` for them.
    pub(super) fn spawn_state(&self) -> SpawnState {
        if self.entity_type == &EntityType::PLAYER {
            SpawnState {
                pos: self.pos.load(),
                velocity: self.velocity.load(),
                pitch: pack_degrees(self.pitch.load()),
                yaw: pack_degrees(self.yaw.load()),
                head_yaw: pack_degrees(self.head_yaw.load()),
            }
        } else {
            SpawnState {
                pos: self.last_sent_pos.load(),
                velocity: self.last_sent_velocity.load(),
                pitch: self.last_sent_pitch.load(Relaxed),
                yaw: self.last_sent_yaw.load(Relaxed),
                head_yaw: self.last_sent_head_yaw.load(Relaxed),
            }
        }
    }

    pub fn create_spawn_packet(&self) -> CSpawnEntity {
        let spawn = self.spawn_state();
        CSpawnEntity::new_packed(
            VarInt(self.entity_id),
            self.entity_uuid,
            VarInt(i32::from(self.entity_type.id)),
            spawn.pos,
            spawn.pitch,
            spawn.yaw,
            spawn.head_yaw, // todo: head_yaw and yaw are swapped, find out why
            self.data.load(Relaxed).into(),
            spawn.velocity,
        )
    }

    pub fn set_synced_data<T: MetadataSerializer + Clone + Send + Sync + 'static>(
        &self,
        tracked: pumpkin_data::tracked_data::TrackedData,
        value: T,
    ) -> bool {
        // Sent by the tracker at end of tick, vanilla `sendDirtyEntityData`.
        self.synched_data.set(tracked, value)
    }

    pub fn send_bedrock_actor_data(&self, bedrock_meta: &SyncedActorDataList) {
        let world = self.world.load();
        let players = world.players.load();
        let mut bedrock_recipients = Vec::new();

        if let Some(tracked) = world.entity_tracker.get_tracked_entity(self.entity_id) {
            if !tracked.has_bedrock_watchers() && self.entity_type != &EntityType::PLAYER {
                return;
            }
            for player in players.iter() {
                if (tracked.is_seen_by(&player.gameprofile.id)
                    || player.entity_id() == self.entity_id)
                    && let ClientPlatform::Bedrock(client) = player.client.as_ref()
                {
                    bedrock_recipients.push(client);
                }
            }
        } else {
            let chunk_pos = self.chunk_pos.load();
            for player in players.iter() {
                if player
                    .watched_section
                    .load()
                    .is_within_distance(chunk_pos.x, chunk_pos.y)
                    && let ClientPlatform::Bedrock(client) = player.client.as_ref()
                {
                    bedrock_recipients.push(client);
                }
            }
        }

        let packet = CSetActorData {
            target_runtime_id: VarULong(self.entity_id as u64),
            actor_data: SyncedActorDataList(bedrock_meta.0.clone()),
            synced_properties: PropertySyncData {
                int_entries_list: std::collections::HashMap::new(),
                float_entries_list: std::collections::HashMap::new(),
            },
            tick: VarULong(0),
        };
        for recipient in bedrock_recipients {
            if let Ok(packet_data) = recipient.serialize_packet(&packet) {
                recipient.try_enqueue_packet(packet_data);
            }
        }
    }

    /// The Java players that should receive entity data for this entity: everyone
    /// tracking it, plus the entity itself when it is a player. Until the entity is
    /// tracked, every player watching its chunk stands in for the tracker.
    pub(super) fn java_metadata_recipients<'a>(
        &self,
        world: &World,
        players: &'a [Arc<Player>],
    ) -> Vec<&'a Arc<Player>> {
        let mut java_recipients = Vec::new();

        if let Some(tracked) = world.entity_tracker.get_tracked_entity(self.entity_id) {
            for player in players {
                if (tracked.is_seen_by(&player.gameprofile.id)
                    || player.entity_id() == self.entity_id)
                    && let ClientPlatform::Java(_) = player.client.as_ref()
                {
                    java_recipients.push(player);
                }
            }
        } else {
            let chunk_pos = self.chunk_pos.load();
            for player in players {
                // A player that just changed world is in the new world's player list but
                // not in its tracker yet, and its watched section still points at the
                // world it left, so it has to be matched on its entity id instead.
                if (player.entity_id() == self.entity_id
                    || player
                        .watched_section
                        .load()
                        .is_within_distance(chunk_pos.x, chunk_pos.y))
                    && let ClientPlatform::Java(_) = player.client.as_ref()
                {
                    java_recipients.push(player);
                }
            }
        }

        java_recipients
    }

    /// Sends the given metadata entries to this entity's viewers, and the matching
    /// actor data to the Bedrock ones when `bedrock_meta` is given.
    pub fn send_meta_data<T: MetadataSerializer>(
        &self,
        meta: &[Metadata<T>],
        bedrock_meta: Option<&SyncedActorDataList>,
    ) {
        let world = self.world.load();
        let players = world.players.load();

        let java_recipients = self.java_metadata_recipients(&world, &players);

        let recipients_by_version =
            World::collect_java_recipients_by_version(java_recipients.into_iter());

        for (version, recipients) in recipients_by_version {
            let mut buf = Vec::new();
            for m in meta {
                let _ = m.write(&mut buf, &version);
            }
            if buf.is_empty() {
                continue;
            }
            buf.put_u8(255);
            let packet = CSetEntityMetadata::new(self.entity_id.into(), buf.into());
            if let Ok(packet_data) = JavaClient::serialize_packet_for_version(&packet, version) {
                for recipient in recipients {
                    recipient.try_enqueue_packet(packet_data.clone());
                }
            }
        }

        if let Some(bedrock_meta) = bedrock_meta {
            self.send_bedrock_actor_data(bedrock_meta);
        }
    }

    /// Sends the synced values that changed since the last call to this entity's
    /// viewers, and does nothing when none did.
    pub fn send_dirty_entity_data(&self) {
        if !self.synched_data.is_dirty() {
            return;
        }

        let world = self.world.load();
        let players = world.players.load();

        // Unwatched, skip the player scan.
        if self.entity_type != &EntityType::PLAYER
            && world
                .entity_tracker
                .get_tracked_entity(self.entity_id)
                .is_some_and(|tracked| !tracked.has_viewers())
        {
            self.synched_data.clear_dirty();
            return;
        }

        let java_recipients = self.java_metadata_recipients(&world, &players);

        if java_recipients.is_empty() {
            // Vanilla `packDirty` clears even without recipients.
            self.synched_data.clear_dirty();
            return;
        }

        let recipients_by_version =
            World::collect_java_recipients_by_version(java_recipients.into_iter());

        for (version, recipients) in recipients_by_version {
            if let Some(buf) = self.synched_data.pack_dirty_for_version(&version) {
                let packet = CSetEntityMetadata::new(self.entity_id.into(), buf);
                if let Ok(packet_data) = JavaClient::serialize_packet_for_version(&packet, version)
                {
                    for recipient in recipients {
                        recipient.try_enqueue_packet(packet_data.clone());
                    }
                }
            }
        }
        self.synched_data.clear_dirty();
    }

    /// Resends every non-default synced value of this entity to its viewers, the entity
    /// itself included when it is a player.
    ///
    /// Clients discard the entity data they cached whenever the entity is re-created on
    /// their side (a respawn or a dimension change re-creates the local player, and a
    /// spawn packet resets the entity for everyone else), so the values have to be pushed
    /// again even though nothing became dirty on our side.
    pub fn refresh_synced_data(&self) {
        let world = self.world.load();
        let players = world.players.load();

        let java_recipients = self.java_metadata_recipients(&world, &players);

        if java_recipients.is_empty() {
            return;
        }

        let recipients_by_version =
            World::collect_java_recipients_by_version(java_recipients.into_iter());

        for (version, recipients) in recipients_by_version {
            if let Some(buf) = self
                .synched_data
                .get_non_default_values_for_version(&version)
            {
                let packet = CSetEntityMetadata::new(self.entity_id.into(), buf);
                if let Ok(packet_data) = JavaClient::serialize_packet_for_version(&packet, version)
                {
                    for recipient in recipients {
                        recipient.try_enqueue_packet(packet_data.clone());
                    }
                }
            }
        }
        self.synched_data.clear_dirty();
    }
}
