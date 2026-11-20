//! Entity pushing and minecart pickup, the parts of `EntityBase::push_entities`.

use std::sync::Arc;

use pumpkin_data::entity::EntityType;
use pumpkin_util::math::boundingbox::BoundingBox;

use super::{Entity, EntityBase};
use crate::world::World;

/// Whether `id` is one of the minecart types (vanilla `AbstractMinecart`).
pub(super) const fn is_minecart_type(id: u16) -> bool {
    id == EntityType::MINECART.id
        || id == EntityType::CHEST_MINECART.id
        || id == EntityType::COMMAND_BLOCK_MINECART.id
        || id == EntityType::FURNACE_MINECART.id
        || id == EntityType::HOPPER_MINECART.id
        || id == EntityType::SPAWNER_MINECART.id
        || id == EntityType::TNT_MINECART.id
}

/// What a minecart's push depends on besides the entities around it.
#[derive(Clone, Copy)]
pub(super) struct MinecartPush {
    pub(super) is_rideable_minecart: bool,
    pub(super) is_vehicle: bool,
    /// The squared horizontal speed.
    pub(super) speed_squared: f64,
}

/// Vanilla's `OldMinecartBehavior.pushAndPickupEntities` (the default 26.3 behaviour). A
/// rideable cart moving faster than 0.1 blocks per tick picks up the first entity that may ride
/// and pushes players, iron golems, minecarts, and everything while it carries someone or the
/// entity already rides. Any other cart, or a rideable one at rest, pushes only the minecarts
/// that are not its passengers. Returns whether it picked up a passenger and pushed nobody.
pub(super) fn minecart_push_and_pickup<T: EntityBase + ?Sized>(
    this: &T,
    dyn_self: &dyn EntityBase,
    world: &Arc<World>,
    entity_bb: &BoundingBox,
    push: MinecartPush,
) -> bool {
    let MinecartPush {
        is_rideable_minecart,
        is_vehicle,
        speed_squared,
    } = push;
    let self_entity = this.get_entity();
    let bb = entity_bb.expand(0.2, 0.0, 0.2);
    let mut picked_up = false;
    let mut pushed = false;

    if is_rideable_minecart && speed_squared > 0.01 {
        let others = world.get_entities_at_box(&bb).into_iter().chain(
            world
                .get_players_at_box(&bb)
                .into_iter()
                .map(|p| p as Arc<dyn EntityBase>),
        );
        for other in others {
            let other_entity = other.get_entity();
            if other_entity.entity_id == self_entity.entity_id || !other.is_pushable() {
                continue;
            }
            let other_type = other_entity.entity_type.id;
            if other.get_player().is_some()
                || other_type == EntityType::IRON_GOLEM.id
                || is_minecart_type(other_type)
                || is_vehicle
                || picked_up
                || other.is_passenger()
            {
                dyn_self.push(other.as_ref());
                pushed = true;
            } else if let Some(self_arc) = world.get_entity_by_id(self_entity.entity_id) {
                self_entity.add_passenger(self_arc, other.clone());
                picked_up = true;
            }
        }
    } else {
        for other in world.get_entities_at_box(&bb) {
            if other.get_entity().entity_id != self_entity.entity_id
                && !this.has_passenger(other.as_ref())
                && other.is_pushable()
                && is_minecart_type(other.get_entity().entity_type.id)
            {
                dyn_self.push(other.as_ref());
                pushed = true;
            }
        }
    }
    picked_up && !pushed
}

/// Any other entity pushes every entity and player overlapping it.
pub(super) fn push_overlapping(
    dyn_self: &dyn EntityBase,
    self_entity: &Entity,
    world: &Arc<World>,
    entity_bb: &BoundingBox,
) {
    let other_entities = world.get_entities_at_box(entity_bb);
    for other in other_entities {
        if other.get_entity().entity_id != self_entity.entity_id {
            dyn_self.push(other.as_ref());
        }
    }

    let players = world.get_players_at_box(entity_bb);
    for player in players {
        if player.get_entity().entity_id != self_entity.entity_id {
            dyn_self.push(player.as_ref());
        }
    }
}
