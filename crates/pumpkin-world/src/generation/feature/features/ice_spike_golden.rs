//! A record of what `IceSpikeFeature::generate` does, made before it is split
//! (`tests/golden/ice_spike.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each site is a 9 x 9 column of a grid of chunks filled with seeded noise (air, dirt, grass,
//! snow, ice, packed ice, stone, glass) above and below a ground level, plus a chosen floor
//! block and an air column over the origin. The feature runs from the same start position for
//! many seeds; after each run the box is compared with its snapshot, the changes are listed
//! as a count, the lowest and highest changed y and an FNV hash of (offset, new state), and the
//! box is restored. Sites cover: snow ground at y 80 with open, mixed, hard and soft noise, a
//! ground at 110 over open air (roots run 50 blocks), ground near the world bottom (-61, -62,
//! -63), an all-air column, stone, ice, packed ice and floating snow floors, and a start
//! position over a chunk border.

use std::collections::HashMap;
use std::fmt::Write;

use pumpkin_data::placed_feature::PlacedFeature;
use pumpkin_data::{Block, BlockState, BlockStateId};
use pumpkin_util::{
    math::{position::BlockPos, vector3::Vector3},
    random::{RandomGenerator, RandomImpl, xoroshiro128::Xoroshiro},
    world_seed::Seed,
};

use super::IceSpikeFeature;
use crate::generation::feature::configured_features::FeatureArgs;
use crate::generation::generator::WorldGenerator;
use crate::generation::get_world_gen;
use crate::generation::proto_chunk::{GenerationCache, ProtoChunk};
use crate::generation::proto_chunk_test::stage_parity::{TestBlockRegistry, TestGridCache};

/// air, dirt, grass block, snow block, ice, packed ice, stone, glass
type Weights = [u64; 8];

const OPEN_ABOVE: Weights = [86, 2, 2, 3, 2, 1, 2, 2];
const SOLID_ABOVE: Weights = [40, 8, 8, 12, 8, 6, 10, 8];
const MIXED_BELOW: Weights = [55, 10, 8, 8, 6, 5, 5, 3];
const SOFT_BELOW: Weights = [60, 10, 8, 10, 6, 6, 0, 0];
const HARD_BELOW: Weights = [30, 8, 5, 5, 5, 5, 30, 12];
const AIR_BELOW: Weights = [100, 0, 0, 0, 0, 0, 0, 0];

const BOX_RADIUS: i32 = 4;
const BOX_BOTTOM: i32 = 44;
const BOX_TOP_ABOVE_GROUND: i32 = 64;

#[derive(Clone, Copy)]
enum Floor {
    Snow,
    Stone,
    Ice,
    PackedIce,
    /// Snow block three above a stone ground.
    Floating,
    /// No floor and no ground: the column is air all the way down.
    None,
}

struct Site {
    name: &'static str,
    ground: i32,
    x: i32,
    z: i32,
    start_above: i32,
    floor: Floor,
    above: Weights,
    below: Weights,
    seeds: u64,
}

const fn site(
    name: &'static str,
    ground: i32,
    (x, z): (i32, i32),
    floor: Floor,
    (above, below): (Weights, Weights),
    seeds: u64,
) -> Site {
    Site {
        name,
        ground,
        x,
        z,
        start_above: 6,
        floor,
        above,
        below,
        seeds,
    }
}

fn sites() -> Vec<Site> {
    let mut sites = ground_sites();
    sites.extend(other_sites());
    sites
}

fn ground_sites() -> Vec<Site> {
    vec![
        site(
            "open80",
            80,
            (3, -5),
            Floor::Snow,
            (OPEN_ABOVE, MIXED_BELOW),
            220,
        ),
        site(
            "solid80",
            80,
            (-6, 7),
            Floor::Snow,
            (SOLID_ABOVE, SOFT_BELOW),
            140,
        ),
        site(
            "hard80",
            80,
            (5, 4),
            Floor::Snow,
            (OPEN_ABOVE, HARD_BELOW),
            100,
        ),
        site(
            "soft80",
            80,
            (-3, -9),
            Floor::Snow,
            (OPEN_ABOVE, SOFT_BELOW),
            140,
        ),
        site(
            "air110",
            110,
            (6, 6),
            Floor::Snow,
            (OPEN_ABOVE, AIR_BELOW),
            80,
        ),
        site(
            "border",
            70,
            (-2, 14),
            Floor::Snow,
            (OPEN_ABOVE, MIXED_BELOW),
            100,
        ),
    ]
}

