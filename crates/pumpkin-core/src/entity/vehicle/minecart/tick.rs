//! The phases of `MinecartEntity::tick`, in the order the tick runs them.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use pumpkin_data::block_properties::PoweredRailLikeProperties;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_protocol::java::server::play::SPlayerInput;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector3::Vector3;

use super::{GRAVITY, MinecartEntity, MinecartKind, RAIL_HEIGHT_OFFSET, container, get_exits};
use crate::entity::EntityBase;
use crate::world::World;

/// The rail a cart is on: its own block, or the block below when only that one is a rail.
#[derive(Clone, Copy)]
pub(super) struct RailAt {
    block_pos: BlockPos,
    block: &'static Block,
    state_id: BlockStateId,
    is_powered_rail: bool,
    is_activator_rail: bool,
    is_on_rails: bool,
}

impl MinecartEntity {
    /// Looks for a rail in the cart's own block, then in the block below it.
    pub(super) fn find_rail(&self, world: &World) -> RailAt {
        let pos = self.vehicle.entity.pos.load();
        let mut block_pos = BlockPos(Vector3::new(
            pos.x.floor() as i32,
            pos.y.floor() as i32,
            pos.z.floor() as i32,
        ));

        let (mut block, mut state_id) = world.get_block_and_state_id(&block_pos);

        let mut is_powered_rail = block.id == Block::POWERED_RAIL.id;
        let mut is_activator_rail = block.id == Block::ACTIVATOR_RAIL.id;
        let mut is_on_rails = is_powered_rail
            || is_activator_rail
            || block.id == Block::RAIL.id
            || block.id == Block::DETECTOR_RAIL.id;

        // If not on rails at current Y level, check the block directly below
        if !is_on_rails {
            let below_block_pos = BlockPos(Vector3::new(
                block_pos.0.x,
                block_pos.0.y - 1,
                block_pos.0.z,
            ));
            let (below_block, below_state_id) = world.get_block_and_state_id(&below_block_pos);
            if below_block.id == Block::RAIL.id
                || below_block.id == Block::POWERED_RAIL.id
                || below_block.id == Block::DETECTOR_RAIL.id
                || below_block.id == Block::ACTIVATOR_RAIL.id
            {
                block_pos = below_block_pos;
                block = below_block;
                state_id = below_state_id;
                is_powered_rail = block.id == Block::POWERED_RAIL.id;
                is_activator_rail = block.id == Block::ACTIVATOR_RAIL.id;
                is_on_rails = true;
            }
        }

        RailAt {
            block_pos,
            block,
            state_id,
            is_powered_rail,
            is_activator_rail,
            is_on_rails,
        }
    }

    /// An activator rail switches a hopper cart off while powered; a detector rail is told
    /// that the cart is on it.
    pub(super) fn touch_rail(&self, world: &Arc<World>, rail: &RailAt) {
        let RailAt {
            block_pos,
            block,
            state_id,
            is_powered_rail,
            is_activator_rail,
            ..
        } = *rail;
        if is_powered_rail || is_activator_rail {
            let props = PoweredRailLikeProperties::from_state_id(state_id);
            let powered = props.powered;

            if is_activator_rail && let MinecartKind::Hopper(minecart) = &self.kind {
                minecart.set_enabled(!powered);
            }
        } else if block.id == Block::DETECTOR_RAIL.id
            && let Some(server) = world.server.upgrade()
        {
            world.block_registry.on_entity_collision(
                block,
                world,
                self,
                &block_pos,
                world.get_block_state(&block_pos),
                &server,
            );
        }
    }

