//! The movement part of `MinecartEntity::tick`: a port of vanilla 26.3
//! `OldMinecartBehavior.tick` (the default movement; the experimental one is not ported).

use std::sync::Arc;
use std::sync::atomic::Ordering;

use pumpkin_data::block_properties::{PoweredRailLikeProperties, RailShape};
use pumpkin_data::entity::EntityType;
use pumpkin_data::tag::{self, Taggable};
use pumpkin_data::{Block, BlockStateId};
use pumpkin_protocol::java::server::play::SPlayerInput;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::math::wrap_degrees;

use super::{MinecartEntity, MinecartKind, is_minecart};
use crate::entity::{Entity, EntityBase};
use crate::world::World;

/// Vanilla `slideSpeed`: what a slope adds to the speed each tick.
const SLIDE_SPEED: f64 = 0.007_812_5;
/// Height of the cart above its rail block.
const RAIL_HEIGHT: f64 = 0.0625;

/// The rail under a cart (vanilla `getCurrentBlockPosOrRailBelow` and the state there).
#[derive(Clone, Copy)]
struct RailAt {
    block_pos: BlockPos,
    block: &'static Block,
    state_id: BlockStateId,
    is_on_rails: bool,
}

/// The block a cart at `pos` stands in, or the one below when that one is a rail.
fn rail_at(world: &World, pos: Vector3<f64>) -> RailAt {
    let mut block_pos = BlockPos(Vector3::new(
        pos.x.floor() as i32,
        pos.y.floor() as i32,
        pos.z.floor() as i32,
    ));
    let below = block_pos.down();
    if world
        .get_block(&below)
        .has_tag(&tag::Block::MINECRAFT_RAILS)
    {
        block_pos = below;
    }
    let (block, state_id) = world.get_block_and_state_id(&block_pos);
    RailAt {
        block_pos,
        block,
        state_id,
        is_on_rails: block.has_tag(&tag::Block::MINECRAFT_RAILS),
    }
}

/// The two ends of a rail shape as block offsets (vanilla `AbstractMinecart.EXITS`).
/// A slope's lower end is one block down: the cart's height is taken from the upper level.
const fn exits(shape: RailShape) -> ([i32; 3], [i32; 3]) {
    match shape {
        RailShape::NorthSouth => ([0, 0, -1], [0, 0, 1]),
        RailShape::EastWest => ([-1, 0, 0], [1, 0, 0]),
        RailShape::AscendingEast => ([-1, -1, 0], [1, 0, 0]),
        RailShape::AscendingWest => ([-1, 0, 0], [1, -1, 0]),
        RailShape::AscendingNorth => ([0, 0, -1], [0, -1, 1]),
        RailShape::AscendingSouth => ([0, -1, -1], [0, 0, 1]),
        RailShape::SouthEast => ([0, 0, 1], [1, 0, 0]),
        RailShape::SouthWest => ([0, 0, 1], [-1, 0, 0]),
        RailShape::NorthWest => ([0, 0, -1], [-1, 0, 0]),
        RailShape::NorthEast => ([0, 0, -1], [1, 0, 0]),
    }
}

/// The x, z of a rail end at block column `xt`, `zt`.
fn end_xz(xt: i32, zt: i32, exit: [i32; 3]) -> (f64, f64) {
    (
        f64::from(xt) + 0.5 + f64::from(exit[0]) * 0.5,
        f64::from(zt) + 0.5 + f64::from(exit[2]) * 0.5,
    )
}

/// How far along the rail between `start` (x, z of one end) and `delta` (to the other end)
/// the point `x`, `z` of the block at `xt`, `zt` lies (vanilla's `progress`).
fn progress(x: f64, z: f64, xt: i32, zt: i32, start: (f64, f64), delta: (f64, f64)) -> f64 {
    if delta.0 == 0.0 {
        z - f64::from(zt)
    } else if delta.1 == 0.0 {
        x - f64::from(xt)
    } else {
        (x - start.0).mul_add(delta.0, (z - start.1) * delta.1) * 2.0
    }
}

