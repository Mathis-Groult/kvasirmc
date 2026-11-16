//! A record of `compute_wall_state` (`tests/golden/wall_state.txt`; rewrite it on purpose with
//! `KVASIR_BLESS=1`), made before it was split.
//!
//! Each case builds a cobblestone wall's surroundings in its own spot of empty chunks (eight
//! blocks apart) and records the wall state computed there: the side blocks first (each kind of
//! neighbour in every side pattern, nothing above), then each kind of block above for a few side
//! patterns.

use std::fmt::Write;
use std::sync::Arc;

use pumpkin_data::Block;
use pumpkin_data::BlockStateId;
use pumpkin_data::block_properties::{
    HorizontalFacing, OakFenceGateLikeProperties, OakFenceLikeProperties,
    ResinBrickWallLikeProperties,
};
use pumpkin_data::block_properties::{NorthWall, SouthWall};
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_world::world::BlockFlags;

use super::compute_wall_state;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;
use crate::world::World;

/// Which sides get the neighbour: north, south, east, west.
const PATTERNS: [(&str, [bool; 4]); 8] = [
    ("none", [false, false, false, false]),
    ("n", [true, false, false, false]),
    ("ns", [true, true, false, false]),
    ("ew", [false, false, true, true]),
    ("ne", [true, false, true, false]),
    ("nse", [true, true, true, false]),
    ("all", [true, true, true, true]),
    ("sw", [false, true, false, true]),
];

fn gate(facing: HorizontalFacing, open: bool) -> BlockStateId {
    let mut props = OakFenceGateLikeProperties::default(&Block::OAK_FENCE_GATE);
    props.facing = facing;
    props.open = open;
    props.to_state_id(&Block::OAK_FENCE_GATE)
}

/// A fence-like block with arms north, south, east, west.
fn fence([north, south, east, west]: [bool; 4], block: &Block) -> BlockStateId {
    let mut props = OakFenceLikeProperties::default(block);
    props.north = north;
    props.south = south;
    props.east = east;
    props.west = west;
    props.to_state_id(block)
}

fn wall(up: bool, north: NorthWall, south: SouthWall) -> BlockStateId {
    let mut props = ResinBrickWallLikeProperties::default(&Block::COBBLESTONE_WALL);
    props.up = up;
    props.north = north;
    props.south = south;
    props.to_state_id(&Block::COBBLESTONE_WALL)
}

fn side_kinds() -> Vec<(&'static str, BlockStateId)> {
    vec![
        ("air", Block::AIR.default_state.id),
        ("wall", Block::COBBLESTONE_WALL.default_state.id),
        ("other_wall", Block::STONE_BRICK_WALL.default_state.id),
        ("stone", Block::STONE.default_state.id),
        ("glass", Block::GLASS.default_state.id),
        ("fence", Block::OAK_FENCE.default_state.id),
        ("pane", Block::GLASS_PANE.default_state.id),
        ("bars", Block::IRON_BARS.default_state.id),
        ("gate_n", gate(HorizontalFacing::North, false)),
        ("gate_e", gate(HorizontalFacing::East, false)),
        ("gate_e_open", gate(HorizontalFacing::East, true)),
        ("torch", Block::TORCH.default_state.id),
        ("leaves", Block::OAK_LEAVES.default_state.id),
        ("stairs", Block::OAK_STAIRS.default_state.id),
    ]
}

