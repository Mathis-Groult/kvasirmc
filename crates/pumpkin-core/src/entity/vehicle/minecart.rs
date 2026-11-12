#[cfg(test)]
mod activator_tests;
mod chest;
mod container;
mod furnace;
mod hopper;
mod rideable;
mod tick;
#[cfg(test)]
mod tick_golden;
mod tnt;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use rand::RngExt;

use crate::{
    entity::{Entity, EntityBase, living::LivingEntity, player::Player},
    server::Server,
};
use pumpkin_data::damage::DamageType;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::tag::{self, Taggable};
use pumpkin_inventory::Inventory;
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_util::GameMode;
use pumpkin_util::math::vector3::Vector3;

use crate::entity::vehicle::vehicle::VehicleEntity;
use chest::ChestMinecart;
use container::MinecartInventory;
use furnace::FurnaceMinecart;
use hopper::HopperMinecart;
use rideable::RideableMinecart;
use tnt::TntMinecart;

const fn get_exits(
    shape: pumpkin_data::block_properties::RailShape,
) -> (Vector3<f64>, Vector3<f64>) {
    use pumpkin_data::block_properties::RailShape;
    match shape {
        RailShape::NorthSouth => (Vector3::new(0.0, 0.0, -1.0), Vector3::new(0.0, 0.0, 1.0)),
        RailShape::EastWest => (Vector3::new(-1.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
        RailShape::AscendingEast => (Vector3::new(-1.0, 0.0, 0.0), Vector3::new(1.0, 1.0, 0.0)),
        RailShape::AscendingWest => (Vector3::new(1.0, 0.0, 0.0), Vector3::new(-1.0, 1.0, 0.0)),
        RailShape::AscendingNorth => (Vector3::new(0.0, 0.0, 1.0), Vector3::new(0.0, 1.0, -1.0)),
        RailShape::AscendingSouth => (Vector3::new(0.0, 0.0, -1.0), Vector3::new(0.0, 1.0, 1.0)),
        RailShape::SouthEast => (Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0)),
        RailShape::SouthWest => (Vector3::new(0.0, 0.0, 1.0), Vector3::new(-1.0, 0.0, 0.0)),
        RailShape::NorthWest => (Vector3::new(0.0, 0.0, -1.0), Vector3::new(-1.0, 0.0, 0.0)),
        RailShape::NorthEast => (Vector3::new(0.0, 0.0, -1.0), Vector3::new(1.0, 0.0, 0.0)),
    }
}

const GRAVITY: f64 = 0.04;
const RAIL_HEIGHT_OFFSET: f64 = 0.0625;

pub struct MinecartEntity {
    pub vehicle: VehicleEntity,
    kind: MinecartKind,
}

enum MinecartKind {
    Rideable(RideableMinecart),
    Chest(ChestMinecart),
    Furnace(FurnaceMinecart),
    Hopper(HopperMinecart),
    Tnt(TntMinecart),
    Other,
}

impl MinecartEntity {
    pub fn new(mut entity: Entity) -> Self {
        entity.blocks_building = true;
        let kind = match entity.entity_type.id {
            id if id == EntityType::MINECART.id => MinecartKind::Rideable(RideableMinecart),
            id if id == EntityType::CHEST_MINECART.id => MinecartKind::Chest(ChestMinecart::new()),
            id if id == EntityType::FURNACE_MINECART.id => {
                MinecartKind::Furnace(FurnaceMinecart::new())
            }
            id if id == EntityType::HOPPER_MINECART.id => {
                MinecartKind::Hopper(HopperMinecart::new())
            }
            id if id == EntityType::TNT_MINECART.id => MinecartKind::Tnt(TntMinecart::new()),
            _ => MinecartKind::Other,
        };
        Self {
            vehicle: VehicleEntity::new(entity),
            kind,
        }
    }

    const fn container(&self) -> Option<&Arc<MinecartInventory>> {
        match &self.kind {
            MinecartKind::Chest(minecart) => Some(minecart.inventory()),
            MinecartKind::Hopper(minecart) => Some(minecart.inventory()),
            _ => None,
        }
    }

    const fn drop_item(&self) -> Option<&'static Item> {
        match &self.kind {
            MinecartKind::Chest(_) => Some(&Item::CHEST_MINECART),
            MinecartKind::Furnace(_) => Some(&Item::FURNACE_MINECART),
            MinecartKind::Hopper(_) => Some(&Item::HOPPER_MINECART),
            MinecartKind::Tnt(_) => Some(&Item::TNT_MINECART),
            _ => None,
        }
    }
}

