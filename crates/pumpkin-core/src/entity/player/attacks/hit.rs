//! The steps of a player's attack: what the weapon adds, the charge, the damage, the mace
//! smash, fire aspect, the critical-hit effect, the sweep and the weapon's wear.

use super::super::super::EntityBase;
use super::super::super::{Entity, combat};
use super::super::{Player, statistics};
use crate::net::ClientPlatform;
use crate::server::Server;
use crate::world::World;
use pumpkin_data::Enchantment;
use pumpkin_data::damage::DamageType;
use pumpkin_data::data_component_impl::{AttributeModifiersImpl, EnchantmentsImpl, Operation};
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::sound::{Sound, SoundCategory};
use pumpkin_protocol::codec::var_ulong::VarULong;
use pumpkin_protocol::java::client::play::{Animation, CEntityAnimation};
use pumpkin_util::math::{boundingbox::BoundingBox, vector3::Vector3};
use std::sync::Arc;

/// What the held item adds to an attack on `victim_entity`: the attack speed modifier, the
/// enchantment damage and the knockback level.
pub(super) fn weapon_bonuses(item_stack: &ItemStack, victim_entity: &Entity) -> (f64, f64, u32) {
    let mut add_speed = 0.0;
    let mut extra_ench_damage = 0.0;
    let mut knockback_level = 0u32;

    {
        let stack = &item_stack;
        if stack.is_empty() {
            // Vanilla fist: base_attack_speed = -2.4
            add_speed = -2.4;
        } else if let Some(modifiers) = stack.get_data_component::<AttributeModifiersImpl>() {
            // Only attack speed is read from the item here.
            // Attack damage is already  part of `base_damage` via the held item's live
            // ATTACK_DAMAGE modifier, re-adding it here would double it.
            for item_mod in modifiers.attribute_modifiers.iter() {
                if item_mod.operation == Operation::AddValue
                    && item_mod.id == "minecraft:base_attack_speed"
                {
                    add_speed = item_mod.amount;
                }
            }
        }
        if let Some(enchantments) = stack.get_data_component::<EnchantmentsImpl>() {
            for (enchantment, level) in enchantments.enchantment.iter() {
                enchantment.modify_damage_against(
                    *level,
                    &mut extra_ench_damage,
                    Some(victim_entity.entity_type),
                );
                let mut kb = 0.0f32;
                enchantment.modify_knockback(*level, &mut kb);
                knockback_level += kb as u32;
            }
        }
    }
    (add_speed, extra_ench_damage, knockback_level)
}

/// Shows the critical-hit effect on the victim to everyone.
pub(super) fn show_critical_hit(world: &World, victim_entity: &Entity) {
    let je_packet =
        CEntityAnimation::new(victim_entity.entity_id.into(), Animation::CriticalEffect);
    let be_packet = pumpkin_protocol::bedrock::server::animate::SAnimate {
        action: pumpkin_protocol::bedrock::server::animate::AnimateAction::CriticalHit,
        target_actor_runtime_id: VarULong(victim_entity.entity_id as u64),
        data: 0.0,
        swing_source: None,
        hand: 0,
    };
    world.broadcast_editioned(&je_packet, &be_packet);
}

/// Fire aspect: sets the victim on fire for the enchantment's duration.
pub(super) fn ignite_victim(item_stack: &ItemStack, victim_entity: &Entity) {
    if let Some(enchantments) = item_stack.get_data_component::<EnchantmentsImpl>() {
        for (enchantment, level) in enchantments.enchantment.iter() {
            for post_effect in enchantment.get_post_attack_effects() {
                if post_effect.affected
                    == Some(pumpkin_data::enchantment::EnchantmentTarget::Victim)
                    && let pumpkin_data::enchantment::EnchantmentEntityEffect::Ignite { duration } =
                        &post_effect.effect
                {
                    let duration_seconds = duration.calculate(*level);
                    victim_entity.set_on_fire_for_ticks((duration_seconds * 20.0) as u32);
                }
            }
        }
    }
}

