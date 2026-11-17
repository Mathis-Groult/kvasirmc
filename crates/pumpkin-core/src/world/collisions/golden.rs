//! A record of what `World::raycast` returns, made before the function was split
//! (`tests/golden/raycast.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! One loaded area of a real server's world holds a fixed layout of full blocks, partial
//! blocks (bottom slab, fence post, stairs, carpet) and `cave_air` (not `air` for the hit check,
//! but without an outline shape). Rays come from fixed lists (axis-aligned, through block edges
//! and corners, starting inside a block, too short to reach anything, zero length) and from a
//! fixed linear congruential generator; each is cast with the debug stick's hit check (any
//! block but air) and with a check that accepts only stone. One line per ray: the hit block and
//! face, or `none`.

use std::fmt::Write;
use std::sync::Arc;

use pumpkin_data::{Block, BlockStateId};
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::world::BlockFlags;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;
use crate::world::World;

/// The area's corner; every block of the layout is placed relative to it.
const AREA: (i32, i32, i32) = (0, 64, 0);

fn put(world: &Arc<World>, pos: &BlockPos, state: BlockStateId) {
    world.set_block_state(
        pos,
        state,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

/// The layout: (x, y, z) offsets from [`AREA`] and the block placed there.
fn layout() -> Vec<((i32, i32, i32), BlockStateId)> {
    let stone = Block::STONE.default_state.id;
    let mut cells = Vec::new();
    for x in 0..16 {
        for z in 0..16 {
            cells.push(((x, 0, z), stone));
        }
    }
    for y in 1..6 {
        for z in 0..16 {
            cells.push(((12, y, z), stone));
        }
    }
    cells.extend([
        ((4, 1, 4), stone),
        ((5, 2, 4), stone),
        ((4, 3, 7), Block::OAK_SLAB.default_state.id),
        ((6, 1, 9), Block::OAK_FENCE.default_state.id),
        ((8, 1, 3), Block::OAK_STAIRS.default_state.id),
        ((3, 1, 11), Block::WHITE_CARPET.default_state.id),
        ((9, 2, 9), Block::CAVE_AIR.default_state.id),
        ((2, 4, 2), stone),
        ((7, 5, 12), Block::OAK_SLAB.default_state.id),
    ]);
    cells
}

fn at(x: f64, y: f64, z: f64) -> Vector3<f64> {
    Vector3::new(
        f64::from(AREA.0) + x,
        f64::from(AREA.1) + y,
        f64::from(AREA.2) + z,
    )
}

/// A point in area coordinates.
type Point = (f64, f64, f64);

/// A ray: start and end.
type Ray = (Point, Point);

/// Hand-picked rays: (start, end) in area coordinates.
const RAYS: &[Ray] = &[
    ((4.5, 3.5, 4.5), (4.5, 0.5, 4.5)),
    ((4.5, 0.5, 4.5), (4.5, 3.5, 4.5)),
    ((0.5, 1.5, 4.5), (11.0, 1.5, 4.5)),
    ((11.5, 1.5, 4.5), (0.5, 1.5, 4.5)),
    ((4.5, 1.5, 0.5), (4.5, 1.5, 15.5)),
    ((4.5, 1.5, 15.5), (4.5, 1.5, 0.5)),
    ((1.0, 2.0, 1.0), (14.0, 2.0, 14.0)),
    ((1.0, 1.0, 1.0), (14.0, 14.0, 14.0)),
    ((0.0, 3.0, 0.0), (6.0, 0.0, 6.0)),
    ((4.5, 1.5, 4.5), (4.6, 1.5, 4.5)),
    ((4.5, 1.5, 4.5), (4.5, 1.5, 4.5)),
    ((2.5, 1.5, 2.5), (2.5, 1.5, 2.6)),
    ((4.5, 5.0, 7.5), (4.5, 1.2, 7.5)),
    ((4.5, 3.75, 7.5), (4.5, 3.25, 7.5)),
    ((0.5, 3.75, 7.5), (8.5, 3.75, 7.5)),
    ((0.5, 3.25, 7.5), (8.5, 3.25, 7.5)),
    ((6.5, 1.5, 6.5), (6.5, 1.5, 12.5)),
    ((5.0, 1.5, 9.5), (8.0, 1.5, 9.5)),
    ((5.5, 1.5, 9.1), (7.5, 1.5, 9.1)),
    ((8.5, 3.0, 3.5), (8.5, 1.0, 3.5)),
    ((8.5, 1.75, 1.0), (8.5, 1.75, 6.0)),
    ((8.5, 1.25, 1.0), (8.5, 1.25, 6.0)),
    ((3.5, 2.0, 11.5), (3.5, 1.0, 11.5)),
    ((0.5, 1.03, 11.5), (6.5, 1.03, 11.5)),
    ((9.5, 4.0, 9.5), (9.5, 1.5, 9.5)),
    ((6.0, 2.5, 9.5), (13.0, 2.5, 9.5)),
    ((9.5, 2.5, 6.0), (9.5, 2.5, 13.0)),
    ((2.5, 6.0, 2.5), (2.5, 3.0, 2.5)),
    ((0.0, 4.0, 0.0), (3.0, 4.0, 3.0)),
    ((3.0, 4.0, 3.0), (0.0, 4.0, 0.0)),
    ((5.0, 3.0, 5.0), (2.0, 4.5, 2.0)),
    ((1.5, 2.5, 1.5), (13.5, 2.5, 1.5)),
    ((14.5, 2.5, 1.5), (5.5, 2.5, 1.5)),
    ((7.5, 6.0, 12.5), (7.5, 5.0, 12.5)),
    ((7.5, 5.4, 10.0), (7.5, 5.4, 15.0)),
    ((7.5, 5.6, 10.0), (7.5, 5.6, 15.0)),
    ((3.0, 2.0, 3.0), (5.0, 2.0, 5.0)),
    ((4.0, 1.5, 3.0), (4.0, 1.5, 6.0)),
    ((5.0, 2.0, 5.0), (3.0, 0.0, 3.0)),
    ((0.5, 0.5, 0.5), (15.5, 0.5, 15.5)),
    // Appended after the first mutation run: rays starting in cave_air on a block border of an
    // axis they do not move along.
    ((9.0, 2.5, 9.5), (9.0, 2.5, 12.0)),
    ((9.5, 2.0, 9.5), (9.5, 2.0, 12.0)),
    ((9.5, 2.5, 9.0), (12.0, 2.5, 9.0)),
    // Rays entering cave_air (no outline: the face comes from the walk) from the other sides.
    ((11.5, 2.5, 9.5), (8.0, 2.5, 9.5)),
    ((9.5, 2.5, 11.5), (9.5, 2.5, 8.0)),
    ((9.5, 1.5, 9.5), (9.5, 3.5, 9.5)),
    // Rays through a block corner exactly (ties between two axes), next to stone (4, 1, 4).
    ((3.5, 2.5, 4.5), (5.5, 0.5, 4.5)),
    ((5.5, 2.5, 4.5), (3.5, 0.5, 4.5)),
    ((4.5, 2.5, 3.5), (4.5, 0.5, 5.5)),
    ((4.5, 2.5, 5.5), (4.5, 0.5, 3.5)),
    ((3.5, 1.5, 3.5), (5.5, 1.5, 5.5)),
    ((5.5, 1.5, 5.5), (3.5, 1.5, 3.5)),
    ((3.5, 1.5, 5.5), (5.5, 1.5, 3.5)),
    ((5.5, 1.5, 3.5), (3.5, 1.5, 5.5)),
];

/// A fixed pseudo-random ray inside the area.
fn generated(seed: &mut u64) -> Ray {
    let mut next = || {
        *seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (*seed >> 40) as f64 / f64::from(1u32 << 24)
    };
    let mut point = |height: f64| (next() * 16.0, 0.5 + next() * height, next() * 16.0);
    (point(6.0), point(6.0))
}

fn hit_text(hit: Option<(BlockPos, pumpkin_data::BlockDirection)>) -> String {
    hit.map_or_else(
        || "none".to_string(),
        |(pos, face)| {
            format!(
                "({}, {}, {}) {face:?}",
                pos.0.x - AREA.0,
                pos.0.y - AREA.1,
                pos.0.z - AREA.2
            )
        },
    )
}

fn cast(out: &mut String, world: &Arc<World>, label: &str, ray: Ray) {
    let ((sx, sy, sz), (ex, ey, ez)) = ray;
    let start = at(sx, sy, sz);
    let end = at(ex, ey, ez);
    let any = world.raycast(start, end, |pos, world| world.get_block(pos) != &Block::AIR);
    let stone = world.raycast(start, end, |pos, world| {
        world.get_block(pos) == &Block::STONE
    });
    let _ = writeln!(
        out,
        "{label} ({sx:.3}, {sy:.3}, {sz:.3}) -> ({ex:.3}, {ey:.3}, {ez:.3}): any {} stone {}",
        hit_text(any),
        hit_text(stone)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn raycast_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    for cx in -1..=1 {
        for cz in -1..=1 {
            world.level.loaded_chunks.insert(
                Vector2::new(cx, cz),
                pumpkin_world::chunk::ChunkData::empty_sync(cx, cz),
            );
        }
    }
    for ((x, y, z), state) in layout() {
        put(
            &world,
            &BlockPos::new(AREA.0 + x, AREA.1 + y, AREA.2 + z),
            state,
        );
    }
    let mut out = String::new();
    for (index, ray) in RAYS.iter().enumerate() {
        cast(&mut out, &world, &format!("fixed {index}"), *ray);
    }
    let mut seed = 7u64;
    for index in 0..200 {
        cast(
            &mut out,
            &world,
            &format!("lcg {index}"),
            generated(&mut seed),
        );
    }
    server.stop();
    script_golden::bless_or_compare("raycast.txt", &out)
}
