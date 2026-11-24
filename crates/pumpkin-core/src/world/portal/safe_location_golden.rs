//! Golden for `NetherPortal::find_safe_location`, one scene per branch
//! (`tests/golden/nether_portal_safe_location.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Every scene lays blocks around its own target (regions [`SPACING`] blocks apart over a void)
//! and records the returned position relative to the target column, the axis, the fallback flag
//! and the height-map top of the target column. Most scenes are [`Room`]s: a stone floor, a
//! stone roof and air between, because the search starts at the height-map top and walks down,
//! so only air under a roof is ever examined.

use std::fmt::Write as _;
use std::sync::Arc;

use pumpkin_data::Block;
use pumpkin_data::block_properties::HorizontalAxis;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_world::chunk::{ChunkData, ChunkHeightmapType, ChunkSections};
use pumpkin_world::world::BlockFlags;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;
use crate::world::World;
use crate::world::border::Worldborder;

use super::NetherPortal;

const SPACING: i32 = 1024;
/// Blocks laid around a target: the search reaches 32 columns, the portal box 3 more.
const REACH: i32 = 36;
/// Stone top of the default ground: portals stand at `GROUND + 1`.
const GROUND: i32 = 63;
/// Floor of the standard room, below the usual target height of 64.
const FLOOR: i32 = 40;

struct Ctx {
    nether: Arc<World>,
    overworld: Arc<World>,
    next: i32,
}

struct Spec<'a> {
    name: &'a str,
    nether: bool,
    ty: i32,
    axis: HorizontalAxis,
    /// World border: centre offset from the target (x, z) and diameter.
    border: Option<(i32, i32, i32)>,
}

const fn spec(name: &str, nether: bool, ty: i32) -> Spec<'_> {
    Spec {
        name,
        nether,
        ty,
        axis: HorizontalAxis::X,
        border: None,
    }
}

