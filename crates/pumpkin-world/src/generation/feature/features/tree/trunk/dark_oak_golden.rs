//! A record of what `DarkOakTrunkPlacer::generate` does, made before it is split
//! (`tests/golden/dark_oak_trunk.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! The placer runs directly on a 3 x 3 grid of chunks at four origins (one on a chunk corner, one
//! across the zero axes), over five terrains (open, mixed ground under the trunk, sparse and dense
//! obstacles of several kinds, stone pillars), five heights, four ground providers and several
//! seeds. Each line has the terrain, the origin, the seed, the height, the provider, every foliage
//! attachment returned, every log position returned, every block the placer changed and the next
//! random number, as text.

use std::fmt::Write;

use pumpkin_data::{Block, BlockState};
use pumpkin_util::{
    math::position::BlockPos,
    random::{RandomGenerator, RandomImpl, xoroshiro128::Xoroshiro},
    world_seed::Seed,
};

use super::DarkOakTrunkPlacer;
use crate::generation::block_predicate::{
    BlockPredicate, MatchingBlocksBlockPredicate, MatchingBlocksWrapper, OffsetBlocksBlockPredicate,
};
use crate::generation::block_state_provider::{
    BlockStateProvider, BlockStateRule, RuleBasedBlockStateProvider, SimpleStateProvider,
};
use crate::generation::feature::features::tree::trunk::{TrunkPlacer, TrunkType};
use crate::generation::generator::WorldGenerator;
use crate::generation::get_world_gen;
use crate::generation::proto_chunk::{GenerationCache, ProtoChunk};
use crate::generation::proto_chunk_test::stage_parity::{TestBlockRegistry, TestGridCache};

#[derive(Clone, Copy)]
enum Terrain {
    Open,
    Mixed,
    Sparse,
    Dense,
    Pillars,
}

impl Terrain {
    const ALL: [Self; 5] = [
        Self::Open,
        Self::Mixed,
        Self::Sparse,
        Self::Dense,
        Self::Pillars,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Mixed => "mixed",
            Self::Sparse => "sparse",
            Self::Dense => "dense",
            Self::Pillars => "pillars",
        }
    }
}

const fn hash(x: i32, y: i32, z: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1)
        ^ (y as u32).wrapping_mul(0x85EB_CA77)
        ^ (z as u32).wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h
}

/// The block of the untouched terrain at (x, y, z): stone below y 63, a ground layer at y 63 and
/// obstacles above it.
fn base(terrain: Terrain, x: i32, y: i32, z: i32) -> &'static Block {
    if y < 63 {
        return &Block::STONE;
    }
    if y == 63 {
        return match terrain {
            Terrain::Open => &Block::GRASS_BLOCK,
            _ => match hash(x, 0, z) % 3 {
                0 | 1 => &Block::GRASS_BLOCK,
                _ => &Block::STONE,
            },
        };
    }
    match terrain {
        Terrain::Open | Terrain::Mixed => &Block::AIR,
        Terrain::Sparse => scatter(x, y, z, 10),
        Terrain::Dense => scatter(x, y, z, 4),
        Terrain::Pillars => {
            if y < 92 && hash(x, 7, z).is_multiple_of(6) {
                &Block::STONE
            } else {
                &Block::AIR
            }
        }
    }
}

/// One obstacle in `every` cells, of one of four kinds (two the placer replaces, one log, one
/// block it refuses).
fn scatter(x: i32, y: i32, z: i32, every: u32) -> &'static Block {
    let h = hash(x, y, z);
    if !h.is_multiple_of(every) {
        return &Block::AIR;
    }
    match (h / every) % 4 {
        0 => &Block::OAK_LEAVES,
        1 => &Block::STONE,
        2 => &Block::OAK_LOG,
        _ => &Block::SHORT_GRASS,
    }
}

fn grid(world_gen: &WorldGenerator, terrain: Terrain, origin: (i32, i32)) -> TestGridCache {
    let mut chunks = std::collections::HashMap::new();
    for chunk_x in -1..=1 {
        for chunk_z in -1..=1 {
            chunks.insert(
                (chunk_x, chunk_z),
                ProtoChunk::new(chunk_x, chunk_z, world_gen),
            );
        }
    }
    let mut grid = TestGridCache {
        center_pos: (0, 0),
        chunks,
    };
    for (x, z) in window(origin) {
        for y in 60..=95 {
            let block = base(terrain, x, y, z);
            if block.id != Block::AIR.id {
                GenerationCache::set_block_state(
                    &mut grid,
                    &pumpkin_util::math::vector3::Vector3::new(x, y, z),
                    block.default_state,
                );
            }
        }
    }
    grid
}

fn window(origin: (i32, i32)) -> impl Iterator<Item = (i32, i32)> {
    (origin.0 - 6..=origin.0 + 7)
        .flat_map(move |x| (origin.1 - 6..=origin.1 + 7).map(move |z| (x, z)))
}

