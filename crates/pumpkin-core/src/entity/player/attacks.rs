//! Attacking and being knocked back.

use super::super::EntityBase;
use super::super::combat::{self, AttackType, player_attack_sound};
use super::{Player, statistics};
use crate::net::ClientPlatform;
use core::f32;
use pumpkin_data::Enchantment;
use pumpkin_data::attributes::Attributes;
use pumpkin_data::damage::DamageType;
use pumpkin_data::data_component_impl::WeaponImpl;
use pumpkin_data::data_component_impl::{AttributeModifiersImpl, EnchantmentsImpl, Operation};
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::sound::{Sound, SoundCategory};
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::codec::var_ulong::VarULong;
use pumpkin_protocol::java::client::play::{Animation, CEntityAnimation, CEntityVelocity};
use pumpkin_util::math::{boundingbox::BoundingBox, vector3::Vector3};
use pumpkin_util::{Difficulty, Hand};
use std::sync::Arc;
use std::sync::atomic::Ordering;

impl Player {
    #[expect(clippy::too_many_lines)]
    pub fn attack(&self, victim: &Arc<dyn EntityBase>) {
        let world = self.world();
        let Some(server) = world.server.upgrade() else {
            return;
        };
        let victim_entity = victim.get_entity();
        let attacker_entity = &self.living_entity.entity;
        let config = &server.advanced_config.pvp;

        let inventory = self.inventory();
        let item_stack = inventory.held_item();
        if !item_stack.is_empty() {
            self.increment_stat(
                statistics::StatisticCategory::Used,
                item_stack.item.id as i32,
                1,
            );
        }

        let base_damage = self
            .living_entity
            .get_attribute_value(&Attributes::ATTACK_DAMAGE);
        let base_attack_speed = 4.0;

        let mut damage_multiplier = 1.0;
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

        let attack_speed = base_attack_speed + add_speed;

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
                return;
            }
        }
        let attack_cooldown_progress = if is_bedrock {
            1.0
        } else {
            self.get_attack_cooldown_progress(f64::from(server.basic_config.tps), 0.5, attack_speed)
        };
        self.last_attacked_ticks.store(0, Ordering::Relaxed);

        // Only reduce attack damage if in cooldown
        // TODO: Enchantments are reduced in the same way, just without the square.
        if attack_cooldown_progress < 1.0 {
            damage_multiplier = attack_cooldown_progress.powi(2).mul_add(0.8, 0.2);
        }

        // Modify the added damage based on the multiplier.
        let mut damage = base_damage * damage_multiplier;
        damage += extra_ench_damage * attack_cooldown_progress;

        if let Some(strength) = self
            .living_entity
            .get_effect(&pumpkin_data::effect::StatusEffect::STRENGTH)
        {
            damage += 3.0 * (f64::from(strength.amplifier) + 1.0);
        }
        if let Some(weakness) = self
            .living_entity
            .get_effect(&pumpkin_data::effect::StatusEffect::WEAKNESS)
        {
            damage -= 4.0 * (f64::from(weakness.amplifier) + 1.0);
        }
        damage = damage.max(0.0);

        let pos = victim_entity.pos.load();
        let attack_type = AttackType::new(self, attack_cooldown_progress as f32);

        if matches!(attack_type, AttackType::Critical) {
            damage *= 1.5;
        }

        let is_mace_smash = matches!(attack_type, AttackType::MaceSmash);
        if is_mace_smash {
            let fall_distance = self.living_entity.fall_distance.load();
            let mut smash_bonus_per_block = 0.0f64;
            if let Some(enchantments) = item_stack.get_data_component::<EnchantmentsImpl>() {
                for (enchantment, level) in enchantments.enchantment.iter() {
                    enchantment.modify_fall_based_damage(*level, &mut smash_bonus_per_block);
                }
            }
            damage += (1.5 + smash_bonus_per_block) * f64::from(fall_distance);
        }

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
            return;
        }

        if is_mace_smash && damage >= 100.0 {
            self.trigger_advancement(crate::entity::player::advancement::trigger::AdvancementTrigger::DealtOverkillDamage);
        }

        if let Some(enchantments) = item_stack.get_data_component::<EnchantmentsImpl>() {
            for (enchantment, level) in enchantments.enchantment.iter() {
                for post_effect in enchantment.get_post_attack_effects() {
                    if post_effect.affected
                        == Some(pumpkin_data::enchantment::EnchantmentTarget::Victim)
                        && let pumpkin_data::enchantment::EnchantmentEntityEffect::Ignite {
                            duration,
                        } = &post_effect.effect
                    {
                        let duration_seconds = duration.calculate(*level);
                        victim_entity.set_on_fire_for_ticks((duration_seconds * 20.0) as u32);
                    }
                }
            }
        }

        if is_mace_smash {
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
                &pos,
            );
        }

        player_attack_sound(&pos, &world, attack_type);

        if matches!(attack_type, AttackType::Critical) {
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

        if victim.get_living_entity().is_some() {
            // Vanilla `Player.attack` adds `LivingEntity.getKnockback()` - the Knockback
            // enchantment bonus, halved - plus 0.5 for a sprint attack, on top of the base
            // knockback the victim's damage handling applies. A plain hit adds nothing.
            // `handle_knockback` halves `strength`, so these are twice the vanilla amount.
            let mut knockback_strength = f64::from(knockback_level);
            match attack_type {
                AttackType::Knockback => knockback_strength += 1.0,
                AttackType::Sweeping => {
                    combat::spawn_sweep_particle(attacker_entity, &world, &pos);

                    let mut sweep_damage = 1.0;
                    if let Some(enchantments) = item_stack.get_data_component::<EnchantmentsImpl>()
                    {
                        for (enchantment, level) in enchantments.enchantment.iter() {
                            if **enchantment == Enchantment::SWEEPING_EDGE {
                                sweep_damage +=
                                    damage as f32 * (*level as f32 / (*level as f32 + 1.0));
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
                _ => {}
            }
            // Vanilla only pushes the victim when the extra knockback is non-zero;
            // `Entity::knockback` halves the current velocity, so calling it with 0.0
            // would still slow the victim down.
            if config.knockback && knockback_strength > 0.0 {
                combat::handle_knockback(attacker_entity, victim.as_ref(), knockback_strength);
            }
        }

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
        self.damage_held_item(Self::combat_weapon_durability_cost(&item_stack));

        // Vanilla `Player#attack` ends the successful-hit branch with
        // `causeFoodExhaustion(0.1F)`. Only landed hits exhaust; the miss/no-damage
        // case returned early above.
        self.add_exhaustion(0.1);

        if config.swing {}
    }

    /// Returns the durability cost for using the held item as a weapon in combat.
    /// Derived from the `Weapon` data component: items without it (e.g. shears, tools
    /// not designed for combat) take no durability damage on attack.
    /// Items with the component use its `item_damage_per_attack` value (default 1;
    /// axes, pickaxes, shovels, and hoes carry a value of 2).
    pub(super) fn combat_weapon_durability_cost(stack: &ItemStack) -> i32 {
        stack
            .get_data_component::<WeaponImpl>()
            .map_or(0, |w| w.item_damage_per_attack as i32)
    }

    pub fn set_velocity(&self, mut velocity: Vector3<f64>) {
        if let Some(player_arc) = self.world().get_player_by_uuid(self.gameprofile.id)
            && let Some(server) = self.world().server.upgrade()
        {
            let mut event =
                crate::plugin::api::events::player::player_velocity::PlayerVelocityEvent {
                    player: player_arc,
                    velocity,
                    cancelled: false,
                };
            server.plugin_manager.fire_blocking(&server, &mut event);
            if event.cancelled {
                return;
            }
            velocity = event.velocity;
        }
        self.living_entity.entity.set_velocity(velocity);
        self.try_send_client_packet(&CEntityVelocity::new(self.entity_id().into(), velocity));
    }

    pub fn apply_knockback(&self, strength: f64, x: f64, z: f64) {
        let current_vel = self.living_entity.entity.velocity.load();
        let norm = x.hypot(z);
        if norm > 0.0 {
            let vx = current_vel.x / 2.0 - (x / norm) * strength;
            let vz = current_vel.z / 2.0 - (z / norm) * strength;
            let vy = (current_vel.y / 2.0 + strength).min(0.4);
            self.set_velocity(Vector3::new(vx, vy, vz));
        }
    }

    pub fn get_attack_cooldown_progress(&self, tps: f64, base_time: f64, attack_speed: f64) -> f64 {
        let x = f64::from(self.last_attacked_ticks.load(Ordering::Acquire)) + base_time;

        let progress_per_tick = tps / attack_speed;
        let progress = x / progress_per_tick;
        progress.clamp(0.0, 1.0)
    }

    /// Vanilla `NeutralMob.isValidPlayerTarget`: not creative or spectator, and the world is
    /// not Peaceful. `abilities.invulnerable` is left out like vanilla, `can_attack` checks it.
    #[must_use]
    pub fn is_valid_mob_target(&self) -> bool {
        !self.is_creative()
            && !self.is_spectator()
            && self.world().level_info.load().difficulty != Difficulty::Peaceful
    }

    /// Swing the hand of the player
    pub fn swing_hand(&self, hand: Hand, all: bool) {
        let world = self.world();
        let entity_id = self.entity_id();

        let je_packet = pumpkin_protocol::java::client::play::CSwingArm::new(
            VarInt(entity_id),
            hand == Hand::Left,
        );

        let be_packet = pumpkin_protocol::bedrock::server::animate::SAnimate {
            action: pumpkin_protocol::bedrock::server::animate::AnimateAction::SwingArm,
            target_actor_runtime_id: pumpkin_protocol::codec::var_ulong::VarULong(entity_id as u64),
            data: 0.0,
            swing_source: None,
            hand: u8::from(hand == Hand::Left),
        };

        if all {
            world.broadcast_editioned(&je_packet, &be_packet);
        } else {
            world.broadcast_packet_except_editioned(&[self.gameprofile.id], &je_packet, &be_packet);
        }
    }
}
