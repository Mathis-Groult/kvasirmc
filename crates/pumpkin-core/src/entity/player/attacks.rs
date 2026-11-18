//! Attacking and being knocked back.

use super::super::EntityBase;
use super::super::combat::{self, AttackType, player_attack_sound};
use super::Player;
use core::f32;
use pumpkin_data::attributes::Attributes;
use pumpkin_data::data_component_impl::WeaponImpl;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::java::client::play::CEntityVelocity;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::{Difficulty, Hand};
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[cfg(test)]
mod effect_tests;
#[cfg(test)]
mod golden;
mod hit;

impl Player {
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
        self.count_weapon_use(&item_stack);

        let base_damage = self
            .living_entity
            .get_attribute_value(&Attributes::ATTACK_DAMAGE);
        let base_attack_speed = 4.0;

        let (add_speed, extra_ench_damage, knockback_level) =
            hit::weapon_bonuses(&item_stack, victim_entity);

        let attack_speed = base_attack_speed + add_speed;

        let Some(attack_cooldown_progress) = self.attack_charge(&server, &item_stack, attack_speed)
        else {
            return;
        };
        self.last_attacked_ticks.store(0, Ordering::Relaxed);

        let mut damage =
            self.charged_damage(base_damage, attack_cooldown_progress, extra_ench_damage);

        let pos = victim_entity.pos.load();
        let attack_type = AttackType::new(self, attack_cooldown_progress as f32);

        if matches!(attack_type, AttackType::Critical) {
            damage *= 1.5;
        }

        let is_mace_smash = matches!(attack_type, AttackType::MaceSmash);
        if is_mace_smash {
            damage += self.mace_smash_bonus(&item_stack);
        }

        if !self.deal_hit(victim, &world, damage, is_mace_smash, &item_stack) {
            return;
        }

        if is_mace_smash && damage >= 100.0 {
            self.trigger_advancement(crate::entity::player::advancement::trigger::AdvancementTrigger::DealtOverkillDamage);
        }

        hit::ignite_victim(&item_stack, victim_entity);

        if is_mace_smash {
            self.finish_mace_smash(&world, &item_stack, &pos);
        }

        player_attack_sound(&pos, &world, attack_type);

        if matches!(attack_type, AttackType::Critical) {
            hit::show_critical_hit(&world, victim_entity);
        }

        self.remember_attack(victim_entity);

        if victim.get_living_entity().is_some() {
            // Vanilla `Player.attack` adds `LivingEntity.getKnockback()` - the Knockback
            // enchantment bonus, halved - plus 0.5 for a sprint attack, on top of the base
            // knockback the victim's damage handling applies. A plain hit adds nothing.
            // `handle_knockback` halves `strength`, so these are twice the vanilla amount.
            let mut knockback_strength = f64::from(knockback_level);
            match attack_type {
                AttackType::Knockback => knockback_strength += 1.0,
                AttackType::Sweeping => {
                    self.sweep(&world, &item_stack, victim_entity, &pos, damage);
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

        self.wear_weapon_and_tire(&item_stack);

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
