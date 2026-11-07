//! Movement state: poses, flying, swimming, speed.

use super::super::EntityBase;
use super::{Player, statistics};
use crate::net::ClientPlatform;
use crate::plugin::api::events::entity::entity_toggle_glide::EntityToggleGlideEvent;
use crate::server::Server;
use core::f32;
use pumpkin_data::BlockState;
use pumpkin_data::data_component_impl::GliderImpl;
use pumpkin_data::entity::EntityPose;
use pumpkin_protocol::bedrock::client::SerializedAbilitiesDataSerializedLayer;
use pumpkin_protocol::bedrock::client::update_abilities::{Ability, CUpdateAbilities};
use pumpkin_protocol::bedrock::client::{
    CommandPermissionLevel, PlayerPermissionLevel, SerializedAbilitiesData,
};
use pumpkin_protocol::java::client::play::CPlayerAbilities;
use pumpkin_util::math::{boundingbox::BoundingBox, position::BlockPos, vector3::Vector3};
use pumpkin_util::permission::PermissionLvl;
use pumpkin_util::{GameMode, Hand};
use std::sync::Arc;
use std::sync::atomic::Ordering;

impl Player {
    pub fn get_off_ground_speed(&self) -> f64 {
        let sprinting = self.get_entity().is_sprinting();

        if !self.get_entity().has_vehicle() {
            let fly_speed =
                self.abilities.try_lock().ok().and_then(|abilities| {
                    abilities.flying.then_some(f64::from(abilities.fly_speed))
                });

            if let Some(flying) = fly_speed {
                return if sprinting { flying * 2.0 } else { flying };
            }
        }

        if sprinting { 0.025_999_999 } else { 0.02 }
    }

    pub fn is_flying(&self) -> bool {
        self.abilities.try_lock().is_ok_and(|a| a.flying)
    }

    pub fn set_sprinting(&self, is_sprinting: bool) {
        self.living_entity.set_sprinting(is_sprinting);
    }

    pub fn try_start_fall_flying(&self, server: &Arc<Server>) {
        let entity = self.get_entity();
        let has_glider = self
            .living_entity
            .entity_equipment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .equipment
            .values()
            .any(|stack| {
                stack.get_data_component::<GliderImpl>().is_some()
                    && !stack.next_damage_will_break()
            });
        let fall_flying = entity.check_fall_flying() && has_glider;
        if fall_flying && !entity.is_fall_flying() {
            let mut event = EntityToggleGlideEvent::new(entity.entity_id, true);
            server.plugin_manager.fire_blocking(server, &mut event);
            if !event.cancelled && event.is_gliding {
                entity.set_fall_flying(true);
                self.living_entity.fall_distance.store(0.0);
                self.update_player_pose();
            }
        }
    }

    pub fn stop_fall_flying(&self, server: &Arc<Server>) {
        let entity = self.get_entity();
        if entity.is_fall_flying() {
            let mut event = EntityToggleGlideEvent::new(entity.entity_id, false);
            server.plugin_manager.fire_blocking(server, &mut event);
            if !event.cancelled && !event.is_gliding {
                entity.set_fall_flying(false);
                self.update_player_pose();
            }
        }
    }

    #[must_use]
    pub fn get_block_speed_factor(&self) -> f32 {
        self.living_entity.get_block_speed_factor()
    }

    #[must_use]
    pub fn is_swimming(&self) -> bool {
        !self.is_flying()
            && self.gamemode.load() != GameMode::Spectator
            && self.get_entity().is_swimming()
    }

    pub fn update_swimming(&self) {
        if self.is_flying() {
            self.get_entity().set_swimming(false);
        } else {
            let entity = self.get_entity();
            let is_sprinting = entity.is_sprinting();
            let in_water = entity.is_in_water();
            let is_passenger = entity.has_vehicle();

            if entity.is_swimming() {
                entity.set_swimming(is_sprinting && in_water && !is_passenger);
            } else {
                let is_under_water = entity.is_under_water();
                let block_pos = entity.block_pos.load();
                let world = entity.world.load();
                let (fluid, _) = world.get_fluid_and_fluid_state(&block_pos);
                let is_water_block = fluid.id == pumpkin_data::fluid::Fluid::WATER.id
                    || fluid.id == pumpkin_data::fluid::Fluid::FLOWING_WATER.id;

                entity.set_swimming(
                    is_sprinting && is_under_water && !is_passenger && is_water_block,
                );
            }
        }
    }

