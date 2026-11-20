use pumpkin_data::HorizontalFacingExt;
use pumpkin_data::block_properties::{AcaciaShelfLikeProperties, SideChainPart};
use pumpkin_data::fluid::Fluid;
use pumpkin_data::game_event::GameEvent;
use pumpkin_data::sound::{Sound, SoundCategory};
use pumpkin_data::{BlockState, BlockStateId};
use pumpkin_inventory::Inventory;
use pumpkin_macros::pumpkin_block_from_tag;
use pumpkin_world::tick::TickPriority;
use pumpkin_world::world::BlockFlags;

use crate::block::blocks::redstone::block_receives_redstone_power;
use crate::block::entities::shelf::ShelfBlockEntity;
use crate::block::{
    BlockActionResult, BlockBehaviour, GetComparatorOutputArgs, GetStateForNeighborUpdateArgs,
    OnNeighborUpdateArgs, OnPlaceArgs, OnStateReplacedArgs, PathComputationType, PlacedArgs,
    UseWithItemArgs,
};
use crate::entity::EntityBase;
use std::sync::Arc;

mod chain;
#[cfg(test)]
mod tests;
mod use_item;
#[cfg(test)]
mod use_tests;

#[pumpkin_block_from_tag("minecraft:wooden_shelves")]
pub struct ShelfBlock;

impl BlockBehaviour for ShelfBlock {
    fn on_place(&self, args: OnPlaceArgs<'_>) -> BlockStateId {
        let mut properties = AcaciaShelfLikeProperties::default(args.block);

        properties.facing = args.player.get_entity().get_horizontal_facing().opposite();
        properties.powered = block_receives_redstone_power(args.world, args.position);
        properties.waterlogged = args.replacing.water_source();

        properties.to_state_id(args.block)
    }

    fn get_state_for_neighbor_update(
        &self,
        args: GetStateForNeighborUpdateArgs<'_>,
    ) -> BlockStateId {
        let props = AcaciaShelfLikeProperties::from_state_id(args.state_id);
        if props.waterlogged {
            args.world.schedule_fluid_tick(
                &Fluid::WATER,
                *args.position,
                Fluid::WATER.flow_speed as u8,
                TickPriority::Normal,
            );
        }
        args.state_id
    }

    fn placed(&self, args: PlacedArgs<'_>) {
        let entity = ShelfBlockEntity::new(*args.position);
        args.world.add_block_entity(Arc::new(entity));
        // Vanilla `ShelfBlock.onPlace`.
        if AcaciaShelfLikeProperties::from_state_id(args.state_id).powered {
            chain::power_up(args.world, *args.position, args.state_id, args.old_state_id);
        } else {
            chain::power_down(args.world, *args.position, args.state_id);
        }
    }

    /// Vanilla `ShelfBlock.useItemOn`: a click on the front swaps the held stack with the shelf
    /// slot, or, on a powered shelf, the hotbar with the whole chain.
    fn use_with_item(&self, args: UseWithItemArgs<'_>) -> BlockActionResult {
        use_item::use_with_item(args)
    }

    /// Vanilla `ShelfBlock.neighborChanged`: a shelf follows the redstone signal around it, drops
    /// its chain links when it loses power, and clicks.
    fn on_neighbor_update(&self, args: OnNeighborUpdateArgs<'_>) {
        let (block, state_id) = args.world.get_block_and_state_id(args.position);
        let mut properties = AcaciaShelfLikeProperties::from_state_id(state_id);
        let powered = block_receives_redstone_power(args.world, args.position);
        if properties.powered == powered {
            return;
        }
        properties.powered = powered;
        if !powered {
            properties.side_chain = SideChainPart::Unconnected;
        }
        let new_state_id = properties.to_state_id(block);
        args.world
            .set_block_state(args.position, new_state_id, BlockFlags::NOTIFY_ALL);
        // `onPlace` of the new state; Pumpkin runs `placed` only when the block type changes.
        if powered {
            chain::power_up(args.world, *args.position, new_state_id, state_id);
        } else {
            chain::power_down(args.world, *args.position, new_state_id);
        }
        let (sound, event) = if powered {
            (Sound::BlockShelfActivate, GameEvent::BlockActivate)
        } else {
            (Sound::BlockShelfDeactivate, GameEvent::BlockDeactivate)
        };
        args.world
            .play_block_sound(sound, SoundCategory::Blocks, *args.position);
        args.world
            .emit_game_event(event.name(), args.position.to_centered_f64());
    }

    /// Vanilla `ShelfBlock.affectNeighborsAfterRemoval`: the neighbours drop their links.
    fn on_state_replaced(&self, args: OnStateReplacedArgs<'_>) {
        chain::power_down(args.world, *args.position, args.old_state_id);
    }

    fn get_comparator_output(&self, args: GetComparatorOutputArgs<'_>) -> Option<u8> {
        // Vanilla reads a shelf only through its back face.
        let props = AcaciaShelfLikeProperties::from_state_id(args.state.id);
        if args.direction != props.facing.to_block_direction().opposite() {
            return Some(0);
        }

        if let Some(block_entity) = args.world.get_block_entity(args.position)
            && let Some(shelf) = block_entity.as_any().downcast_ref::<ShelfBlockEntity>()
        {
            let b0 = u8::from(!shelf.get_stack(0).is_empty());
            let b1 = u8::from(!shelf.get_stack(1).is_empty());
            let b2 = u8::from(!shelf.get_stack(2).is_empty());
            Some(b0 | (b1 << 1) | (b2 << 2))
        } else {
            None
        }
    }

    fn is_pathfindable(&self, state: &BlockState, computation_type: PathComputationType) -> bool {
        computation_type == PathComputationType::Water && state.is_waterlogged()
    }
}
