//! What a block of redstone dust reads from the world around it: the power given by everything
//! that is not dust, and the power of the dust it is linked to.

use pumpkin_data::block_properties::RedstoneWireLikeProperties;
use pumpkin_data::{Block, BlockDirection};
use pumpkin_util::math::position::BlockPos;

use crate::world::World;

use super::get_redstone_power_no_dust;

type RedstoneWireProperties = RedstoneWireLikeProperties;

/// The power a dust block at `pos` settles at, judging by its neighbours as they are now.
pub(super) fn calculate_target_strength(world: &World, pos: &BlockPos) -> u8 {
    let block_signal = get_block_signal(world, pos);
    if block_signal == 15 {
        return 15;
    }
    let wire_signal = get_incoming_wire_signal(world, pos);
    block_signal.max(wire_signal)
}

pub(super) fn get_block_signal(world: &World, pos: &BlockPos) -> u8 {
    let mut max_signal = 0;
    for side in BlockDirection::all() {
        let neighbor_pos = pos.offset(side.to_offset());
        let (neighbor_block, neighbor_state) = world.get_block_and_state(&neighbor_pos);
        let signal =
            get_redstone_power_no_dust(neighbor_block, neighbor_state, world, neighbor_pos, side);
        if signal == 15 {
            return 15;
        }
        max_signal = max_signal.max(signal);
    }
    max_signal
}

fn get_incoming_wire_signal(world: &World, pos: &BlockPos) -> u8 {
    let mut max_wire_signal = 0;
    let up_pos = pos.up();
    let is_up_conductor = world.get_block_state(&up_pos).is_solid_block();

    for direction in BlockDirection::horizontal() {
        let neighbor_pos = pos.offset(direction.to_offset());
        let (neighbor_block, neighbor_state) = world.get_block_and_state(&neighbor_pos);

        // Same level
        if neighbor_block == &Block::REDSTONE_WIRE {
            let wire = RedstoneWireProperties::from_state_id(neighbor_state.id);
            max_wire_signal = max_wire_signal.max(wire.power);
        }

        // Wire UP: if pos.up() is not a solid conductor, and neighbor is a solid conductor
        if !is_up_conductor && neighbor_state.is_solid_block() {
            let neighbor_up_pos = neighbor_pos.up();
            let (up_block, up_state) = world.get_block_and_state(&neighbor_up_pos);
            if up_block == &Block::REDSTONE_WIRE {
                let wire = RedstoneWireProperties::from_state_id(up_state.id);
                max_wire_signal = max_wire_signal.max(wire.power);
            }
        } else if !neighbor_state.is_solid_block() {
            // Wire DOWN: if neighbor is not a solid conductor
            let neighbor_down_pos = neighbor_pos.down();
            let (down_block, down_state) = world.get_block_and_state(&neighbor_down_pos);
            if down_block == &Block::REDSTONE_WIRE {
                let wire = RedstoneWireProperties::from_state_id(down_state.id);
                max_wire_signal = max_wire_signal.max(wire.power);
            }
        }
    }

    max_wire_signal.saturating_sub(1)
}