impl EntityBase for MinecartEntity {
    fn write_custom_nbt(&self, nbt: &mut NbtCompound) {
        match &self.kind {
            MinecartKind::Chest(minecart) => minecart.write_nbt(nbt),
            MinecartKind::Furnace(minecart) => minecart.write_nbt(nbt),
            MinecartKind::Hopper(minecart) => minecart.write_nbt(nbt),
            MinecartKind::Tnt(minecart) => minecart.write_nbt(nbt),
            MinecartKind::Rideable(_) | MinecartKind::Other => {}
        }
    }

    fn read_custom_nbt(&self, nbt: &NbtCompound) {
        match &self.kind {
            MinecartKind::Chest(minecart) => minecart.read_nbt(nbt),
            MinecartKind::Furnace(minecart) => minecart.read_nbt(nbt),
            MinecartKind::Hopper(minecart) => minecart.read_nbt(nbt),
            MinecartKind::Tnt(minecart) => minecart.read_nbt(nbt),
            MinecartKind::Rideable(_) | MinecartKind::Other => {}
        }
    }

    fn tick(&self, caller: &dyn EntityBase, _server: &Server) {
        self.vehicle.tick();
        if let MinecartKind::Furnace(minecart) = &self.kind {
            minecart.tick(&self.vehicle.entity);
        }

        let world = self.vehicle.entity.world.load();
        let rail = self.find_rail(&world);
        self.touch_rail(&world, &rail);
        self.apply_powered_rail(&rail);

        if let MinecartKind::Tnt(minecart) = &self.kind
            && minecart.tick(&self.vehicle.entity)
        {
            return;
        }

        self.apply_driver_input(&rail);
        let velocity = self.follow_rail(&rail);

        if velocity.length() > 0.001 && !self.move_and_slow_down(caller, &world, &rail, velocity) {
            return;
        }

        if let MinecartKind::Hopper(minecart) = &self.kind {
            minecart.tick(&self.vehicle.entity);
        }
    }

    fn get_entity(&self) -> &Entity {
        &self.vehicle.entity
    }

    fn get_living_entity(&self) -> Option<&LivingEntity> {
        None
    }

    fn is_pushable(&self) -> bool {
        true
    }

