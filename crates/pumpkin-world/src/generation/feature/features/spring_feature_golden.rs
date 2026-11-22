//! A record of what `SpringFeatureFeature::generate` does, made before it is split
//! (`tests/golden/spring_feature.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Many small cells (a centre block with the block above and its four side and bottom
//! neighbours) are built in one grid of chunks, four blocks apart so that cells never touch.
//! The cells cover the block above (valid, other, air), the centre block (air, valid, other)
//! and every stone / air / glass / dirt pattern of the five neighbours (all 1024 for a valid
//! block above and an air centre, 64 sampled ones for the other pairs). Each cell runs the
//! feature over two valid-block sets (stone only; stone and dirt), `requires_block_below` on and
//! off, and every rock count and hole count from 0 to 5. A line lists the (rock, hole) pairs
//! that returned true, with `w` when the centre became the spring state, `x` when it changed
//! to something else, and `!` after a false return that changed it. The centre is reset after
//! every run.

use std::collections::HashMap;
use std::fmt::Write;

use pumpkin_data::{Block, BlockState};
use pumpkin_util::{
    math::{position::BlockPos, vector3::Vector3},
    random::{RandomGenerator, xoroshiro128::Xoroshiro},
    world_seed::Seed,
};

use super::{BlockWrapper, SpringFeatureFeature};
use crate::generation::generator::WorldGenerator;
use crate::generation::get_world_gen;
use crate::generation::proto_chunk::{GenerationCache, ProtoChunk};
use crate::generation::proto_chunk_test::stage_parity::{TestBlockRegistry, TestGridCache};

const MATERIALS: usize = 4;

fn material(index: usize) -> &'static BlockState {
    match index {
        0 => Block::STONE.default_state,
        1 => Block::AIR.default_state,
        2 => Block::GLASS.default_state,
        _ => Block::DIRT.default_state,
    }
}

const fn material_name(index: usize) -> char {
    match index {
        0 => 'S',
        1 => 'A',
        2 => 'G',
        _ => 'D',
    }
}

/// One cell: the materials above, at the centre and at west, east, north, south and below.
struct Cell {
    up: usize,
    centre: usize,
    sides: [usize; 5],
}

impl Cell {
    fn label(&self) -> String {
        let mut label = String::new();
        label.push(material_name(self.up));
        label.push(material_name(self.centre));
        label.push(':');
        for side in self.sides {
            label.push(material_name(side));
        }
        label
    }
}

fn sides_of(pattern: usize) -> [usize; 5] {
    let mut sides = [0; 5];
    let mut rest = pattern;
    for side in &mut sides {
        *side = rest % MATERIALS;
        rest /= MATERIALS;
    }
    sides
}

fn cells() -> Vec<Cell> {
    let mut cells = Vec::new();
    for up in 0..MATERIALS {
        for centre in 0..MATERIALS {
            if up == 0 && centre == 1 {
                for pattern in 0..1024 {
                    cells.push(Cell {
                        up,
                        centre,
                        sides: sides_of(pattern),
                    });
                }
            } else {
                for k in 0..64 {
                    cells.push(Cell {
                        up,
                        centre,
                        sides: sides_of((k * 17 + up * 5 + centre * 3) % 1024),
                    });
                }
            }
        }
    }
    cells
}

fn position(index: usize) -> BlockPos {
    let column = index % 11;
    let row = (index / 11) % 11;
    let layer = index / 121;
    BlockPos::new(
        -12 + 4 * column as i32,
        2 + 4 * layer as i32,
        -12 + 4 * row as i32,
    )
}

fn grid(world_gen: &WorldGenerator, cells: &[Cell]) -> TestGridCache {
    let mut chunks = HashMap::new();
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
    for (index, cell) in cells.iter().enumerate() {
        let pos = position(index);
        let places = [
            (pos.0, cell.centre),
            (Vector3::new(pos.0.x, pos.0.y + 1, pos.0.z), cell.up),
            (Vector3::new(pos.0.x - 1, pos.0.y, pos.0.z), cell.sides[0]),
            (Vector3::new(pos.0.x + 1, pos.0.y, pos.0.z), cell.sides[1]),
            (Vector3::new(pos.0.x, pos.0.y, pos.0.z - 1), cell.sides[2]),
            (Vector3::new(pos.0.x, pos.0.y, pos.0.z + 1), cell.sides[3]),
            (Vector3::new(pos.0.x, pos.0.y - 1, pos.0.z), cell.sides[4]),
        ];
        for (at, kind) in places {
            GenerationCache::set_block_state(&mut grid, &at, material(kind));
        }
    }
    grid
}

fn feature(
    set: usize,
    requires_block_below: bool,
    rock_count: i32,
    hole_count: i32,
) -> SpringFeatureFeature {
    SpringFeatureFeature {
        state: Block::WATER.default_state,
        requires_block_below,
        rock_count,
        hole_count,
        valid_blocks: if set == 0 {
            BlockWrapper::Single("minecraft:stone".to_owned())
        } else {
            BlockWrapper::Multi(vec!["stone".to_owned(), "minecraft:dirt".to_owned()])
        },
    }
}

fn run_cell(grid: &mut TestGridCache, pos: BlockPos) -> String {
    let before = GenerationCache::get_block_state(grid, &pos.0);
    let spring = Block::WATER.default_state.id.as_u16();
    let mut line = String::new();
    for set in 0..2 {
        for below in [false, true] {
            let _ = write!(line, " S{set}b{}", u8::from(below));
            for rock in 0..=5 {
                for hole in 0..=5 {
                    let mut random = RandomGenerator::Xoroshiro(Xoroshiro::from_seed(7));
                    let returned = feature(set, below, rock, hole).generate(
                        &TestBlockRegistry,
                        grid,
                        &mut random,
                        pos,
                    );
                    let after = GenerationCache::get_block_state(grid, &pos.0);
                    let changed = after != before;
                    let mark = match (returned, changed) {
                        (true, true) if after.as_u16() == spring => "w",
                        (true, true) => "x",
                        (false, true) => "!",
                        (true, false) => "=",
                        (false, false) => "",
                    };
                    if returned || changed {
                        let _ = write!(line, " {rock},{hole}{mark}");
                    }
                    GenerationCache::set_block_state(grid, &pos.0, before.to_state());
                }
            }
        }
    }
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
    let cells = cells();
    let mut grid = grid(&world_gen, &cells);
    let mut report = String::new();
    for (index, cell) in cells.iter().enumerate() {
        let line = run_cell(&mut grid, position(index));
        let _ = writeln!(report, "{} {}{line}", index, cell.label());
    }
    report
}

#[test]
fn spring_feature() -> Result<(), Box<dyn std::error::Error>> {
    let report = report();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("spring_feature.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