fn other_sites() -> Vec<Site> {
    vec![
        site(
            "low61",
            -61,
            (4, 3),
            Floor::Snow,
            (OPEN_ABOVE, MIXED_BELOW),
            40,
        ),
        site(
            "low62",
            -62,
            (-5, 4),
            Floor::Snow,
            (OPEN_ABOVE, MIXED_BELOW),
            40,
        ),
        site(
            "low63",
            -63,
            (2, -7),
            Floor::Snow,
            (OPEN_ABOVE, MIXED_BELOW),
            6,
        ),
        site(
            "low40",
            -40,
            (-4, -3),
            Floor::Snow,
            (SOLID_ABOVE, MIXED_BELOW),
            60,
        ),
        site("none", 0, (1, 1), Floor::None, (OPEN_ABOVE, AIR_BELOW), 4),
        site(
            "stone",
            80,
            (3, -5),
            Floor::Stone,
            (OPEN_ABOVE, MIXED_BELOW),
            4,
        ),
        site("ice", 80, (3, -5), Floor::Ice, (OPEN_ABOVE, MIXED_BELOW), 4),
        site(
            "packed",
            80,
            (3, -5),
            Floor::PackedIce,
            (OPEN_ABOVE, MIXED_BELOW),
            4,
        ),
        site(
            "floating",
            80,
            (-3, 5),
            Floor::Floating,
            (OPEN_ABOVE, SOFT_BELOW),
            40,
        ),
    ]
}

fn material(index: usize) -> &'static BlockState {
    match index {
        0 => Block::AIR.default_state,
        1 => Block::DIRT.default_state,
        2 => Block::GRASS_BLOCK.default_state,
        3 => Block::SNOW_BLOCK.default_state,
        4 => Block::ICE.default_state,
        5 => Block::PACKED_ICE.default_state,
        6 => Block::STONE.default_state,
        _ => Block::GLASS.default_state,
    }
}

