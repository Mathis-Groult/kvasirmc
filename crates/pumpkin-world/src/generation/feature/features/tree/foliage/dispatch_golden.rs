//! A record of what `FoliageType::generate` does for every foliage variant, made before it is
//! split (`tests/golden/foliage_dispatch.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Every variant runs on a 3 x 3 grid of chunks with flat ground, with the tree node above it, at
//! two origins (one in negative coordinates), over two seeds and six (height, radius, offset,
//! giant trunk) edge sets. Each line has the variant, the origin, the seed, the parameters, every
//! position returned, how many blocks the call changed with a hash of them, and the next random
//! number.

use std::fmt::Write;

use pumpkin_data::Block;
use pumpkin_util::{
    math::{
        int_provider::{IntProvider, NormalIntProvider, UniformIntProvider},
        position::BlockPos,
        vector3::Vector3,
    },
    random::{RandomGenerator, RandomImpl, xoroshiro128::Xoroshiro},
    world_seed::Seed,
};

use super::{
    FoliageType, acacia::AcaciaFoliagePlacer, blob::BlobFoliagePlacer, bush::BushFoliagePlacer,
    cherry::CherryFoliagePlacer, dark_oak::DarkOakFoliagePlacer, fancy::LargeOakFoliagePlacer,
    jungle::JungleFoliagePlacer, mega_pine::MegaPineFoliagePlacer, pine::PineFoliagePlacer,
    poplar::PoplarFoliagePlacer, random_spread::RandomSpreadFoliagePlacer,
    spruce::SpruceFoliagePlacer,
};
use crate::generation::block_state_provider::{BlockStateProvider, SimpleStateProvider};
use crate::generation::feature::features::tree::TreeNode;
use crate::generation::generator::WorldGenerator;
use crate::generation::proto_chunk::{GenerationCache, ProtoChunk};
use crate::generation::proto_chunk_test::stage_parity::TestGridCache;
use crate::generation::{
    get_world_gen,
    positions::chunk_pos::{start_block_x, start_block_z},
};

const GROUND: i32 = 64;
const NAMES: [&str; 12] = [
    "blob",
    "spruce",
    "pine",
    "acacia",
    "bush",
    "fancy",
    "jungle",
    "mega_pine",
    "dark_oak",
    "random_spread",
    "cherry",
    "poplar",
];

/// (foliage height, radius, offset, giant trunk): zero and growing values, with and without a
/// giant trunk.
const PARAMS: [(i32, i32, i32, bool); 6] = [
    (0, 0, 0, false),
    (1, 1, 0, false),
    (3, 2, 1, false),
    (4, 3, 0, true),
    (5, 3, 2, true),
    (2, 1, 0, true),
];

const fn uniform(min: i32, max: i32) -> IntProvider {
    IntProvider::Object(NormalIntProvider::Uniform(UniformIntProvider {
        min_inclusive: min,
        max_inclusive: max,
    }))
}

/// A fresh foliage type of variant `index` (`NAMES` order); the settings differ from vanilla's on
/// purpose so that a mix-up between two variants shows.
fn variant(index: usize) -> FoliageType {
    match index {
        0 => FoliageType::Blob(BlobFoliagePlacer { height: 3 }),
        1 => FoliageType::Spruce(SpruceFoliagePlacer {
            trunk_height: uniform(1, 3),
        }),
        2 => FoliageType::Pine(PineFoliagePlacer {
            height: uniform(2, 4),
        }),
        3 => FoliageType::Acacia(AcaciaFoliagePlacer),
        4 => FoliageType::Bush(BushFoliagePlacer { height: 2 }),
        5 => FoliageType::Fancy(LargeOakFoliagePlacer { height: 4 }),
        6 => FoliageType::Jungle(JungleFoliagePlacer { height: 2 }),
        7 => FoliageType::MegaPine(MegaPineFoliagePlacer {
            crown_height: uniform(3, 6),
        }),
        8 => FoliageType::DarkOak(DarkOakFoliagePlacer),
        9 => FoliageType::RandomSpread(RandomSpreadFoliagePlacer {
            foliage_height: uniform(2, 4),
            leaf_placement_attempts: 12,
        }),
        10 => FoliageType::Cherry(CherryFoliagePlacer {
            height: uniform(4, 5),
            wide_bottom_layer_hole_chance: 0.25,
            corner_hole_chance: 0.5,
            hanging_leaves_chance: 0.5,
            hanging_leaves_extension_chance: 0.33,
        }),
        _ => FoliageType::Poplar(PoplarFoliagePlacer {
            height: uniform(3, 5),
            side_hole_chance: 0.25,
        }),
    }
}

