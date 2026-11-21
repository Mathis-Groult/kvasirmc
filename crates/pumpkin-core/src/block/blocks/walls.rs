use crate::block::{
    BlockBehaviour, GetStateForNeighborUpdateArgs, OnPlaceArgs, PathComputationType,
    is_exception_for_connection,
};
use crate::world::World;
use pumpkin_data::block_properties::EastWall;
use pumpkin_data::block_properties::HorizontalFacing;
use pumpkin_data::block_properties::NorthWall;
use pumpkin_data::block_properties::SouthWall;
use pumpkin_data::block_properties::WestWall;
use pumpkin_data::fluid::Fluid;
use pumpkin_data::tag::Taggable;
use pumpkin_data::{Block, BlockDirection, BlockState, BlockStateId, tag};
use pumpkin_macros::pumpkin_block_from_tag;
use pumpkin_util::math::position::BlockPos;
use pumpkin_world::tick::TickPriority;

type FenceGateProperties = pumpkin_data::block_properties::OakFenceGateLikeProperties;
type WallProperties = pumpkin_data::block_properties::ResinBrickWallLikeProperties;

mod cover;

#[pumpkin_block_from_tag("minecraft:walls")]
pub struct WallBlock;

impl BlockBehaviour for WallBlock {
    fn on_place(&self, args: OnPlaceArgs<'_>) -> BlockStateId {
        let mut wall_props = WallProperties::default(args.block);
        wall_props.waterlogged = args.replacing.water_source();

        compute_wall_state(wall_props, args.world, args.block, args.position)
    }

    fn get_state_for_neighbor_update(
        &self,
        args: GetStateForNeighborUpdateArgs<'_>,
    ) -> BlockStateId {
        let wall_props = WallProperties::from_state_id(args.state_id);
        if wall_props.waterlogged {
            args.world.schedule_fluid_tick(
                &Fluid::WATER,
                *args.position,
                Fluid::WATER.flow_speed as u8,
                TickPriority::Normal,
            );
        }
        compute_wall_state(wall_props, args.world, args.block, args.position)
    }

    fn is_pathfindable(&self, _state: &BlockState, _computation_type: PathComputationType) -> bool {
        false
    }
}

pub fn compute_wall_state(
    mut wall_props: WallProperties,
    world: &World,
    block: &Block,
    block_pos: &BlockPos,
) -> BlockStateId {
    let above_pos = block_pos.up();
    let (block_above, block_above_state) = world.get_block_and_state(&above_pos);
    let face = cover::bottom_face(block_above_state, &above_pos);

    for direction in HorizontalFacing::all() {
        let other_block_pos = block_pos.offset(direction.to_offset());
        let (other_block, other_block_state) = world.get_block_and_state(&other_block_pos);

        let connected = is_connected(block, direction, other_block, other_block_state);

        let shape = if connected {
            let raise = cover::covers_side(&face, direction);
            if raise {
                WallShape::Tall
            } else {
                WallShape::Low
            }
        } else {
            WallShape::None
        };

        match direction {
            HorizontalFacing::North => wall_props.north = shape.into(),
            HorizontalFacing::South => wall_props.south = shape.into(),
            HorizontalFacing::East => wall_props.east = shape.into(),
            HorizontalFacing::West => wall_props.west = shape.into(),
        }
    }

    wall_props.up = post_is_raised(wall_props, block_above, block_above_state, &face);
    wall_props.to_state_id(block)
}

/// `WallBlock.shouldRaisePost`: whether the wall shows its post, from its sides and the block above.
fn post_is_raised(
    wall_props: WallProperties,
    block_above: &Block,
    block_above_state: &BlockState,
    face: &[cover::Footprint],
) -> bool {
    if block_above.has_tag(&tag::Block::MINECRAFT_WALLS)
        && WallProperties::from_state_id(block_above_state.id).up
    {
        return true;
    }
    let (north, south) = (
        wall_props.north == NorthWall::None,
        wall_props.south == SouthWall::None,
    );
    let (east, west) = (
        wall_props.east == EastWall::None,
        wall_props.west == WestWall::None,
    );
    // All four sides empty, or the north/south or east/west pair differs.
    if north != south || east != west || (north && east) {
        return true;
    }
    let tall_pair = (wall_props.north == NorthWall::Tall && wall_props.south == SouthWall::Tall)
        || (wall_props.east == EastWall::Tall && wall_props.west == WestWall::Tall);
    if tall_pair {
        return false;
    }
    block_above.has_tag(&tag::Block::MINECRAFT_WALL_POST_OVERRIDE) || cover::covers_post(face)
}

fn is_connected(
    block: &Block,
    direction: HorizontalFacing,
    other_block: &Block,
    other_block_state: &BlockState,
) -> bool {
    let not_exception = !is_exception_for_connection(other_block);
    let mut connected = other_block == block
        || (not_exception
            && ((other_block_state.is_solid() && other_block_state.is_full_cube())
                || other_block_state.is_side_solid(BlockDirection::from_cardinal_direction(
                    direction.opposite(),
                ))))
        || other_block.has_tag(&tag::Block::MINECRAFT_WALLS)
        || other_block.has_tag(&tag::Block::MINECRAFT_BARS)
        || other_block.has_tag(&tag::Block::C_GLASS_PANES);

    // fence gates do not pass is_side_solid check
    if !connected && other_block.has_tag(&tag::Block::MINECRAFT_FENCE_GATES) {
        let fence_props = FenceGateProperties::from_state_id(other_block_state.id);
        if fence_props.facing == direction.rotate_clockwise()
            || fence_props.facing == direction.rotate_counter_clockwise()
        {
            connected = true;
        }
    }
    connected
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WallShape {
    None,
    Low,
    Tall,
}

impl From<WallShape> for NorthWall {
    fn from(value: WallShape) -> Self {
        match value {
            WallShape::None => Self::None,
            WallShape::Low => Self::Low,
            WallShape::Tall => Self::Tall,
        }
    }
}

impl From<WallShape> for SouthWall {
    fn from(value: WallShape) -> Self {
        match value {
            WallShape::None => Self::None,
            WallShape::Low => Self::Low,
            WallShape::Tall => Self::Tall,
        }
    }
}

impl From<WallShape> for EastWall {
    fn from(value: WallShape) -> Self {
        match value {
            WallShape::None => Self::None,
            WallShape::Low => Self::Low,
            WallShape::Tall => Self::Tall,
        }
    }
}

impl From<WallShape> for WestWall {
    fn from(value: WallShape) -> Self {
        match value {
            WallShape::None => Self::None,
            WallShape::Low => Self::Low,
            WallShape::Tall => Self::Tall,
        }
    }
}

#[cfg(test)]
mod golden;