fn mix(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn noise_index(weights: &Weights, key: u64) -> usize {
    let total: u64 = weights.iter().sum();
    let mut roll = mix(key) % total;
    for (index, weight) in weights.iter().enumerate() {
        if roll < *weight {
            return index;
        }
        roll -= weight;
    }
    0
}

fn empty_grid(world_gen: &WorldGenerator) -> TestGridCache {
    let mut chunks = HashMap::new();
    for chunk_x in -1..=1 {
        for chunk_z in -1..=1 {
            chunks.insert(
                (chunk_x, chunk_z),
                ProtoChunk::new(chunk_x, chunk_z, world_gen),
            );
        }
    }
    TestGridCache {
        center_pos: (0, 0),
        chunks,
    }
}

fn floor_blocks(floor: Floor) -> (Option<&'static BlockState>, Option<&'static BlockState>) {
    match floor {
        Floor::Snow => (Some(Block::SNOW_BLOCK.default_state), None),
        Floor::Stone => (Some(Block::STONE.default_state), None),
        Floor::Ice => (Some(Block::ICE.default_state), None),
        Floor::PackedIce => (Some(Block::PACKED_ICE.default_state), None),
        Floor::Floating => (
            Some(Block::STONE.default_state),
            Some(Block::SNOW_BLOCK.default_state),
        ),
        Floor::None => (None, None),
    }
}

/// Fills the box of one site: noise everywhere, then the origin column.
fn fill_site(grid: &mut TestGridCache, index: usize, site: &Site) {
    let top = site.ground + BOX_TOP_ABOVE_GROUND;
    for dx in -BOX_RADIUS..=BOX_RADIUS {
        for dz in -BOX_RADIUS..=BOX_RADIUS {
            for y in BOX_BOTTOM.min(site.ground - 8)..=top {
                let weights = if y > site.ground {
                    &site.above
                } else {
                    &site.below
                };
                let key = (index as u64) << 48
                    ^ ((dx + 16) as u64) << 40
                    ^ ((dz + 16) as u64) << 32
                    ^ ((y + 128) as u64);
                let state = material(noise_index(weights, key));
                let at = Vector3::new(site.x + dx, y, site.z + dz);
                GenerationCache::set_block_state(grid, &at, state);
            }
        }
    }
    let (ground_block, hover_block) = floor_blocks(site.floor);
    for y in (site.ground + 1)..=(site.ground + site.start_above) {
        let at = Vector3::new(site.x, y, site.z);
        GenerationCache::set_block_state(grid, &at, Block::AIR.default_state);
    }
    if let Some(state) = ground_block {
        let at = Vector3::new(site.x, site.ground, site.z);
        GenerationCache::set_block_state(grid, &at, state);
    } else {
        for y in -64..=(site.ground + site.start_above) {
            let at = Vector3::new(site.x, y, site.z);
            GenerationCache::set_block_state(grid, &at, Block::AIR.default_state);
        }
    }
    if let Some(state) = hover_block {
        let at = Vector3::new(site.x, site.ground + 3, site.z);
        GenerationCache::set_block_state(grid, &at, state);
    }
}

fn box_range(site: &Site) -> (i32, i32) {
    let bottom = if matches!(site.floor, Floor::None) {
        -64
    } else {
        BOX_BOTTOM.min(site.ground - 8).min(site.ground - 10)
    };
    (bottom.max(-64), site.ground + BOX_TOP_ABOVE_GROUND)
}

fn box_cells(site: &Site) -> Vec<Vector3<i32>> {
    let (bottom, top) = box_range(site);
    let mut cells = Vec::new();
    for dx in -BOX_RADIUS..=BOX_RADIUS {
        for dz in -BOX_RADIUS..=BOX_RADIUS {
            for y in bottom..=top {
                cells.push(Vector3::new(site.x + dx, y, site.z + dz));
            }
        }
    }
    cells
}

fn snapshot(grid: &TestGridCache, cells: &[Vector3<i32>]) -> Vec<BlockStateId> {
    cells
        .iter()
        .map(|at| GenerationCache::get_block_state(grid, at))
        .collect()
}

struct Outcome {
    returned: bool,
    changed: usize,
    lowest: i32,
    highest: i32,
    hash: u64,
}

fn fnv(hash: u64, value: u64) -> u64 {
    (hash ^ value).wrapping_mul(0x0000_0100_0000_01b3)
}

fn run_seed(
    grid: &mut TestGridCache,
    site: &Site,
    cells: &[Vector3<i32>],
    before: &[BlockStateId],
    seed: u64,
) -> Outcome {
    let start = BlockPos::new(site.x, site.ground + site.start_above, site.z);
    let mut random = RandomGenerator::Xoroshiro(Xoroshiro::from_seed(seed));
    let args = FeatureArgs {
        block_registry: &TestBlockRegistry,
        min_y: -64,
        height: 384,
        feature_name: PlacedFeature::IceSpike,
        pos: start,
    };
    let returned = IceSpikeFeature::generate(grid, args, &mut random);
    let mut outcome = Outcome {
        returned,
        changed: 0,
        lowest: i32::MAX,
        highest: i32::MIN,
        hash: 0xcbf2_9ce4_8422_2325,
    };
    for (at, old) in cells.iter().zip(before) {
        let now = GenerationCache::get_block_state(grid, at);
        if now == *old {
            continue;
        }
        outcome.changed += 1;
        outcome.lowest = outcome.lowest.min(at.y);
        outcome.highest = outcome.highest.max(at.y);
        for part in [
            (at.x - site.x + 16) as u64,
            (at.y + 128) as u64,
            (at.z - site.z + 16) as u64,
            u64::from(now.as_u16()),
        ] {
            outcome.hash = fnv(outcome.hash, part);
        }
        let restored = old.to_state();
        GenerationCache::set_block_state(grid, at, restored);
    }
    outcome
}

/// Seeds whose tall-spike draw (`next_bounded_i32(60) == 0`, after the 4, 4 and 2 draws of a
/// spike with width above 1) comes out differently for a bound of 59 or 61, found by replaying
/// those draws. They are rare (about 1 in 3,500), so scanning seeds in order would not hit them.
fn sliver_seeds() -> Vec<u64> {
    let draw = |seed: u64, bound: i32| {
        let mut random = RandomGenerator::Xoroshiro(Xoroshiro::from_seed(seed));
        let _ = random.next_bounded_i32(4);
        let height = random.next_bounded_i32(4) + 7;
        let width = height / 4 + random.next_bounded_i32(2);
        (width > 1, random.next_bounded_i32(bound) == 0)
    };
    let mut classes: [Vec<u64>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    for seed in 1000..400_000 {
        let (wide, at_60) = draw(seed, 60);
        if !wide {
            continue;
        }
        let at_59 = draw(seed, 59).1;
        let at_61 = draw(seed, 61).1;
        let class = match (at_60, at_59, at_61) {
            (true, false, _) => 0,
            (false, true, _) => 1,
            (true, _, false) => 2,
            (false, _, true) => 3,
            _ => continue,
        };
        if classes[class].len() < 2 {
            classes[class].push(seed);
        }
    }
    classes.into_iter().flatten().collect()
}

fn record_site(
    report: &mut String,
    world_gen: &WorldGenerator,
    index: usize,
    site: &Site,
    seeds: &[u64],
) {
    let mut grid = empty_grid(world_gen);
    fill_site(&mut grid, index, site);
    let cells = box_cells(site);
    let before = snapshot(&grid, &cells);
    for &seed in seeds {
        let outcome = run_seed(&mut grid, site, &cells, &before, seed);
        let _ = writeln!(
            report,
            "{} {seed} returned={} changed={} y={}..{} hash={:016x}",
            site.name,
            outcome.returned,
            outcome.changed,
            outcome.lowest,
            outcome.highest,
            outcome.hash
        );
    }
}

fn report() -> String {
    let world_gen = get_world_gen(
        Seed(1),
        pumpkin_data::dimension::Dimension::OVERWORLD,
        true,
        Vec::new(),
        "minecraft:plains".to_owned(),
    );
    let mut report = String::new();
    let sites = sites();
    for (index, site) in sites.iter().enumerate() {
        let seeds: Vec<u64> = (0..site.seeds).collect();
        record_site(&mut report, &world_gen, index, site, &seeds);
    }
    let sliver = site(
        "sliver",
        80,
        (-1, 3),
        Floor::Snow,
        (OPEN_ABOVE, MIXED_BELOW),
        0,
    );
    record_site(
        &mut report,
        &world_gen,
        sites.len(),
        &sliver,
        &sliver_seeds(),
    );
    report
}

#[test]
fn ice_spike() -> Result<(), Box<dyn std::error::Error>> {
    let report = report();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("ice_spike.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
