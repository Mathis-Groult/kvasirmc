//! Sending packets to the players who should see them.

use super::World;
use crate::{
    entity::{Entity, EntityBase, player::Player},
    net::{ClientPlatform, bedrock::BedrockClient, java::JavaClient},
};
use bytes::BufMut;
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::entity::EntityStatus;
use pumpkin_data::packet::CURRENT_MC_VERSION;
use pumpkin_protocol::java::client::play::{CRemoveMobEffect, CUpdateMobEffect};
use pumpkin_protocol::java::client::play::{CSetEntityMetadata, Metadata};
use pumpkin_protocol::{
    BClientPacket, ClientPacket,
    bedrock::server::actor_event::{ActorEventID, SActorEvent},
    codec::{var_int::VarInt, var_ulong::VarULong},
    java::client::play::{CDamageEvent, CEntityStatus},
};
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::version::JavaMinecraftVersion;
use std::collections::BTreeMap;
use std::sync::Arc;
use tracing::error;

impl World {
    /// Broadcasts an entity status update / event to all players tracking the specified entity,
    /// and to the entity itself if it is a player.
    /// Matching Vanilla's `ServerLevel.broadcastEntityEvent(entity, event)`.
    pub fn broadcast_entity_event(
        &self,
        entity: &Entity,
        java_status: EntityStatus,
        bedrock_status: ActorEventID,
    ) {
        let je_packet = CEntityStatus::new(entity.entity_id, java_status as i8);
        if bedrock_status == ActorEventID::None {
            self.send_to_tracking_players_and_self(entity, &je_packet);
        } else {
            let be_packet = SActorEvent {
                target_runtime_id: VarULong(entity.entity_id as u64),
                event_id: bedrock_status,
                data: VarInt(0),
                fire_at_position: None,
            };
            self.send_to_tracking_players_and_self_editioned(entity, &je_packet, &be_packet);
        }
    }

    /// Broadcasts a damage event to all players tracking the specified entity,
    /// and to the entity itself if it is a player.
    /// Matching Vanilla's `ServerLevel.broadcastDamageEvent(entity, source)`.
    pub fn broadcast_damage_event(
        &self,
        entity: &Entity,
        damage_type_id: i32,
        source_entity_id: Option<i32>,
        cause_entity_id: Option<i32>,
        position: Option<Vector3<f64>>,
    ) {
        let je_packet = CDamageEvent::new(
            entity.entity_id.into(),
            damage_type_id.into(),
            source_entity_id.map(Into::into),
            cause_entity_id.map(Into::into),
            position,
        );
        self.send_to_tracking_players_and_self(entity, &je_packet);
    }

    /// Sends an entity status update to all players tracking the specified entity.
    pub fn send_entity_status(
        &self,
        entity: &Entity,
        java_status: EntityStatus,
        bedrock_status: ActorEventID,
    ) {
        self.broadcast_entity_event(entity, java_status, bedrock_status);
    }

    pub fn send_remove_mob_effect(&self, entity: &Entity, effect_type: &'static StatusEffect) {
        let je_packet =
            CRemoveMobEffect::new(entity.entity_id.into(), VarInt(i32::from(effect_type.id)));

        let be_packet = pumpkin_protocol::bedrock::client::CMobEffect {
            target_runtime_id: VarULong(entity.entity_id as u64),
            event_id: pumpkin_protocol::bedrock::client::CMobEffect::EVENT_REMOVE,
            effect_id: VarInt(effect_type.to_bedrock_id()),
            effect_amplifier: VarInt(0),
            show_particles: false,
            effect_duration_ticks: VarInt(0),
            tick: VarULong(0),
            ambient: false,
        };
        self.send_to_tracking_players_and_self_editioned(entity, &je_packet, &be_packet);
    }

    pub fn send_add_mob_effect(&self, entity: &Entity, effect: &pumpkin_data::potion::Effect) {
        let mut flags: i8 = 0;
        if effect.ambient {
            flags |= 0x01;
        }
        if effect.show_particles {
            flags |= 0x02;
        }
        if effect.show_icon {
            flags |= 0x04;
        }

        let je_packet = CUpdateMobEffect::new(
            VarInt(entity.entity_id),
            VarInt(i32::from(effect.effect_type.id)),
            VarInt(i32::from(effect.amplifier)),
            VarInt(effect.duration),
            flags,
        );

        let be_packet = pumpkin_protocol::bedrock::client::CMobEffect {
            target_runtime_id: VarULong(entity.entity_id as u64),
            event_id: pumpkin_protocol::bedrock::client::CMobEffect::EVENT_ADD,
            effect_id: VarInt(effect.effect_type.to_bedrock_id()),
            effect_amplifier: VarInt(i32::from(effect.amplifier)),
            show_particles: effect.show_particles,
            effect_duration_ticks: VarInt(effect.duration),
            tick: VarULong(0),
            ambient: effect.ambient,
        };

        self.send_to_tracking_players_and_self_editioned(entity, &je_packet, &be_packet);
    }

