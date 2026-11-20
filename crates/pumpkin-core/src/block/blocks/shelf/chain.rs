//! Vanilla 26.3 `SideChainPartBlock`: powered shelves of one facing that touch sideways link into
//! a chain of at most three, and each shelf's `side_chain` says where it sits in it.

use std::sync::Arc;

use pumpkin_data::block_properties::{
    AcaciaShelfLikeProperties as ShelfProperties, HorizontalFacing, SideChainPart,
};
use pumpkin_data::tag::{self, Taggable};
use pumpkin_data::{Block, BlockDirection, BlockStateId, HorizontalFacingExt};
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::world::BlockFlags;

use crate::world::World;

/// `ShelfBlock.getMaxChainLength`.
const MAX_CHAIN_LENGTH: i32 = 3;

/// `SideChainPart` rules (`isChainEnd`, `whenConnectedTo*`, `whenDisconnectedFrom*`).
trait PartRules: Sized {
    fn is_connected(&self) -> bool;
    fn is_chain_end(&self) -> bool;
    fn is_connection_towards(&self, side: Self) -> bool;
    fn when_connected_to_the_right(self) -> Self;
    fn when_connected_to_the_left(self) -> Self;
    fn when_disconnected_from_the_right(self) -> Self;
    fn when_disconnected_from_the_left(self) -> Self;
}

impl PartRules for SideChainPart {
    fn is_connected(&self) -> bool {
        *self != Self::Unconnected
    }

    fn is_chain_end(&self) -> bool {
        *self != Self::Center
    }

    fn is_connection_towards(&self, side: Self) -> bool {
        *self == Self::Center || *self == side
    }

    fn when_connected_to_the_right(self) -> Self {
        match self {
            Self::Unconnected | Self::Left => Self::Left,
            Self::Right | Self::Center => Self::Center,
        }
    }

    fn when_connected_to_the_left(self) -> Self {
        match self {
            Self::Unconnected | Self::Right => Self::Right,
            Self::Center | Self::Left => Self::Center,
        }
    }

    fn when_disconnected_from_the_right(self) -> Self {
        match self {
            Self::Unconnected | Self::Left => Self::Unconnected,
            Self::Right | Self::Center => Self::Right,
        }
    }

    fn when_disconnected_from_the_left(self) -> Self {
        match self {
            Self::Unconnected | Self::Right => Self::Unconnected,
            Self::Center | Self::Left => Self::Left,
        }
    }
}

/// What stands beside a shelf: nothing connectable, or a powered shelf of the same facing.
#[derive(Clone, Copy)]
enum Neighbor {
    Empty,
    Shelf { pos: BlockPos, part: SideChainPart },
}

impl Neighbor {
    fn is_unconnectable_or_chain_end(self) -> bool {
        match self {
            Self::Empty => true,
            Self::Shelf { part, .. } => part.is_chain_end(),
        }
    }

    fn connects_towards(self, side: SideChainPart) -> bool {
        match self {
            Self::Empty => false,
            Self::Shelf { part, .. } => part.is_connection_towards(side),
        }
    }

    /// Moves the shelf's part with `next` and writes it, like `SideChainNeighbor.connectTo*`.
    fn update_part(self, world: &Arc<World>, next: fn(SideChainPart) -> SideChainPart) {
        if let Self::Shelf { pos, part } = self {
            set_part(world, pos, next(part));
        }
    }
}

/// The shelves left and right of `center`, as seen from the front (`SideChainPartBlock.Neighbors`).
struct Neighbors<'a> {
    world: &'a Arc<World>,
    facing: HorizontalFacing,
    center: BlockPos,
}

impl Neighbors<'_> {
    /// `facing.getClockWise()` steps.
    fn left(&self, steps: i32) -> Neighbor {
        self.at(self.facing.to_block_direction().rotate_clockwise(), steps)
    }

    /// `facing.getCounterClockWise()` steps.
    fn right(&self, steps: i32) -> Neighbor {
        self.at(
            self.facing.to_block_direction().rotate_counter_clockwise(),
            steps,
        )
    }

    fn at(&self, direction: BlockDirection, steps: i32) -> Neighbor {
        let offset = direction.to_offset();
        let pos = self.center.offset(Vector3::new(
            offset.x * steps,
            offset.y * steps,
            offset.z * steps,
        ));
        let (block, state_id) = self.world.get_block_and_state_id(&pos);
        match shelf_part(block, state_id) {
            Some((facing, part)) if facing == self.facing => Neighbor::Shelf { pos, part },
            _ => Neighbor::Empty,
        }
    }
}