/// The point on the rail nearest `pos`, with the height of the rail there (vanilla
/// `OldMinecartBehavior.getPos`); `None` off rails.
fn track_pos(world: &World, pos: Vector3<f64>) -> Option<Vector3<f64>> {
    let rail = rail_at(world, pos);
    if !rail.is_on_rails {
        return None;
    }
    let (xt, yt, zt) = (rail.block_pos.0.x, rail.block_pos.0.y, rail.block_pos.0.z);
    let (exit0, exit1) = exits(rail_shape(rail.block, rail.state_id));
    let (x0, z0) = end_xz(xt, zt, exit0);
    let (x1, z1) = end_xz(xt, zt, exit1);
    let y0 = f64::from(yt) + RAIL_HEIGHT + f64::from(exit0[1]) * 0.5;
    let y1 = f64::from(yt) + RAIL_HEIGHT + f64::from(exit1[1]) * 0.5;
    let (x_delta, y_delta, z_delta) = (x1 - x0, (y1 - y0) * 2.0, z1 - z0);
    let along = progress(pos.x, pos.z, xt, zt, (x0, z0), (x_delta, z_delta));
    let mut y = y_delta.mul_add(along, y0);
    if y_delta < 0.0 {
        y += 1.0;
    } else if y_delta > 0.0 {
        y += 0.5;
    }
    Some(Vector3::new(
        x_delta.mul_add(along, x0),
        y,
        z_delta.mul_add(along, z0),
    ))
}

/// Vanilla `ServerPlayer.getLastClientMoveIntent`: the movement keys turned by the yaw.
fn move_intent(input: i8, yaw: f32) -> Vector3<f64> {
    let axis = |positive: i8, negative: i8| match (input & positive != 0, input & negative != 0) {
        (true, false) => 1.0,
        (false, true) => -1.0,
        _ => 0.0,
    };
    let strafe: f64 = axis(SPlayerInput::LEFT, SPlayerInput::RIGHT);
    let forward: f64 = axis(SPlayerInput::FORWARD, SPlayerInput::BACKWARD);
    let length_squared = strafe.mul_add(strafe, forward * forward);
    if length_squared < 1.0e-7 {
        return Vector3::new(0.0, 0.0, 0.0);
    }
    let length = if length_squared > 1.0 {
        length_squared.sqrt()
    } else {
        1.0
    };
    let (strafe, forward) = (strafe / length, forward / length);
    let radians = yaw.to_radians();
    let (sin, cos) = (f64::from(radians.sin()), f64::from(radians.cos()));
    Vector3::new(
        strafe.mul_add(cos, -forward * sin),
        0.0,
        forward.mul_add(cos, strafe * sin),
    )
}

impl MinecartEntity {
    /// Vanilla `OldMinecartBehavior.tick` on the server.
    pub(super) fn tick_movement(&self, caller: &dyn EntityBase, world: &Arc<World>) {
        let entity = &self.vehicle.entity;
        let start = entity.pos.load();
        let start_yaw = entity.yaw.load();
        self.apply_gravity();
        let rail = rail_at(world, start);
        if rail.is_on_rails {
            self.move_along_track(caller, world, &rail);
            if rail.block.id == Block::ACTIVATOR_RAIL.id {
                self.activate(PoweredRailLikeProperties::from_state_id(rail.state_id).powered);
            }
        } else {
            self.come_off_track(caller);
        }
        self.touch_detector_rail(world);
        self.turn_with_motion(start, start_yaw);
        self.push_and_pick_up(world);
    }

    fn in_water(&self) -> bool {
        self.vehicle.entity.touching_water.load(Ordering::Relaxed)
    }

    fn apply_gravity(&self) {
        let entity = &self.vehicle.entity;
        if entity.has_no_gravity.load(Ordering::Relaxed) {
            return;
        }
        let gravity = if self.in_water() { 0.005 } else { 0.04 };
        let mut velocity = entity.velocity.load();
        velocity.y -= gravity;
        entity.velocity.store(velocity);
    }

    /// Vanilla `getMaxSpeed`: 0.4 blocks a tick on land, half in water; a furnace cart half
    /// of that on land and three quarters of it in water.
    fn max_speed(&self) -> f64 {
        let base = if self.in_water() { 0.2 } else { 0.4 };
        match self.kind {
            MinecartKind::Furnace(_) if self.in_water() => base * 0.75,
            MinecartKind::Furnace(_) => base * 0.5,
            _ => base,
        }
    }

