//! Taking damage and armour reduction.

use super::super::EntityBase;
use super::LivingEntity;
use crate::entity::breath::DROWNING_INTERVAL;
use crate::entity::combat::CombatRules;
use crate::entity::player::statistics::{CustomStatistic, StatisticCategory};
use pumpkin_data::Enchantment;
use pumpkin_data::attributes::Attributes;
use pumpkin_data::damage::DamageType;
use pumpkin_data::data_component_impl::{AttributeModifiersImpl, EnchantmentsImpl, EquipmentSlot};
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::tag::{self, Taggable};
use pumpkin_util::Difficulty;
use pumpkin_util::math::vector3::Vector3;
use std::sync::atomic::Ordering::Relaxed;

#[cfg(test)]
mod damage_golden;
mod damage_steps;

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
        // Like vanilla, negative damage is still a hit, just of zero.
        amount = amount.max(0.0);

        let Some(amount) = self.fire_damage_events(amount, damage_type, position, source, cause)
        else {
            return false;
        };

        let world = self.entity.world.load();
        if self.fire_damage_is_prevented(&world, damage_type) {
            return false;
        }
        let amount = self.freeze_scaled(amount, damage_type);

        // Check for shield blocking before armor/magic/cooldown
        let attack = damage_steps::Attack {
            damage_type,
            position,
            source,
            cause,
        };
        if self.blocked_by_shield(caller, &world, amount, attack) {
            return false;
        }

        let effective_amount = self.absorb_damage(caller, amount, damage_type, cause.or(source));

        // Apply hurt cooldown logic
        let Some((damage_amount, play_sound)) =
            self.apply_hurt_cooldown(damage_type, effective_amount)
        else {
            return false;
        };

        // Finalize state
        self.last_damage_taken.store(amount);
        let damage_amount = damage_amount.max(0.0);

        self.record_damage_source(&world, damage_type, source, cause);

        let Some(server) = world.server.upgrade() else {
            return false;
        };
        self.anger_at_attacker(damage_type, source, cause);
        self.announce_hurt(&world, &server, damage_type, position, source, cause);
        self.voice_and_knockback(caller, play_sound, source);

        let hurt = self.apply_actual_hurt(caller, damage_type, damage_amount, source, cause);
        if hurt.damage_to_health > 0.0 || hurt.absorbed > 0.0 {
            self.record_in_combat_tracker(&world, damage_type, effective_amount, source, cause);
        }
        self.die_when_health_is_gone(&world, hurt.new_health, damage_type, source, cause);

        true
    }

    pub fn damage(&self, caller: &dyn EntityBase, amount: f32, damage_type: DamageType) -> bool {
        self.damage_with_context(caller, amount, damage_type, None, None, None)
    }
}
