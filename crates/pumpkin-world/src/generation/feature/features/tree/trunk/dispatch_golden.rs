//! A record of what `TrunkType::generate` (and `TrunkPlacer::generate`, which forwards to it)
//! does for every trunk variant, made before it is split
//! (`tests/golden/trunk_dispatch.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Every variant runs on a 3 x 3 grid of chunks with flat ground, at two origins (one in negative
//! coordinates), over several heights and seeds, once straight through `TrunkType::generate` and
//! (fewer cases) through the `TrunkPlacer` wrapper. The ground below the trunk is set by the
//! first provider on odd seeds and refused (no rule matches) on even ones. Each line has the
//! variant, the path, the origin, the seed, the height, every foliage attachment returned, every
//! log position returned, every block the call changed, and the next random number.

use std::fmt::Write;

use pumpkin_data::{Block, BlockState};
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
    TrunkPlacer, TrunkType, bending::BendingTrunkPlacer, cherry::CherryTrunkPlacer,
    dark_oak::DarkOakTrunkPlacer, fancy::FancyTrunkPlacer, forking::ForkingTrunkPlacer,
    giant::GiantTrunkPlacer, mega_jungle::MegaJungleTrunkPlacer, poplar::PoplarTrunkPlacer,
    straight::StraightTrunkPlacer, upwards_branching::UpwardsBranchingTrunkPlacer,
};
use crate::generation::block_state_provider::{
    BlockStateProvider, RuleBasedBlockStateProvider, SimpleStateProvider,
};
use crate::generation::generator::WorldGenerator;
use crate::generation::proto_chunk::{GenerationCache, ProtoChunk};
use crate::generation::proto_chunk_test::stage_parity::{TestBlockRegistry, TestGridCache};
use crate::generation::{
    get_world_gen,
    positions::chunk_pos::{start_block_x, start_block_z},
};

const GROUND: i32 = 64;
const NAMES: [&str; 10] = [
    "straight",
    "forking",
    "giant",
    "mega_jungle",
    "dark_oak",
    "fancy",
    "bending",
    "upwards_branching",
    "cherry",
    "poplar",
];

const fn uniform(min: i32, max: i32) -> IntProvider {
    IntProvider::Object(NormalIntProvider::Uniform(UniformIntProvider {
        min_inclusive: min,
        max_inclusive: max,
    }))
}

