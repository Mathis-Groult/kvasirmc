//! The stages of `LivingEntity::tick_movement`, after vanilla `LivingEntity.aiStep`: motion
//! damping, the AI step, jumping and swimming up, travel, block collisions, pushing and the
//! end of the mob's AI step.

use super::super::LivingEntity;
use crate::entity::EntityBase;
use kvasir_tickprof::parts::Part;
use pumpkin_data::damage::DamageType;
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::entity::EntityType;
use pumpkin_util::math::vector3::Vector3;
use std::sync::atomic::Ordering::{Relaxed, SeqCst};

/// Upward push of a swimming entity with the jump key down, per tick.
const SWIM_UP_SPEED: f64 = 0.04;
/// Ticks between two jumps while the jump key stays down.
const JUMP_COOLDOWN: u8 = 10;

impl LivingEntity {
    pub(super) fn count_down_jump_cooldown(&self) {
        if self.jumping_cooldown.load(Relaxed) != 0 {
            self.jumping_cooldown.fetch_sub(1, Relaxed);
        }
    }

    /// Sideways input fades by 2 % each tick.
    pub(super) fn damp_movement_input(&self) {
        let mut input = self.movement_input.load();
        input.x *= 0.98;
        input.z *= 0.98;
        self.movement_input.store(input);
    }

    /// An immobile entity stops jumping and sideways input; a mob whose AI runs plans this
    /// tick's input before anything moves.
    pub(super) fn ai_step_inputs(&self, caller: &dyn EntityBase) {
        if caller.is_immobile() {
            self.jumping.store(false, Relaxed);
            let input = self.movement_input.load();
            self.movement_input.store(Vector3::new(0.0, input.y, 0.0));
        } else if let Some(mob) = caller.get_mob()
            && !mob.get_mob_entity().is_no_ai()
        {
            // Prepare old motion before controllers produce this tick's inputs.
            Part::AiStep.time(|| mob.get_mob_entity().server_ai_step(mob, caller));
        }
    }

    /// With the jump key down: swim up in a fluid, or jump from the ground once the cooldown
    /// is over. Without it, or when the entity does not swim (a flying player), the cooldown
    /// resets.
    pub(super) fn apply_jump_input(&self, should_swim_in_fluids: bool) {
        if self.jumping.load(SeqCst) && should_swim_in_fluids {
            self.jump_or_swim_up();
        } else {
            self.jumping_cooldown.store(0, SeqCst);
        }
    }

    fn jump_or_swim_up(&self) {
        let in_lava = self.entity.touching_lava.load(SeqCst);
        let in_water = self.entity.touching_water.load(SeqCst);
        let fluid_height = if in_lava {
            self.entity.lava_height.load()
        } else {
            self.entity.water_height.load()
        };
        let on_ground = self.entity.on_ground.load(SeqCst);

        if (in_water || in_lava) && (!on_ground || fluid_height > self.get_swim_height()) {
            let mut velocity = self.entity.velocity.load();
            velocity.y += SWIM_UP_SPEED;
            self.entity.velocity.store(velocity);
        } else if on_ground && self.jumping_cooldown.load(SeqCst) == 0 {
            self.jump();
            self.jumping_cooldown.store(JUMP_COOLDOWN, SeqCst);
        }
    }

    pub(super) fn reset_fall_distance_when_floating(&self) {
        if self.has_effect(&StatusEffect::SLOW_FALLING)
            || self.has_effect(&StatusEffect::LEVITATION)
        {
            self.fall_distance.store(0.0);
        }
    }

    /// Moves the entity the way its kind travels: by the mob's own rule, in a fluid or in air.
    /// A mob without AI neither moves nor falls (vanilla `Mob.isEffectiveAi`).
    pub(super) fn travel_this_tick(&self, caller: &dyn EntityBase, should_swim_in_fluids: bool) {
        let effective_ai = caller
            .get_mob()
            .is_none_or(|mob| !mob.get_mob_entity().is_no_ai());

        Part::Travel.time(|| {
            if !effective_ai || caller.get_mob().is_some_and(|mob| mob.travel(caller)) {
                return;
            }
            let touching_water = self.entity.touching_water.load(SeqCst);
            // Strider is the only entity that has canWalkOnFluid = false
            if (touching_water || self.entity.touching_lava.load(SeqCst))
                && should_swim_in_fluids
                && self.entity.entity_type != &EntityType::STRIDER
            {
                self.travel_in_fluid(caller, touching_water);
            } else {
                // TODO: Gliding
                self.travel_in_air(caller);
            }
        });
    }

    pub(super) fn collide_with_blocks(&self, caller: &dyn EntityBase) {
        let suffocating = Part::BlockCollisions.time(|| self.entity.tick_block_collisions(caller));
        if suffocating {
            caller.damage(caller, 1.0, DamageType::IN_WALL);
        }
    }

    /// Pushing apart is only skipped between ticks of a dormant mob: nobody sees it.
    pub(super) fn push_unless_dormant(&self, caller: &dyn EntityBase) {
        let dormant_off_tick = caller.get_mob().is_some_and(|mob| {
            let mob = mob.get_mob_entity();
            mob.is_dormant() && !mob.is_decision_tick()
        });
        if !dormant_off_tick {
            Part::PushEntities.time(|| self.push_entities(caller));
        }
    }

    pub(super) fn end_mob_ai_step(&self, caller: &dyn EntityBase) {
        Part::MobAiStepEnd.time(|| {
            if let Some(ageable) = caller.get_mob().and_then(|mob| mob.as_ageable())
                && self.entity.is_alive()
                && !self.is_dead_or_dying()
            {
                ageable.ageable_ai_step();
            }

            if let Some(mob) = caller.get_mob() {
                mob.mob_ai_step();
                mob.get_mob_entity()
                    .body_rotation_control
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .client_tick(mob);
            }
        });
    }
}
