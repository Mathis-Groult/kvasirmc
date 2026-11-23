//! A record of which blocks `Explosion::get_blocks_to_destroy` picks, made before the function
//! was split along vanilla's `ServerExplosion.calculateExplodedPositions` (ray loop, per-step
//! resistance; `tests/golden/explosion_blocks.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene builds a terrain of 4 x 4 chunks, puts an explosion in it and prints how many
//! blocks the explosion picked, how many ray strengths it drew, and an FNV hash of the sorted
//! (position relative to the scene, state id) list plus a count per block name. The ray strength
//! (vanilla's `random.nextFloat()`) is pinned by a thread-local the function reads in tests: a
//! seeded xorshift sequence or a constant.
//! Terrains: stone, dirt, strata of five materials, dirt with air and stone pockets, fluids of
//! every kind (sources, flowing, falling, lava, waterlogged slabs), a mix of sixteen blocks
//! (air, cave air, glass, bedrock, TNT, cobweb ...) and rails on a floor with blocks above them.
//! Places: the middle of a chunk, a chunk corner, the world's bottom and top edges, chunks
//! missing, and the horizontal limit at +30 000 000 and -30 000 000 (a loaded chunk behind it
//! must stay untouched). Powers 0 to 10; damage calculators: default, wind charge, immune tag,
//! no block damage, and a calculator with its own resistance and threshold.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fmt::Write;
use std::sync::Arc;

use pumpkin_data::block_properties::WhiteWoolSlabLikeProperties;
use pumpkin_data::fluid::{Fluid, FluidProperties, Level};
use pumpkin_data::tag;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::chunk::ChunkData;

use crate::block::fluid::flowing_trait::FlowingFluid;
use crate::block::fluid::lava::FlowingLava;
use crate::block::fluid::water::FlowingWater;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;
use crate::entity::projectile::wind_charge::WIND_CHARGE_EXPLOSION_DAMAGE_CALCULATOR;
use crate::world::World;

use super::{
    BlockInteraction, Explosion, ExplosionDamageCalculator, SimpleExplosionDamageCalculator,
};

/// Chunks per side of a scene's terrain.
const REGION_CHUNKS: i32 = 4;
/// Chunk distance between the origins of two scenes.
const SCENE_STRIDE: i32 = 8;

/// Where the ray strengths come from.
#[derive(Clone, Copy, Debug)]
enum Rays {
    /// A xorshift64 sequence from this nonzero seed.
    Seeded(u64),
    /// The same value for every ray.
    Const(f32),
}

thread_local! {
    static RAYS: Cell<Option<Rays>> = const { Cell::new(None) };
    static DRAWN: Cell<u32> = const { Cell::new(0) };
}

/// The next ray strength of the running scene; `None` outside a scene (the function then draws
/// from the real random source).
pub(super) fn next_ray_value() -> Option<f32> {
    RAYS.with(|cell| {
        let rays = cell.get()?;
        DRAWN.with(|drawn| drawn.set(drawn.get() + 1));
        match rays {
            Rays::Const(value) => Some(value),
            Rays::Seeded(seed) => {
                let mut x = seed;
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                cell.set(Some(Rays::Seeded(x)));
                Some((x >> 40) as f32 / 16_777_216.0)
            }
        }
    })
}

#[derive(Clone, Copy, Debug)]
enum Terrain {
    Air,
    Stone,
    Dirt,
    Strata,
    Pockets,
    Fluids,
    Mixed,
    Rails,
}

/// Which chunks of the region are left unloaded.
#[derive(Clone, Copy, Debug)]
enum Gap {
    None,
    Checker,
    All,
}

/// Which chunk the region starts at.
#[derive(Clone, Copy, Debug)]
enum Origin {
    Scene,
    PositiveLimit,
    NegativeLimit,
    ZLimit,
}

#[derive(Clone, Copy, Debug)]
enum Calc {
    Default,
    Wind,
    Immune,
    NoBlocks,
    Own,
}

