//! The steps of `LivingEntity::damage_with_context`, one function per step.
//!
//! `damage_with_context` runs them in a fixed order; each step either lets the hit go on or ends
//! it, and the statements are the ones the single long method had, in the same order.

use std::sync::Arc;

use super::super::super::EntityBase;
use super::super::{LAST_HURT_BY_PLAYER_MEMORY_TIME, LivingEntity};
use crate::entity::combat::{FallLocation, knockback_after_resistance};
use crate::entity::player::statistics::{CustomStatistic, StatisticCategory};
use crate::server::Server;
use crate::world::World;
use pumpkin_data::attributes::Attributes;
use pumpkin_data::damage::DamageType;
use pumpkin_data::data_component_impl::EquipmentSlot;
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item_stack::{DamageResult, ItemStack};
use pumpkin_data::sound::{Sound, SoundCategory};
use pumpkin_data::tag::{self, Taggable};
use pumpkin_protocol::bedrock::server::actor_event::{ActorEventID, SActorEvent};
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::codec::var_ulong::VarULong;
use pumpkin_protocol::java::client::play::CHurtAnimation;
use pumpkin_util::Hand;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector3::Vector3;
use std::sync::atomic::Ordering::Relaxed;

/// Where a hit came from: its type, the place it came from, the entity that dealt it and the one
/// that caused it.
#[derive(Clone, Copy)]
pub(super) struct Attack<'a> {
    pub damage_type: DamageType,
    pub position: Option<Vector3<f64>>,
    pub source: Option<&'a dyn EntityBase>,
    pub cause: Option<&'a dyn EntityBase>,
}

/// What a hit did to health and absorption.
pub(super) struct ActualHurt {
    pub damage_to_health: f32,
    pub absorbed: f32,
    pub new_health: f32,
}

impl LivingEntity {
    /// Lets plugins change or cancel the hit. Returns the damage after them, or nothing when one
    /// cancelled it.
    pub(super) fn fire_damage_events(
        &self,
        amount: f32,
        damage_type: DamageType,
        position: Option<Vector3<f64>>,
        source: Option<&dyn EntityBase>,
        cause: Option<&dyn EntityBase>,
    ) -> Option<f32> {
        let mut amount = amount;
        let mut damage_event =
            crate::plugin::api::events::entity::entity_damage::EntityDamageEvent::new(
                self.entity.entity_id,
                damage_type,
                amount,
            );
        if let Some(server) = self.entity.world.load().server.upgrade() {
            server
                .plugin_manager
                .fire_blocking(&server, &mut damage_event);
        }
        if damage_event.cancelled {
            return None;
        }
        amount = damage_event.damage;

        if let Some(damager) = source.or(cause) {
            let mut by_entity_event =
                crate::plugin::api::events::entity::entity_damage_by_entity::EntityDamageByEntityEvent {
                    entity_id: self.entity.entity_id,
                    damager_id: damager.get_entity().entity_id,
                    damage: amount,
                    cause: format!("{damage_type:?}"),
                    cancelled: false,
                };
            if let Some(server) = self.entity.world.load().server.upgrade() {
                server
                    .plugin_manager
                    .fire_blocking(&server, &mut by_entity_event);
            }
            if by_entity_event.cancelled {
                return None;
            }
            amount = by_entity_event.damage;
        } else if position.is_some()
            || matches!(
                damage_type,
                DamageType::CACTUS
                    | DamageType::SWEET_BERRY_BUSH
                    | DamageType::CAMPFIRE
                    | DamageType::HOT_FLOOR
                    | DamageType::STALAGMITE
            )
        {
            let damager_pos = position.map(|p| {
                BlockPos(Vector3::new(
                    p.x.floor() as i32,
                    p.y.floor() as i32,
                    p.z.floor() as i32,
                ))
            });
            let mut by_block_event =
                crate::plugin::api::events::entity::entity_damage_by_block::EntityDamageByBlockEvent {
                    entity_id: self.entity.entity_id,
                    damager_pos,
                    damage: amount,
                    cause: format!("{damage_type:?}"),
                    cancelled: false,
                };
            if let Some(server) = self.entity.world.load().server.upgrade() {
                server
                    .plugin_manager
                    .fire_blocking(&server, &mut by_block_event);
            }
            if by_block_event.cancelled {
                return None;
            }
            amount = by_block_event.damage;
        }
        Some(amount)
    }

