//! Taking damage and armour reduction.

use super::super::EntityBase;
use super::{LAST_HURT_BY_PLAYER_MEMORY_TIME, LivingEntity};
use crate::entity::breath::DROWNING_INTERVAL;
use crate::entity::combat::{CombatRules, FallLocation, knockback_after_resistance};
use crate::entity::player::statistics::{CustomStatistic, StatisticCategory};
use pumpkin_data::Enchantment;
use pumpkin_data::attributes::Attributes;
use pumpkin_data::data_component_impl::{AttributeModifiersImpl, EnchantmentsImpl, EquipmentSlot};
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item_stack::{DamageResult, ItemStack};
use pumpkin_data::sound::SoundCategory;
use pumpkin_data::tag::{self, Taggable};
use pumpkin_data::{damage::DamageType, sound::Sound};
use pumpkin_protocol::bedrock::server::actor_event::{ActorEventID, SActorEvent};
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::codec::var_ulong::VarULong;
use pumpkin_protocol::java::client::play::CHurtAnimation;
use pumpkin_util::Difficulty;
use pumpkin_util::Hand;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector3::Vector3;
use std::sync::atomic::Ordering::Relaxed;

impl LivingEntity {
    #[must_use]
    pub fn should_take_drowning_damage(&self) -> bool {
        self.entity.air_supply.load(Relaxed) <= -DROWNING_INTERVAL
    }

    pub fn can_attack(&self, target: &dyn EntityBase) -> bool {
        if target.get_player().is_some()
            && self.entity.world.load().level_info.load().difficulty == Difficulty::Peaceful
        {
            return false;
        }
        target.can_be_seen_as_enemy()
    }

