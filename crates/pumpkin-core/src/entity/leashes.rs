//! Leashes.

use super::{DelayedLeash, Entity, EntityBase, LEASH_RESTORE_TIMEOUT};
use crate::entity::{
    ai::brain::memory::codec::{block_pos_from_tag, block_pos_to_tag},
    decoration::leash_knot::LeashKnotEntity,
};
use pumpkin_data::entity::EntityType;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_nbt::{compound::NbtCompound, tag::NbtTag};
use std::sync::{Arc, atomic::Ordering::Relaxed};

impl Entity {
    pub fn leash_to(&self, holder: Arc<dyn EntityBase>) {
        let holder_entity_id = holder.get_entity().entity_id;
        let world = self.world.load();
        if let Some(server) = world.server.upgrade()
            && let Some(player) = holder.get_player()
            && let Some(player_arc) = world.get_player_by_uuid(player.gameprofile.id)
        {
            let mut event =
                crate::plugin::api::events::player::player_leash_entity::PlayerLeashEntityEvent {
                    player: player_arc,
                    entity_id: self.entity_id,
                    holder_id: holder_entity_id,
                    cancelled: false,
                };
            server.plugin_manager.fire_blocking(&server, &mut event);
            if event.cancelled {
                return;
            }
        }

        self.attach_leash(holder);
    }

    pub fn attach_leash(&self, holder: Arc<dyn EntityBase>) {
        let holder_entity_id = holder.get_entity().entity_id;
        *self
            .delayed_leash
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        *self
            .leashed_to
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(holder);

        let je_packet = pumpkin_protocol::java::client::play::CSetEntityLink::new(
            self.entity_id,
            holder_entity_id,
            true,
        );
        let be_packet = pumpkin_protocol::bedrock::client::CSetActorLink {
            link: pumpkin_protocol::bedrock::client::common::ActorLink {
                ridden_unique_id: pumpkin_protocol::codec::var_long::VarLong(self.entity_id as i64),
                rider_unique_id: pumpkin_protocol::codec::var_long::VarLong(
                    holder_entity_id as i64,
                ),
                link_type: 1, // Leash link
                immediate: true,
                rider_initiated: false,
                vehicle_angular_velocity: 0.0,
            },
        };

        self.world.load().broadcast_to_chunk_editioned(
            self.chunk_pos.load(),
            &je_packet,
            &be_packet,
        );
    }

    pub fn unleash(&self) {
        let world = self.world.load();
        if let Some(server) = world.server.upgrade() {
            let mut event =
                crate::plugin::api::events::entity::entity_unleash::EntityUnleashEvent::new(
                    self.entity_id,
                    "unleashed".to_string(),
                );
            server.plugin_manager.fire_blocking(&server, &mut event);
            if event.cancelled {
                return;
            }
        }

        let old_holder = self
            .leashed_to
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if old_holder.is_none() {
            return;
        }

        if let Some(holder) = &old_holder
            && let Some(server) = world.server.upgrade()
            && let Some(player) = holder.get_player()
            && let Some(player_arc) = world.get_player_by_uuid(player.gameprofile.id)
        {
            let mut event = crate::plugin::api::events::player::player_unleash_entity::PlayerUnleashEntityEvent {
                player: player_arc,
                entity_id: self.entity_id,
                cancelled: false,
            };
            server.plugin_manager.fire_blocking(&server, &mut event);
        }

        self.send_leash_removed();
    }

    pub fn remove_leash(&self) {
        let old_holder = self
            .leashed_to
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if old_holder.is_some() {
            self.send_leash_removed();
        }
    }

    pub(super) fn send_leash_removed(&self) {
        let je_packet =
            pumpkin_protocol::java::client::play::CSetEntityLink::new(self.entity_id, -1, true);
        let be_packet = pumpkin_protocol::bedrock::client::CSetActorLink {
            link: pumpkin_protocol::bedrock::client::common::ActorLink {
                ridden_unique_id: pumpkin_protocol::codec::var_long::VarLong(self.entity_id as i64),
                rider_unique_id: pumpkin_protocol::codec::var_long::VarLong(-1),
                link_type: 0, // Unlink
                immediate: true,
                rider_initiated: false,
                vehicle_angular_velocity: 0.0,
            },
        };

        self.world.load().broadcast_to_chunk_editioned(
            self.chunk_pos.load(),
            &je_packet,
            &be_packet,
        );
    }