    pub(super) const fn is_auto_spin_attack() -> bool {
        // TODO: Track active auto-spin/riptide state and return true while it is active.
        false
    }

    pub(super) fn can_fit_pose(&self, pose: EntityPose) -> bool {
        let entity = self.get_entity();
        let dimensions = entity.get_dimensions(pose);
        let position = entity.pos.load();
        let aabb = BoundingBox::new_from_pos(position.x, position.y, position.z, &dimensions);
        entity
            .world
            .load()
            .is_space_empty(aabb.contract_all(1.0E-7))
    }

    #[must_use]
    pub fn get_desired_pose(&self) -> EntityPose {
        let entity = self.get_entity();
        if self.is_sleeping() {
            EntityPose::Sleeping
        } else if self.is_swimming() {
            EntityPose::Swimming
        } else if entity.is_fall_flying() {
            EntityPose::FallFlying
        } else if Self::is_auto_spin_attack() {
            EntityPose::SpinAttack
        } else if entity.is_sneaking() && !self.is_flying() {
            EntityPose::Crouching
        } else {
            EntityPose::Standing
        }
    }

    pub fn update_player_pose(&self) {
        if !self.can_fit_pose(EntityPose::Swimming) {
            return;
        }

        self.update_swimming();
        let desired_pose = self.get_desired_pose();
        let actual_pose = if self.gamemode.load() == GameMode::Spectator
            || self.get_entity().has_vehicle()
            || self.can_fit_pose(desired_pose)
        {
            desired_pose
        } else if self.can_fit_pose(EntityPose::Crouching) {
            EntityPose::Crouching
        } else {
            EntityPose::Swimming
        };

        self.get_entity().set_pose(actual_pose);
    }

    pub fn set_movement_locked(&self, locked: bool) {
        self.is_movement_locked.store(locked, Ordering::Relaxed);
    }

    pub fn is_movement_locked(&self) -> bool {
        self.is_movement_locked.load(Ordering::Relaxed)
    }

    pub fn set_freeze_ticks(&self, ticks: i32) {
        self.living_entity.entity.set_frozen_ticks(ticks);
    }

    pub fn get_freeze_ticks(&self) -> i32 {
        self.living_entity.entity.get_frozen_ticks()
    }

    pub fn jump(&self) {
        self.stats
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .increment_custom(statistics::CustomStatistic::Jump, 1);
        if self.living_entity.entity.is_sprinting() {
            self.add_exhaustion(0.2);
        } else {
            self.add_exhaustion(0.05);
        }
    }

    /// Vanilla `playMuffledStepSound`: the block under a carpet or under water, quieter and lower.
    pub(super) fn play_muffled_step_sound(&self, state: &BlockState) {
        let sound_type = state.get_sound_type();
        self.play_entity_sound(
            sound_type.step_sound,
            sound_type.volume * 0.05,
            sound_type.pitch * 0.8,
        );
    }

    pub fn progress_motion(&self, delta_pos: Vector3<f64>) {
        // TODO: gliding...
        let entity = &self.living_entity.entity;
        let (rate, distance) = if self.is_swimming() || entity.is_submerged_in_water() {
            (0.01, delta_pos.length())
        } else if entity.is_in_water() {
            (0.01, delta_pos.horizontal_length())
        } else if self.living_entity.climbing.load(Ordering::Relaxed) {
            return;
        } else if entity.on_ground.load(Ordering::Relaxed) {
            let rate = if entity.is_sprinting() { 0.1 } else { 0.0 };
            (rate, delta_pos.horizontal_length())
        } else {
            return;
        };

        let delta = (distance * 100.0).round() as f32;
        if delta > 0.0 {
            self.add_exhaustion(rate * delta * 0.01);
        }
    }

