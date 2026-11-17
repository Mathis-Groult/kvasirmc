//! Entity pushing and minecart pickup, the parts of `EntityBase::push_entities`.

use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

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

/// An empty rideable minecart takes the first entity near it that may ride (not an iron golem,
/// a minecart or a passenger, pushable, off its riding cooldown) as its passenger.
pub(super) fn minecart_pick_up(
    self_entity: &Entity,
    world: &Arc<World>,
    entity_bb: &BoundingBox,
) -> bool {
    let pickup_bb = entity_bb.expand(0.2, 0.0, 0.2);
    let other_entities = world.get_entities_at_box(&pickup_bb);

    for other in other_entities {
        if other.get_entity().entity_id != self_entity.entity_id {
            let other_type = other.get_entity().entity_type.id;
            let is_iron_golem = other_type == EntityType::IRON_GOLEM.id;
            let is_other_minecart = is_minecart_type(other_type);

            if !is_iron_golem
                && !is_other_minecart
                && !other.is_passenger()
                && other.is_pushable()
                && other.get_entity().riding_cooldown.load(Relaxed) == 0
                && let Some(self_arc) = world.get_entity_by_id(self_entity.entity_id)
            {
                self_entity.add_passenger(self_arc, other.clone());
                return true;
            }
        }
    }
    false
}

/// What a minecart's push depends on besides the entities around it.
#[derive(Clone, Copy)]
pub(super) struct MinecartPush {
    pub(super) is_rideable_minecart: bool,
    pub(super) is_vehicle: bool,
}

/// A minecart pushes the entities touching it (rideable: anything pushable that is not riding,
/// plus iron golems and minecarts, or everything when it carries someone; other minecarts:
/// only minecarts that are not their passengers) and, when rideable, the players. Returns
/// whether it pushed anything.
pub(super) fn minecart_push<T: EntityBase + ?Sized>(
    this: &T,
    dyn_self: &dyn EntityBase,
    world: &Arc<World>,
    entity_bb: &BoundingBox,
    push: MinecartPush,
) -> bool {
    let MinecartPush {
        is_rideable_minecart,
        is_vehicle,
    } = push;
    let self_entity = this.get_entity();
    let mut pushed = false;
    let push_bb = entity_bb.expand(1.0e-7, 1.0e-7, 1.0e-7);

    let other_entities = world.get_entities_at_box(&push_bb);
    for other in other_entities {
        if other.get_entity().entity_id != self_entity.entity_id {
            let other_type = other.get_entity().entity_type.id;
            let is_other_minecart = is_minecart_type(other_type);
            let is_iron_golem = other_type == EntityType::IRON_GOLEM.id;

            if is_rideable_minecart {
                if (is_iron_golem
                    || is_other_minecart
                    || is_vehicle
                    || !other.get_entity().has_vehicle())
                    && other.is_pushable()
                {
                    dyn_self.push(other.as_ref());
                    pushed = true;
                }
            } else if !this.has_passenger(other.as_ref())
                && other.is_pushable()
                && is_other_minecart
            {
                dyn_self.push(other.as_ref());
                pushed = true;
            }
        }
    }

    let players = world.get_players_at_box(&push_bb);
    for player in players {
        if player.get_entity().entity_id != self_entity.entity_id && is_rideable_minecart {
            dyn_self.push(player.as_ref());
            pushed = true;
            // Non-rideable minecarts (hoppers, chests) do not push players in vanilla.
        }
    }
    pushed
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
