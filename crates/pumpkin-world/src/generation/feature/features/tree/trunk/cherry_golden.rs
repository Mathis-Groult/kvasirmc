//! A record of what `CherryTrunkPlacer::generate` does, made before it is split
//! (`tests/golden/cherry_trunk.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! The placer runs directly on a 3 x 3 grid of chunks, on flat ground and on hilly ground with
//! stone pillars around the trunk (so some log placements are refused), at two origins (one in
//! negative coordinates), over several placer settings and seeds. Each line has the setting, the
//! terrain, the origin, the seed, the height, every foliage attachment returned, every log
//! position returned, and every block the placer changed, as text.

use std::fmt::Write;

use pumpkin_data::{Block, BlockState};
use pumpkin_util::{
    math::{
        int_provider::{IntProvider, NormalIntProvider, UniformIntProvider},
        position::BlockPos,
    },
    random::{RandomGenerator, RandomImpl, xoroshiro128::Xoroshiro},
    world_seed::Seed,
};

use super::CherryTrunkPlacer;
use crate::generation::block_state_provider::{
    BlockStateProvider, RuleBasedBlockStateProvider, SimpleStateProvider,
};
use crate::generation::feature::features::tree::trunk::{TrunkPlacer, TrunkType};
use crate::generation::generator::WorldGenerator;
use crate::generation::proto_chunk::{GenerationCache, ProtoChunk};
use crate::generation::proto_chunk_test::stage_parity::{TestBlockRegistry, TestGridCache};
use crate::generation::{get_world_gen, positions::chunk_pos::start_block_x};

const fn uniform(min: i32, max: i32) -> IntProvider {
    IntProvider::Object(NormalIntProvider::Uniform(UniformIntProvider {
        min_inclusive: min,
        max_inclusive: max,
    }))
}

/// One placer setting: branch count, horizontal length, start offset, end offset, tree height.
struct Setting {
    name: &'static str,
    count: IntProvider,
    horizontal: IntProvider,
    start: (i32, i32),
    end: IntProvider,
    height: u32,
}

fn settings() -> Vec<Setting> {
    vec![
        Setting {
            name: "vanilla",
            count: uniform(1, 3),
            horizontal: uniform(2, 4),
            start: (-4, -3),
            end: uniform(-1, 0),
            height: 7,
        },
        Setting {
            name: "one",
            count: IntProvider::Constant(1),
            horizontal: IntProvider::Constant(3),
            start: (-4, -3),
            end: uniform(-1, 0),
            height: 8,
        },
        Setting {
            name: "two",
            count: IntProvider::Constant(2),
            horizontal: uniform(2, 4),
            start: (-3, 0),
            end: uniform(-2, 1),
            height: 6,
        },
        Setting {
            name: "three",
            count: IntProvider::Constant(3),
            horizontal: uniform(1, 3),
            start: (-5, -2),
            end: uniform(-3, 2),
            height: 9,
        },
        Setting {
            name: "short",
            count: uniform(1, 3),
            horizontal: uniform(2, 3),
            start: (-9, -6),
            end: uniform(0, 3),
            height: 4,
        },
        Setting {
            name: "tall",
            count: uniform(2, 3),
            horizontal: IntProvider::Constant(2),
            start: (-2, 1),
            end: uniform(-4, -1),
            height: 12,
        },
    ]
}

#[derive(Clone, Copy)]
enum Ground {
    Flat,
    Hilly,
}

impl Ground {
    const fn name(self) -> &'static str {
        match self {
            Self::Flat => "flat",
            Self::Hilly => "hilly",
        }
    }

    /// The first free height of the column (x, z).
    const fn top(self, x: i32, z: i32) -> i32 {
        match self {
            Self::Flat => 64,
            Self::Hilly => 64 + (x * 7 + z * 3).rem_euclid(5),
        }
    }

    /// The pillars: tall stone columns beside the trunk that refuse log placements.
    const fn pillar(self, x: i32, z: i32) -> bool {
        matches!(self, Self::Hilly) && (x * 5 + z * 11).rem_euclid(7) == 0
    }
}