    /// Updates the current abilities the player has.
    pub fn send_abilities_update(&self) {
        let abilities = *self
            .abilities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match self.client.as_ref() {
            ClientPlatform::Java(java) => {
                let mut b = 0;

                if abilities.invulnerable {
                    b |= 1;
                }
                if abilities.flying {
                    b |= 2;
                }
                if abilities.allow_flying {
                    b |= 4;
                }
                if abilities.creative {
                    b |= 8;
                }
                let packet = CPlayerAbilities::new(b, abilities.fly_speed, abilities.walk_speed);
                if let Ok(data) = java.serialize_packet(&packet) {
                    java.try_enqueue_packet(data);
                }
            }
            ClientPlatform::Bedrock(bedrock) => {
                let is_op = self.permission_lvl.load() == PermissionLvl::Four;
                let is_spectator = self.gamemode.load() == GameMode::Spectator;

                // 1. Permission Mapping
                let player_perm = if is_op {
                    PlayerPermissionLevel::Operator
                } else {
                    PlayerPermissionLevel::Member
                };
                let command_perm = if is_op {
                    CommandPermissionLevel::GameDirectors
                } else {
                    CommandPermissionLevel::Any
                };

                // 2. Build the Ability Bitmask
                let mut ability_value: u32 = 0;

                // Helper closure to set bits using your enum
                let mut set_ability = |ability: Ability, enabled: bool| {
                    if enabled {
                        ability_value |= 1 << (ability as u32);
                    }
                };

                // Base Permissions
                set_ability(Ability::MayFly, abilities.allow_flying);
                set_ability(Ability::Flying, abilities.flying);
                set_ability(
                    Ability::Invulnerable,
                    abilities.invulnerable || abilities.creative,
                );

                // Operator Specifics
                set_ability(Ability::OperatorCommands, is_op);
                set_ability(Ability::Teleport, is_op);

                // Interaction Permissions (Disabled for Spectators)
                let can_interact = !is_spectator;
                set_ability(Ability::Build, can_interact);
                set_ability(Ability::Mine, can_interact);
                set_ability(Ability::DoorsAndSwitches, can_interact);
                set_ability(Ability::OpenContainers, can_interact);
                set_ability(Ability::AttackPlayers, can_interact);
                set_ability(Ability::AttackMobs, can_interact);

                // Creative/Spectator Extras
                set_ability(Ability::Instabuild, abilities.creative);
                set_ability(Ability::NoClip, is_spectator);

                // 3. Construct the Layers
                let mut layers = vec![SerializedAbilitiesDataSerializedLayer {
                    serialized_layer: 0, // LAYER_BASE
                    // 0x3FFFF defines the first 18 bits as "provided" by this packet
                    abilities_set: (1 << Ability::AbilityCount as u32) - 1,
                    ability_value,
                    fly_speed: 0.05,
                    vertical_fly_speed: 1.0,
                    walk_speed: 0.1,
                }];

                if is_spectator {
                    layers.push(SerializedAbilitiesDataSerializedLayer {
                        serialized_layer: 1,
                        abilities_set: 1 << (Ability::Flying as u32),
                        ability_value: 1 << (Ability::Flying as u32),
                        fly_speed: 0.05,
                        vertical_fly_speed: 1.0,
                        walk_speed: 0.1,
                    });
                }

                let packet = CUpdateAbilities {
                    data: SerializedAbilitiesData {
                        target_player_raw_id: self.entity_id().into(),
                        player_permissions: player_perm,
                        command_permissions: command_perm,
                        layers,
                    },
                };

                if let Ok(data) = bedrock.serialize_packet(&packet) {
                    bedrock.try_enqueue_packet(data);
                }
            }
        }
    }