    /// A powered rail speeds the cart up (or starts a cart at rest along its yaw) or, when
    /// unpowered, brakes it; a powered activator rail primes a TNT cart or makes a rideable
    /// cart eject its passengers and wobble.
    pub(super) fn apply_powered_rail(&self, rail: &RailAt) {
        let RailAt {
            state_id,
            is_powered_rail,
            is_activator_rail,
            ..
        } = *rail;
        if is_powered_rail || is_activator_rail {
            let props = PoweredRailLikeProperties::from_state_id(state_id);
            let powered = props.powered;

            if powered {
                if is_powered_rail {
                    let mut velocity = self.vehicle.entity.velocity.load();
                    let speed = velocity.length();
                    if speed > 0.01 {
                        let new_speed = (speed + 0.06).min(0.4);
                        velocity = velocity
                            .normalize()
                            .multiply(new_speed, new_speed, new_speed);
                        self.vehicle.entity.velocity.store(velocity);
                    } else {
                        let yaw = self.vehicle.entity.yaw.load();
                        let push_dir = Vector3::new(
                            -f64::from((yaw.to_radians()).sin()),
                            0.0,
                            f64::from((yaw.to_radians()).cos()),
                        );
                        self.vehicle
                            .entity
                            .velocity
                            .store(push_dir.multiply(0.1, 0.1, 0.1));
                    }
                    self.vehicle.entity.send_velocity();
                } else if is_activator_rail {
                    match &self.kind {
                        MinecartKind::Tnt(minecart) => {
                            minecart.prime(&self.vehicle.entity, 80);
                        }
                        MinecartKind::Rideable(_) => {
                            // Vanilla ejects every passenger. Release the passenger lock before
                            // removing them: removal takes it again, and a std mutex locked twice
                            // on one thread never returns (the tick froze the server, #31).
                            let p_ids: Vec<i32> = self
                                .vehicle
                                .entity
                                .passengers
                                .try_lock()
                                .map(|passengers| {
                                    passengers
                                        .iter()
                                        .map(|p| p.get_entity().entity_id)
                                        .collect()
                                })
                                .unwrap_or_default();
                            if !p_ids.is_empty() {
                                let world = self.vehicle.entity.world.load();
                                let vid = self.vehicle.entity.entity_id;
                                if let Some(v) = world.get_entity_by_id(vid) {
                                    for pid in p_ids {
                                        v.get_entity().remove_passenger_sync(pid);
                                    }
                                }
                            }
                            if self.vehicle.get_hurt_time() == 0 {
                                self.vehicle.set_hurt_dir(-self.vehicle.get_hurt_dir());
                                self.vehicle.set_hurt_time(10);
                                self.vehicle.set_damage(50.0);
                                self.vehicle.send_wobble_metadata();
                            }
                        }
                        _ => {}
                    }
                }
            } else if is_powered_rail {
                let mut velocity = self.vehicle.entity.velocity.load();
                velocity = velocity.multiply(0.5, 0.5, 0.5);
                if velocity.length() < 0.01 {
                    velocity = Vector3::new(0.0, 0.0, 0.0);
                }
                self.vehicle.entity.velocity.store(velocity);
                self.vehicle.entity.send_velocity();
            }
        }
    }

    /// A player in the first seat pushes the cart along the rails with forward or backward.
    pub(super) fn apply_driver_input(&self, rail: &RailAt) {
        let is_on_rails = rail.is_on_rails;
        let mut velocity = self.vehicle.entity.velocity.load();

        let mut has_driver = false;
        let mut driver_input = 0;
        let mut driver_yaw = 0.0f32;

        if let Ok(passengers) = self.vehicle.entity.passengers.try_lock()
            && let Some(passenger) = passengers.first()
            && let Some(player) = passenger.get_player()
        {
            driver_input = player.last_input.load(Ordering::Relaxed);
            driver_yaw = player.get_entity().yaw.load();
            has_driver = true;
        }

        if has_driver && is_on_rails {
            let forward = driver_input & SPlayerInput::FORWARD != 0;
            let backward = driver_input & SPlayerInput::BACKWARD != 0;

            let mut force_dir = Vector3::new(0.0, 0.0, 0.0);
            if forward {
                let yaw_rad = f64::from(driver_yaw).to_radians();
                force_dir.x = -yaw_rad.sin();
                force_dir.z = yaw_rad.cos();
            } else if backward {
                let yaw_rad = f64::from(driver_yaw).to_radians();
                force_dir.x = yaw_rad.sin();
                force_dir.z = -yaw_rad.cos();
            }

            if forward || backward {
                velocity.x += force_dir.x * 0.02;
                velocity.z += force_dir.z * 0.02;

                let speed = velocity.x.hypot(velocity.z);
                if speed > 0.15 {
                    #[allow(clippy::suboptimal_flops)]
                    let old_speed = self
                        .vehicle
                        .entity
                        .velocity
                        .load()
                        .x
                        .hypot(self.vehicle.entity.velocity.load().z);

                    let max_speed = old_speed.clamp(0.15, 0.4);
                    if speed > max_speed {
                        velocity.x = (velocity.x / speed) * max_speed;
                        velocity.z = (velocity.z / speed) * max_speed;
                    }
                }
                self.vehicle.entity.velocity.store(velocity);
                self.vehicle.entity.send_velocity();
            }
        }
    }