impl Player {
    /// Counts the use of the held item, if any.
    pub(super) fn count_weapon_use(&self, item_stack: &ItemStack) {
        if !item_stack.is_empty() {
            self.increment_stat(
                statistics::StatisticCategory::Used,
                item_stack.item.id as i32,
                1,
            );
        }
    }

    /// The attack charge (1.0 for Bedrock players), or `None` when the item needs a minimum
    /// charge that is not reached (Java only).
    pub(super) fn attack_charge(
        &self,
        server: &Server,
        item_stack: &ItemStack,
        attack_speed: f64,
    ) -> Option<f64> {
        let is_bedrock = matches!(self.client.as_ref(), ClientPlatform::Bedrock(_));
        if !is_bedrock
            && let Some(min_charge) = item_stack
                .get_data_component::<pumpkin_data::data_component_impl::MinimumAttackChargeImpl>(
            )
            && min_charge.charge > 0.0
        {
            let charge_progress = self.get_attack_cooldown_progress(
                f64::from(server.basic_config.tps),
                5.0,
                attack_speed,
            );
            if (charge_progress as f32) < min_charge.charge {
                return None;
            }
        }
        let attack_cooldown_progress = if is_bedrock {
            1.0
        } else {
            self.get_attack_cooldown_progress(f64::from(server.basic_config.tps), 0.5, attack_speed)
        };
        Some(attack_cooldown_progress)
    }

    /// The damage of a hit at this charge, with the enchantment damage, never below zero.
    /// Strength and weakness are already in `base_damage`: their effects modify the
    /// `ATTACK_DAMAGE` attribute (vanilla `Player.attack` reads only that attribute).
    pub(super) fn charged_damage(
        base_damage: f64,
        attack_cooldown_progress: f64,
        extra_ench_damage: f64,
    ) -> f64 {
        // Only reduce attack damage if in cooldown
        // TODO: Enchantments are reduced in the same way, just without the square.
        let damage_multiplier = if attack_cooldown_progress < 1.0 {
            attack_cooldown_progress.powi(2).mul_add(0.8, 0.2)
        } else {
            1.0
        };

        // Modify the added damage based on the multiplier.
        let mut damage = base_damage * damage_multiplier;
        damage += extra_ench_damage * attack_cooldown_progress;

        damage.max(0.0)
    }

    /// The extra damage of a mace smash: per block fallen, 1.5 plus the density bonus.
    pub(super) fn mace_smash_bonus(&self, item_stack: &ItemStack) -> f64 {
        let fall_distance = self.living_entity.fall_distance.load();
        let mut smash_bonus_per_block = 0.0f64;
        if let Some(enchantments) = item_stack.get_data_component::<EnchantmentsImpl>() {
            for (enchantment, level) in enchantments.enchantment.iter() {
                enchantment.modify_fall_based_damage(*level, &mut smash_bonus_per_block);
            }
        }
        (1.5 + smash_bonus_per_block) * f64::from(fall_distance)
    }

    /// Deals the hit to the victim; on no damage, plays the no-damage sound and returns false.
    pub(super) fn deal_hit(
        &self,
        victim: &Arc<dyn EntityBase>,
        world: &World,
        damage: f64,
        is_mace_smash: bool,
        item_stack: &ItemStack,
    ) -> bool {
        if !victim.damage_with_context(
            victim.as_ref(),
            damage as f32,
            if is_mace_smash {
                DamageType::MACE_SMASH
            } else {
                item_stack
                    .get_data_component::<pumpkin_data::data_component_impl::DamageTypeImpl>()
                    .map_or(DamageType::PLAYER_ATTACK, |dt| dt.damage_type)
            },
            None,
            Some(self),
            Some(self),
        ) {
            world.play_sound(
                Sound::EntityPlayerAttackNodamage,
                SoundCategory::Players,
                &self.living_entity.entity.pos.load(),
            );
            return false;
        }
        true
    }

