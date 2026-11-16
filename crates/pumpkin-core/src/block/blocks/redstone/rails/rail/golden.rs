//! A record of `compute_curved_rail_shape` (`tests/golden/rail_shape.txt`; rewrite it on purpose
//! with `KVASIR_BLESS=1`), made before it was split.
//!
//! Each case puts rails around one spot of empty chunks (eight blocks apart) and records the
//! shape computed there: every subset of flat neighbours for both default shapes, with and
//! without a signal; then neighbours one block up (slopes), one block down, a locked neighbour and
//! neighbours already pointing elsewhere.

use std::fmt::Write;
use std::sync::Arc;

use pumpkin_data::Block;
use pumpkin_data::BlockStateId;
use pumpkin_data::block_properties::{RailLikeProperties, RailShape};
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_world::world::BlockFlags;

use super::compute_curved_rail_shape;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;
use crate::world::World;

/// North, south, east, west.
const OFFSETS: [(i32, i32); 4] = [(0, -1), (0, 1), (1, 0), (-1, 0)];
const SIDE_NAMES: [&str; 4] = ["n", "s", "e", "w"];

fn rail(shape: RailShape) -> BlockStateId {
    let mut props = RailLikeProperties::default(&Block::RAIL);
    props.shape = shape;
    props.to_state_id(&Block::RAIL)
}

fn put(world: &Arc<World>, pos: &BlockPos, state: BlockStateId) {
    world.set_block_state(
        pos,
        state,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

/// The shape pointing back at the centre from side `side`.
const fn toward_centre(side: usize) -> RailShape {
    if side < 2 {
        RailShape::NorthSouth
    } else {
        RailShape::EastWest
    }
}

/// One neighbour: which side, its height offset and its shape.
type Neighbour = (usize, i32, RailShape);

fn case(
    world: &Arc<World>,
    x: i32,
    neighbours: &[Neighbour],
    default_flat: RailShape,
    has_signal: bool,
) -> RailShape {
    for (side, dy, shape) in neighbours {
        let (dx, dz) = OFFSETS[*side];
        put(world, &BlockPos::new(x + dx, 64 + dy, dz), rail(*shape));
    }
    compute_curved_rail_shape(world, &BlockPos::new(x, 64, 0), default_flat, has_signal)
}

fn name(neighbours: &[Neighbour]) -> String {
    if neighbours.is_empty() {
        return "none".to_string();
    }
    neighbours
        .iter()
        .map(|(side, dy, shape)| format!("{}{dy:+}:{shape:?}", SIDE_NAMES[*side]))
        .collect::<Vec<_>>()
        .join(",")
}

/// Hand-made layouts: slopes, rails below, a locked neighbour, neighbours pointing elsewhere.
fn special_cases() -> Vec<Vec<Neighbour>> {
    vec![
        vec![(0, 1, RailShape::NorthSouth)],
        vec![(1, 1, RailShape::NorthSouth)],
        vec![(0, 1, RailShape::NorthSouth), (1, 0, RailShape::NorthSouth)],
        vec![(0, 0, RailShape::NorthSouth), (1, 1, RailShape::NorthSouth)],
        vec![(0, 1, RailShape::NorthSouth), (1, 1, RailShape::NorthSouth)],
        vec![(2, 1, RailShape::EastWest)],
        vec![(3, 1, RailShape::EastWest)],
        vec![(2, 1, RailShape::EastWest), (3, 1, RailShape::EastWest)],
        vec![(2, 0, RailShape::EastWest), (3, 1, RailShape::EastWest)],
        vec![(0, -1, RailShape::NorthSouth)],
        vec![(2, -1, RailShape::EastWest), (1, 0, RailShape::NorthSouth)],
        vec![(0, 1, RailShape::NorthSouth), (2, 0, RailShape::EastWest)],
        vec![(2, 0, RailShape::NorthSouth)],
        vec![(0, 0, RailShape::EastWest), (1, 0, RailShape::EastWest)],
        vec![(0, 0, RailShape::SouthEast), (2, 0, RailShape::NorthWest)],
        vec![(0, 0, RailShape::AscendingNorth)],
    ]
}

#[tokio::test(flavor = "multi_thread")]
async fn rail_shape_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    for cx in -1..=80 {
        for cz in -1..=0 {
            world.level.loaded_chunks.insert(
                Vector2::new(cx, cz),
                pumpkin_world::chunk::ChunkData::empty_sync(cx, cz),
            );
        }
    }

    let mut layouts: Vec<Vec<Neighbour>> = (0..16)
        .map(|mask: usize| {
            (0..4)
                .filter(|side| mask & (1 << side) != 0)
                .map(|side| (side, 0, toward_centre(side)))
                .collect()
        })
        .collect();
    layouts.extend(special_cases());

    let mut now = String::new();
    let mut x = 0;
    for layout in &layouts {
        for default_flat in [RailShape::NorthSouth, RailShape::EastWest] {
            for has_signal in [false, true] {
                let shape = case(&world, x, layout, default_flat, has_signal);
                writeln!(
                    now,
                    "{} default {default_flat:?} signal {has_signal} -> {shape:?}",
                    name(layout)
                )?;
                x += 8;
            }
        }
    }

    // A locked east neighbour: a north-south rail with rails north and south pointing back.
    for default_flat in [RailShape::NorthSouth, RailShape::EastWest] {
        put(
            &world,
            &BlockPos::new(x + 1, 64, -1),
            rail(RailShape::NorthSouth),
        );
        put(
            &world,
            &BlockPos::new(x + 1, 64, 1),
            rail(RailShape::NorthSouth),
        );
        let shape = case(
            &world,
            x,
            &[(2, 0, RailShape::NorthSouth)],
            default_flat,
            false,
        );
        writeln!(now, "locked e default {default_flat:?} -> {shape:?}")?;
        x += 8;
    }

    // Appended after the mutation review: curves next to a raised neighbour, so the slope step
    // must only touch straight shapes.
    let curves: [Vec<Neighbour>; 4] = [
        vec![(1, 0, RailShape::NorthSouth), (2, 1, RailShape::EastWest)],
        vec![(0, 0, RailShape::NorthSouth), (3, 1, RailShape::EastWest)],
        vec![(1, 1, RailShape::NorthSouth), (2, 0, RailShape::EastWest)],
        vec![(0, 1, RailShape::NorthSouth), (3, 0, RailShape::EastWest)],
    ];
    for layout in &curves {
        let shape = case(&world, x, layout, RailShape::NorthSouth, false);
        writeln!(
            now,
            "{} default NorthSouth signal false -> {shape:?}",
            name(layout)
        )?;
        x += 8;
    }

    script_golden::bless_or_compare("rail_shape.txt", &now)?;
    server.stop();
    Ok(())
}
