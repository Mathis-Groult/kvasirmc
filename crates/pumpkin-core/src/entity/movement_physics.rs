//! Velocity, collisions and moving through the world.

use super::{Entity, EntityBase, PUSH_OUT_MIN_SPEED, PUSH_OUT_RANDOM_SPEED};
use crate::{server::Server, world::World};
use pumpkin_data::BlockState;
use pumpkin_data::block_properties::blocks_movement;
use pumpkin_data::{Block, BlockDirection};
use pumpkin_data::{
    entity::{EntityType, MovementEmission},
    game_event::GameEvent,
};
use pumpkin_protocol::bedrock::client::CSetActorMotion;
use pumpkin_protocol::{codec::var_ulong::VarULong, java::client::play::CEntityVelocity};
use pumpkin_util::math::vector3::Axis;
use pumpkin_util::math::{boundingbox::BoundingBox, position::BlockPos, vector3::Vector3};
use std::sync::atomic::Ordering::{self, Relaxed};

#[cfg(test)]
mod golden;

impl Entity {
    pub fn add_velocity(&self, velocity: Vector3<f64>) {
        self.set_velocity(self.velocity.load() + velocity);
    }

    pub fn set_velocity(&self, velocity: Vector3<f64>) {
        self.velocity.store(velocity);
        self.send_velocity();
    }

    /// Vanilla `hurtMarked` path: immediate, to watchers and self.
    pub fn send_velocity(&self) {
        let velocity = self.velocity.load();
        self.last_sent_velocity.store(velocity);
        self.world
            .load()
            .send_to_tracking_players_and_self_editioned(
                self,
                &CEntityVelocity::new(self.entity_id.into(), velocity),
                &CSetActorMotion {
                    target_runtime_id: VarULong(self.entity_id as u64),
                    motion: Vector3::new(velocity.x as f32, velocity.y as f32, velocity.z as f32),
                    tick: VarULong(0),
                },
            );
    }

    #[expect(clippy::float_cmp)]
    pub(super) fn adjust_movement_for_collisions(
        &self,
        movement: Vector3<f64>,
        caller: &dyn EntityBase,
    ) -> Vector3<f64> {
        self.vertical_collision.store(false, Ordering::SeqCst);
        if movement.length_squared() == 0.0 {
            return movement;
        }

        self.on_ground.store(false, Ordering::SeqCst);
        self.supporting_block_pos.store(None);
        self.horizontal_collision.store(false, Ordering::SeqCst);

        let bounding_box = self.bounding_box.load();

        let (collisions, block_positions) = self
            .world
            .load()
            .get_block_collisions(bounding_box.stretch(movement), caller);

        if collisions.is_empty() {
            return movement;
        }

        let mut adjusted_movement = movement;

        // Y-Axis adjustment
        if movement.get_axis(Axis::Y) != 0.0 {
            let mut max_time = 1.0;
            let mut positions = block_positions.into_iter();
            if let Some((mut collisions_len, mut position)) = positions.next() {
                let mut supporting_block_pos = None;

                for (i, inert_box) in collisions.iter().enumerate() {
                    if i == collisions_len {
                        let Some((next_len, next_pos)) = positions.next() else {
                            break;
                        };
                        collisions_len = next_len;
                        position = next_pos;
                    }

                    if let Some(collision_time) = bounding_box.calculate_collision_time(
                        inert_box,
                        adjusted_movement,
                        Axis::Y,
                        max_time,
                    ) {
                        max_time = collision_time;

                        // If the entity is moving downwards and collides, set the supporting block position
                        if movement.get_axis(Axis::Y) < 0.0 {
                            supporting_block_pos = Some(position);
                        }
                    }
                }

                if max_time != 1.0 {
                    self.vertical_collision.store(true, Ordering::SeqCst);
                    let changed_component = adjusted_movement.get_axis(Axis::Y) * max_time;
                    adjusted_movement.set_axis(Axis::Y, changed_component);
                }

                self.on_ground
                    .store(supporting_block_pos.is_some(), Ordering::SeqCst);
                self.supporting_block_pos.store(supporting_block_pos);
            }
        }

        let mut horizontal_collision = false;

        for axis in Axis::horizontal() {
            if movement.get_axis(axis) == 0.0 {
                continue;
            }

            let mut max_time = 1.0;

            for inert_box in &collisions {
                if let Some(collision_time) = bounding_box.calculate_collision_time(
                    inert_box,
                    adjusted_movement,
                    axis,
                    max_time,
                ) {
                    max_time = collision_time;
                }
            }

            if max_time != 1.0 {
                let changed_component = adjusted_movement.get_axis(axis) * max_time;
                adjusted_movement.set_axis(axis, changed_component);
                horizontal_collision = true;
            }
        }

        self.horizontal_collision
            .store(horizontal_collision, Ordering::SeqCst);

        adjusted_movement
    }