fn above_kinds() -> Vec<(&'static str, BlockStateId)> {
    vec![
        ("air", Block::AIR.default_state.id),
        ("stone", Block::STONE.default_state.id),
        ("wall_post", wall(true, NorthWall::None, SouthWall::None)),
        ("wall_ns_low", wall(false, NorthWall::Low, SouthWall::Low)),
        ("wall_n_tall", wall(false, NorthWall::Tall, SouthWall::None)),
        ("fence", Block::OAK_FENCE.default_state.id),
        (
            "fence_ns",
            fence([true, true, false, false], &Block::OAK_FENCE),
        ),
        (
            "fence_e",
            fence([false, false, true, false], &Block::OAK_FENCE),
        ),
        (
            "pane_w",
            fence([false, false, false, true], &Block::GLASS_PANE),
        ),
        (
            "bars_ns",
            fence([true, true, false, false], &Block::IRON_BARS),
        ),
        ("gate_n", gate(HorizontalFacing::North, false)),
        ("gate_n_open", gate(HorizontalFacing::North, true)),
        ("gate_e", gate(HorizontalFacing::East, false)),
        ("torch", Block::TORCH.default_state.id),
        ("sign", Block::OAK_SIGN.default_state.id),
        ("plate", Block::STONE_PRESSURE_PLATE.default_state.id),
        ("glass", Block::GLASS.default_state.id),
        ("carpet", Block::WHITE_CARPET.default_state.id),
        // Appended after the mutation run: single arms, so each side reads its own arm.
        (
            "fence_s",
            fence([false, true, false, false], &Block::OAK_FENCE),
        ),
        (
            "pane_n",
            fence([true, false, false, false], &Block::GLASS_PANE),
        ),
    ]
}

/// The block and its properties.
fn describe(state: BlockStateId) -> String {
    let block = Block::from_state_id(state);
    let props = block
        .properties(state)
        .map(|props| format!("{:?}", props.to_props()))
        .unwrap_or_default();
    format!("{} {props}", block.name)
}

fn put(world: &Arc<World>, pos: &BlockPos, state: BlockStateId) {
    world.set_block_state(
        pos,
        state,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

/// Builds one case at `x` and returns the computed wall state.
fn case(
    world: &Arc<World>,
    x: i32,
    sides: [bool; 4],
    side: BlockStateId,
    above: BlockStateId,
) -> String {
    let pos = BlockPos::new(x, 64, 0);
    let offsets = [(0, -1), (0, 1), (1, 0), (-1, 0)];
    for (on, (dx, dz)) in sides.iter().zip(offsets) {
        if *on {
            put(world, &BlockPos::new(x + dx, 64, dz), side);
        }
    }
    put(world, &pos.up(), above);
    let props = ResinBrickWallLikeProperties::default(&Block::COBBLESTONE_WALL);
    describe(compute_wall_state(
        props,
        world,
        &Block::COBBLESTONE_WALL,
        &pos,
    ))
}

#[tokio::test(flavor = "multi_thread")]
async fn wall_state_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    for cx in -1..=200 {
        for cz in -1..=0 {
            world.level.loaded_chunks.insert(
                Vector2::new(cx, cz),
                pumpkin_world::chunk::ChunkData::empty_sync(cx, cz),
            );
        }
    }

    let mut now = String::new();
    let mut x = 0;
    for (side_name, side) in side_kinds() {
        for (pattern, sides) in PATTERNS {
            let state = case(&world, x, sides, side, Block::AIR.default_state.id);
            writeln!(now, "{side_name} {pattern} / air -> {state}")?;
            x += 8;
        }
    }
    for (above_name, above) in above_kinds() {
        for (pattern, sides) in [
            PATTERNS[0],
            PATTERNS[2],
            PATTERNS[3],
            PATTERNS[4],
            PATTERNS[6],
        ] {
            let wall_side = Block::COBBLESTONE_WALL.default_state.id;
            let state = case(&world, x, sides, wall_side, above);
            writeln!(now, "wall {pattern} / {above_name} -> {state}")?;
            x += 8;
        }
    }

    // Appended after the split's mutation review: side patterns the table above lacks (a single
    // west or south side, three sides without south or east), so each `&&` of the
    // straight-line and cross checks matters.
    let extra = [
        ("w", [false, false, false, true]),
        ("s", [false, true, false, false]),
        ("new", [true, false, true, true]),
        ("nsw", [true, true, false, true]),
        ("sew", [false, true, true, true]),
    ];
    for (pattern, sides) in extra {
        let wall_side = Block::COBBLESTONE_WALL.default_state.id;
        for (above_name, above) in [
            ("air", Block::AIR.default_state.id),
            ("torch", Block::TORCH.default_state.id),
            ("stone", Block::STONE.default_state.id),
        ] {
            let state = case(&world, x, sides, wall_side, above);
            writeln!(now, "wall {pattern} / {above_name} -> {state}")?;
            x += 8;
        }
    }

    script_golden::bless_or_compare("wall_state.txt", &now)?;
    server.stop();
    Ok(())
}