    pub fn send_to_tracking_players<P: ClientPacket + Sync>(&self, entity: &Entity, packet: &P) {
        if let Some(tracked) = self.entity_tracker.get_tracked_entity(entity.entity_id) {
            tracked.send_to_tracking_players(packet, self);
        }
    }

    pub fn send_to_tracking_players_bedrock<P: BClientPacket + Sync>(
        &self,
        entity: &Entity,
        packet: &P,
    ) {
        if let Some(tracked) = self.entity_tracker.get_tracked_entity(entity.entity_id) {
            tracked.send_to_tracking_players_bedrock(packet, self);
        }
    }

    pub fn send_to_tracking_players_editioned<J: ClientPacket + Sync, B: BClientPacket + Sync>(
        &self,
        entity: &Entity,
        je_packet: &J,
        be_packet: &B,
    ) {
        if let Some(tracked) = self.entity_tracker.get_tracked_entity(entity.entity_id) {
            tracked.send_to_tracking_players_editioned(je_packet, be_packet, self);
        }
    }

    pub fn send_to_tracking_players_and_self<P: ClientPacket + Sync>(
        &self,
        entity: &Entity,
        packet: &P,
    ) {
        if let Some(tracked) = self.entity_tracker.get_tracked_entity(entity.entity_id) {
            tracked.send_to_tracking_players_and_self(packet, self);
        }
    }

    pub fn send_to_tracking_players_and_self_editioned<
        J: ClientPacket + Sync,
        B: BClientPacket + Sync,
    >(
        &self,
        entity: &Entity,
        je_packet: &J,
        be_packet: &B,
    ) {
        if let Some(tracked) = self.entity_tracker.get_tracked_entity(entity.entity_id) {
            tracked.send_to_tracking_players_and_self_editioned(je_packet, be_packet, self);
        } else if let Some(player) = self.get_player_by_id(entity.entity_id) {
            player.try_enqueue_packet_editioned(je_packet, be_packet);
        }
    }

    pub fn send_to_tracking_players_filtered<P: ClientPacket + Sync, F: Fn(&Player) -> bool>(
        &self,
        entity: &Entity,
        packet: &P,
        filter: F,
    ) {
        if let Some(tracked) = self.entity_tracker.get_tracked_entity(entity.entity_id) {
            tracked.send_to_tracking_players_filtered(packet, self, filter);
        }
    }

    pub fn send_to_tracking_players_filtered_editioned<
        J: ClientPacket + Sync,
        B: BClientPacket + Sync,
        F: Fn(&Player) -> bool,
    >(
        &self,
        entity: &Entity,
        je_packet: &J,
        be_packet: &B,
        filter: F,
    ) {
        if let Some(tracked) = self.entity_tracker.get_tracked_entity(entity.entity_id) {
            tracked.send_to_tracking_players_filtered_editioned(je_packet, be_packet, self, filter);
        }
    }