    #[allow(clippy::too_many_lines)]
    fn push(&self, entity: &dyn EntityBase) {
        let self_entity = self.get_entity();
        let other_entity = entity.get_entity();

        if self_entity.no_physics.load(Ordering::Relaxed)
            || other_entity.no_physics.load(Ordering::Relaxed)
        {
            return;
        }

        {
            let passengers = self_entity
                .passengers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if passengers
                .iter()
                .any(|p| p.get_entity().entity_id == other_entity.entity_id)
            {
                return;
            }
        }
        {
            let passengers = other_entity
                .passengers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if passengers
                .iter()
                .any(|p| p.get_entity().entity_id == self_entity.entity_id)
            {
                return;
            }
        }

        let mut xa = other_entity.pos.load().x - self_entity.pos.load().x;
        let mut za = other_entity.pos.load().z - self_entity.pos.load().z;
        let mut dd = xa * xa + za * za;
        if dd >= 1.0E-4 {
            dd = dd.sqrt();
            xa /= dd;
            za /= dd;
            let mut pow = 1.0 / dd;
            if pow > 1.0 {
                pow = 1.0;
            }
            xa *= pow;
            za *= pow;
            xa *= 0.1;
            za *= 0.1;
            xa *= 0.5;
            za *= 0.5;

            let is_other_minecart = other_entity.entity_type.id == EntityType::MINECART.id
                || other_entity.entity_type.id == EntityType::CHEST_MINECART.id
                || other_entity.entity_type.id == EntityType::COMMAND_BLOCK_MINECART.id
                || other_entity.entity_type.id == EntityType::FURNACE_MINECART.id
                || other_entity.entity_type.id == EntityType::HOPPER_MINECART.id
                || other_entity.entity_type.id == EntityType::SPAWNER_MINECART.id
                || other_entity.entity_type.id == EntityType::TNT_MINECART.id;

            if is_other_minecart {
                let xo = self_entity.velocity.load().x;
                let zo = self_entity.velocity.load().z;

                let dir = Vector3::new(xo, 0.0, zo).normalize();
                let facing = Vector3::new(
                    f64::from(self_entity.yaw.load().to_radians().cos()),
                    0.0,
                    f64::from(self_entity.yaw.load().to_radians().sin()),
                )
                .normalize();

                let dot = dir.dot(&facing).abs();
                if dot >= 0.8 {
                    let vel = self_entity.velocity.load();
                    let ovel = other_entity.velocity.load();

                    let is_self_furnace =
                        self_entity.entity_type.id == EntityType::FURNACE_MINECART.id;
                    let is_other_furnace =
                        other_entity.entity_type.id == EntityType::FURNACE_MINECART.id;

                    if is_other_furnace && !is_self_furnace {
                        self_entity.velocity.store(vel.multiply(0.2, 1.0, 0.2));
                        let mut new_self_vel = self_entity.velocity.load();
                        new_self_vel.x += ovel.x - xa;
                        new_self_vel.z += ovel.z - za;
                        self_entity.velocity.store(new_self_vel);
                        self_entity.send_velocity();

                        other_entity.velocity.store(ovel.multiply(0.95, 1.0, 0.95));
                        other_entity.send_velocity();
                    } else if !is_other_furnace && is_self_furnace {
                        other_entity.velocity.store(ovel.multiply(0.2, 1.0, 0.2));
                        let mut new_other_vel = other_entity.velocity.load();
                        new_other_vel.x += vel.x + xa;
                        new_other_vel.z += vel.z + za;
                        other_entity.velocity.store(new_other_vel);
                        other_entity.send_velocity();

                        self_entity.velocity.store(vel.multiply(0.95, 1.0, 0.95));
                        self_entity.send_velocity();
                    } else {
                        #[allow(clippy::manual_midpoint)]
                        let xdd = (ovel.x + vel.x) / 2.0;
                        #[allow(clippy::manual_midpoint)]
                        let zdd = (ovel.z + vel.z) / 2.0;

                        self_entity.velocity.store(vel.multiply(0.2, 1.0, 0.2));
                        let mut new_self_vel = self_entity.velocity.load();
                        new_self_vel.x += xdd - xa;
                        new_self_vel.z += zdd - za;
                        self_entity.velocity.store(new_self_vel);
                        self_entity.send_velocity();

                        other_entity.velocity.store(ovel.multiply(0.2, 1.0, 0.2));
                        let mut new_other_vel = other_entity.velocity.load();
                        new_other_vel.x += xdd + xa;
                        new_other_vel.z += zdd + za;
                        other_entity.velocity.store(new_other_vel);
                        other_entity.send_velocity();
                    }
                }
            } else {
                if !self_entity.has_passengers() && self.is_pushable() {
                    let mut vel = self_entity.velocity.load();
                    vel.x -= xa;
                    vel.z -= za;
                    self_entity.velocity.store(vel);
                    self_entity.send_velocity();
                }

                if !other_entity.has_passengers() && entity.is_pushable() {
                    let mut vel = other_entity.velocity.load();
                    vel.x += xa / 4.0;
                    vel.z += za / 4.0;
                    other_entity.velocity.store(vel);
                    other_entity.send_velocity();
                }
            }
        }
    }

    fn is_collidable(&self, _entity: Option<Box<dyn EntityBase>>) -> bool {
        true
    }

    fn init_data_tracker(&self) {
        self.vehicle.send_wobble_metadata();
        if let MinecartKind::Furnace(minecart) = &self.kind {
            minecart.init_data_tracker(&self.vehicle.entity);
        }
    }

    fn can_hit(&self) -> bool {
        self.vehicle.entity.is_alive()
    }

