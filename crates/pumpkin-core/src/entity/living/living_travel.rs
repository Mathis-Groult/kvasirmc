//! Walking, swimming, flying, climbing and jumping.

use super::super::EntityBase;
use super::{
    LivingEntity, SPEED_MODIFIER_SPRINTING_AMOUNT, SPEED_MODIFIER_SPRINTING_ID,
    is_allowed_by_team_rules, random_teleport_coordinate,
};
use crate::entity::attributes::Modifier;
use crate::entity::attributes::ModifierOperation;
use kvasir_tickprof::parts::Part;
use pumpkin_data::attributes::Attributes;
use pumpkin_data::damage::DamageType;
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::entity::EntityType;
use pumpkin_data::fluid::Fluid;
use pumpkin_data::game_rules::{GameRule, GameRuleValue};
use pumpkin_util::math::boundingbox::BoundingBox;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector3::Vector3;
use rand::RngExt;
use std::sync::Arc;
use std::sync::atomic::Ordering::{Relaxed, SeqCst};

#[cfg(test)]
mod movement_golden;

impl LivingEntity {
    pub fn get_speed(&self) -> f32 {
        self.speed.load()
    }

    pub fn set_speed(&self, speed: f32) {
        self.speed.store(speed);
    }

    pub fn swing_hand(&self) {
        let world = self.entity.world.load();
        let entity_id = self.entity_id();

        let je_packet =
            pumpkin_protocol::java::client::play::CSwingArm::new(entity_id.into(), false);
        let be_packet = pumpkin_protocol::bedrock::server::animate::SAnimate {
            action: pumpkin_protocol::bedrock::server::animate::AnimateAction::SwingArm,
            target_actor_runtime_id: pumpkin_protocol::codec::var_ulong::VarULong(entity_id as u64),
            data: 0.0,
            swing_source: None,
            hand: 0,
        };

        world.broadcast_editioned(&je_packet, &be_packet);
    }

    pub fn swing_off_hand(&self) {
        let world = self.entity.world.load();
        let entity_id = self.entity_id();

        let je_packet = pumpkin_protocol::java::client::play::CEntityAnimation::new(
            entity_id.into(),
            pumpkin_protocol::java::client::play::Animation::SwingOffhand,
        );
        let be_packet = pumpkin_protocol::bedrock::server::animate::SAnimate {
            action: pumpkin_protocol::bedrock::server::animate::AnimateAction::SwingArm,
            target_actor_runtime_id: pumpkin_protocol::codec::var_ulong::VarULong(entity_id as u64),
            data: 0.0,
            swing_source: None,
            hand: 1,
        };

        world.broadcast_editioned(&je_packet, &be_packet);
    }