    /// Snaps the cart onto its rail and turns its velocity along it; off rails it falls.
    /// Returns the velocity the cart moves by this tick.
    pub(super) fn follow_rail(&self, rail: &RailAt) -> Vector3<f64> {
        let RailAt {
            block_pos,
            block,
            state_id,
            is_on_rails,
            ..
        } = *rail;
        let mut velocity = self.vehicle.entity.velocity.load();

        if is_on_rails {
            use pumpkin_data::block_properties::RailShape;

            let shape = rail_shape(block, state_id);

            let pos = self.vehicle.entity.pos.load();
            let block_center_bottom = Vector3::new(
                f64::from(block_pos.0.x) + 0.5,
                f64::from(block_pos.0.y),
                f64::from(block_pos.0.z) + 0.5,
            );

            let (exit0, exit1) = get_exits(shape);
            let exit0 = exit0.multiply(0.5, 0.5, 0.5);
            let exit1 = exit1.multiply(0.5, 0.5, 0.5);

            let in_corner = exit0.x != exit1.x && exit0.z != exit1.z;
            let mut target_position = pos;

            if in_corner {
                let from0to1 = exit1 - exit0;
                let from0topos = pos - block_center_bottom - exit0;
                let dot_num = from0to1.dot(&from0topos);
                let dot_den = from0to1.dot(&from0to1);
                if dot_den != 0.0 {
                    let travel_vector_from0 =
                        from0to1.multiply(dot_num / dot_den, dot_num / dot_den, dot_num / dot_den);
                    target_position = block_center_bottom.add(&exit0).add(&travel_vector_from0);
                }
            } else {
                let z_snap = (exit0.x - exit1.x).abs() > 1e-5;
                let x_snap = (exit0.z - exit1.z).abs() > 1e-5;
                if x_snap {
                    target_position.x = block_center_bottom.x;
                }
                if z_snap {
                    target_position.z = block_center_bottom.z;
                }
            }

            target_position.y = match shape {
                RailShape::AscendingEast
                | RailShape::AscendingWest
                | RailShape::AscendingNorth
                | RailShape::AscendingSouth => pos.y,
                _ => f64::from(block_pos.0.y) + RAIL_HEIGHT_OFFSET,
            };
            self.vehicle.entity.pos.store(target_position);

            let horizontal_in_direction = Vector3::new(exit1.x, 0.0, exit1.z);
            let mut horizontal_out_direction = Vector3::new(exit0.x, 0.0, exit0.z);

            if velocity.dot(&horizontal_out_direction) < velocity.dot(&horizontal_in_direction) {
                horizontal_out_direction = horizontal_in_direction;
            }

            let out_position = block_center_bottom.add(&horizontal_out_direction).add(
                &horizontal_out_direction
                    .normalize()
                    .multiply(1e-5, 1e-5, 1e-5),
            );

            let mut towards_out = out_position - target_position;
            towards_out.y = 0.0;
            let towards_length = towards_out.length();
            if towards_length > 1e-5 {
                towards_out = towards_out.normalize();
                let speed = velocity.length();
                velocity = towards_out.multiply(speed, speed, speed);
            }

            velocity.y = 0.0;
            self.vehicle.entity.velocity.store(velocity);
        } else if !self.vehicle.entity.on_ground.load(Ordering::Relaxed) {
            velocity.y -= GRAVITY;
            self.vehicle.entity.velocity.store(velocity);
        }

        velocity
    }