    /// Applies knockback to the entity, following vanilla Minecraft's mechanics.
    /// `LivingEntity.takeKnockback()`
    /// This function calculates the entity's new velocity based on the specified knockback strength and direction.
    ///
    /// Knockback resistance is not applied here, because it is a `LivingEntity`
    /// attribute and this is an `Entity` method. Callers modelling vanilla's
    /// `LivingEntity.knockback` scale `strength` with
    /// `combat::knockback_after_resistance` first; callers modelling vanilla's raw
    /// `Entity.push` (such as the ender dragon) pass `strength` unscaled.
    pub fn apply_knockback(&self, strength: f64, mut x: f64, mut z: f64) {
        if strength <= 0.0 {
            return;
        }

        self.velocity_dirty.store(true, Ordering::SeqCst);

        // This has some vanilla magic

        while x.mul_add(x, z * z) < 1.0E-5 {
            x = (rand::random::<f64>() - rand::random::<f64>()) * 0.01;

            z = (rand::random::<f64>() - rand::random::<f64>()) * 0.01;
        }

        let var8 = Vector3::new(x, 0.0, z).normalize() * strength;

        let velocity = self.velocity.load();

        self.velocity.store(Vector3::new(
            velocity.x / 2.0 - var8.x,
            if self.on_ground.load(Relaxed) {
                (velocity.y / 2.0 + strength).min(0.4)
            } else {
                velocity.y
            },
            velocity.z / 2.0 - var8.z,
        ));
    }

    pub fn check_zero_velo(&self) {
        let mut motion = self.velocity.load();

        if self.entity_type == &EntityType::PLAYER {
            if motion.horizontal_length_squared() < 9.0E-6 {
                motion.x = 0.0;

                motion.z = 0.0;
            }
        } else {
            if motion.x.abs() < 0.003 {
                motion.x = 0.0;
            }

            if motion.z.abs() < 0.003 {
                motion.z = 0.0;
            }
        }

        if motion.y.abs() < 0.003 {
            motion.y = 0.0;
        }

        self.velocity.store(motion);
    }

    #[expect(dead_code)]
    pub(super) const fn tick_block_underneath() {
        // let world = self.world.read();

        // let (pos, block, state) = self.get_block_with_y_offset(0.2);

        // world
        //     .block_registry
        //     .on_stepped_on(&world, caller, pos, block, state)
        //     ;

        // TODO: Add this to on_stepped_on

        /*


        if self.on_ground.load(Ordering::SeqCst) {


            let (_pos, block, state) = self.get_block_with_y_offset(0.2);


            if let Some(live) = living {


                if block == Block::CAMPFIRE


                    || block == Block::SOUL_CAMPFIRE


                        && CampfireLikeProperties::from_state_id(state.id).r#signal_fire


                {


                    let _ = live.damage(1.0, DamageType::CAMPFIRE);


                }





                if block == Block::MAGMA_BLOCK {


                    let _ = live.damage(1.0, DamageType::HOT_FLOOR);


                }


            }


        }


        */
    }