    /// Whether fire damage is prevented by the game rule or by fire resistance.
    pub(super) fn fire_damage_is_prevented(&self, world: &World, damage_type: DamageType) -> bool {
        let is_fire_damage = damage_type.has_tag(&tag::DamageType::MINECRAFT_IS_FIRE);

        // Fire damage can be prevented by either game rules or fire resistance
        if is_fire_damage {
            // Check game rule for fire damage (only for players)
            if self.entity.entity_type == &EntityType::PLAYER
                && !world.level_info.load().game_rules.fire_damage
            {
                return true;
            }

            // Check for fire resistance effect
            if self.has_effect(&StatusEffect::FIRE_RESISTANCE)
                && !damage_type.has_tag(&tag::DamageType::MINECRAFT_BYPASSES_EFFECTS)
            {
                return true;
            }
        }
        false
    }

    /// Entities the freeze rule picks out take five times the freezing damage.
    pub(super) fn freeze_scaled(&self, amount: f32, damage_type: DamageType) -> f32 {
        // Vanilla parity: entities in FREEZE_HURTS_EXTRA_TYPES take 5x freezing damage.
        if damage_type == DamageType::FREEZE
            && self
                .entity
                .entity_type
                .has_tag(&tag::EntityType::MINECRAFT_FREEZE_HURTS_EXTRA_TYPES)
        {
            return amount * 5.0;
        }
        amount
    }

    /// A raised shield facing the hit blocks it: sounds, statistic, wear on the shield, and a
    /// cooldown when the attacker's weapon disables shields. Returns whether it blocked.
    pub(super) fn blocked_by_shield(
        &self,
        caller: &dyn EntityBase,
        world: &Arc<World>,
        amount: f32,
        attack: Attack<'_>,
    ) -> bool {
        let Attack {
            damage_type,
            position,
            source,
            cause,
        } = attack;
        if !self.is_blocking() || damage_type.has_tag(&tag::DamageType::MINECRAFT_BYPASSES_SHIELD) {
            return false;
        }
        let Some(pos) = position else {
            return false;
        };
        let player_pos = self.entity.pos.load();
        let look_vec = Vector3::rotation_vector(0.0, self.entity.yaw.load() as f64);
        let mut source_to_player = (player_pos - pos).normalize();
        source_to_player.y = 0.0;

        if source_to_player.dot(&look_vec) < 0.0 {
            world.play_sound(Sound::ItemShieldBlock, SoundCategory::Players, &player_pos);
            world.play_bedrock_level_sound("item.shield.block", &player_pos, 0);

            if let Some(player) = caller.get_player() {
                player.increment_stat(
                    StatisticCategory::Custom,
                    CustomStatistic::DamageBlockedByShield as i32,
                    (amount * 10.0).round() as i32,
                );
            }
            let active_hand = *self
                .active_hand
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(hand) = active_hand {
                let slot = if hand == Hand::Left {
                    EquipmentSlot::OFF_HAND
                } else {
                    EquipmentSlot::MAIN_HAND
                };
                let blocking_item_key =
                    self.wear_blocking_item(caller, world, slot, amount, player_pos);
                self.disable_shield_after_block(
                    caller,
                    world,
                    source,
                    cause,
                    player_pos,
                    blocking_item_key,
                );
            }
            return true;
        }
        false
    }