fn grid(world_gen: &WorldGenerator, ground: Ground) -> TestGridCache {
    let mut chunks = std::collections::HashMap::new();
    for chunk_x in -1..=1 {
        for chunk_z in -1..=1 {
            let mut chunk = ProtoChunk::new(chunk_x, chunk_z, world_gen);
            for local_x in 0..16 {
                for local_z in 0..16 {
                    let x = start_block_x(chunk_x) + local_x;
                    let z =
                        crate::generation::positions::chunk_pos::start_block_z(chunk_z) + local_z;
                    let top = ground.top(x, z);
                    for y in 40..top {
                        chunk.set_block_state(x, y, z, Block::STONE.default_state);
                    }
                    if ground.pillar(x, z) {
                        for y in top..top + 14 {
                            chunk.set_block_state(x, y, z, Block::STONE.default_state);
                        }
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

fn state_text(grid: &TestGridCache, x: i32, y: i32, z: i32) -> u16 {
    GenerationCache::get_block_state(grid, &pumpkin_util::math::vector3::Vector3::new(x, y, z))
        .as_u16()
}

/// Every block that differs between `after` and `before`, in a fixed order.
fn changes(before: &TestGridCache, after: &TestGridCache) -> String {
    let mut text = String::new();
    for x in -16..32 {
        for z in -16..32 {
            for y in 50..110 {
                let new = state_text(after, x, y, z);
                if new != state_text(before, x, y, z) {
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

fn run_case(
    world_gen: &WorldGenerator,
    setting: &Setting,
    ground: Ground,
    origin: (i32, i32),
    seed: u64,
    dirt: &BlockStateProvider,
    trunk_state: &BlockState,
) -> String {
    let before = grid(world_gen, ground);
    let mut after = grid(world_gen, ground);
    let placer = CherryTrunkPlacer {
        count: setting.count.clone(),
        horizontal_length: setting.horizontal.clone(),
        start_offset_from_top: UniformIntProvider {
            min_inclusive: setting.start.0,
            max_inclusive: setting.start.1,
        },
        end_offset_from_top: setting.end.clone(),
    };
    let wrapper = TrunkPlacer {
        base_height: 4,
        height_rand_a: 2,
        height_rand_b: 0,
        r#type: TrunkType::Cherry(CherryTrunkPlacer {
            count: IntProvider::Constant(1),
            horizontal_length: IntProvider::Constant(1),
            start_offset_from_top: UniformIntProvider {
                min_inclusive: 0,
                max_inclusive: 0,
            },
            end_offset_from_top: IntProvider::Constant(0),
        }),
    };
    let start = BlockPos::new(origin.0, ground.top(origin.0, origin.1), origin.1);
    let mut random = RandomGenerator::Xoroshiro(Xoroshiro::from_seed(seed));
    let (nodes, logs) = placer.generate(
        &TestBlockRegistry,
        &wrapper,
        setting.height,
        start,
        &mut after,
        &mut random,
        dirt,
        trunk_state,
    );
    let mut line = format!(
        "{} {} {},{} seed {seed} h {} nodes",
        setting.name,
        ground.name(),
        origin.0,
        origin.1,
        setting.height
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
    let _ = write!(line, " blocks{}", changes(&before, &after));
    let _ = write!(line, " next {}", random.next_i32());
    line.push('\n');
    line
}

fn dirt_providers() -> [BlockStateProvider; 2] {
    [
        BlockStateProvider::Simple(SimpleStateProvider {
            state: Block::DIRT.default_state,
        }),
        BlockStateProvider::Rule(RuleBasedBlockStateProvider {
            fallback: None,
            rules: Vec::new(),
        }),
    ]
}

fn report() -> String {
    let world_gen = get_world_gen(
        Seed(1),
        pumpkin_data::dimension::Dimension::OVERWORLD,
        true,
        Vec::new(),
        "minecraft:plains".to_owned(),
    );
    let [dirt, no_dirt] = dirt_providers();
    let trunk_state = Block::CHERRY_LOG.default_state;
    let mut report = String::new();
    for setting in settings() {
        for ground in [Ground::Flat, Ground::Hilly] {
            for origin in [(8, 8), (-5, 21)] {
                for seed in 1..=5u64 {
                    // The ground below the trunk is set by the first provider on odd seeds and
                    // refused (no rule matches) on even ones.
                    let provider = if seed % 2 == 1 { &dirt } else { &no_dirt };
                    report.push_str(&run_case(
                        &world_gen,
                        &setting,
                        ground,
                        origin,
                        seed,
                        provider,
                        trunk_state,
                    ));
                }
            }
        }
    }
    report
}

#[test]
fn cherry_trunk() -> Result<(), Box<dyn std::error::Error>> {
    let report = report();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("cherry_trunk.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