    pub fn tick_block_collisions(&self, caller: &dyn EntityBase) -> bool {
        if !self.is_affected_by_blocks() {
            return false;
        }

        let bounding_box = self.bounding_box.load();
        let aabb = bounding_box.expand(-1.0e-7, -1.0e-7, -1.0e-7);

        let min = aabb.min_block_pos();
        let max = aabb.max_block_pos();

        let eye_height = self.get_eye_height();
        let eye_width = f64::from(self.width()) * 0.8;
        let mut eye_level_box = aabb;
        let shrink_x = (aabb.max.x - aabb.min.x - eye_width) / 2.0;
        let shrink_z = (aabb.max.z - aabb.min.z - eye_width) / 2.0;
        eye_level_box.min.x += shrink_x;
        eye_level_box.max.x -= shrink_x;
        eye_level_box.min.z += shrink_z;
        eye_level_box.max.z -= shrink_z;
        eye_level_box.min.y += eye_height;
        eye_level_box.max.y = eye_level_box.min.y;

        let mut suffocating = false;
        let world = self.world.load();

        for pos in BlockPos::iterate(min, max) {
            let (block, state) = world.get_block_and_state(&pos);
            if state.is_air() {
                continue;
            }

            let check_suffocation = !suffocating && state.is_suffocating();

            World::check_collision(
                &bounding_box,
                pos,
                state,
                check_suffocation,
                |collision_shape: &BoundingBox| {
                    if collision_shape.intersects(&eye_level_box) {
                        suffocating = true;
                    }
                },
            );

            let collision_shape = if block == &Block::POWDER_SNOW {
                crate::block::blocks::powder_snow::inside_collision_shape_for_entity(caller, &pos)
            } else {
                world
                    .block_registry
                    .get_inside_collision_shape(block, &world, state, &pos)
            };

            if bounding_box.intersects(&collision_shape.at_pos(pos)) {
                if block == &Block::POWDER_SNOW {
                    self.is_in_powder_snow.store(true, Relaxed);
                }
                if let Some(server_arc) = world.server.upgrade() {
                    world.block_registry.on_entity_collision(
                        block,
                        &world,
                        caller,
                        &pos,
                        state,
                        &server_arc,
                    );
                }
            }
        }

        suffocating
    }

    pub fn update_last_pos(&self) -> Vector3<f64> {
        let pos = self.pos.load();
        let old = self.last_pos.load();
        self.movement.store(pos - old);
        self.last_pos.store(pos);
        old
    }

    pub(super) fn update_velocity_from_input(&self, movement_input: Vector3<f64>, speed: f64) {
        let final_input = self.movement_input_to_velocity(movement_input, speed);

        self.velocity.store(self.velocity.load() + final_input);
    }

    pub(super) fn movement_input_to_velocity(
        &self,
        movement_input: Vector3<f64>,
        speed: f64,
    ) -> Vector3<f64> {
        let yaw = f64::from(self.yaw.load()).to_radians();

        let dist = movement_input.length_squared();

        if dist < 1.0e-7 {
            return Vector3::default();
        }

        let input = if dist > 1.0 {
            movement_input.normalize() * speed
        } else {
            movement_input * speed
        };

        let sin = yaw.sin();

        let cos = yaw.cos();

        Vector3::new(
            input.x.mul_add(cos, -(input.z * sin)),
            input.y,
            input.z.mul_add(cos, input.x * sin),
        )
    }

    #[must_use]
    pub fn get_block_pos_below_that_affects_my_movement(&self) -> BlockPos {
        self.get_pos_with_y_offset(0.500_001).0
    }