    /// Calculates damage after armor reduction, mirroring vanilla `LivingEntity.getDamageAfterArmorAbsorb`.
    pub fn get_damage_after_armor_absorb(
        &self,
        damage: f32,
        damage_type: &DamageType,
        attacker: Option<&dyn EntityBase>,
    ) -> f32 {
        if damage_type.has_tag(&tag::DamageType::MINECRAFT_BYPASSES_ARMOR) {
            return damage;
        }

        let mut armor = 0.0f32;
        let mut toughness = 0.0f32;
        {
            let equipment_lock = self
                .entity_equipment
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for slot in [
                EquipmentSlot::HEAD,
                EquipmentSlot::CHEST,
                EquipmentSlot::LEGS,
                EquipmentSlot::FEET,
            ] {
                if let Some(stack) = equipment_lock.equipment.get(&slot)
                    && !stack.is_empty()
                    && let Some(modifiers) = stack.get_data_component::<AttributeModifiersImpl>()
                {
                    for modifier in modifiers.attribute_modifiers.iter() {
                        if modifier.r#type == &Attributes::ARMOR {
                            armor += modifier.amount as f32;
                        } else if modifier.r#type == &Attributes::ARMOR_TOUGHNESS {
                            toughness += modifier.amount as f32;
                        }
                    }
                }
            }
        }

        let breach_level = attacker
            .and_then(|att| {
                let player = att.get_player()?;
                let hand_stack = player
                    .inventory()
                    .get_stack_in_hand(pumpkin_util::Hand::Right);
                let level = hand_stack.get_enchantment_level(&Enchantment::BREACH);
                (level > 0).then_some(level as u32)
            })
            .unwrap_or(0);

        CombatRules::get_damage_after_absorb(damage, armor, toughness, breach_level)
    }

    /// Calculates damage after magic/resistance/enchantment reduction, mirroring vanilla `LivingEntity.getDamageAfterMagicAbsorb`.
    pub fn get_damage_after_magic_absorb(
        &self,
        mut damage: f32,
        damage_type: &DamageType,
        caller: &dyn EntityBase,
        cause: Option<&dyn EntityBase>,
    ) -> f32 {
        if damage_type.has_tag(&tag::DamageType::MINECRAFT_BYPASSES_EFFECTS) {
            return damage;
        }

        // 1. Resistance Effect (evaluated before enchantments)
        if !damage_type.has_tag(&tag::DamageType::MINECRAFT_BYPASSES_RESISTANCE)
            && let Some(effect) = self.get_effect(&StatusEffect::RESISTANCE)
        {
            let absorb_value = (effect.amplifier + 1) * 5;
            let absorb = 25 - absorb_value;
            let v = damage * absorb as f32;
            let old_damage = damage;
            damage = (v / 25.0).max(0.0);
            let damage_resisted = old_damage - damage;
            if damage_resisted > 0.0 {
                if let Some(victim_player) = caller.get_player() {
                    victim_player.increment_stat(
                        StatisticCategory::Custom,
                        CustomStatistic::DamageResisted as i32,
                        (damage_resisted * 10.0).round() as i32,
                    );
                } else if let Some(attacker_player) = cause.and_then(|c| c.get_player()) {
                    attacker_player.increment_stat(
                        StatisticCategory::Custom,
                        CustomStatistic::DamageDealtResisted as i32,
                        (damage_resisted * 10.0).round() as i32,
                    );
                }
            }
        }

        if damage <= 0.0 {
            return 0.0;
        }

        // 2. Enchantment Protection
        if damage_type.has_tag(&tag::DamageType::MINECRAFT_BYPASSES_ENCHANTMENTS) {
            return damage;
        }

        let mut epf = 0.0f32;
        {
            let equipment_lock = self
                .entity_equipment
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for slot in [
                EquipmentSlot::HEAD,
                EquipmentSlot::CHEST,
                EquipmentSlot::LEGS,
                EquipmentSlot::FEET,
            ] {
                if let Some(stack) = equipment_lock.equipment.get(&slot)
                    && !stack.is_empty()
                    && let Some(enchantments) = stack.get_data_component::<EnchantmentsImpl>()
                {
                    for (enchantment, level) in enchantments.enchantment.iter() {
                        enchantment.modify_damage_protection_against(*level, damage_type, &mut epf);
                    }
                }
            }
        }

        if epf > 0.0 {
            damage = CombatRules::get_damage_after_magic_absorb(damage, epf);
        }

        damage
    }

    #[allow(clippy::too_many_lines)]
    pub fn damage_with_context(
        &self,
        caller: &dyn EntityBase,
        amount: f32,
        damage_type: DamageType,
        position: Option<Vector3<f64>>,
        source: Option<&dyn EntityBase>,
        cause: Option<&dyn EntityBase>,
    ) -> bool {
        let mut amount = amount;

        // Check invulnerability before applying damage
        if self.entity.is_invulnerable_to(&damage_type, cause) {
            return false;
        }

        if self.health.load() <= 0.0 || self.dead.load(Relaxed) {
            return false; // Dying or dead
        }

        // Like vanilla, negative damage is still a hit, just of zero.
        amount = amount.max(0.0);

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
            return false;
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
                return false;
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
                return false;
            }
            amount = by_block_event.damage;
        }

        let world = self.entity.world.load();
        let is_fire_damage = damage_type.has_tag(&tag::DamageType::MINECRAFT_IS_FIRE);

        // Fire damage can be prevented by either game rules or fire resistance
        if is_fire_damage {
            // Check game rule for fire damage (only for players)
            if self.entity.entity_type == &EntityType::PLAYER
                && !world.level_info.load().game_rules.fire_damage
            {
                return false;
            }

            // Check for fire resistance effect
            if self.has_effect(&StatusEffect::FIRE_RESISTANCE)
                && !damage_type.has_tag(&tag::DamageType::MINECRAFT_BYPASSES_EFFECTS)
            {
                return false;
            }
        }

        // Vanilla parity: entities in FREEZE_HURTS_EXTRA_TYPES take 5x freezing damage.
        if damage_type == DamageType::FREEZE
            && self
                .entity
                .entity_type
                .has_tag(&tag::EntityType::MINECRAFT_FREEZE_HURTS_EXTRA_TYPES)
        {
            amount *= 5.0;
        }

        // Check for shield blocking before armor/magic/cooldown
        if self.is_blocking()
            && !damage_type.has_tag(&tag::DamageType::MINECRAFT_BYPASSES_SHIELD)
            && let Some(pos) = position
        {
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
                            world.play_sound(
                                Sound::ItemShieldBreak,
                                SoundCategory::Players,
                                &player_pos,
                            );
                            world.play_bedrock_level_sound("item.shield.break", &player_pos, 0);
                        }
                    }
                }

                return false;
            }
        }

        // Vanilla parity: 1. Armor absorb
        let damage_after_armor =
            self.get_damage_after_armor_absorb(amount, &damage_type, cause.or(source));

        let effective_amount = self.get_damage_after_magic_absorb(
            damage_after_armor,
            &damage_type,
            caller,
            cause.or(source),
        );

        // These damage types bypass the hurt cooldown and death protection
        let bypasses_cooldown_protection =
            damage_type == DamageType::GENERIC_KILL || damage_type == DamageType::OUT_OF_WORLD;

        // Apply hurt cooldown logic
        let last_damage = self.last_damage_taken.load();
        let (damage_amount, play_sound) =
            if self.hurt_cooldown.load(Relaxed) > 10 && !bypasses_cooldown_protection {
                if effective_amount <= last_damage {
                    return false;
                }
                (effective_amount - last_damage, false)
            } else {
                self.hurt_cooldown.store(20, Relaxed);
                (effective_amount, self.health.load() > effective_amount)
            };

        // Finalize state
        self.last_damage_taken.store(amount);
        let damage_amount = damage_amount.max(0.0);

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

        let Some(server) = world.server.upgrade() else {
            return false;
        };
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

        // Vanilla parity: actuallyHurt
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

        if dmg_to_health > 0.0 || absorbed_damage > 0.0 {
            let current_tick = world.get_world_age();
            let fall_location = FallLocation::get_current_fall_location(self, &world);
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

        true
    }

    pub fn damage(&self, caller: &dyn EntityBase, amount: f32, damage_type: DamageType) -> bool {
        self.damage_with_context(caller, amount, damage_type, None, None, None)
    }
}