    fn damage_with_context(
        &self,
        _caller: &dyn EntityBase,
        amount: f32,
        damage_type: DamageType,
        _position: Option<Vector3<f64>>,
        source: Option<&dyn EntityBase>,
        cause: Option<&dyn EntityBase>,
    ) -> bool {
        let creative = source
            .and_then(EntityBase::get_player)
            .is_some_and(|player| player.gamemode.load() == GameMode::Creative);

        if let MinecartKind::Tnt(minecart) = &self.kind
            && damage_type == DamageType::ARROW
            && self.vehicle.entity.fire_ticks.load(Ordering::Relaxed) > 0
        {
            let projectile_speed_squared = cause
                .map(|entity| entity.get_entity().velocity.load().length_squared())
                .unwrap_or_default();
            minecart.explode(&self.vehicle.entity, projectile_speed_squared);
            if self.vehicle.entity.is_removed() {
                return true;
            }
        }

        let will_break = self.vehicle.entity.is_alive()
            && (creative || self.vehicle.get_damage() + amount * 10.0 > 40.0);

        if let MinecartKind::Tnt(minecart) = &self.kind
            && will_break
            && !creative
        {
            let velocity = self.vehicle.entity.velocity.load();
            let speed_squared = velocity.x.mul_add(velocity.x, velocity.z * velocity.z);
            let ignites = damage_type.has_tag(&tag::DamageType::MINECRAFT_IS_FIRE)
                || damage_type.has_tag(&tag::DamageType::MINECRAFT_IS_EXPLOSION)
                || self.vehicle.entity.fire_ticks.load(Ordering::Relaxed) > 0;
            if ignites || speed_squared >= 0.01 {
                self.vehicle.apply_damage_wobble(amount);
                let fuse = rand::rng().random_range(0..20) + rand::rng().random_range(0..20);
                if self
                    .vehicle
                    .entity
                    .world
                    .load()
                    .level_info
                    .load()
                    .game_rules
                    .tnt_explodes
                {
                    minecart.prime(&self.vehicle.entity, fuse);
                } else {
                    minecart.set_fuse(fuse);
                }
                return true;
            }
        }

        let damaged = self.vehicle.damage_with_context(amount, source);

        if will_break && !creative && self.vehicle.entity.is_removed() {
            let world = self.vehicle.entity.world.load();
            if world.level_info.load().game_rules.entity_drops {
                let position = self.vehicle.entity.block_pos.load();
                if let Some(container) = self.container()
                    && container.claim_drops()
                {
                    container.unpack_loot(&crate::world::loot::LootContextParameters {
                        world: Some(&world),
                        position: Some(self.vehicle.entity.pos.load()),
                        ..Default::default()
                    });
                    let inventory: Arc<dyn Inventory> = container.clone();
                    world.scatter_inventory(&position, &inventory);
                }
                if let Some(item) = self.drop_item() {
                    world.drop_stack(&position, ItemStack::new(1, item));
                }
            }
        }

        damaged
    }

    fn interact(&self, player: &Arc<Player>, item_stack: &mut ItemStack) -> bool {
        match &self.kind {
            MinecartKind::Chest(minecart) => {
                minecart.interact(&self.vehicle.entity, player);
                true
            }
            MinecartKind::Furnace(minecart) => {
                minecart.interact(&self.vehicle.entity, player, item_stack)
            }
            MinecartKind::Hopper(minecart) => {
                minecart.interact(&self.vehicle.entity, player);
                true
            }
            MinecartKind::Rideable(_) => RideableMinecart::interact(&self.vehicle.entity, player),
            MinecartKind::Tnt(_) | MinecartKind::Other => false,
        }
    }

    fn on_player_collision(&self, player: &Arc<Player>) {
        if self.vehicle.entity.has_passenger(player.entity_id()) {
            return;
        }

        if player.is_spectator() {
            return;
        }

        let player_pos = player.get_entity().pos.load();
        let minecart_pos = self.vehicle.entity.pos.load();

        let mut diff_x = minecart_pos.x - player_pos.x;
        let mut diff_z = minecart_pos.z - player_pos.z;

        let dist_sq = diff_x * diff_x + diff_z * diff_z;
        if dist_sq > 0.0001 {
            let dist = dist_sq.sqrt();
            diff_x /= dist;
            diff_z /= dist;

            let push_force = 0.1;
            let mut vel = self.vehicle.entity.velocity.load();
            vel.x += diff_x * push_force;
            vel.z += diff_z * push_force;

            let horizontal_speed = vel.x.hypot(vel.z);
            if horizontal_speed > 0.4 {
                vel.x = (vel.x / horizontal_speed) * 0.4;
                vel.z = (vel.z / horizontal_speed) * 0.4;
            }

            self.vehicle.entity.velocity.store(vel);
            self.vehicle.entity.send_velocity();
        }
    }

    fn move_entity(&self, caller: &dyn EntityBase, motion: Vector3<f64>) {
        let to_position = self.vehicle.entity.pos.load().add(&motion);
        self.vehicle.entity.move_entity(caller, motion);
        let entity_id = self.vehicle.entity.entity_id;
        let world = self.vehicle.entity.world.load().clone();
        if let Some(dyn_self) = world.get_entity_by_id(entity_id) {
            let should_continue = dyn_self.push_entities(caller);
            if should_continue {
                let current_pos = dyn_self.get_entity().pos.load();
                let back_motion = to_position.sub(&current_pos);
                dyn_self.get_entity().move_entity(caller, back_motion);
            }
        }
    }

    fn cast_any(&self) -> &dyn std::any::Any {
        self
    }
}