    #[must_use]
    #[expect(clippy::float_cmp)]
    pub fn get_block_speed_factor(&self) -> f32 {
        let world = self.world.load();
        let (block, _state) = world.get_block_and_state(&self.block_pos.load());
        let speed_factor_here = block.get_speed_factor();
        if block != &Block::WATER && block != &Block::BUBBLE_COLUMN {
            if speed_factor_here == 1.0 {
                let below_pos = self.get_block_pos_below_that_affects_my_movement();
                let (below_block, _below_state) = world.get_block_and_state(&below_pos);
                below_block.get_speed_factor()
            } else {
                speed_factor_here
            }
        } else {
            speed_factor_here
        }
    }

    #[expect(clippy::float_cmp)]
    pub(super) fn get_jump_velocity_multiplier(&self) -> f32 {
        let f = self
            .world
            .load()
            .get_block(&self.block_pos.load())
            .jump_velocity_multiplier;

        let g = self
            .get_block_with_y_offset(0.500_001)
            .1
            .jump_velocity_multiplier;

        if f == 1f32 { g } else { f }
    }

    pub fn move_pos(&self, delta: Vector3<f64>) {
        self.set_pos(self.pos.load() + delta);
    }

    // Does not send movement. That must be done separately
    pub fn move_entity(&self, caller: &dyn EntityBase, mut motion: Vector3<f64>) {
        if caller.get_player().is_some() {
            return;
        }

        if self.no_physics.load(Ordering::Relaxed) {
            self.move_pos(motion);
            self.horizontal_collision.store(false, Ordering::Relaxed);
            self.vertical_collision.store(false, Ordering::Relaxed);
            self.on_ground.store(false, Ordering::Relaxed);

            return;
        }

        let movement_multiplier = self.movement_multiplier.swap(Vector3::default());

        if movement_multiplier.length_squared() > 1.0e-7 {
            motion = motion.multiply(
                movement_multiplier.x,
                movement_multiplier.y,
                movement_multiplier.z,
            );

            self.velocity.store(Vector3::default());
        }

        let final_move = self.adjust_movement_for_collisions(motion, caller);

        self.move_pos(final_move);

        let velocity_multiplier = f64::from(caller.get_block_speed_factor());

        self.velocity.store(final_move * velocity_multiplier);

        if let Some(living) = caller.get_living_entity() {
            let on_ground = self.on_ground.load(Ordering::SeqCst);
            living.fall(caller, final_move.y, on_ground, false);
        }

        let (effect_pos, effect_block, effect_state) = self.get_block_with_y_offset(0.2);
        if motion.y != final_move.y {
            let world = self.world.load();
            world
                .block_registry
                .update_entity_movement_after_fall_on(effect_block, caller);
        }

        if self.is_removed() {
            return;
        }
        self.emit_movement(caller, final_move, &effect_pos, effect_state);
    }

    /// The emission part of vanilla `Entity.move` for movement the client already resolved,
    /// which is how players move.
    pub fn emit_client_movement(&self, caller: &dyn EntityBase, movement: Vector3<f64>) {
        let (effect_pos, _, effect_state) = self.get_block_with_y_offset(0.2);
        self.emit_movement(caller, movement, &effect_pos, effect_state);
    }

    pub(super) fn emit_movement(
        &self,
        caller: &dyn EntityBase,
        movement: Vector3<f64>,
        effect_pos: &BlockPos,
        effect_state: &'static BlockState,
    ) {
        let emission = caller.movement_emission();
        if emission.emits_anything() && !caller.is_passenger() {
            self.apply_movement_emission(caller, emission, movement, effect_pos, effect_state);
        }
    }