/// Every block of the window that differs from the untouched terrain, in a fixed order.
fn changes(grid: &TestGridCache, terrain: Terrain, origin: (i32, i32)) -> String {
    let mut text = String::new();
    for (x, z) in window(origin) {
        for y in 60..=95 {
            let new = GenerationCache::get_block_state(
                grid,
                &pumpkin_util::math::vector3::Vector3::new(x, y, z),
            );
            if new.as_u16() != base(terrain, x, y, z).default_state.id.as_u16() {
                let _ = write!(text, " {x},{y},{z}={}", new.as_u16());
            }
        }
    }
    text
}

fn positions(list: &[BlockPos]) -> String {
    let mut text = String::new();
    for pos in list {
        let _ = write!(text, " {},{},{}", pos.0.x, pos.0.y, pos.0.z);
    }
    text
}

fn simple(block: &Block) -> BlockStateProvider {
    BlockStateProvider::Simple(SimpleStateProvider {
        state: block.default_state,
    })
}

/// Four ground providers: always dirt; dirt on grass only; never; dirt on grass, coarse dirt
/// elsewhere.
fn providers() -> [BlockStateProvider; 4] {
    let on_grass = || BlockStateRule {
        if_true: BlockPredicate::MatchingBlocks(MatchingBlocksBlockPredicate {
            offset: OffsetBlocksBlockPredicate { offset: None },
            blocks: MatchingBlocksWrapper::Single("minecraft:grass_block".to_owned()),
        }),
        then: simple(&Block::DIRT),
    };
    [
        simple(&Block::DIRT),
        BlockStateProvider::Rule(RuleBasedBlockStateProvider {
            fallback: None,
            rules: vec![on_grass()],
        }),
        BlockStateProvider::Rule(RuleBasedBlockStateProvider {
            fallback: None,
            rules: Vec::new(),
        }),
        BlockStateProvider::Rule(RuleBasedBlockStateProvider {
            fallback: Some(Box::new(simple(&Block::COARSE_DIRT))),
            rules: vec![on_grass()],
        }),
    ]
}

struct Case<'a> {
    terrain: Terrain,
    origin: (i32, i32),
    seed: u64,
    height: u32,
    provider: usize,
    dirt: &'a BlockStateProvider,
}

fn run_case(world_gen: &WorldGenerator, case: &Case, trunk_state: &BlockState) -> String {
    let mut after = grid(world_gen, case.terrain, case.origin);
    let wrapper = TrunkPlacer {
        base_height: 6,
        height_rand_a: 2,
        height_rand_b: 1,
        r#type: TrunkType::DarkOak(DarkOakTrunkPlacer),
    };
    let start = BlockPos::new(case.origin.0, 64, case.origin.1);
    let mut random = RandomGenerator::Xoroshiro(Xoroshiro::from_seed(case.seed));
    let (nodes, logs) = DarkOakTrunkPlacer::generate(
        &TestBlockRegistry,
        &wrapper,
        case.height,
        start,
        &mut after,
        &mut random,
        case.dirt,
        trunk_state,
    );
    let mut line = format!(
        "{} {},{} seed {} h {} provider {} nodes",
        case.terrain.name(),
        case.origin.0,
        case.origin.1,
        case.seed,
        case.height,
        case.provider
    );
    for node in &nodes {
        let _ = write!(
            line,
            " {},{},{}/{}/{}",
            node.center.0.x,
            node.center.0.y,
            node.center.0.z,
            node.foliage_radius,
            node.giant_trunk
        );
    }
    let _ = write!(line, " logs{}", positions(&logs));
    let _ = write!(
        line,
        " blocks{}",
        changes(&after, case.terrain, case.origin)
    );
    let _ = write!(line, " next {}", random.next_i32());
    line.push('\n');
    line
}

fn report() -> String {
    let world_gen = get_world_gen(
        Seed(1),
        pumpkin_data::dimension::Dimension::OVERWORLD,
        true,
        Vec::new(),
        "minecraft:plains".to_owned(),
    );
    let providers = providers();
    let trunk_state = Block::DARK_OAK_LOG.default_state;
    let mut report = String::new();
    for (terrain_index, terrain) in Terrain::ALL.into_iter().enumerate() {
        for (origin_index, origin) in [(8, 8), (15, 15), (-1, -1), (-5, 21)]
            .into_iter()
            .enumerate()
        {
            for height in [1, 3, 6, 9, 14] {
                for seed in 1..=6u64 {
                    let provider = (seed as usize + origin_index + terrain_index) % 4;
                    report.push_str(&run_case(
                        &world_gen,
                        &Case {
                            terrain,
                            origin,
                            seed,
                            height,
                            provider,
                            dirt: &providers[provider],
                        },
                        trunk_state,
                    ));
                }
            }
        }
    }
    report
}

#[test]
fn dark_oak_trunk() -> Result<(), Box<dyn std::error::Error>> {
    let report = report();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("dark_oak_trunk.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