/// `isConnectable` (a powered wooden shelf) with the facing and part of its state.
fn shelf_part(block: &Block, state_id: BlockStateId) -> Option<(HorizontalFacing, SideChainPart)> {
    if !block.has_tag(&tag::Block::MINECRAFT_WOODEN_SHELVES) {
        return None;
    }
    let props = ShelfProperties::from_state_id(state_id);
    props.powered.then_some((props.facing, props.side_chain))
}

const fn neighbors_of(
    world: &Arc<World>,
    pos: BlockPos,
    facing: HorizontalFacing,
) -> Neighbors<'_> {
    Neighbors {
        world,
        facing,
        center: pos,
    }
}

/// `setPart`: writes the shelf's `side_chain` when it differs (`setBlockAndUpdate`).
fn set_part(world: &Arc<World>, pos: BlockPos, part: SideChainPart) {
    let (block, state_id) = world.get_block_and_state_id(&pos);
    let mut props = ShelfProperties::from_state_id(state_id);
    if props.side_chain != part {
        props.side_chain = part;
        world.set_block_state(&pos, props.to_state_id(block), BlockFlags::NOTIFY_ALL);
    }
}

/// `getAllBlocksConnectedTo(..).size()`: the shelf itself plus the neighbours that link toward it.
fn connected_count(world: &Arc<World>, pos: BlockPos) -> i32 {
    let (block, state_id) = world.get_block_and_state_id(&pos);
    let Some((facing, _)) = shelf_part(block, state_id) else {
        return 0;
    };
    let neighbors = neighbors_of(world, pos, facing);
    let mut count = 1;
    count += links_towards(|steps| neighbors.left(steps), SideChainPart::Left);
    count += links_towards(|steps| neighbors.right(steps), SideChainPart::Right);
    count
}

/// `addBlocksConnectingTowards`: walks away from the shelf until a chain end.
fn links_towards(neighbor: impl Fn(i32) -> Neighbor, side: SideChainPart) -> i32 {
    let mut count = 0;
    for steps in 1..MAX_CHAIN_LENGTH {
        let next = neighbor(steps);
        if next.connects_towards(side) {
            count += 1;
        }
        if next.is_unconnectable_or_chain_end() {
            break;
        }
    }
    count
}

/// `canConnect`: a non-empty side chain that still fits the maximum with the shelf's own.
const fn can_connect(side_length: i32, own_length: i32) -> bool {
    side_length > 0 && own_length + side_length <= MAX_CHAIN_LENGTH
}

/// `isBeingUpdatedByNeighbor`: a neighbour's chain update is what changed this shelf.
fn is_being_updated_by_neighbor(state_id: BlockStateId, old_state_id: BlockStateId) -> bool {
    let old_block = Block::from_state_id(old_state_id);
    let now_connected = ShelfProperties::from_state_id(state_id)
        .side_chain
        .is_connected();
    let was_connected =
        shelf_part(old_block, old_state_id).is_some_and(|(_, part)| part.is_connected());
    now_connected || was_connected
}

/// `updateSelfAndNeighborsOnPoweringUp`: a shelf that just got power links to the shelves beside it.
pub(super) fn power_up(
    world: &Arc<World>,
    pos: BlockPos,
    state_id: BlockStateId,
    old_state_id: BlockStateId,
) {
    let block = world.get_block(&pos);
    let Some((facing, _)) = shelf_part(block, state_id) else {
        return;
    };
    if is_being_updated_by_neighbor(state_id, old_state_id) {
        return;
    }
    let neighbors = neighbors_of(world, pos, facing);
    let left = neighbors.left(1);
    let right = neighbors.right(1);
    let left_length = connected_length(world, left);
    let right_length = connected_length(world, right);

    let mut part = SideChainPart::Unconnected;
    let mut length = 1;
    if can_connect(left_length, length) {
        part = part.when_connected_to_the_left();
        left.update_part(world, SideChainPart::when_connected_to_the_right);
        length += left_length;
    }
    if can_connect(right_length, length) {
        part = part.when_connected_to_the_right();
        right.update_part(world, SideChainPart::when_connected_to_the_left);
    }
    set_part(world, pos, part);
}

fn connected_length(world: &Arc<World>, neighbor: Neighbor) -> i32 {
    match neighbor {
        Neighbor::Shelf { pos, .. } => connected_count(world, pos),
        Neighbor::Empty => 0,
    }
}

/// `updateNeighborsAfterPoweringDown`: the shelves beside a shelf that lost power or was removed
/// drop their link to it.
pub(super) fn power_down(world: &Arc<World>, pos: BlockPos, state_id: BlockStateId) {
    let facing = ShelfProperties::from_state_id(state_id).facing;
    let neighbors = neighbors_of(world, pos, facing);
    neighbors
        .left(1)
        .update_part(world, SideChainPart::when_disconnected_from_the_right);
    neighbors
        .right(1)
        .update_part(world, SideChainPart::when_disconnected_from_the_left);
}