const fn on_z(mut s: Spec<'_>) -> Spec<'_> {
    s.axis = HorizontalAxis::Z;
    s
}

const fn bordered(mut s: Spec<'_>, dx: i32, dz: i32, diameter: i32) -> Spec<'_> {
    s.border = Some((dx, dz, diameter));
    s
}

/// A stone floor (3 layers, top at `floor`), `air` blocks of air and a 3 layer stone roof, over
/// the columns `x` and `z` (offsets from the target).
#[derive(Clone, Copy)]
struct Room {
    x: (i32, i32),
    z: (i32, i32),
    floor: i32,
    air: i32,
}

const fn room(x: (i32, i32), z: (i32, i32), floor: i32, air: i32) -> Room {
    Room { x, z, floor, air }
}

/// The standard room: 9 x 9 columns around the target, 4 blocks of air.
const fn standard() -> Room {
    room((-4, 4), (-4, 4), FLOOR, 4)
}

fn put(world: &Arc<World>, x: i32, y: i32, z: i32, block: &Block) {
    if y < world.min_y || y >= world.min_y + world.dimension.height {
        return;
    }
    world.set_block_state(
        &BlockPos::new(x, y, z),
        block.default_state.id,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

fn fill(world: &Arc<World>, x: (i32, i32), y: (i32, i32), z: (i32, i32), block: &Block) {
    for px in x.0..=x.1 {
        for pz in z.0..=z.1 {
            for py in y.0..=y.1 {
                put(world, px, py, pz, block);
            }
        }
    }
}

fn build(world: &Arc<World>, ox: i32, oz: i32, list: &[Room]) {
    for r in list {
        let x = (ox + r.x.0, ox + r.x.1);
        let z = (oz + r.z.0, oz + r.z.1);
        fill(world, x, (r.floor - 2, r.floor), z, &Block::STONE);
        let roof = r.floor + r.air + 1;
        fill(world, x, (roof, roof + 2), z, &Block::STONE);
    }
}

fn rooms(list: Vec<Room>) -> impl Fn(&Arc<World>, i32, i32) {
    move |world, ox, oz| build(world, ox, oz, &list)
}

/// The standard room with a full layer of `block` at height `y` (offsets x, z -4..=4).
fn layered(block: Block, y: i32, air: i32) -> impl Fn(&Arc<World>, i32, i32) {
    move |world, ox, oz| {
        build(world, ox, oz, &[room((-4, 4), (-4, 4), FLOOR, air)]);
        fill(world, (ox - 4, ox + 4), (y, y), (oz - 4, oz + 4), &block);
    }
}

/// An empty chunk laid out for the world's own height (`ChunkData::empty` is the overworld's:
/// in the nether its height map would read 64 blocks too high).
fn empty_chunk(world: &World, cx: i32, cz: i32) -> Arc<ChunkData> {
    let mut chunk = ChunkData::empty(cx, cz);
    let sections = usize::try_from(world.dimension.height / 16).unwrap();
    chunk.section = ChunkSections::new(sections, world.min_y);
    Arc::new(chunk)
}

fn line(
    out: &mut String,
    name: &str,
    top: i32,
    ox: i32,
    oz: i32,
    found: Option<(BlockPos, HorizontalAxis, bool)>,
) {
    match found {
        None => writeln!(out, "{name}: none").unwrap(),
        Some((pos, axis, fallback)) => writeln!(
            out,
            "{name}: ({}, {}, {}) {axis:?}{} top={top}",
            pos.0.x - ox,
            pos.0.y,
            pos.0.z - oz,
            if fallback { " fallback" } else { "" }
        )
        .unwrap(),
    }
}

fn run(ctx: &mut Ctx, out: &mut String, s: &Spec<'_>, setup: impl Fn(&Arc<World>, i32, i32)) {
    let world = if s.nether {
        &ctx.nether
    } else {
        &ctx.overworld
    };
    let ox = ctx.next * SPACING;
    let oz = 0;
    ctx.next += 1;
    for cx in ((ox - 40) >> 4)..=((ox + 40) >> 4) {
        for cz in ((oz - 40) >> 4)..=((oz + 40) >> 4) {
            world
                .level
                .loaded_chunks
                .insert(Vector2::new(cx, cz), empty_chunk(world, cx, cz));
        }
    }
    setup(world, ox, oz);
    let saved = s.border.map(|(dx, dz, diameter)| {
        let mut border = world.worldborder.lock().unwrap();
        let old = std::mem::replace(
            &mut *border,
            Worldborder::new(
                f64::from(ox + dx),
                f64::from(oz + dz),
                f64::from(diameter),
                0,
                5,
                300,
            ),
        );
        drop(border);
        old
    });
    let top = world.get_heightmap_height(ChunkHeightmapType::MotionBlocking, ox, oz);
    let found = NetherPortal::find_safe_location(world, BlockPos::new(ox, s.ty, oz), s.axis);
    if let Some(old) = saved {
        *world.worldborder.lock().unwrap() = old;
    }
    line(out, s.name, top, ox, oz, found);
}

fn fallback_scenes(ctx: &mut Ctx, out: &mut String) {
    // Branch: nothing to stand on anywhere -> fallback, height clamped to 70..top-9, shifted by
    // one block against the axis direction.
    for (name, nether, ty) in [
        ("nether_void_y64", true, 64),
        ("nether_void_y100", true, 100),
        ("nether_void_y118", true, 118),
        ("nether_void_y119", true, 119),
        ("nether_void_y200", true, 200),
        ("overworld_void_y64", false, 64),
        ("overworld_void_y200", false, 200),
        ("overworld_void_y310", false, 310),
        ("overworld_void_y311", false, 311),
        ("overworld_void_y400", false, 400),
    ] {
        run(ctx, out, &spec(name, nether, ty), |_, _, _| {});
        let z_name = format!("{name}_z");
        run(ctx, out, &on_z(spec(&z_name, nether, ty)), |_, _, _| {});
    }
}

fn flat_scenes(ctx: &mut Ctx, out: &mut String) {
    // Branch: a flat stone ground: the search starts on the surface block.
    for (name, nether, ty) in [
        ("nether_flat", true, 64),
        ("nether_flat_high_target", true, 100),
        ("overworld_flat", false, 64),
    ] {
        let ground = |w: &Arc<World>, ox: i32, oz: i32| {
            fill(
                w,
                (ox - REACH, ox + REACH),
                (GROUND - 2, GROUND),
                (oz - REACH, oz + REACH),
                &Block::STONE,
            );
        };
        run(ctx, out, &spec(name, nether, ty), ground);
        let z_name = format!("{name}_z");
        run(ctx, out, &on_z(spec(&z_name, nether, ty)), ground);
    }
}

fn cave_scenes(ctx: &mut Ctx, out: &mut String) {
    // Branch: air under a roof; 3 blocks of air are too few, 4 are enough.
    for (world, nether) in [("nether", true), ("overworld", false)] {
        for air in [3, 4, 5] {
            let name = format!("{world}_cave_air{air}");
            run(
                ctx,
                out,
                &spec(&name, nether, 64),
                rooms(vec![room((-4, 4), (-4, 4), FLOOR, air)]),
            );
        }
        let name = format!("{world}_cave_air4_z");
        run(
            ctx,
            out,
            &on_z(spec(&name, nether, 64)),
            rooms(vec![standard()]),
        );
    }
}

fn limit_scenes(ctx: &mut Ctx, out: &mut String) {
    // Branch: world top (nether ceiling at 127, roof above it), world bottom, air down to the
    // bottom of the world.
    for (name, nether, floor, air) in [
        ("nether_top_fits", true, 122, 4),
        ("nether_top_too_high", true, 123, 4),
        ("nether_low_roof", true, 118, 4),
        ("overworld_top_fits", false, 314, 4),
        ("overworld_top_open", false, 315, 4),
        ("nether_bottom", true, 0, 4),
        ("overworld_bottom", false, -64, 4),
        ("nether_roof_over_void", true, -10, 71),
        ("overworld_roof_over_void", false, -100, 159),
    ] {
        let setup = rooms(vec![room((-4, 4), (-4, 4), floor, air)]);
        run(ctx, out, &spec(name, nether, 64), setup);
    }
}

fn stack_scenes(ctx: &mut Ctx, out: &mut String) {
    // Branch: several air runs in one column: a too small one is skipped, the search goes on
    // below it; the nearest height to the target wins.
    let stacked = || {
        rooms(vec![
            room((-4, 4), (-4, 4), 60, 3),
            room((-4, 4), (-4, 4), FLOOR, 4),
        ])
    };
    run(ctx, out, &spec("nether_stack_y64", true, 64), stacked());
    run(ctx, out, &spec("nether_stack_y30", true, 30), stacked());
    run(ctx, out, &spec("overworld_stack_y64", false, 64), stacked());
}

fn order_scenes(ctx: &mut Ctx, out: &mut String) {
    // Branch: ideal spots compared by squared distance; first found wins a tie (columns are
    // scanned x first, then z).
    let at = |x: (i32, i32), z: (i32, i32)| room(x, z, FLOOR, 4);
    let cases = [
        ("far_first", vec![at((-24, -16), (-4, 4)), standard()]),
        ("far_last", vec![standard(), at((16, 24), (-4, 4))]),
        ("tie_x", vec![at((-9, -3), (-4, 4)), at((4, 10), (-4, 4))]),
        ("tie_z", vec![at((-4, 4), (-9, -3)), at((-4, 4), (4, 10))]),
        ("x_vs_z", vec![at((10, 18), (-4, 4)), at((-4, 4), (6, 14))]),
        (
            "height_beats_distance",
            vec![room((6, 14), (-4, 4), 63, 4), room((-4, 4), (-4, 4), 30, 4)],
        ),
    ];
    for (name, list) in cases {
        let name = format!("nether_order_{name}");
        run(ctx, out, &spec(&name, true, 64), rooms(list));
    }
}

fn acceptable_scenes(ctx: &mut Ctx, out: &mut String) {
    // Branch: a one block wide strip gives a spot valid at offset 0 only (acceptable, not
    // ideal): used only when no ideal spot exists anywhere, nearest first.
    let strip = |x: (i32, i32)| room(x, (0, 0), FLOOR, 4);
    let lane = room((0, 0), (-4, 4), FLOOR, 4);
    let cases = [
        ("strip_x", vec![strip((-4, 4))]),
        ("strip_z", vec![lane]),
        ("plus", vec![strip((-4, 4)), lane]),
        (
            "ideal_far",
            vec![strip((-4, 4)), room((16, 24), (-4, 4), FLOOR, 4)],
        ),
        ("far_first", vec![strip((-24, -16)), strip((-4, 4))]),
        ("far_last", vec![strip((-4, 4)), strip((16, 24))]),
    ];
    for (name, list) in cases {
        let name = format!("nether_acceptable_{name}");
        run(ctx, out, &spec(&name, true, 64), rooms(list));
    }
}

fn fluid_scenes(ctx: &mut Ctx, out: &mut String) {
    // Branch: what counts as air (replaceable, not a liquid) and as a floor (solid block).
    let cases = [
        ("lava", layered(Block::LAVA, FLOOR + 1, 5)),
        ("water", layered(Block::WATER, FLOOR + 1, 5)),
        ("grass", layered(Block::SHORT_GRASS, FLOOR + 1, 5)),
        ("snow", layered(Block::SNOW, FLOOR + 1, 5)),
        ("glass", layered(Block::GLASS, FLOOR + 1, 5)),
        ("cobweb", layered(Block::COBWEB, FLOOR + 1, 5)),
        ("water_high", layered(Block::WATER, FLOOR + 4, 6)),
    ];
    for (name, setup) in cases {
        let name = format!("nether_fluid_{name}");
        run(ctx, out, &spec(&name, true, 64), setup);
    }
    let two_layers = |w: &Arc<World>, ox: i32, oz: i32| {
        build(w, ox, oz, &[room((-4, 4), (-4, 4), FLOOR, 6)]);
        fill(
            w,
            (ox - 4, ox + 4),
            (FLOOR + 1, FLOOR + 2),
            (oz - 4, oz + 4),
            &Block::WATER,
        );
    };
    run(
        ctx,
        out,
        &spec("nether_fluid_water_deep", true, 64),
        two_layers,
    );
}

fn border_scenes(ctx: &mut Ctx, out: &mut String) {
    // Branch: columns outside the world border are skipped; the fallback is clamped into it.
    let wide = rooms(vec![room((-12, 12), (-4, 4), FLOOR, 4)]);
    let deep = rooms(vec![room((-4, 4), (-12, 12), FLOOR, 4)]);
    run(
        ctx,
        out,
        &bordered(spec("border_east", true, 64), 8, 0, 10),
        &wide,
    );
    run(
        ctx,
        out,
        &bordered(spec("border_west", true, 64), -8, 0, 10),
        &wide,
    );
    run(
        ctx,
        out,
        &bordered(spec("border_south", true, 64), 0, 8, 10),
        &deep,
    );
    run(
        ctx,
        out,
        &bordered(spec("border_north", true, 64), 0, -8, 10),
        &deep,
    );
    run(
        ctx,
        out,
        &bordered(spec("border_all_out", true, 64), 40, 0, 10),
        &wide,
    );
    for (name, dx, dz, diameter) in [
        ("east", 40, 0, 10),
        ("west", -40, 0, 10),
        ("south", 0, 40, 10),
        ("north", 0, -40, 10),
        ("tiny", 0, 0, 1),
    ] {
        let fallback = format!("border_void_{name}");
        run(
            ctx,
            out,
            &bordered(spec(&fallback, true, 100), dx, dz, diameter),
            |_, _, _| {},
        );
        let fallback_z = format!("border_void_{name}_z");
        let scene = bordered(on_z(spec(&fallback_z, true, 100)), dx, dz, diameter);
        run(ctx, out, &scene, |_, _, _| {});
    }
}

fn edge_scenes(ctx: &mut Ctx, out: &mut String) {
    // Branch: the search reaches 32 columns each way on both axes, no further. A one block wide
    // strip leaves a single spot, exactly at the named column offset.
    for f in [-33, -32, 32, 33] {
        let name = format!("nether_edge_x{f}");
        let strip = room((f - 1, f + 2), (0, 0), FLOOR, 4);
        run(ctx, out, &spec(&name, true, 64), rooms(vec![strip]));
        let name = format!("nether_edge_z{f}");
        let lane = room((0, 0), (f - 1, f + 2), FLOOR, 4);
        run(ctx, out, &spec(&name, true, 64), rooms(vec![lane]));
    }
}

fn tie_scenes(ctx: &mut Ctx, out: &mut String) {
    // Branch: two spots at the same distance: the one met first wins, scanning x then z upward.
    let at_x = |f: i32| room((f - 1, f + 2), (0, 0), FLOOR, 4);
    let at_z = |f: i32| room((-1, 2), (f, f), FLOOR, 4);
    let cases = [
        ("x", vec![at_x(-10), at_x(10)]),
        ("x_reversed", vec![at_x(10), at_x(-10)]),
        ("z", vec![at_z(-10), at_z(10)]),
        ("z_reversed", vec![at_z(10), at_z(-10)]),
    ];
    for (name, list) in cases {
        let name = format!("nether_tie_{name}");
        run(ctx, out, &spec(&name, true, 64), rooms(list));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn nether_portal_safe_location_golden() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let worlds = server.worlds.load();
    let nether = worlds
        .iter()
        .find(|w| w.dimension.has_ceiling)
        .expect("a nether-like world")
        .clone();
    let overworld = worlds[0].clone();
    let mut out = String::new();
    for (name, world) in [("nether", &nether), ("overworld", &overworld)] {
        let d = &world.dimension;
        let (min, height, logical) = (world.min_y, d.height, d.logical_height);
        writeln!(
            out,
            "{name}: min_y={min} height={height} logical_height={logical}"
        )
        .unwrap();
    }
    let mut ctx = Ctx {
        nether,
        overworld,
        next: 1,
    };
    fallback_scenes(&mut ctx, &mut out);
    flat_scenes(&mut ctx, &mut out);
    cave_scenes(&mut ctx, &mut out);
    limit_scenes(&mut ctx, &mut out);
    stack_scenes(&mut ctx, &mut out);
    order_scenes(&mut ctx, &mut out);
    acceptable_scenes(&mut ctx, &mut out);
    fluid_scenes(&mut ctx, &mut out);
    border_scenes(&mut ctx, &mut out);
    edge_scenes(&mut ctx, &mut out);
    tie_scenes(&mut ctx, &mut out);
    server.stop();
    script_golden::bless_or_compare("nether_portal_safe_location.txt", &out)
}