fn grid(world_gen: &WorldGenerator) -> TestGridCache {
    let mut chunks = std::collections::HashMap::new();
    for chunk_x in -1..=1 {
        for chunk_z in -1..=1 {
            let mut chunk = ProtoChunk::new(chunk_x, chunk_z, world_gen);
            for local_x in 0..16 {
                for local_z in 0..16 {
                    let x = start_block_x(chunk_x) + local_x;
                    let z = start_block_z(chunk_z) + local_z;
                    for y in 40..GROUND {
                        chunk.set_block_state(x, y, z, Block::STONE.default_state);
                    }
                }
            }
            chunks.insert((chunk_x, chunk_z), chunk);
        }
    }
    TestGridCache {
        center_pos: (0, 0),
        chunks,
    }
}

fn state_id(grid: &TestGridCache, x: i32, y: i32, z: i32) -> u16 {
    GenerationCache::get_block_state(grid, &Vector3::new(x, y, z)).as_u16()
}

/// How many blocks differ between `after` and `before` around the origin, and a hash of them.
fn changes(before: &TestGridCache, after: &TestGridCache, origin: (i32, i32)) -> String {
    let mut count = 0u32;
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for x in origin.0 - 14..origin.0 + 15 {
        for z in origin.1 - 14..origin.1 + 15 {
            for y in GROUND - 3..GROUND + 30 {
                let new = state_id(after, x, y, z);
                if new != state_id(before, x, y, z) {
                    count += 1;
                    for value in [x, y, z, i32::from(new)] {
                        hash = (hash ^ u64::from(value.cast_unsigned()))
                            .wrapping_mul(0x0100_0000_01b3);
                    }
                }
            }
        }
    }
    format!("{count} {hash:016x}")
}

fn positions(list: &[BlockPos]) -> String {
    let mut text = String::new();
    for pos in list {
        let _ = write!(text, " {},{},{}", pos.0.x, pos.0.y, pos.0.z);
    }
    text
}

/// What one case runs: the variant, where, which seed, which parameter set.
#[derive(Clone, Copy)]
struct Case {
    index: usize,
    origin: (i32, i32),
    seed: u64,
    params: (i32, i32, i32, bool),
}

fn run_case(world_gen: &WorldGenerator, leaves: &BlockStateProvider, case: Case) -> String {
    let (height, radius, offset, giant_trunk) = case.params;
    let before = grid(world_gen);
    let mut after = grid(world_gen);
    let node = TreeNode {
        center: BlockPos::new(case.origin.0, GROUND + 6, case.origin.1),
        foliage_radius: radius,
        giant_trunk,
    };
    let mut random = RandomGenerator::Xoroshiro(Xoroshiro::from_seed(case.seed));
    let placed = variant(case.index).generate(
        &mut after,
        &mut random,
        &node,
        height,
        radius,
        offset,
        leaves,
    );
    let mut line = format!(
        "{} {},{} seed {} h {} r {} o {} giant {} placed",
        NAMES[case.index],
        case.origin.0,
        case.origin.1,
        case.seed,
        height,
        radius,
        offset,
        giant_trunk
    );
    let _ = write!(line, "{}", positions(&placed));
    let _ = write!(line, " blocks {}", changes(&before, &after, case.origin));
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
    let leaves = BlockStateProvider::Simple(SimpleStateProvider {
        state: Block::OAK_LEAVES.default_state,
    });
    let mut report = String::new();
    for index in 0..NAMES.len() {
        for origin in [(8, 8), (-5, 21)] {
            for seed in [1u64, 2] {
                for params in PARAMS {
                    let case = Case {
                        index,
                        origin,
                        seed,
                        params,
                    };
                    report.push_str(&run_case(&world_gen, &leaves, case));
                }
            }
        }
    }
    report
}

#[test]
fn foliage_dispatch() -> Result<(), Box<dyn std::error::Error>> {
    let report = report();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("foliage_dispatch.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