    /// Vanilla `applyNaturalSlowdown` (furnace and container carts have their own).
    fn natural_slowdown(&self, velocity: Vector3<f64>) -> Vector3<f64> {
        let entity = &self.vehicle.entity;
        if let MinecartKind::Furnace(minecart) = &self.kind {
            return self.base_slowdown(minecart.velocity(entity, velocity));
        }
        if let Some(inventory) = self.container() {
            return super::container::velocity(entity, inventory, velocity);
        }
        self.base_slowdown(velocity)
    }

    /// The plain cart's slowdown: 0.997 with a rider, 0.96 without, and 0.95 more in water.
    fn base_slowdown(&self, velocity: Vector3<f64>) -> Vector3<f64> {
        let keep = if self.vehicle.entity.has_passengers() {
            0.997
        } else {
            0.96
        };
        let slowed = velocity.multiply(keep, 0.0, keep);
        if self.in_water() {
            slowed.multiply(0.95, 0.95, 0.95)
        } else {
            slowed
        }
    }

    /// Vanilla `Entity.move` for a cart: Pumpkin's `move_entity` stores the distance moved
    /// as the new velocity, vanilla keeps the velocity and only zeroes the axes that hit
    /// something, then applies the block speed factor.
    fn move_keeping_velocity(&self, caller: &dyn EntityBase, delta: Vector3<f64>) {
        let entity = &self.vehicle.entity;
        let velocity = entity.velocity.load();
        let before = entity.pos.load();
        entity.move_entity(caller, delta);
        let moved = entity.pos.load() - before;
        let blocked = |wanted: f64, got: f64| (wanted - got).abs() > 1.0e-5;
        let mut next = velocity;
        if entity.horizontal_collision.load(Ordering::Relaxed) {
            if blocked(delta.x, moved.x) {
                next.x = 0.0;
            }
            if blocked(delta.z, moved.z) {
                next.z = 0.0;
            }
        }
        if entity.vertical_collision.load(Ordering::Relaxed) {
            next.y = 0.0;
        }
        let factor = f64::from(caller.get_block_speed_factor());
        entity.velocity.store(next.multiply(factor, 1.0, factor));
    }

    /// Vanilla `AbstractMinecart.comeOffTrack`.
    fn come_off_track(&self, caller: &dyn EntityBase) {
        let entity = &self.vehicle.entity;
        let max = self.max_speed();
        let mut velocity = entity.velocity.load();
        velocity.x = velocity.x.clamp(-max, max);
        velocity.z = velocity.z.clamp(-max, max);
        if entity.on_ground.load(Ordering::Relaxed) {
            velocity = velocity.multiply(0.5, 0.5, 0.5);
        }
        entity.velocity.store(velocity);
        self.move_keeping_velocity(caller, velocity);
        if !entity.on_ground.load(Ordering::Relaxed) {
            entity
                .velocity
                .store(entity.velocity.load().multiply(0.95, 0.95, 0.95));
        }
    }