    /// Wears the shield down by the damage it took and returns the key of the item that blocked.
    fn wear_blocking_item(
        &self,
        caller: &dyn EntityBase,
        world: &Arc<World>,
        slot: EquipmentSlot,
        amount: f32,
        player_pos: Vector3<f64>,
    ) -> Option<String> {
        let mut blocking_item_key = None;
        let durability_damage = (amount / 1.0).floor().max(1.0) as i32;
        if let Some(player) = caller.get_player() {
            let hand_slot = if slot == EquipmentSlot::OFF_HAND {
                Hand::Left
            } else {
                Hand::Right
            };
            let stack_before = player.inventory.get_stack_in_hand(hand_slot);
            if !stack_before.is_empty() {
                blocking_item_key = Some(stack_before.item.registry_key.to_string());
            }
            let broke = player.damage_item_in_slot(&slot, durability_damage);
            let empty = player.inventory.get_stack_in_hand(hand_slot).is_empty();
            if broke && empty {
                self.clear_active_hand();
            }
        } else {
            let mut equipment_guard = self
                .entity_equipment
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(stack) = equipment_guard.equipment.get_mut(&slot) {
                let break_sound = stack
                    .get_data_component::<pumpkin_data::data_component_impl::BreakSoundImpl>()
                    .and_then(|bs| match bs.sound {
                        pumpkin_data::data_component_impl::IdOr::Id(s) => Some(s),
                        pumpkin_data::data_component_impl::IdOr::Value(_) => None,
                    });
                if stack.damage_item(durability_damage) == DamageResult::Broken {
                    if let Some(sound) = break_sound {
                        world.play_sound(sound, SoundCategory::Neutral, &player_pos);
                    }
                    world.send_entity_status(
                        &self.entity,
                        crate::entity::equipment_break_status(&slot),
                        ActorEventID::None,
                    );
                    *stack = ItemStack::EMPTY.clone();
                    let broken_stack = stack.clone();
                    drop(equipment_guard);

                    self.send_equipment_changes(&[(slot, broken_stack)]);
                    self.clear_active_hand();
                }
            }
        }
        blocking_item_key
    }

    /// An attacker whose weapon disables shields puts the blocking item on cooldown.
    fn disable_shield_after_block(
        &self,
        caller: &dyn EntityBase,
        world: &Arc<World>,
        source: Option<&dyn EntityBase>,
        cause: Option<&dyn EntityBase>,
        player_pos: Vector3<f64>,
        blocking_item_key: Option<String>,
    ) {
        let disable_seconds = cause.or(source).map_or(0.0, |attacker| {
            if attacker.get_entity().entity_type == &EntityType::WARDEN {
                5.0
            } else if let Some(attacker_player) = attacker.get_player() {
                attacker_player
                    .inventory()
                    .held_item()
                    .get_data_component::<pumpkin_data::data_component_impl::WeaponImpl>()
                    .map_or(0.0, |w| w.disable_blocking_for_seconds)
            } else if let Some(attacker_living) = attacker.get_living_entity() {
                attacker_living
                    .entity_equipment
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(&EquipmentSlot::MAIN_HAND)
                    .get_data_component::<pumpkin_data::data_component_impl::WeaponImpl>()
                    .map_or(0.0, |w| w.disable_blocking_for_seconds)
            } else {
                0.0
            }
        });
        if disable_seconds > 0.0 {
            let cooldown_ticks = (disable_seconds * 20.0).round() as i32;
            if cooldown_ticks > 0 {
                if let Some(player) = caller.get_player()
                    && let Some(blocking_key) = blocking_item_key
                {
                    player.start_cooldown(blocking_key, cooldown_ticks);
                }
                self.clear_active_hand();
                world.play_sound(Sound::ItemShieldBreak, SoundCategory::Players, &player_pos);
                world.play_bedrock_level_sound("item.shield.break", &player_pos, 0);
            }
        }
    }

    /// Armour, then resistance and protection, take their share of the damage.
    pub(super) fn absorb_damage(
        &self,
        caller: &dyn EntityBase,
        amount: f32,
        damage_type: DamageType,
        attacker: Option<&dyn EntityBase>,
    ) -> f32 {
        // Vanilla parity: 1. Armor absorb
        let damage_after_armor = self.get_damage_after_armor_absorb(amount, &damage_type, attacker);

        self.get_damage_after_magic_absorb(damage_after_armor, &damage_type, caller, attacker)
    }