    /// After a landed mace smash: resets the fall distance, applies wind burst and plays the
    /// smash sound at the victim.
    pub(super) fn finish_mace_smash(
        &self,
        world: &World,
        item_stack: &ItemStack,
        pos: &Vector3<f64>,
    ) {
        let fall_distance = self.living_entity.fall_distance.load();
        self.living_entity.fall_distance.store(0.0);
        if let Some(enchantments) = item_stack.get_data_component::<EnchantmentsImpl>() {
            for (enchantment, level) in enchantments.enchantment.iter() {
                if **enchantment == Enchantment::WIND_BURST {
                    let boost_y = 0.5 + 0.25 * (*level as f64);
                    let vel = self.living_entity.entity.velocity.load();
                    self.living_entity
                        .entity
                        .velocity
                        .store(Vector3::new(vel.x, boost_y, vel.z));
                }
            }
        }
        world.play_sound(
            if fall_distance > 5.0 {
                Sound::ItemMaceSmashGroundHeavy
            } else {
                Sound::ItemMaceSmashGround
            },
            SoundCategory::Players,
            pos,
        );
    }

    /// Remembers the victim as the last attacked entity, with the attacker's tick.
    pub(super) fn remember_attack(&self, victim_entity: &Entity) {
        self.living_entity.last_attacking_id.store(
            victim_entity.entity_id,
            std::sync::atomic::Ordering::Relaxed,
        );
        self.living_entity.last_attack_time.store(
            self.living_entity
                .entity
                .tick_count
                .load(std::sync::atomic::Ordering::Relaxed),
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    /// A sweep: the particle, then the sweep damage to every other entity around the victim
    /// (sweeping edge adds a share of the hit's damage).
    pub(super) fn sweep(
        &self,
        world: &World,
        item_stack: &ItemStack,
        victim_entity: &Entity,
        pos: &Vector3<f64>,
        damage: f64,
    ) {
        let attacker_entity = &self.living_entity.entity;
        combat::spawn_sweep_particle(attacker_entity, world, pos);

        let mut sweep_damage = 1.0;
        if let Some(enchantments) = item_stack.get_data_component::<EnchantmentsImpl>() {
            for (enchantment, level) in enchantments.enchantment.iter() {
                if **enchantment == Enchantment::SWEEPING_EDGE {
                    sweep_damage += damage as f32 * (*level as f32 / (*level as f32 + 1.0));
                }
            }
        }

        let search_box = BoundingBox::new(
            Vector3::new(pos.x - 1.0, pos.y - 0.5, pos.z - 1.0),
            Vector3::new(pos.x + 1.0, pos.y + 0.5, pos.z + 1.0),
        );
        let victims = world.get_all_at_box(&search_box);
        for other_victim in victims {
            if other_victim.get_entity().entity_id != victim_entity.entity_id
                && other_victim.get_entity().entity_id != attacker_entity.entity_id
            {
                other_victim.damage_with_context(
                    other_victim.as_ref(),
                    sweep_damage,
                    DamageType::PLAYER_ATTACK,
                    None,
                    Some(self),
                    Some(self),
                );
            }
        }
    }

    /// After a landed hit: the weapon's durability cost and the hit's exhaustion.
    pub(super) fn wear_weapon_and_tire(&self, item_stack: &ItemStack) {
        // NOTE: TOCTOU race condition in single-player context.
        // The weapon cost is computed (cost = 1 or 2) with item_stack locked, then damage_held_item
        // re-acquires the lock. In async multi-task scenarios, another task could theoretically
        // swap the held item between these operations, causing the cost to apply to the wrong item.
        // Mitigation options (in priority order):
        // 1. Create damage_held_item_with_lock(&self, item_stack: MutexGuard, amount) variant
        //    to hold the lock across both computation and application.
        // 2. Refactor compute cost as a closure: damage_held_item(self, |stack| -> i32 { ... })
        // 3. In practice, single-player scenarios are safe (this is not multiplayer). Document
        //    as a known limitation if refactoring is deemed too invasive.
        self.damage_held_item(Self::combat_weapon_durability_cost(item_stack));

        // Vanilla `Player#attack` ends the successful-hit branch with
        // `causeFoodExhaustion(0.1F)`. Only landed hits exhaust; the miss/no-damage
        // case returned early above.
        self.add_exhaustion(0.1);
    }
}