    /// Vanilla `OldMinecartBehavior.moveAlongTrack`.
    fn move_along_track(&self, caller: &dyn EntityBase, world: &World, rail: &RailAt) {
        let entity = &self.vehicle.entity;
        let pos = entity.pos.load();
        let old_track_pos = track_pos(world, pos);
        let shape = rail_shape(rail.block, rail.state_id);
        let is_powered_rail = rail.block.id == Block::POWERED_RAIL.id;
        let powered =
            is_powered_rail && PoweredRailLikeProperties::from_state_id(rail.state_id).powered;

        let y = self.slide_down_slope(shape, rail.block_pos);
        let (exit0, exit1) = exits(shape);
        self.turn_along_rail(exit0, exit1);
        let started_by_rider = self.apply_rider_intent();
        if is_powered_rail && !powered && !started_by_rider {
            let velocity = entity.velocity.load();
            if velocity.horizontal_length() < 0.03 {
                entity.velocity.store(Vector3::new(0.0, 0.0, 0.0));
            } else {
                entity.velocity.store(velocity.multiply(0.5, 0.0, 0.5));
            }
        }

        let (xt, zt) = (rail.block_pos.0.x, rail.block_pos.0.z);
        let (x0, z0) = end_xz(xt, zt, exit0);
        let (x1, z1) = end_xz(xt, zt, exit1);
        let delta = (x1 - x0, z1 - z0);
        let along = progress(pos.x, pos.z, xt, zt, (x0, z0), delta);
        entity.set_pos(Vector3::new(
            delta.0.mul_add(along, x0),
            y,
            delta.1.mul_add(along, z0),
        ));

        let scale = if entity.has_passengers() { 0.75 } else { 1.0 };
        let max = self.max_speed();
        let velocity = entity.velocity.load();
        self.move_keeping_velocity(
            caller,
            Vector3::new(
                (scale * velocity.x).clamp(-max, max),
                0.0,
                (scale * velocity.z).clamp(-max, max),
            ),
        );
        climb_to_raised_end(entity, rail.block_pos, exit0, exit1);
        entity
            .velocity
            .store(self.natural_slowdown(entity.velocity.load()));
        follow_slope_height(entity, world, old_track_pos);
        turn_into_next_block(entity, rail.block_pos);
        if powered {
            boost_on_powered_rail(entity, world, shape, rail.block_pos);
        }
    }

    /// A slope pulls the cart downhill; returns the height the cart is put at.
    fn slide_down_slope(&self, shape: RailShape, block_pos: BlockPos) -> f64 {
        let entity = &self.vehicle.entity;
        let slide = if self.in_water() {
            SLIDE_SPEED * 0.2
        } else {
            SLIDE_SPEED
        };
        let mut velocity = entity.velocity.load();
        let y = f64::from(block_pos.0.y);
        match shape {
            RailShape::AscendingEast => velocity.x -= slide,
            RailShape::AscendingWest => velocity.x += slide,
            RailShape::AscendingNorth => velocity.z += slide,
            RailShape::AscendingSouth => velocity.z -= slide,
            _ => return y,
        }
        entity.velocity.store(velocity);
        y + 1.0
    }

    /// Points the velocity along the rail, keeping its sense and its speed (at most 2).
    fn turn_along_rail(&self, exit0: [i32; 3], exit1: [i32; 3]) {
        let entity = &self.vehicle.entity;
        let velocity = entity.velocity.load();
        let mut x_delta = f64::from(exit1[0] - exit0[0]);
        let mut z_delta = f64::from(exit1[2] - exit0[2]);
        let length = x_delta.hypot(z_delta);
        if velocity.x.mul_add(x_delta, velocity.z * z_delta) < 0.0 {
            x_delta = -x_delta;
            z_delta = -z_delta;
        }
        let speed = velocity.horizontal_length().min(2.0);
        entity.velocity.store(Vector3::new(
            speed * x_delta / length,
            velocity.y,
            speed * z_delta / length,
        ));
    }

    /// A player in the first seat holding a movement key nudges a (nearly) stopped cart
    /// toward where the keys point. True when it did (an unpowered powered rail then does
    /// not brake).
    fn apply_rider_intent(&self) -> bool {
        let entity = &self.vehicle.entity;
        let intent = entity
            .passengers
            .try_lock()
            .ok()
            .and_then(|passengers| passengers.first().cloned())
            .and_then(|passenger| {
                passenger.get_player().map(|player| {
                    move_intent(
                        player.last_input.load(Ordering::Relaxed),
                        player.get_entity().yaw.load(),
                    )
                })
            });
        let Some(intent) = intent else {
            return false;
        };
        let velocity = entity.velocity.load();
        if intent.length_squared() > 0.0 && velocity.horizontal_length_squared() < 0.01 {
            entity.velocity.store(Vector3::new(
                intent.x.mul_add(0.001, velocity.x),
                velocity.y,
                intent.z.mul_add(0.001, velocity.z),
            ));
            return true;
        }
        false
    }