    /// Keyed by encode version: always `CURRENT_MC_VERSION`, older clients are converted
    /// per connection on enqueue by the multiversion plugin.
    // TODO: collapse to a plain recipient list with a single serialize.
    pub(crate) fn collect_java_recipients_by_version<'a>(
        players: impl Iterator<Item = &'a Arc<Player>>,
    ) -> BTreeMap<JavaMinecraftVersion, Vec<&'a JavaClient>> {
        let mut recipients_by_version: BTreeMap<JavaMinecraftVersion, Vec<&'a JavaClient>> =
            BTreeMap::new();
        for player in players {
            if let ClientPlatform::Java(java_client) = player.client.as_ref() {
                recipients_by_version
                    .entry(CURRENT_MC_VERSION)
                    .or_default()
                    .push(java_client);
            }
        }
        recipients_by_version
    }

    pub fn broadcast_java_clients<'a, P: ClientPacket>(
        packet: &P,
        recipients: impl Iterator<Item = &'a JavaClient>,
    ) {
        let mut recipients_by_version: BTreeMap<JavaMinecraftVersion, Vec<&JavaClient>> =
            BTreeMap::new();
        for client in recipients {
            recipients_by_version
                .entry(CURRENT_MC_VERSION)
                .or_default()
                .push(client);
        }
        Self::broadcast_java_grouped(packet, recipients_by_version);
    }

    pub(super) fn broadcast_java_grouped<P: ClientPacket>(
        packet: &P,
        recipients_by_version: BTreeMap<JavaMinecraftVersion, Vec<&JavaClient>>,
    ) {
        for (version, recipients) in recipients_by_version {
            let packet_data = match JavaClient::serialize_packet_for_version(packet, version) {
                Ok(packet_data) => packet_data,
                Err(pumpkin_protocol::ser::WritingError::UnsupportedVersion(_)) => {
                    continue;
                }
                Err(err) => {
                    error!(
                        "Failed to serialize packet {} for version {:?}: {}",
                        std::any::type_name::<P>(),
                        version,
                        err
                    );
                    continue;
                }
            };

            for recipient in recipients {
                recipient.try_enqueue_packet(packet_data.clone());
            }
        }
    }

    pub(super) fn broadcast_bedrock_grouped<'a, P: BClientPacket>(
        packet: &P,
        recipients: impl Iterator<Item = &'a Arc<BedrockClient>>,
    ) {
        for recipient in recipients {
            match recipient.serialize_packet(packet) {
                Ok(packet_data) => recipient.try_enqueue_packet(packet_data),
                Err(err) => {
                    error!(
                        "Failed to serialize bedrock packet {}: {}",
                        std::any::type_name::<P>(),
                        err
                    );
                }
            }
        }
    }

    /// Broadcasts a packet to all connected players within the world.
    /// Please avoid this as we want to replace it with `broadcast_editioned`
    ///
    /// Sends the specified packet to every player currently logged in to the world.
    ///
    /// **Note:** This function acquires a lock on the `current_players` map, ensuring thread safety.
    pub fn broadcast_packet_all<P: ClientPacket>(&self, packet: &P) {
        let players = self.players.load();
        let recipients_by_version = Self::collect_java_recipients_by_version(players.iter());
        Self::broadcast_java_grouped(packet, recipients_by_version);
    }

    // This should replace broadcast_packet_all at some point
    pub fn broadcast_editioned<J: ClientPacket, B: BClientPacket>(
        &self,
        je_packet: &J,
        be_packet: &B,
    ) {
        let players = self.players.load();
        let je_recipients_by_version = Self::collect_java_recipients_by_version(players.iter());

        Self::broadcast_java_grouped(je_packet, je_recipients_by_version);
        Self::broadcast_bedrock_grouped(
            be_packet,
            players.iter().filter_map(|p| match p.client.as_ref() {
                ClientPlatform::Bedrock(be) => Some(be),
                ClientPlatform::Java(_) => None,
            }),
        );
    }

    pub fn broadcast_packet_except_editioned<J: ClientPacket, B: BClientPacket>(
        &self,
        except: &[uuid::Uuid],
        je_packet: &J,
        be_packet: &B,
    ) {
        let players = self.players.load();
        let mut java_recipients = Vec::new();
        let mut bedrock_recipients = Vec::new();

        for p in players.iter() {
            if except.contains(&p.gameprofile.id) {
                continue;
            }
            match p.client.as_ref() {
                ClientPlatform::Java(_) => java_recipients.push(p),
                ClientPlatform::Bedrock(be_client) => bedrock_recipients.push(be_client),
            }
        }

        let recipients_by_version =
            Self::collect_java_recipients_by_version(java_recipients.into_iter());
        Self::broadcast_java_grouped(je_packet, recipients_by_version);
        Self::broadcast_bedrock_grouped(be_packet, bedrock_recipients.into_iter());
    }

    /// Broadcasts the skin layers of a player, encoding the metadata for each Java client's own
    /// protocol version since the tracked data index differs between versions.
    pub(super) fn broadcast_skin_parts<B: BClientPacket>(
        &self,
        except: &[uuid::Uuid],
        entity_id: i32,
        skin_parts: u8,
        be_packet: &B,
    ) {
        let players = self.players.load();
        let mut java_recipients = Vec::new();
        let mut bedrock_recipients = Vec::new();

        for p in players.iter() {
            if except.contains(&p.gameprofile.id) {
                continue;
            }
            match p.client.as_ref() {
                ClientPlatform::Java(_) => java_recipients.push(p),
                ClientPlatform::Bedrock(be_client) => bedrock_recipients.push(be_client),
            }
        }

        let recipients_by_version =
            Self::collect_java_recipients_by_version(java_recipients.into_iter());

        for (version, recipients) in recipients_by_version {
            let mut buf = Vec::new();
            for meta in [
                Metadata::new(
                    pumpkin_data::tracked_data::player::PLAYER_MODE_CUSTOMISATION,
                    skin_parts,
                ),
                Metadata::new(
                    pumpkin_data::tracked_data::player::PLAYER_MODE_CUSTOMIZATION_ID,
                    skin_parts,
                ),
            ] {
                let _ = meta.write(&mut buf, &version);
            }
            buf.put_u8(255);
            let packet = CSetEntityMetadata::new(entity_id.into(), buf.into());
            if let Ok(packet_data) = JavaClient::serialize_packet_for_version(&packet, version) {
                for recipient in recipients {
                    recipient.try_enqueue_packet(packet_data.clone());
                }
            }
        }

        Self::broadcast_bedrock_grouped(be_packet, bedrock_recipients.into_iter());
    }

    /// Broadcasts a packet to all connected players within the world, excluding the specified players.
    ///
    /// Sends the specified packet to every player currently logged in to the world, excluding the players listed in the `except` parameter.
    ///
    /// **Note:** This function acquires a lock on the `current_players` map, ensuring thread safety.
    pub fn broadcast_packet_except<P: ClientPacket>(&self, except: &[uuid::Uuid], packet: &P) {
        let players = self.players.load();
        let recipients_by_version = Self::collect_java_recipients_by_version(
            players
                .iter()
                .filter(|candidate| !except.contains(&candidate.gameprofile.id)),
        );
        Self::broadcast_java_grouped(packet, recipients_by_version);
    }

    /// Broadcasts a packet to all players who currently have the target chunk loaded.
    pub fn broadcast_to_chunk<P: ClientPacket>(&self, chunk_pos: Vector2<i32>, packet: &P) {
        let players = self.players.load();

        let recipients = players.iter().filter(|p| {
            p.watched_section
                .load()
                .is_within_distance(chunk_pos.x, chunk_pos.y)
        });

        let recipients_by_version = Self::collect_java_recipients_by_version(recipients);
        Self::broadcast_java_grouped(packet, recipients_by_version);
    }

    pub fn broadcast_to_chunk_bedrock<P: BClientPacket>(
        &self,
        chunk_pos: Vector2<i32>,
        packet: &P,
    ) {
        let players = self.players.load();
        let recipients = players.iter().filter_map(|player| {
            if player
                .watched_section
                .load()
                .is_within_distance(chunk_pos.x, chunk_pos.y)
                && let ClientPlatform::Bedrock(client) = player.client.as_ref()
            {
                return Some(client);
            }
            None
        });
        Self::broadcast_bedrock_grouped(packet, recipients);
    }

    pub fn broadcast_to_chunk_editioned<J: ClientPacket, B: BClientPacket>(
        &self,
        chunk_pos: Vector2<i32>,
        je_packet: &J,
        be_packet: &B,
    ) {
        let players = self.players.load();
        let mut java_recipients = Vec::new();
        let mut bedrock_recipients = Vec::new();

        let recipients = players.iter().filter(|p| {
            p.watched_section
                .load()
                .is_within_distance(chunk_pos.x, chunk_pos.y)
        });

        for p in recipients {
            match p.client.as_ref() {
                ClientPlatform::Java(_) => java_recipients.push(p),
                ClientPlatform::Bedrock(be_client) => bedrock_recipients.push(be_client),
            }
        }

        let recipients_by_version =
            Self::collect_java_recipients_by_version(java_recipients.into_iter());
        Self::broadcast_java_grouped(je_packet, recipients_by_version);
        Self::broadcast_bedrock_grouped(be_packet, bedrock_recipients.into_iter());
    }

    /// Broadcasts a packet to chunk watchers, excluding specific players.
    pub fn broadcast_to_chunk_except<P: ClientPacket>(
        &self,
        chunk_pos: Vector2<i32>,
        except: &[uuid::Uuid],
        packet: &P,
    ) {
        let players = self.players.load();

        let recipients = players.iter().filter(|p| {
            if except.contains(&p.get_entity().entity_uuid) {
                return false;
            }
            p.watched_section
                .load()
                .is_within_distance(chunk_pos.x, chunk_pos.y)
        });

        let recipients_by_version = Self::collect_java_recipients_by_version(recipients);
        Self::broadcast_java_grouped(packet, recipients_by_version);
    }

    pub fn broadcast_to_chunk_except_editioned<J: ClientPacket, B: BClientPacket>(
        &self,
        chunk_pos: Vector2<i32>,
        except: &[uuid::Uuid],
        je_packet: &J,
        be_packet: &B,
    ) {
        let players = self.players.load();
        let recipients = players.iter().filter(|p| {
            if except.contains(&p.get_entity().entity_uuid) {
                return false;
            }
            p.watched_section
                .load()
                .is_within_distance(chunk_pos.x, chunk_pos.y)
        });

        let mut java_recipients = Vec::new();
        let mut bedrock_recipients = Vec::new();

        for p in recipients {
            match p.client.as_ref() {
                ClientPlatform::Java(_) => java_recipients.push(p),
                ClientPlatform::Bedrock(be_client) => bedrock_recipients.push(be_client),
            }
        }

        let je_recipients_by_version =
            Self::collect_java_recipients_by_version(java_recipients.into_iter());
        Self::broadcast_java_grouped(je_packet, je_recipients_by_version);
        Self::broadcast_bedrock_grouped(be_packet, bedrock_recipients.into_iter());
    }
}