    pub(super) fn tick_movement(&self, caller: &dyn EntityBase) {
        if let Some(mob) = caller.get_mob() {
            mob.before_mob_ai_step();
        }
        if self.jumping_cooldown.load(Relaxed) != 0 {
            self.jumping_cooldown.fetch_sub(1, Relaxed);
        }

        let should_swim_in_fluids = caller.get_player().is_none_or(|player| !player.is_flying());

        self.entity.check_zero_velo();

        let mut movement_input = self.movement_input.load();

        movement_input.x *= 0.98;

        movement_input.z *= 0.98;

        self.movement_input.store(movement_input);

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

        if self.jumping.load(SeqCst) && should_swim_in_fluids {
            let in_lava = self.entity.touching_lava.load(SeqCst);

            let in_water = self.entity.touching_water.load(SeqCst);

            let fluid_height = if in_lava {
                self.entity.lava_height.load()
            } else {
                self.entity.water_height.load()
            };

            let swim_height = self.get_swim_height();

            let on_ground = self.entity.on_ground.load(SeqCst);

            if (in_water || in_lava) && (!on_ground || fluid_height > swim_height) {
                // Swim upward

                let mut velo = self.entity.velocity.load();

                velo.y += 0.04;

                self.entity.velocity.store(velo);
            } else if (on_ground || in_water && fluid_height <= swim_height)
                && self.jumping_cooldown.load(SeqCst) == 0
            {
                self.jump();

                self.jumping_cooldown.store(10, SeqCst);
            }
        } else {
            self.jumping_cooldown.store(0, SeqCst);
        }

        if self.has_effect(&StatusEffect::SLOW_FALLING)
            || self.has_effect(&StatusEffect::LEVITATION)
        {
            self.fall_distance.store(0.0);
        }

        let touching_water = self.entity.touching_water.load(SeqCst);

        // Vanilla Mob.isEffectiveAi: NoAI mobs don't travel, so they neither move nor fall.
        let effective_ai = caller
            .get_mob()
            .is_none_or(|mob| !mob.get_mob_entity().is_no_ai());

        // Strider is the only entity that has canWalkOnFluid = false

        Part::Travel.time(|| {
            if !effective_ai || caller.get_mob().is_some_and(|mob| mob.travel(caller)) {
            } else if (touching_water || self.entity.touching_lava.load(SeqCst))
                && should_swim_in_fluids
                && self.entity.entity_type != &EntityType::STRIDER
            {
                self.travel_in_fluid(caller, touching_water);
            } else {
                // TODO: Gliding

                self.travel_in_air(caller);
            }
        });

        let suffocating = Part::BlockCollisions.time(|| self.entity.tick_block_collisions(caller));

        if suffocating {
            caller.damage(caller, 1.0, DamageType::IN_WALL);
        }

        // Pushing apart is only skipped between ticks of a dormant mob: nobody sees it.
        let dormant_off_tick = caller.get_mob().is_some_and(|mob| {
            let mob = mob.get_mob_entity();
            mob.is_dormant() && !mob.is_decision_tick()
        });
        if !dormant_off_tick {
            Part::PushEntities.time(|| self.push_entities(caller));
        }

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

    pub(super) fn push_entities(&self, dyn_self: &dyn EntityBase) {
        let world = self.entity.world.load();
        let entity_bb = self.entity.bounding_box.load();

        // Almost always nothing overlaps, so the team rules (a scoreboard lock for each entity)
        // are looked at only for what does.
        let overlapping: Vec<Arc<dyn EntityBase>> = world
            .get_all_at_box(&entity_bb)
            .into_iter()
            .filter(|entity| {
                entity.get_entity().entity_id != self.entity.entity_id
                    && !entity.is_spectator()
                    && entity.is_pushable()
            })
            .collect();
        if overlapping.is_empty() {
            return;
        }
        let own_team = dyn_self.get_team();
        let pushable: Vec<Arc<dyn EntityBase>> = overlapping
            .into_iter()
            .filter(|entity| {
                is_allowed_by_team_rules(own_team.as_ref(), entity.get_team().as_ref())
            })
            .collect();

        if pushable.is_empty() {
            return;
        }

        // Entity cramming check
        let max_cramming = match world.get_game_rule(&GameRule::MaxEntityCramming) {
            GameRuleValue::Int(value) => value,
            GameRuleValue::Bool(_) => 0,
        };
        if max_cramming > 0
            && pushable.len() as i64 > max_cramming - 1
            && rand::random::<u32>().is_multiple_of(4)
        {
            let count = pushable
                .iter()
                .filter(|entity| !entity.is_passenger())
                .count();
            if count as i64 > max_cramming - 1 {
                dyn_self.damage(dyn_self, 6.0, DamageType::CRAMMING);
            }
        }

        for entity in pushable {
            entity.push(dyn_self);
        }
    }

    pub(crate) fn compute_modified_friction(friction: f32, modifier: f32) -> f64 {
        f64::from((1.0 - (1.0 - friction) * modifier).clamp(0.0, 1.0))
    }

    /// `getFrictionInfluencedSpeed`.
    pub(crate) fn friction_influenced_speed(speed: f64, block_friction: f64) -> f64 {
        if block_friction > 0.6 {
            speed * 0.216_000_02 / block_friction.powi(3)
        } else {
            speed
        }
    }

    pub(super) fn get_block_friction(&self) -> f64 {
        if self.entity.on_ground.load(Relaxed) {
            Self::compute_modified_friction(
                self.entity
                    .get_block_with_y_offset(0.500_001)
                    .1
                    .slipperiness,
                self.get_attribute_value(&Attributes::FRICTION_MODIFIER) as f32,
            )
        } else {
            1.0
        }
    }

    /// Decays player velocity like vanilla `travelInAir` friction.
    pub(super) fn apply_travel_friction(&self) {
        let mut velo = self.entity.velocity.load();
        if velo.x == 0.0 && velo.z == 0.0 {
            return;
        }

        let block_friction = self.get_block_friction();
        let friction = block_friction
            * Self::compute_modified_friction(
                0.91,
                self.get_attribute_value(&Attributes::AIR_DRAG_MODIFIER) as f32,
            );

        velo.x *= friction;

        velo.z *= friction;

        self.entity.velocity.store(velo);
    }

    pub(super) fn travel_in_air(&self, caller: &dyn EntityBase) {
        // applyMovementInput

        let effective_speed = if caller.get_mob().is_some() {
            f64::from(self.get_speed())
        } else {
            self.get_attribute_value(&Attributes::MOVEMENT_SPEED)
        };

        let air_drag_modifier = self.get_attribute_value(&Attributes::AIR_DRAG_MODIFIER) as f32;
        let air_drag = Self::compute_modified_friction(0.91, air_drag_modifier);
        let block_friction = self.get_block_friction();
        let speed = if self.entity.on_ground.load(Relaxed) {
            Self::friction_influenced_speed(effective_speed, block_friction)
        } else {
            caller
                .get_player()
                .map_or(0.02, super::super::player::Player::get_off_ground_speed)
        };
        let friction = block_friction * air_drag;

        self.entity
            .update_velocity_from_input(self.movement_input.load(), speed);

        self.apply_climbing_speed();

        self.make_move(caller);

        let mut velo = self.entity.velocity.load();

        let can_powder_snow_climb = if self.entity.was_in_powder_snow.load(Relaxed) {
            crate::block::blocks::powder_snow::can_entity_walk_on_powder_snow(caller)
        } else {
            false
        };

        if (self.entity.horizontal_collision.load(SeqCst) || self.jumping.load(SeqCst))
            && (self.climbing.load(Relaxed) || can_powder_snow_climb)
        {
            velo.y = 0.2;
        }

        let levitation = self.get_effect(&StatusEffect::LEVITATION);

        if let Some(lev) = levitation {
            velo.y += 0.05f64.mul_add(f64::from(lev.amplifier + 1), -velo.y) * 0.2;
        } else {
            velo.y -= self.get_effective_gravity(caller);

            // TODO: If world is not loaded: replace effective gravity with:

            // if below world's bottom y then -0.1, else 0.0
        }

        // If entity has no drag: store velo and return

        velo.x *= friction;

        velo.z *= friction;

        velo.y *= caller.get_y_velocity_drag().unwrap_or_else(|| {
            if caller.is_flutterer() {
                air_drag
            } else {
                Self::compute_modified_friction(0.98, air_drag_modifier)
            }
        });

        self.entity.velocity.store(velo);
    }

    pub(crate) fn travel_flying(&self, caller: &dyn EntityBase) {
        let (speed, drag) = if self.entity.is_in_water() {
            (f64::from(0.02f32), f64::from(0.8f32))
        } else if self.entity.touching_lava.load(Relaxed) {
            (f64::from(0.02f32), 0.5)
        } else {
            (f64::from(self.get_speed()), f64::from(0.91f32))
        };
        self.entity
            .update_velocity_from_input(self.movement_input.load(), speed);
        self.make_move(caller);
        self.entity
            .velocity
            .store(self.entity.velocity.load() * drag);
    }

    pub(crate) fn travel_in_water(&self, caller: &dyn EntityBase, gravity: f64, falling: bool) {
        let old_y = self.entity.pos.load().y;
        let movement_input = self.movement_input.load();
        let effective_speed = if caller.get_mob().is_some() {
            f64::from(self.get_speed())
        } else {
            self.get_attribute_value(&Attributes::MOVEMENT_SPEED)
        };
        let mut friction = if self.entity.sprinting.load(Relaxed) {
            0.9
        } else {
            f64::from(self.water_movement_speed_multiplier)
        };

        let mut speed = 0.02;

        // Apply water movement efficiency attribute
        let mut water_movement_efficiency =
            self.get_attribute_value(&Attributes::WATER_MOVEMENT_EFFICIENCY);

        if water_movement_efficiency > 0.0 {
            if !self.entity.on_ground.load(SeqCst) {
                water_movement_efficiency *= 0.5;
            }

            friction += (0.546_000_06 - friction) * water_movement_efficiency;
            speed += (effective_speed - speed) * water_movement_efficiency;
        }

        if self.has_effect(&StatusEffect::DOLPHINS_GRACE) {
            friction = 0.96;
        }

        self.entity
            .update_velocity_from_input(movement_input, speed);

        self.make_move(caller);

        let mut velo = self.entity.velocity.load();
        if self.entity.horizontal_collision.load(SeqCst) && self.climbing.load(Relaxed) {
            velo.y = 0.2;
        }

        velo = velo.multiply(friction, 0.8, friction);

        self.apply_fluid_moving_speed(&mut velo.y, gravity, falling);
        self.entity.velocity.store(velo);
        self.jump_out_of_fluid(old_y);
    }

    pub(super) fn travel_in_fluid(&self, caller: &dyn EntityBase, water: bool) {
        let movement_input = self.movement_input.load();

        let falling = self.entity.velocity.load().y <= 0.0;
        let gravity = self.get_effective_gravity(caller);

        if water {
            if let Some(mob) = caller.get_mob() {
                mob.travel_in_water(caller, gravity, falling);
            } else {
                self.travel_in_water(caller, gravity, falling);
            }
        } else {
            let old_y = self.entity.pos.load().y;
            self.entity.update_velocity_from_input(movement_input, 0.02);

            self.make_move(caller);

            let mut velo = self.entity.velocity.load();

            if self.entity.lava_height.load() <= self.get_swim_height() {
                velo.x *= 0.5;
                velo.z *= 0.5;
                velo.y *= 0.8;

                self.apply_fluid_moving_speed(&mut velo.y, gravity, falling);
            } else {
                velo = velo * 0.5;
            }

            if gravity != 0.0 {
                velo.y -= gravity / 4.0; // Negative gravity = buoyancy
            }

            self.entity.velocity.store(velo);
            self.jump_out_of_fluid(old_y);
        }
    }

    pub(super) fn jump_out_of_fluid(&self, old_y: f64) {
        let mut velo = self.entity.velocity.load();
        if !self.entity.horizontal_collision.load(SeqCst) {
            return;
        }
        let lift = velo.y + f64::from(0.6f32) - self.entity.pos.load().y + old_y;
        let bounding_box = self
            .entity
            .bounding_box
            .load()
            .shift(Vector3::new(velo.x, lift, velo.z));
        let world = self.entity.world.load();
        if world.is_space_empty(bounding_box) && !world.contains_any_liquid(bounding_box) {
            velo.y = f64::from(0.3f32);
            self.entity.velocity.store(velo);
        }
    }

    pub(super) fn apply_fluid_moving_speed(&self, dy: &mut f64, gravity: f64, falling: bool) {
        if gravity != 0.0 && !self.entity.sprinting.load(Relaxed) {
            if falling && (*dy - 0.005).abs() >= 0.003 && (*dy - gravity / 16.0).abs() < 0.003 {
                *dy = -0.003;
            } else {
                *dy -= gravity / 16.0;
            }
        }
    }

    pub(crate) fn make_move(&self, caller: &dyn EntityBase) {
        self.entity.move_entity(caller, self.entity.velocity.load());

        self.check_climbing();
    }

    pub(super) fn check_climbing(&self) {
        // If spectator: return false

        // TODO
        // let mut pos = self.entity.block_pos.load();

        // let world = self.entity.world.read().await;

        // let (block, state) = world.get_block_and_state(&pos);

        // let name = block.properties(state.id).map(|props| props.name());

        // if let Some(name) = name {
        //     if name == "LadderLikeProperties"
        //         || name == "ScaffoldingLikeProperties"
        //         || name == "CaveVinesLikeProperties"
        //         || name == "CaveVinesPlantLikeProperties"
        //     {
        //         self.climbing.store(true, Relaxed);

        //         self.climbing_pos.store(Some(pos));

        //         return;
        //     }

        //     if name == "OakTrapdoorLikeProperties" {
        //         let trapdoor = OakTrapdoorLikeProperties::from_state_id(state.id);

        //         pos.0.y -= 1;

        //         let (down_block, down_state) = world.get_block_and_state(&pos);

        //         let is_ladder = down_block
        //             .properties(down_state.id)
        //             .is_some_and(|down_props| down_props.name() == "LadderLikeProperties");

        //         if is_ladder {
        //             let ladder = LadderLikeProperties::from_state_id(down_state.id);

        //             if trapdoor.r#facing == ladder.r#facing {
        //                 self.climbing.store(true, Relaxed);

        //                 self.climbing_pos.store(Some(pos));

        //                 return;
        //             }
        //         }
        //     }
        // }

        self.climbing.store(false, Relaxed);

        if self.entity.on_ground.load(SeqCst) {
            self.climbing_pos.store(None);
        }
    }

    pub(super) fn apply_climbing_speed(&self) {
        if self.climbing.load(Relaxed) {
            self.fall_distance.store(0.0);

            let mut velo = self.entity.velocity.load();

            let pos = 0.15;

            let neg = -0.15;

            if velo.x < neg {
                velo.x = neg;
            } else if velo.x > pos {
                velo.x = pos;
            }

            if velo.z < neg {
                velo.z = neg;
            } else if velo.z > pos {
                velo.z = pos;
            }

            velo.y = velo.y.max(neg);

            // TODO
            // if velo.y < 0.0
            //     && self.entity.entity_type == &EntityType::PLAYER
            //     && self.entity.sneaking.load(Relaxed)
            // {
            //     let block = self
            //         .entity
            //         .world
            //         .read()
            //         .await
            //         .get_block(&self.entity.block_pos.load())
            //         .await;

            //     if let Some(props) = block.properties(block.default_state.id) {
            //         if props.name() == "ScaffoldingLikeProperties" {
            //             velo.y = 0.0;
            //         }
            //     }
            // }

            self.entity.velocity.store(velo);
        }
    }

    pub fn get_swim_height(&self) -> f64 {
        let eye_height = self.entity.get_eye_height();

        if self.entity.entity_type == &EntityType::BREEZE {
            eye_height
        } else if eye_height < 0.4 {
            0.0
        } else {
            0.4
        }
    }

    pub(super) fn jump(&self) {
        let jump = self.get_jump_velocity(1.0);

        if jump <= 1.0e-5 {
            return;
        }

        let mut velo = self.entity.velocity.load();

        velo.y = jump.max(velo.y);

        if self.entity.sprinting.load(Relaxed) {
            let yaw = f64::from(self.entity.yaw.load()).to_radians();

            velo.x -= yaw.sin() * 0.2;
            velo.z += yaw.cos() * 0.2;
        }

        self.entity.velocity.store(velo);

        self.entity.velocity_dirty.store(true, SeqCst);
    }

    pub(crate) fn get_jump_velocity(&self, mut strength: f64) -> f64 {
        strength *= self.get_attribute_value(&Attributes::JUMP_STRENGTH);
        strength *= f64::from(self.entity.get_jump_velocity_multiplier());
        if let Some(effect) = self.get_effect(&StatusEffect::JUMP_BOOST) {
            strength += 0.1 * f64::from(effect.amplifier + 1);
        }
        strength
    }

    pub fn get_movement(&self) -> Vector3<f64> {
        self.entity.movement.load()
    }

    pub fn set_sprinting(&self, is_sprinting: bool) {
        self.entity.set_sprinting(is_sprinting);
        self.update_attribute(&Attributes::MOVEMENT_SPEED, |speed| {
            speed.remove_modifier(SPEED_MODIFIER_SPRINTING_ID);
            if is_sprinting {
                speed.add_or_replace_modifier(Modifier {
                    id: SPEED_MODIFIER_SPRINTING_ID.to_string(),
                    amount: SPEED_MODIFIER_SPRINTING_AMOUNT,
                    operation: ModifierOperation::MultiplyTotal,
                });
            }
        });
        crate::entity::attributes::send_attribute_updates_for_living(
            self,
            vec![Attributes::MOVEMENT_SPEED],
        );
    }

    #[must_use]
    pub fn get_block_speed_factor(&self) -> f32 {
        let efficiency = self.get_attribute_value(&Attributes::MOVEMENT_EFFICIENCY) as f32;
        let super_factor = self.entity.get_block_speed_factor();
        super_factor + efficiency * (1.0 - super_factor)
    }

    pub(super) fn find_random_teleport_target(&self, diameter: f32) -> Option<Vector3<f64>> {
        let center = self.entity.pos.load();
        let world = self.entity.world.load();
        let bottom_y = world.get_bottom_y();
        let top_y = world.get_top_y();
        let dimensions = self.entity.entity_dimension.load();
        let mut rng = rand::rng();

        'attempts: for _ in 0..Self::RANDOM_TELEPORT_ATTEMPTS {
            let target_x = random_teleport_coordinate(center.x, diameter, rng.random());
            let target_z = random_teleport_coordinate(center.z, diameter, rng.random());
            let sampled_y = random_teleport_coordinate(center.y, diameter, rng.random())
                .clamp(f64::from(bottom_y + 1), f64::from(top_y));
            let mut block_y = sampled_y.floor() as i32;
            let block_x = target_x.floor() as i32;
            let block_z = target_z.floor() as i32;

            loop {
                if block_y <= bottom_y {
                    continue 'attempts;
                }

                let below = BlockPos::new(block_x, block_y - 1, block_z);
                let Some(below_state) = world.get_block_state_if_loaded(&below) else {
                    continue 'attempts;
                };
                if below_state.is_solid() {
                    break;
                }
                block_y -= 1;
            }

            let target = Vector3::new(target_x, f64::from(block_y), target_z);
            let bounding_box = BoundingBox::new_from_pos(target.x, target.y, target.z, &dimensions);

            for block_pos in
                BlockPos::iterate(bounding_box.min_block_pos(), bounding_box.max_block_pos())
            {
                if world.get_block_state_if_loaded(&block_pos).is_none()
                    || world.get_fluid(&block_pos).id != Fluid::EMPTY.id
                {
                    continue 'attempts;
                }
            }

            if world.is_space_empty(bounding_box) {
                return Some(target);
            }
        }

        None
    }
}