    /// Vanilla `activateMinecart` of each kind, for the activator rail under the cart.
    fn activate(&self, powered: bool) {
        match &self.kind {
            MinecartKind::Hopper(minecart) => minecart.set_enabled(!powered),
            MinecartKind::Tnt(minecart) if powered => minecart.prime(&self.vehicle.entity, 80),
            MinecartKind::Rideable(_) if powered => self.eject_and_wobble(),
            _ => {}
        }
    }

    /// A powered activator rail makes a rideable cart drop its passengers and wobble.
    fn eject_and_wobble(&self) {
        // Release the passenger lock before removing them: removal takes it again, and a std
        // mutex locked twice on one thread never returns (the tick froze the server, #31).
        let ids: Vec<i32> = self
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
        if !ids.is_empty() {
            let world = self.vehicle.entity.world.load();
            if let Some(vehicle) = world.get_entity_by_id(self.vehicle.entity.entity_id) {
                for id in ids {
                    vehicle.get_entity().remove_passenger_sync(id);
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

    /// A detector rail is told that the cart is on it.
    fn touch_detector_rail(&self, world: &Arc<World>) {
        let rail = rail_at(world, self.vehicle.entity.pos.load());
        if rail.block.id == Block::DETECTOR_RAIL.id
            && let Some(server) = world.server.upgrade()
        {
            world.block_registry.on_entity_collision(
                rail.block,
                world,
                self,
                &rail.block_pos,
                world.get_block_state(&rail.block_pos),
                &server,
            );
        }
    }

    /// The cart faces the way it moved; it flips instead of turning by about half a turn.
    fn turn_with_motion(&self, start: Vector3<f64>, start_yaw: f32) {
        let entity = &self.vehicle.entity;
        let pos = entity.pos.load();
        let (x_diff, z_diff) = (start.x - pos.x, start.z - pos.z);
        let mut yaw = entity.yaw.load();
        if x_diff.mul_add(x_diff, z_diff * z_diff) > 0.001 {
            yaw = z_diff.atan2(x_diff).to_degrees() as f32;
            if self.flipped.load(Ordering::Relaxed) {
                yaw += 180.0;
            }
        }
        let turn = wrap_degrees(yaw - start_yaw);
        if !(-170.0..170.0).contains(&turn) {
            yaw += 180.0;
            self.flipped.fetch_xor(true, Ordering::Relaxed);
        }
        entity.pitch.store(0.0);
        entity.yaw.store(yaw % 360.0);
    }

    /// Vanilla `OldMinecartBehavior.pushAndPickupEntities`: a moving rideable cart picks up
    /// a mob it runs into (not a player, iron golem or cart) or pushes it; any other cart
    /// only pushes the carts it touches.
    fn push_and_pick_up(&self, world: &Arc<World>) {
        let entity = &self.vehicle.entity;
        let hitbox = entity.bounding_box.load().expand(0.2, 0.0, 0.2);
        let own_id = entity.entity_id;
        let others = world
            .get_entities_at_box(&hitbox)
            .into_iter()
            .filter(|other| other.get_entity().entity_id != own_id);
        let rideable = matches!(self.kind, MinecartKind::Rideable(_));
        if !(rideable && entity.velocity.load().horizontal_length_squared() >= 0.01) {
            for other in others {
                if !self.has_passenger(other.as_ref())
                    && other.is_pushable()
                    && is_minecart(other.get_entity().entity_type)
                {
                    other.push(self);
                }
            }
            return;
        }
        for other in others.filter(|other| other.is_pushable() && !other.is_spectator()) {
            let kind = other.get_entity().entity_type;
            if kind.id != EntityType::IRON_GOLEM.id
                && !is_minecart(kind)
                && !entity.has_passengers()
                && !other.is_passenger()
                && other.get_entity().riding_cooldown.load(Ordering::Relaxed) == 0
                && let Some(vehicle) = world.get_entity_by_id(own_id)
            {
                entity.add_passenger(vehicle, other);
            } else {
                other.push(self);
            }
        }
        for player in world.get_players_at_box(&hitbox) {
            if !player.is_spectator() {
                player.push(self);
            }
        }
    }
}

/// A cart that reached the raised end of a slope steps up onto it.
fn climb_to_raised_end(entity: &Entity, block_pos: BlockPos, exit0: [i32; 3], exit1: [i32; 3]) {
    let pos = entity.pos.load();
    let column = (
        pos.x.floor() as i32 - block_pos.0.x,
        pos.z.floor() as i32 - block_pos.0.z,
    );
    for exit in [exit0, exit1] {
        if exit[1] != 0 && column == (exit[0], exit[2]) {
            entity.set_pos(Vector3::new(pos.x, pos.y + f64::from(exit[1]), pos.z));
            return;
        }
    }
}

/// The cart takes the rail's height at its new place, and speeds up by what it lost in
/// height (slows down by what it gained).
fn follow_slope_height(entity: &Entity, world: &World, old_track_pos: Option<Vector3<f64>>) {
    let pos = entity.pos.load();
    let (Some(new_track_pos), Some(old_track_pos)) = (track_pos(world, pos), old_track_pos) else {
        return;
    };
    let gain = (old_track_pos.y - new_track_pos.y) * 0.05;
    let velocity = entity.velocity.load();
    let speed = velocity.horizontal_length();
    if speed > 0.0 {
        let factor = (speed + gain) / speed;
        entity
            .velocity
            .store(velocity.multiply(factor, 1.0, factor));
    }
    entity.set_pos(Vector3::new(pos.x, new_track_pos.y, pos.z));
}

/// A cart that left its rail block keeps its speed toward the block it entered.
fn turn_into_next_block(entity: &Entity, block_pos: BlockPos) {
    let pos = entity.pos.load();
    let x_step = pos.x.floor() as i32 - block_pos.0.x;
    let z_step = pos.z.floor() as i32 - block_pos.0.z;
    if x_step != 0 || z_step != 0 {
        let velocity = entity.velocity.load();
        let speed = velocity.horizontal_length();
        entity.velocity.store(Vector3::new(
            speed * f64::from(x_step),
            velocity.y,
            speed * f64::from(z_step),
        ));
    }
}

/// A powered rail adds 0.06 to a moving cart's speed; a stopped cart is started away from a
/// solid block at one end of a straight rail (vanilla `isRedstoneConductor`).
fn boost_on_powered_rail(entity: &Entity, world: &World, shape: RailShape, block_pos: BlockPos) {
    let velocity = entity.velocity.load();
    let speed = velocity.horizontal_length();
    if speed > 0.01 {
        entity.velocity.store(Vector3::new(
            (velocity.x / speed).mul_add(0.06, velocity.x),
            velocity.y,
            (velocity.z / speed).mul_add(0.06, velocity.z),
        ));
        return;
    }
    let conductor = |dx: i32, dz: i32| {
        let pos = BlockPos(block_pos.0.add(&Vector3::new(dx, 0, dz)));
        world.get_block_state(&pos).is_solid_block()
    };
    let mut next = velocity;
    match shape {
        RailShape::EastWest => {
            if conductor(-1, 0) {
                next.x = 0.02;
            } else if conductor(1, 0) {
                next.x = -0.02;
            }
        }
        RailShape::NorthSouth => {
            if conductor(0, -1) {
                next.z = 0.02;
            } else if conductor(0, 1) {
                next.z = -0.02;
            }
        }
        _ => return,
    }
    entity.velocity.store(next);
}

/// The shape of a rail block; the powered, detector and activator rails have only the
/// straight shapes.
fn rail_shape(block: &Block, state_id: BlockStateId) -> RailShape {
    use pumpkin_data::block_properties::RailLikeProperties;
    use pumpkin_data::block_properties::RailShapeStraight;

    if block.id == Block::RAIL.id {
        RailLikeProperties::from_state_id(state_id).shape
    } else {
        match PoweredRailLikeProperties::from_state_id(state_id).shape {
            RailShapeStraight::NorthSouth => RailShape::NorthSouth,
            RailShapeStraight::EastWest => RailShape::EastWest,
            RailShapeStraight::AscendingEast => RailShape::AscendingEast,
            RailShapeStraight::AscendingWest => RailShape::AscendingWest,
            RailShapeStraight::AscendingNorth => RailShape::AscendingNorth,
            RailShapeStraight::AscendingSouth => RailShape::AscendingSouth,
        }
    }
}