    /// Vanilla `applyMovementEmissionAndPlaySound`.
    pub(super) fn apply_movement_emission(
        &self,
        caller: &dyn EntityBase,
        emission: MovementEmission,
        movement: Vector3<f64>,
        effect_pos: &BlockPos,
        effect_state: &'static BlockState,
    ) {
        let moved_distance = (movement.length() * 0.6) as f32;
        let horizontal_moved_distance = (movement.horizontal_length() * 0.6) as f32;
        let supporting_pos = self.get_pos_with_y_offset(1.0e-5).0;
        let (supporting_block, supporting_state) =
            self.world.load().get_block_and_state(&supporting_pos);
        let climbing = Self::is_state_climbable(supporting_block);
        self.move_dist.store(
            self.move_dist.load()
                + if climbing {
                    moved_distance
                } else {
                    horizontal_moved_distance
                },
        );
        self.fly_dist.store(self.fly_dist.load() + moved_distance);

        if self.move_dist.load() > self.next_step.load() && !supporting_state.is_air() {
            let only_effect_state = supporting_pos == *effect_pos;
            let mut produced_side_effects = self.vibration_and_sound_effects_from_block(
                caller,
                effect_pos,
                effect_state,
                emission.emits_sounds(),
                only_effect_state,
                movement,
            );
            if !only_effect_state {
                produced_side_effects |= self.vibration_and_sound_effects_from_block(
                    caller,
                    &supporting_pos,
                    supporting_state,
                    false,
                    emission.emits_events(),
                    movement,
                );
            }

            if produced_side_effects {
                self.next_step.store(caller.next_step());
            } else if self.is_in_water() {
                self.next_step.store(caller.next_step());
                if emission.emits_sounds() {
                    self.water_swim_sound(caller);
                }
                if emission.emits_events() {
                    self.emit_own_game_event(caller, GameEvent::Swim);
                }
            }
        } else if supporting_state.is_air() && caller.is_flapping() {
            // Vanilla `processFlappingMovement`.
            caller.on_flap();
            if emission.emits_events() {
                self.emit_own_game_event(caller, GameEvent::Flap);
            }
        }
    }

    pub fn push_out_of_blocks(&self, center_pos: Vector3<f64>) {
        let block_pos = BlockPos::floored_v(center_pos);

        let delta = center_pos.sub(&block_pos.0.to_f64());

        let mut min_dist = f64::MAX;

        let mut direction = BlockDirection::Up;

        for dir in BlockDirection::all() {
            if dir == BlockDirection::Down {
                continue;
            }

            let offset = dir.to_offset();

            if self
                .world
                .load()
                .get_block_state(&block_pos.offset(offset))
                .is_full_cube()
            {
                continue;
            }

            let component = delta.get_axis(dir.to_axis().into());

            let dist = if dir.positive() {
                1.0 - component
            } else {
                component
            };

            if dist < min_dist {
                min_dist = dist;

                direction = dir;
            }
        }

        let amplitude = rand::random::<f64>().mul_add(PUSH_OUT_RANDOM_SPEED, PUSH_OUT_MIN_SPEED);

        let axis = direction.to_axis().into();

        let sign = if direction.positive() { 1.0 } else { -1.0 };

        let mut velo = self.velocity.load();

        velo = velo * 0.75;

        velo.set_axis(axis, sign * amplitude);

        self.velocity.store(velo);
    }

    /// Applies knockback to the entity, following vanilla Minecraft's mechanics.
    ///
    /// This function calculates the entity's new velocity based on the specified knockback strength and direction.
    pub fn knockback(&self, strength: f64, x: f64, z: f64) {
        // This has some vanilla magic
        let mut x = x;
        let mut z = z;
        while x.mul_add(x, z * z) < 1.0E-5 {
            x = (rand::random::<f64>() - rand::random::<f64>()) * 0.01;
            z = (rand::random::<f64>() - rand::random::<f64>()) * 0.01;
        }

        let var8 = Vector3::new(x, 0.0, z).normalize() * strength;
        let velocity = self.velocity.load();
        self.velocity.store(Vector3::new(
            velocity.x / 2.0 - var8.x,
            if self.on_ground.load(Relaxed) {
                (velocity.y / 2.0 + strength).min(0.4)
            } else {
                velocity.y
            },
            velocity.z / 2.0 - var8.z,
        ));
    }