    /// Moves the cart, carries its passengers along and slows it down. False when a TNT cart
    /// exploded against a wall: the tick ends there.
    pub(super) fn move_and_slow_down(
        &self,
        caller: &dyn EntityBase,
        world: &World,
        rail: &RailAt,
        velocity: Vector3<f64>,
    ) -> bool {
        let RailAt {
            block_pos,
            is_on_rails,
            ..
        } = *rail;
        self.move_entity(caller, velocity);

        if let MinecartKind::Tnt(minecart) = &self.kind
            && self
                .vehicle
                .entity
                .horizontal_collision
                .load(Ordering::Relaxed)
            && velocity.x.mul_add(velocity.x, velocity.z * velocity.z) >= 0.01
        {
            minecart.explode(
                &self.vehicle.entity,
                velocity.x.mul_add(velocity.x, velocity.z * velocity.z),
            );
            return false;
        }

        let new_pos = self.vehicle.entity.pos.load();

        if let Ok(passengers) = self.vehicle.entity.passengers.try_lock() {
            for passenger in passengers.iter() {
                passenger.get_entity().set_pos(new_pos);
            }
        }

        #[allow(clippy::useless_let_if_seq)]
        let mut friction = 0.95; // Vanilla minecart air drag

        if is_on_rails {
            let has_passengers = self
                .vehicle
                .entity
                .passengers
                .try_lock()
                .is_ok_and(|p| !p.is_empty());
            friction = if has_passengers { 0.99 } else { 0.96 };
        } else {
            let below_block_pos = BlockPos(Vector3::new(
                block_pos.0.x,
                block_pos.0.y - 1,
                block_pos.0.z,
            ));
            let below_block = world.get_block(&below_block_pos);

            let is_on_ground = self.vehicle.entity.on_ground.load(Ordering::Relaxed)
                || (below_block.id != Block::AIR.id
                    && below_block.id != Block::WATER.id
                    && below_block.id != Block::LAVA.id);
            let is_in_water = self.vehicle.entity.touching_water.load(Ordering::Relaxed)
                || below_block.id == Block::WATER.id;

            if is_on_ground {
                friction = 0.5;
            } else if is_in_water {
                friction = 0.95;
            }
        }

        let mut next_vel = if is_on_rails && let MinecartKind::Furnace(minecart) = &self.kind {
            minecart.velocity(&self.vehicle.entity, velocity)
        } else if is_on_rails && let Some(inventory) = self.container() {
            container::velocity(&self.vehicle.entity, inventory, velocity)
        } else {
            velocity.multiply(friction, friction, friction)
        };
        if next_vel.length() < 0.005 {
            next_vel = Vector3::new(0.0, 0.0, 0.0);
        }
        self.vehicle.entity.velocity.store(next_vel);
        if next_vel.length_squared() == 0.0 {
            self.vehicle.entity.send_velocity();
        }

        true
    }
}

/// The shape of a rail block; the powered, detector and activator rails have only the
/// straight shapes.
fn rail_shape(block: &Block, state_id: BlockStateId) -> pumpkin_data::block_properties::RailShape {
    use pumpkin_data::block_properties::RailLikeProperties;
    use pumpkin_data::block_properties::{RailShape, RailShapeStraight};

    if block.id == Block::RAIL.id {
        let props = RailLikeProperties::from_state_id(state_id);
        props.shape
    } else {
        let props = PoweredRailLikeProperties::from_state_id(state_id);
        match props.shape {
            RailShapeStraight::NorthSouth => RailShape::NorthSouth,
            RailShapeStraight::EastWest => RailShape::EastWest,
            RailShapeStraight::AscendingEast => RailShape::AscendingEast,
            RailShapeStraight::AscendingWest => RailShape::AscendingWest,
            RailShapeStraight::AscendingNorth => RailShape::AscendingNorth,
            RailShapeStraight::AscendingSouth => RailShape::AscendingSouth,
        }
    }
}