/// A calculator of its own: stone is see-through, water costs nothing, everything else costs
/// its blast resistance; only a ray still stronger than 1.0 destroys.
struct Own;

impl ExplosionDamageCalculator for Own {
    fn get_block_explosion_resistance(
        &self,
        _explosion: &Explosion,
        _world: &World,
        _pos: &BlockPos,
        block: &Block,
        fluid: &pumpkin_data::fluid::FluidState,
    ) -> Option<f32> {
        if block.id == Block::STONE.id {
            None
        } else if block.id == Block::WATER.id {
            Some(0.0)
        } else {
            Some(fluid.blast_resistance.max(block.blast_resistance))
        }
    }

    fn should_block_explode(
        &self,
        _explosion: &Explosion,
        _world: &World,
        _pos: &BlockPos,
        _block: &Block,
        power: f32,
    ) -> bool {
        power > 1.0
    }
}

impl Calc {
    fn build(self) -> Option<Arc<dyn ExplosionDamageCalculator>> {
        match self {
            Self::Default => None,
            Self::Wind => Some(WIND_CHARGE_EXPLOSION_DAMAGE_CALCULATOR.clone()),
            Self::Immune => Some(Arc::new(SimpleExplosionDamageCalculator::new(
                true,
                true,
                None,
                Some(&tag::Block::MINECRAFT_BASE_STONE_OVERWORLD),
            ))),
            Self::NoBlocks => Some(Arc::new(SimpleExplosionDamageCalculator::new(
                true, false, None, None,
            ))),
            Self::Own => Some(Arc::new(Own)),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Scene {
    terrain: Terrain,
    /// Explosion centre, relative to the region's first block.
    centre: (f64, f64, f64),
    power: f32,
    rays: Rays,
    calc: Calc,
    rails: bool,
    gap: Gap,
    origin: Origin,
}

const MIDDLE: (f64, f64, f64) = (24.5, 64.5, 24.5);

const fn scene(terrain: Terrain, power: f32) -> Scene {
    Scene {
        terrain,
        centre: MIDDLE,
        power,
        rays: Rays::Seeded(0x9E37_79B9_7F4A_7C15),
        calc: Calc::Default,
        rails: false,
        gap: Gap::None,
        origin: Origin::Scene,
    }
}

const fn at(mut s: Scene, x: f64, y: f64, z: f64) -> Scene {
    s.centre = (x, y, z);
    s
}

const fn seeded(mut s: Scene, seed: u64) -> Scene {
    s.rays = Rays::Seeded(seed);
    s
}

const fn constant(mut s: Scene, value: f32) -> Scene {
    s.rays = Rays::Const(value);
    s
}

const fn with_calc(mut s: Scene, calc: Calc) -> Scene {
    s.calc = calc;
    s
}

const fn with_rails(mut s: Scene) -> Scene {
    s.rails = true;
    s
}

const fn with_gap(mut s: Scene, gap: Gap) -> Scene {
    s.gap = gap;
    s
}

const fn with_origin(mut s: Scene, origin: Origin) -> Scene {
    s.origin = origin;
    s
}

fn terrain_scenes() -> Vec<Scene> {
    let mut scenes = Vec::new();
    for terrain in [
        Terrain::Stone,
        Terrain::Dirt,
        Terrain::Strata,
        Terrain::Pockets,
        Terrain::Fluids,
        Terrain::Mixed,
    ] {
        for power in [1.0, 4.0, 6.0, 10.0] {
            scenes.push(scene(terrain, power));
        }
    }
    scenes
}

fn ray_scenes() -> Vec<Scene> {
    vec![
        seeded(scene(Terrain::Dirt, 10.0), 0x1234_5678_9ABC_DEF1),
        seeded(scene(Terrain::Mixed, 10.0), 0x0F0F_0F0F_1357_9BDF),
        seeded(scene(Terrain::Pockets, 10.0), 77),
        constant(scene(Terrain::Dirt, 6.0), 0.0),
        constant(scene(Terrain::Dirt, 6.0), 0.999_999),
        constant(scene(Terrain::Stone, 10.0), 0.0),
        constant(scene(Terrain::Stone, 10.0), 0.999_999),
        scene(Terrain::Dirt, 0.0),
        scene(Terrain::Dirt, 0.5),
        scene(Terrain::Dirt, 2.5),
        scene(Terrain::Air, 10.0),
    ]
}

fn edge_scenes() -> Vec<Scene> {
    vec![
        at(scene(Terrain::Dirt, 10.0), 24.5, -63.5, 24.5),
        at(scene(Terrain::Mixed, 6.0), 24.5, -62.5, 24.5),
        at(scene(Terrain::Dirt, 6.0), 24.5, -64.5, 24.5),
        at(scene(Terrain::Dirt, 10.0), 24.5, 318.5, 24.5),
        at(scene(Terrain::Mixed, 6.0), 24.5, 319.5, 24.5),
        at(scene(Terrain::Dirt, 6.0), 24.5, 320.5, 24.5),
        at(scene(Terrain::Dirt, 10.0), 32.0, 64.5, 32.0),
        at(scene(Terrain::Mixed, 6.0), 16.0, 64.5, 48.0),
        at(scene(Terrain::Pockets, 10.0), 47.9, 64.0, 16.1),
    ]
}

fn gap_and_limit_scenes() -> Vec<Scene> {
    vec![
        with_gap(scene(Terrain::Dirt, 10.0), Gap::Checker),
        with_gap(scene(Terrain::Mixed, 10.0), Gap::Checker),
        with_gap(scene(Terrain::Dirt, 10.0), Gap::All),
        with_origin(
            at(scene(Terrain::Dirt, 10.0), 28.5, 64.5, 24.5),
            Origin::PositiveLimit,
        ),
        with_origin(
            at(scene(Terrain::Dirt, 10.0), 36.5, 64.5, 24.5),
            Origin::NegativeLimit,
        ),
        with_origin(
            at(scene(Terrain::Dirt, 10.0), 24.5, 64.5, 28.5),
            Origin::ZLimit,
        ),
    ]
}

fn rail_and_calc_scenes() -> Vec<Scene> {
    let rails = at(scene(Terrain::Rails, 4.0), 24.5, 66.5, 24.5);
    let mut scenes = vec![
        with_rails(rails),
        with_rails(at(scene(Terrain::Rails, 10.0), 24.5, 66.5, 24.5)),
        at(scene(Terrain::Rails, 10.0), 24.5, 66.5, 24.5),
        with_rails(at(scene(Terrain::Rails, 6.0), 24.5, 64.5, 24.5)),
    ];
    for calc in [Calc::Wind, Calc::Immune, Calc::NoBlocks, Calc::Own] {
        scenes.push(with_calc(scene(Terrain::Dirt, 10.0), calc));
        scenes.push(with_calc(scene(Terrain::Mixed, 10.0), calc));
        scenes.push(with_calc(scene(Terrain::Stone, 6.0), calc));
    }
    for power in [4.0, 6.0, 10.0] {
        scenes.push(with_calc(scene(Terrain::Fluids, power), Calc::Own));
    }
    scenes.push(with_calc(scene(Terrain::Fluids, 10.0), Calc::Immune));
    scenes
}

fn scenes() -> Vec<Scene> {
    let mut all = terrain_scenes();
    all.extend(ray_scenes());
    all.extend(edge_scenes());
    all.extend(gap_and_limit_scenes());
    all.extend(rail_and_calc_scenes());
    all
}

fn waterlogged_slab() -> BlockStateId {
    let mut slab = WhiteWoolSlabLikeProperties::default(&Block::OAK_SLAB);
    slab.waterlogged = true;
    slab.to_state_id(&Block::OAK_SLAB)
}

fn flowing_water(level: Level, falls: bool) -> BlockStateId {
    FlowingWater
        .get_flowing(&Fluid::FLOWING_WATER, level, falls)
        .to_state_id(&Fluid::FLOWING_WATER)
}

fn flowing_lava(level: Level) -> BlockStateId {
    FlowingLava
        .get_flowing(&Fluid::FLOWING_LAVA, level, false)
        .to_state_id(&Fluid::FLOWING_LAVA)
}

/// The block states a terrain draws from, by index.
fn palette(terrain: Terrain) -> Vec<BlockStateId> {
    let air = Block::AIR.default_state.id;
    let id = |block: &Block| block.default_state.id;
    match terrain {
        Terrain::Air => vec![air],
        Terrain::Stone => vec![id(&Block::STONE)],
        Terrain::Dirt => vec![id(&Block::DIRT)],
        Terrain::Strata => vec![
            id(&Block::DIRT),
            id(&Block::OBSIDIAN),
            id(&Block::WATER),
            id(&Block::LAVA),
            id(&Block::GLASS),
        ],
        Terrain::Pockets => vec![id(&Block::DIRT), air, id(&Block::STONE)],
        Terrain::Fluids => vec![
            id(&Block::WATER),
            flowing_water(Level::L7, false),
            flowing_water(Level::L4, false),
            flowing_water(Level::L1, true),
            id(&Block::LAVA),
            flowing_lava(Level::L6),
            waterlogged_slab(),
            id(&Block::KELP),
            id(&Block::SEAGRASS),
            id(&Block::DIRT),
            id(&Block::DIRT),
            air,
            air,
            air,
            air,
            air,
        ],
        Terrain::Mixed => vec![
            air,
            id(&Block::CAVE_AIR),
            id(&Block::STONE),
            id(&Block::DIRT),
            id(&Block::GLASS),
            id(&Block::OBSIDIAN),
            id(&Block::BEDROCK),
            id(&Block::WATER),
            id(&Block::LAVA),
            id(&Block::TNT),
            id(&Block::OAK_PLANKS),
            id(&Block::COBWEB),
            id(&Block::NETHERRACK),
            waterlogged_slab(),
            id(&Block::SAND),
            air,
        ],
        Terrain::Rails => vec![
            id(&Block::STONE),
            id(&Block::RAIL),
            id(&Block::POWERED_RAIL),
            id(&Block::DETECTOR_RAIL),
            id(&Block::ACTIVATOR_RAIL),
            id(&Block::DIRT),
            air,
        ],
    }
}

fn mix(x: i32, y: i32, z: i32) -> usize {
    let h = x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663) ^ z.wrapping_mul(83_492_791);
    (h as u32 >> 7) as usize
}

/// The palette index at (x, z) relative to the region and absolute y.
fn index(terrain: Terrain, x: i32, y: i32, z: i32) -> usize {
    match terrain {
        Terrain::Air | Terrain::Stone | Terrain::Dirt => 0,
        Terrain::Strata => y.rem_euclid(5) as usize,
        Terrain::Pockets => match mix(x, y, z) % 7 {
            0 => 1,
            1 => 2,
            _ => 0,
        },
        Terrain::Fluids | Terrain::Mixed => mix(x, y, z) % 16,
        Terrain::Rails => match y {
            ..64 => 0,
            64 if z % 2 == 0 => 1 + x.rem_euclid(4) as usize,
            64 => 5,
            65 if mix(x, y, z).is_multiple_of(3) => 0,
            _ => 6,
        },
    }
}

fn origin_chunk(origin: Origin, index: usize) -> (i32, i32) {
    match origin {
        Origin::Scene => (index as i32 * SCENE_STRIDE, 0),
        Origin::PositiveLimit => (1_874_998, 0),
        Origin::NegativeLimit => (-1_875_002, 0),
        Origin::ZLimit => (0, 1_874_998),
    }
}

fn is_gap(gap: Gap, dx: i32, dz: i32) -> bool {
    match gap {
        Gap::None => false,
        Gap::Checker => (dx + dz) % 2 == 1,
        Gap::All => true,
    }
}

/// Lays the scene's terrain into freshly made chunks and loads them.
fn lay_terrain(world: &World, scene: &Scene, origin: (i32, i32)) {
    let pal = palette(scene.terrain);
    let air = Block::AIR.default_state.id;
    let y_mid = scene.centre.1.floor() as i32;
    let y_low = (y_mid - 20).max(world.get_bottom_y());
    let y_high = (y_mid + 20).min(world.get_top_y());
    for dx in 0..REGION_CHUNKS {
        for dz in 0..REGION_CHUNKS {
            if is_gap(scene.gap, dx, dz) {
                continue;
            }
            let (cx, cz) = (origin.0 + dx, origin.1 + dz);
            let chunk = ChunkData::empty_sync(cx, cz);
            for x in 0..16 {
                for z in 0..16 {
                    for y in y_low..=y_high {
                        let state = pal[index(scene.terrain, dx * 16 + x, y, dz * 16 + z)];
                        if state != air {
                            chunk
                                .section
                                .set_block_absolute_y(x as usize, y, z as usize, state);
                        }
                    }
                }
            }
            world
                .level
                .loaded_chunks
                .insert(Vector2::new(cx, cz), chunk);
        }
    }
}

fn explosion_of(scene: &Scene, origin: (i32, i32)) -> Explosion {
    let base = (f64::from(origin.0 * 16), f64::from(origin.1 * 16));
    let pos = Vector3::new(
        base.0 + scene.centre.0,
        scene.centre.1,
        base.1 + scene.centre.2,
    );
    let mut explosion = Explosion::new(scene.power, pos, BlockInteraction::Destroy);
    if let Some(calc) = scene.calc.build() {
        explosion = explosion.with_damage_calculator(calc);
    }
    if scene.rails {
        explosion = explosion.preserving_rails();
    }
    explosion
}

/// FNV-1a over the sorted (relative position, state id) list.
fn hash_of(entries: &[(i32, i32, i32, u16)]) -> u64 {
    let mut hash = 0xCBF2_9CE4_8422_2325u64;
    for (x, y, z, state) in entries {
        for value in [*x, *y, *z, i32::from(*state)] {
            for byte in value.to_le_bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
    }
    hash
}

fn run_scene(world: &World, index: usize, scene: &Scene, out: &mut String) {
    let origin = origin_chunk(scene.origin, index);
    lay_terrain(world, scene, origin);
    let explosion = explosion_of(scene, origin);
    DRAWN.with(|drawn| drawn.set(0));
    RAYS.with(|rays| rays.set(Some(scene.rays)));
    let picked = explosion.get_blocks_to_destroy(world);
    RAYS.with(|rays| rays.set(None));
    let (base_x, base_z) = (origin.0 * 16, origin.1 * 16);
    let mut entries: Vec<(i32, i32, i32, u16)> = Vec::new();
    let mut names: BTreeMap<&str, u32> = BTreeMap::new();
    for (pos, (block, state)) in &picked {
        entries.push((
            pos.0.x - base_x,
            pos.0.y,
            pos.0.z - base_z,
            state.id.as_u16(),
        ));
        *names.entry(block.name).or_default() += 1;
    }
    entries.sort_unstable();
    let drawn = DRAWN.with(Cell::get);
    let _ = write!(
        out,
        "{index} {:?} power {} {:?} {:?} rails {} gap {:?} {:?} at {:?}: {} blocks, {drawn} rays, hash {:016x}",
        scene.terrain,
        scene.power,
        scene.rays,
        scene.calc,
        scene.rails,
        scene.gap,
        scene.origin,
        scene.centre,
        entries.len(),
        hash_of(&entries),
    );
    for (name, count) in &names {
        let _ = write!(out, " {name}={count}");
    }
    out.push('\n');
}

#[tokio::test(flavor = "multi_thread")]
async fn explosion_blocks_are_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server
        .worlds
        .load()
        .first()
        .cloned()
        .ok_or("the server has no world")?;
    let mut out = String::new();
    for (index, scene) in scenes().iter().enumerate() {
        run_scene(&world, index, scene, &mut out);
    }
    server.stop();
    script_golden::bless_or_compare("explosion_blocks.txt", &out)
}