    pub fn check_block_collision(entity: &dyn EntityBase, server: &Server) {
        let aabb = entity.get_entity().bounding_box.load();
        let blockpos = BlockPos::new(
            (aabb.min.x + 0.001).floor() as i32,
            (aabb.min.y + 0.001).floor() as i32,
            (aabb.min.z + 0.001).floor() as i32,
        );
        let blockpos1 = BlockPos::new(
            (aabb.max.x - 0.001).floor() as i32,
            (aabb.max.y - 0.001).floor() as i32,
            (aabb.max.z - 0.001).floor() as i32,
        );
        let world = entity.get_entity().world.load();

        for x in blockpos.0.x..=blockpos1.0.x {
            for y in blockpos.0.y..=blockpos1.0.y {
                for z in blockpos.0.z..=blockpos1.0.z {
                    let pos = BlockPos::new(x, y, z);
                    let (block, state) = world.get_block_and_state(&pos);
                    let block_outlines = state.get_block_outline_shapes_at(&pos);

                    if state.outline_shapes.is_empty() {
                        world
                            .block_registry
                            .on_entity_collision(block, &world, entity, &pos, state, server);
                        let fluid = world.get_fluid(&pos);
                        world
                            .block_registry
                            .on_entity_collision_fluid(fluid, entity);
                        continue;
                    }
                    for outline in block_outlines {
                        let outline_aabb = outline.at_pos(pos);
                        if outline_aabb.intersects(&aabb) {
                            world
                                .block_registry
                                .on_entity_collision(block, &world, entity, &pos, state, server);
                            let fluid = world.get_fluid(&pos);
                            world
                                .block_registry
                                .on_entity_collision_fluid(fluid, entity);
                            break;
                        }
                    }
                }
            }
        }
    }

    #[must_use]
    pub fn is_affected_by_blocks(&self) -> bool {
        !self.is_removed() && !self.no_physics.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn is_in_wall(&self) -> bool {
        if self.no_physics.load(Ordering::Relaxed) {
            return false;
        }

        let eye_pos = self.get_eye_pos();
        let half_width = (f64::from(self.entity_dimension.load().width) * 0.8) / 2.0;
        let eye_bb = BoundingBox::new(
            Vector3::new(eye_pos.x - half_width, eye_pos.y, eye_pos.z - half_width),
            Vector3::new(
                eye_pos.x + half_width,
                eye_pos.y + 1.0e-6,
                eye_pos.z + half_width,
            ),
        );
        let min = eye_bb.min_block_pos();
        let max = eye_bb.max_block_pos();
        let world = self.world.load();

        for pos in BlockPos::iterate(min, max) {
            let (block, state) = world.get_block_and_state(&pos);
            if state.is_air() {
                continue;
            }

            if blocks_movement(state, block.id) && state.is_full_cube() {
                return true;
            }
        }

        false
    }

    pub fn check_out_of_world(&self, dyn_self: &dyn EntityBase) {
        if self.pos.load().y < f64::from(self.world.load().dimension.min_y) - 64.0 {
            dyn_self.tick_in_void(dyn_self);
        }
    }

    pub fn slow_movement(&self, state: &BlockState, multiplier: Vector3<f64>) {
        match self.entity_type.id {
            v if v == EntityType::PLAYER.id => {
                if let Some(player_entity) = self.get_player()
                    && player_entity.is_flying()
                {
                    return;
                }
            }
            v if (v == EntityType::SPIDER.id || v == EntityType::CAVE_SPIDER.id)
                && Block::from_state_id(state.id).id == Block::COBWEB.id =>
            {
                return;
            }
            v if v == EntityType::WITHER.id => {
                return;
            }
            _ => {}
        }
        if let Some(living) = self.get_living_entity() {
            living.fall_distance.store(0f32);
        }
        self.movement_multiplier.store(multiplier);
    }
}