/// A fresh trunk type of variant `index` (`NAMES` order); the settings differ from vanilla's on
/// purpose so that a mix-up between two variants shows.
fn variant(index: usize) -> TrunkType {
    match index {
        0 => TrunkType::Straight(StraightTrunkPlacer),
        1 => TrunkType::Forking(ForkingTrunkPlacer),
        2 => TrunkType::Giant(GiantTrunkPlacer),
        3 => TrunkType::MegaJungle(MegaJungleTrunkPlacer),
        4 => TrunkType::DarkOak(DarkOakTrunkPlacer),
        5 => TrunkType::Fancy(FancyTrunkPlacer),
        6 => TrunkType::Bending(BendingTrunkPlacer {
            min_height_for_leaves: 3,
            bend_length: uniform(1, 3),
        }),
        7 => TrunkType::UpwardsBranching(UpwardsBranchingTrunkPlacer {
            extra_branch_steps: uniform(1, 4),
            place_branch_per_log_probability: 0.6,
            extra_branch_length: uniform(0, 3),
            can_grow_through: &[],
        }),
        8 => TrunkType::Cherry(CherryTrunkPlacer {
            count: uniform(1, 3),
            horizontal_length: uniform(2, 4),
            start_offset_from_top: UniformIntProvider {
                min_inclusive: -4,
                max_inclusive: -3,
            },
            end_offset_from_top: uniform(-1, 0),
        }),
        _ => TrunkType::Poplar(PoplarTrunkPlacer {
            trunk_height_above_branches: uniform(1, 3),
            branch_amount: uniform(2, 4),
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

/// Every block that differs between `after` and `before` around the origin, in a fixed order.
fn changes(before: &TestGridCache, after: &TestGridCache, origin: (i32, i32)) -> String {
    let mut text = String::new();
    for x in origin.0 - 12..origin.0 + 13 {
        for z in origin.1 - 12..origin.1 + 13 {
            for y in GROUND - 3..GROUND + 24 {
                let new = state_id(after, x, y, z);
                if new != state_id(before, x, y, z) {
                    let _ = write!(text, " {x},{y},{z}={new}");
                }
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

/// What one case runs: the variant, the path into the engine, where, which seed, how tall.
#[derive(Clone, Copy)]
struct Case {
    index: usize,
    wrapped: bool,
    origin: (i32, i32),
    seed: u64,
    height: u32,
}

struct Inputs {
    world_gen: Box<WorldGenerator>,
    dirt: BlockStateProvider,
    no_dirt: BlockStateProvider,
    trunk_state: &'static BlockState,
}

fn run_case(inputs: &Inputs, case: Case) -> String {
    let before = grid(&inputs.world_gen);
    let mut after = grid(&inputs.world_gen);
    let wrapper = TrunkPlacer {
        base_height: 4,
        height_rand_a: 2,
        height_rand_b: 1,
        r#type: variant(case.index),
    };
    let start = BlockPos::new(case.origin.0, GROUND, case.origin.1);
    let mut random = RandomGenerator::Xoroshiro(Xoroshiro::from_seed(case.seed));
    let provider = if case.seed % 2 == 1 {
        &inputs.dirt
    } else {
        &inputs.no_dirt
    };
    let (nodes, logs) = if case.wrapped {
        wrapper.generate(
            &TestBlockRegistry,
            case.height,
            start,
            &mut after,
            &mut random,
            provider,
            inputs.trunk_state,
        )
    } else {
        variant(case.index).generate(
            &TestBlockRegistry,
            &wrapper,
            case.height,
            start,
            &mut after,
            &mut random,
            provider,
            inputs.trunk_state,
        )
    };
    let mut line = format!(
        "{} {} {},{} seed {} h {} nodes",
        NAMES[case.index],
        if case.wrapped { "placer" } else { "type" },
        case.origin.0,
        case.origin.1,
        case.seed,
        case.height
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
    let _ = write!(line, " blocks{}", changes(&before, &after, case.origin));
    let _ = write!(line, " next {}", random.next_i32());
    line.push('\n');
    line
}

fn inputs() -> Inputs {
    Inputs {
        world_gen: get_world_gen(
            Seed(1),
            pumpkin_data::dimension::Dimension::OVERWORLD,
            true,
            Vec::new(),
            "minecraft:plains".to_owned(),
        ),
        dirt: BlockStateProvider::Simple(SimpleStateProvider {
            state: Block::DIRT.default_state,
        }),
        no_dirt: BlockStateProvider::Rule(RuleBasedBlockStateProvider {
            fallback: None,
            rules: Vec::new(),
        }),
        trunk_state: Block::OAK_LOG.default_state,
    }
}

/// Straight through the type: two origins x four seeds x three heights.
fn type_cases(index: usize) -> Vec<Case> {
    let mut cases = Vec::new();
    for origin in [(8, 8), (-5, 21)] {
        for seed in 1..=4u64 {
            for height in [5, 9, 13] {
                cases.push(Case {
                    index,
                    wrapped: false,
                    origin,
                    seed,
                    height,
                });
            }
        }
    }
    cases
}

/// Through the wrapper: one origin x two seeds x two heights.
fn wrapped_cases(index: usize) -> Vec<Case> {
    let mut cases = Vec::new();
    for seed in [3u64, 6] {
        for height in [6, 11] {
            cases.push(Case {
                index,
                wrapped: true,
                origin: (-5, 21),
                seed,
                height,
            });
        }
    }
    cases
}

fn report() -> String {
    let inputs = inputs();
    let mut report = String::new();
    for index in 0..NAMES.len() {
        for case in type_cases(index).into_iter().chain(wrapped_cases(index)) {
            report.push_str(&run_case(&inputs, case));
        }
    }
    report
}

#[test]
fn trunk_dispatch() -> Result<(), Box<dyn std::error::Error>> {
    let report = report();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("trunk_dispatch.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