    /// The hurt cooldown: a second hit during it only counts for what it adds over the first.
    /// Returns the damage to apply and whether to play the hurt sound, or nothing when the hit
    /// does not count.
    pub(super) fn apply_hurt_cooldown(
        &self,
        damage_type: DamageType,
        effective_amount: f32,
    ) -> Option<(f32, bool)> {
        // These damage types bypass the hurt cooldown and death protection
        let bypasses_cooldown_protection =
            damage_type == DamageType::GENERIC_KILL || damage_type == DamageType::OUT_OF_WORLD;

        // Apply hurt cooldown logic
        let last_damage = self.last_damage_taken.load();
        let result = if self.hurt_cooldown.load(Relaxed) > 10 && !bypasses_cooldown_protection {
            if effective_amount <= last_damage {
                return None;
            }
            (effective_amount - last_damage, false)
        } else {
            self.hurt_cooldown.store(20, Relaxed);
            (effective_amount, self.health.load() > effective_amount)
        };
        Some(result)
    }

    /// Remembers what hurt the entity, once the hit is confirmed.
    pub(super) fn record_damage_source(
        &self,
        world: &World,
        damage_type: DamageType,
        source: Option<&dyn EntityBase>,
        cause: Option<&dyn EntityBase>,
    ) {
        // Record the source once the hit is confirmed.
        *self
            .last_damage_type
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(damage_type);
        self.last_damage_attacker.store(
            cause
                .or(source)
                .map_or(0, |attacker| attacker.get_entity().entity_id),
            Relaxed,
        );
        self.last_damage_stamp.store(world.get_world_age(), Relaxed);
    }

    /// An attacker that is a living entity becomes the one this entity is angry at.
    pub(super) fn anger_at_attacker(
        &self,
        damage_type: DamageType,
        source: Option<&dyn EntityBase>,
        cause: Option<&dyn EntityBase>,
    ) {
        if let Some(attacker) = cause.or(source)
            && attacker.get_living_entity().is_some()
            && !damage_type.has_tag(&tag::DamageType::MINECRAFT_NO_ANGER)
            && (damage_type != DamageType::WIND_CHARGE
                || !self
                    .entity
                    .entity_type
                    .has_tag(&tag::EntityType::MINECRAFT_NO_ANGER_FROM_WIND_CHARGE))
        {
            self.set_last_hurt_by_mob(Some(attacker));
        }
    }

    /// Tells the players around about the hurt: animation and damage event.
    pub(super) fn announce_hurt(
        &self,
        world: &World,
        server: &Server,
        damage_type: DamageType,
        position: Option<Vector3<f64>>,
        source: Option<&dyn EntityBase>,
        cause: Option<&dyn EntityBase>,
    ) {
        let config = &server.advanced_config.pvp;

        if config.hurt_animation {
            let entity_id = self.entity.entity_id;
            let hurt_yaw = source.map_or(0.0, |source| {
                let src = source.get_entity().pos.load();
                let tgt = self.entity.pos.load();
                (src.z - tgt.z).atan2(src.x - tgt.x).to_degrees() as f32 - self.entity.yaw.load()
            });
            let hurt_event = SActorEvent {
                target_runtime_id: VarULong(entity_id as u64),
                event_id: ActorEventID::Hurt,
                data: VarInt(0),
                fire_at_position: None,
            };
            let hurt_animation = CHurtAnimation::new(entity_id.into(), hurt_yaw);
            world.send_to_tracking_players_and_self_editioned(
                &self.entity,
                &hurt_animation,
                &hurt_event,
            );
        }

        world.broadcast_damage_event(
            &self.entity,
            i32::from(damage_type.id),
            source.map(|e| e.get_entity().entity_id),
            cause.map(|e| e.get_entity().entity_id),
            position,
        );
    }

    /// The hurt sound and the knockback from the source.
    pub(super) fn voice_and_knockback(
        &self,
        caller: &dyn EntityBase,
        play_sound: bool,
        source: Option<&dyn EntityBase>,
    ) {
        self.no_action_time.store(0, Relaxed);
        if play_sound {
            if let Some(mob) = caller.get_mob() {
                mob.reset_ambient_sound_time();
            }
            self.make_voice_sound(caller, self.hurt_sound(caller));

            if let Some(source) = source {
                let source_pos = source.get_entity().pos.load();
                let target_pos = self.entity.pos.load();
                let dx = source_pos.x - target_pos.x;
                let dz = source_pos.z - target_pos.z;
                let resistance = self.get_attribute_value(&Attributes::KNOCKBACK_RESISTANCE);
                self.entity
                    .apply_knockback(knockback_after_resistance(0.4, resistance), dx, dz);
            }
        }
    }