    pub fn write_leash_data(&self, nbt: &mut NbtCompound) {
        let holder = self
            .leashed_to
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let leash = match holder {
            Some(holder) if holder.get_entity().entity_type == &EntityType::LEASH_KNOT => {
                Some(DelayedLeash::Knot(holder.get_entity().block_pos.load()))
            }
            Some(holder) => Some(DelayedLeash::Entity(holder.get_entity().entity_uuid)),
            None => *self
                .delayed_leash
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        };
        match leash {
            Some(DelayedLeash::Entity(uuid)) => {
                let mut tag = NbtCompound::new();
                tag.put_uuid("UUID", uuid);
                nbt.put_compound("leash", tag);
            }
            Some(DelayedLeash::Knot(pos)) => nbt.put("leash", block_pos_to_tag(pos)),
            None => {}
        }
    }

    pub fn read_leash_data(&self, nbt: &NbtCompound) {
        let leash = match nbt.get("leash") {
            Some(NbtTag::Compound(tag)) => tag.get_uuid("UUID").map(DelayedLeash::Entity),
            Some(pos @ NbtTag::IntArray(_)) => block_pos_from_tag(pos).map(DelayedLeash::Knot),
            _ => None,
        };
        if leash.is_none() {
            self.remove_leash();
        }
        *self
            .leashed_to
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        *self
            .delayed_leash
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = leash;
    }

    pub(super) fn restore_leash_from_save(&self, delayed: DelayedLeash) {
        let world = self.world.load();
        let holder = match delayed {
            DelayedLeash::Entity(uuid) => world.get_entity_by_uuid(uuid).or_else(|| {
                world
                    .get_player_by_uuid(uuid)
                    .map(|p| p as Arc<dyn EntityBase>)
            }),
            DelayedLeash::Knot(pos) => Some(LeashKnotEntity::get_or_create(&world, pos)),
        };
        if let Some(holder) = holder {
            self.attach_leash(holder);
        } else if self.tick_count.load(Relaxed) > LEASH_RESTORE_TIMEOUT {
            self.spawn_at_location(ItemStack::new(1, &pumpkin_data::item::Item::LEAD));
            *self
                .delayed_leash
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        }
    }

    pub fn tick_leash(&self) {
        let delayed = *self
            .delayed_leash
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(delayed) = delayed {
            self.restore_leash_from_save(delayed);
        }

        let holder = {
            let Ok(guard) = self.leashed_to.try_lock() else {
                return;
            };
            guard.clone()
        };

        if let Some(holder) = holder {
            let holder_entity = holder.get_entity();

            // Drop leash if entity or holder is removed or dead
            if !self.is_alive() || !holder_entity.is_alive() {
                self.unleash();
                return;
            }

            let self_pos = self.pos.load();
            let holder_pos = holder_entity.pos.load();
            let diff = self_pos - holder_pos;
            let distance = diff.length();

            if distance > Self::LEASH_SNAP_DISTANCE {
                // Too far: snap/break leash and drop lead item
                self.unleash();
                let lead_item =
                    pumpkin_data::item_stack::ItemStack::new(1, &pumpkin_data::item::Item::LEAD);
                self.world
                    .load()
                    .drop_stack(&self.block_pos.load(), lead_item);
            } else if distance > Self::LEASH_ELASTIC_DISTANCE {
                // Elastic pull force towards leash holder
                let dir = (holder_pos - self_pos).normalize();
                let pull_strength = (distance - Self::LEASH_ELASTIC_DISTANCE) * 0.11;
                let current_vel = self.velocity.load();
                self.velocity.store(current_vel + dir * pull_strength);
                self.velocity_dirty.store(true, Relaxed);
            }
        }
    }

    pub fn is_leashed(&self) -> bool {
        self.leashed_to
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
    }
}