    pub fn update_bedrock_shield_sneaking(&self) {
        if !matches!(self.client.as_ref(), ClientPlatform::Bedrock(_)) {
            return;
        }
        if !self.get_entity().is_sneaking() {
            if self.living_entity.is_blocking() {
                self.living_entity.clear_active_hand();
            }
            return;
        }

        let is_blocking = self.living_entity.is_blocking();
        let shield_hand_and_stack = [Hand::Left, Hand::Right].into_iter().find_map(|hand| {
            let stack = self.inventory.get_stack_in_hand(hand);
            (stack
                .get_data_component::<pumpkin_data::data_component_impl::BlocksAttacksImpl>()
                .is_some()
                && !self.is_on_cooldown(stack.item.registry_key))
            .then_some((hand, stack))
        });

        match (is_blocking, shield_hand_and_stack) {
            (true, Some((hand, stack))) => {
                let current_hand = *self
                    .living_entity
                    .active_hand
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if current_hand != Some(hand) {
                    self.living_entity.set_active_hand(hand, stack, 72000);
                }
            }
            (true, None) => {
                self.living_entity.clear_active_hand();
            }
            (false, Some((hand, stack))) => {
                self.living_entity.set_active_hand(hand, stack, 72000);
            }
            (false, None) => {}
        }
    }

    pub fn set_allow_flight(&self, allow: bool) {
        self.abilities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .allow_flying = allow;
        self.send_abilities_update();
    }

    pub fn set_flying(&self, flying: bool) {
        if flying {
            self.living_entity.fall_distance.store(0.0);
        }
        self.abilities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .flying = flying;
        self.send_abilities_update();
    }

    pub fn set_fly_speed(&self, speed: f32) {
        self.abilities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .fly_speed = speed;
        self.send_abilities_update();
    }

    pub fn set_walk_speed(&self, speed: f32) {
        self.abilities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .walk_speed = speed;
        self.send_abilities_update();
    }

    /// Returns the main non-air `BlockPos` underneath the player.
    pub fn get_supporting_block_pos(&self) -> Option<BlockPos> {
        let entity = self.get_entity();
        let entity_pos = entity.pos.load();
        let aabb = entity.bounding_box.load();
        let world = self.world();

        // Create the thin bounding box directly underneath the entity's feet
        let footprint = BoundingBox::new(
            Vector3::new(aabb.min.x, aabb.min.y - 1.0e-6, aabb.min.z),
            Vector3::new(aabb.max.x, aabb.min.y, aabb.max.z),
        );

        let min_pos = footprint.min_block_pos();
        let max_pos = footprint.max_block_pos();

        let mut closest_candidate = None;
        let mut min_dist_sq = f64::MAX;

        // Iterate through candidates
        for pos in BlockPos::iterate(min_pos, max_pos) {
            let (_, state) = world.get_block_and_state(&pos);

            // Only consider physical blocks
            if state.is_air() {
                continue;
            }

            // Calculate distance squared from the block's center to the entity's position
            let block_center_x = f64::from(pos.0.x) + 0.5;
            let block_center_y = f64::from(pos.0.y) + 0.5;
            let block_center_z = f64::from(pos.0.z) + 0.5;

            let dx = block_center_x - entity_pos.x;
            let dy = block_center_y - entity_pos.y;
            let dz = block_center_z - entity_pos.z;
            let dist_sq = dx * dx + dy * dy + dz * dz;

            // Pick the block with the smallest distance
            if dist_sq < min_dist_sq {
                min_dist_sq = dist_sq;
                closest_candidate = Some(pos);
            } else if (dist_sq - min_dist_sq).abs() < f64::EPSILON {
                // If the distance is the same, pick the block with the smallest y, then z, then x
                if let Some(best_pos) = closest_candidate {
                    let is_smaller = pos.0.y < best_pos.0.y
                        || (pos.0.y == best_pos.0.y && pos.0.z < best_pos.0.z)
                        || (pos.0.y == best_pos.0.y
                            && pos.0.z == best_pos.0.z
                            && pos.0.x < best_pos.0.x);

                    if is_smaller {
                        closest_candidate = Some(pos);
                    }
                }
            }
        }

        // Return the closest block if we found one
        if closest_candidate.is_some() {
            return closest_candidate;
        }

        // Fallback to the block directly underneath the player's position if no candidates were found
        let fallback_pos = BlockPos::new(
            entity_pos.x.floor() as i32,
            (entity_pos.y - 0.2).floor() as i32,
            entity_pos.z.floor() as i32,
        );

        let state = world.get_block_state(&fallback_pos);
        (!state.is_air()).then_some(fallback_pos)
    }
}