    /// Takes the damage from absorption first, then from health, and credits the attackers.
    pub(super) fn apply_actual_hurt(
        &self,
        caller: &dyn EntityBase,
        damage_type: DamageType,
        damage_amount: f32,
        source: Option<&dyn EntityBase>,
        cause: Option<&dyn EntityBase>,
    ) -> ActualHurt {
        let original_damage = damage_amount;
        let current_abs = self.absorption.load();
        let dmg_to_health = (original_damage - current_abs).max(0.0);
        let absorbed_damage = original_damage - dmg_to_health;

        if absorbed_damage > 0.0 {
            let new_abs = (current_abs - absorbed_damage).max(0.0);
            self.set_absorption(new_abs);

            if let Some(player) = caller.get_player() {
                player.increment_stat(
                    StatisticCategory::Custom,
                    CustomStatistic::DamageAbsorbed as i32,
                    (absorbed_damage * 10.0).round() as i32,
                );
            }

            if let Some(attacker_player) = cause.or(source).and_then(|c| c.get_player()) {
                attacker_player.increment_stat(
                    StatisticCategory::Custom,
                    CustomStatistic::DamageDealtAbsorbed as i32,
                    (absorbed_damage * 10.0).round() as i32,
                );
            }
        }

        let max_h = self.get_max_health();
        let new_health = (self.health.load() - dmg_to_health).clamp(0.0, max_h);

        if dmg_to_health > 0.0 {
            if let Some(player) = caller.get_player() {
                if damage_type.exhaustion > 0.0 {
                    player.add_exhaustion(damage_type.exhaustion);
                }
                player.increment_stat(
                    StatisticCategory::Custom,
                    CustomStatistic::DamageTaken as i32,
                    (dmg_to_health * 10.0).round() as i32,
                );
            }

            self.set_health(new_health);

            if let Some(attacker_player) = cause.or(source).and_then(|c| c.get_player()) {
                attacker_player.increment_stat(
                    StatisticCategory::Custom,
                    CustomStatistic::DamageDealt as i32,
                    (dmg_to_health * 10.0).round() as i32,
                );
                self.last_hurt_by_player
                    .store(Some(attacker_player.gameprofile.id));
                self.last_hurt_by_player_memory_time
                    .store(LAST_HURT_BY_PLAYER_MEMORY_TIME, Relaxed);
            }
        }
        ActualHurt {
            damage_to_health: dmg_to_health,
            absorbed: absorbed_damage,
            new_health,
        }
    }

    /// Adds the hit to the combat tracker.
    pub(super) fn record_in_combat_tracker(
        &self,
        world: &Arc<World>,
        damage_type: DamageType,
        effective_amount: f32,
        source: Option<&dyn EntityBase>,
        cause: Option<&dyn EntityBase>,
    ) {
        let current_tick = world.get_world_age();
        let fall_location = FallLocation::get_current_fall_location(self, world);
        let fall_distance = self.fall_distance.load();

        {
            let mut tracker = self
                .combat_tracker
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            tracker.record_damage(
                current_tick,
                self.health.load() > 0.0 && !self.dead.load(Relaxed),
                fall_distance,
                fall_location,
                damage_type,
                effective_amount,
                source,
                cause,
            );
        }
    }

    /// Fires the death event and runs the death when the hit took the last health.
    pub(super) fn die_when_health_is_gone(
        &self,
        world: &World,
        new_health: f32,
        damage_type: DamageType,
        source: Option<&dyn EntityBase>,
        cause: Option<&dyn EntityBase>,
    ) {
        if new_health <= 0.0 {
            let mut death_event =
                crate::plugin::api::events::entity::entity_death::EntityDeathEvent::new(
                    self.entity.entity_id,
                    0,
                );
            if let Some(server) = world.server.upgrade() {
                server
                    .plugin_manager
                    .fire_blocking(&server, &mut death_event);
            }
            self.on_death(damage_type, source, cause);
        }
    }
}
