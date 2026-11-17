//! A record of fluid spreading choices (`tests/golden/fluid_spread.txt`; rewrite it on purpose
//! with `KVASIR_BLESS=1`), made before `get_in_flow_down_distance_iterative` was split.
//!
//! Each case is an ASCII map laid on a stone floor in its own area of empty chunks: `C` the fluid
//! source being spread, `#` a stone wall, `S` another source of the same fluid, `7` / `6` / `5` /
//! `1` the same fluid flowing at that level, `F` falling flowing fluid, `Z` a falling source, `o` a
//! source of the other fluid, `W` a waterlogged slab, `D` a door, `g` short grass, `c` a carpet,
//! `.` open floor; floor cells: `H` a hole (air), `B` a source of the same fluid, `K` a falling
//! source. Per case: the spread directions `get_spread` returns with their new state ids, and the
//! down-flow distance seen from each side. Lava runs in the overworld (reach 3) and the nether
//! (reach 5).

use std::fmt::Write;
use std::sync::Arc;

use pumpkin_data::block_properties::WhiteWoolSlabLikeProperties;
use pumpkin_data::fluid::{Fluid, FluidProperties, Level};
use pumpkin_data::{Block, BlockDirection, BlockStateId};
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_world::world::BlockFlags;

use super::{get_in_flow_down_distance_iterative, get_spread};
use crate::block::fluid::flowing_trait::FlowingFluid;
use crate::block::fluid::lava::FlowingLava;
use crate::block::fluid::water::FlowingWater;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;
use crate::world::World;

const SIDES: [BlockDirection; 4] = [
    BlockDirection::North,
    BlockDirection::South,
    BlockDirection::West,
    BlockDirection::East,
];

/// Name and map rows (north first); every map is 13 wide with `C` in the middle.
const MAPS: &[(&str, [&str; 13])] = &[
    (
        "flat",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "......C......",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "hole east 1",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "......CH.....",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "hole north 2",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            "......H......",
            ".............",
            "......C......",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "hole west 4",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "..H...C......",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "hole south 5",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "......C......",
            ".............",
            ".............",
            ".............",
            ".............",
            "......H......",
            ".............",
        ],
    ),
    (
        "two holes same distance",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            "......H......",
            ".............",
            "......C......",
            ".............",
            "......H......",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "diagonal hole",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            "........H....",
            ".............",
            "......C......",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "hole behind a wall",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "......C#H....",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "wall with a gap",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".......#.....",
            ".......#.....",
            "......C#H....",
            ".......#.....",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "source in the way",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "......CSH....",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "corridor",
        [
            "#############",
            "#############",
            "#############",
            "#############",
            "#############",
            "#############",
            "#.....C...H.#",
            "#############",
            "#############",
            "#############",
            "#############",
            "#############",
            "#############",
        ],
    ),
    (
        "walled in",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "......#......",
            ".....#C#.....",
            "......#......",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "maze",
        [
            ".............",
            ".............",
            ".............",
            "....#####....",
            "....#...#....",
            "....#.#.#....",
            "....#.C.#H...",
            "....#.###....",
            "....#........",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "holes all around",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "......H......",
            ".....HCH.....",
            "......H......",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "hole west 3",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "...H..C......",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "hole east 6",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "......C.....H",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "flowing neighbours",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "......7......",
            ".....FCZ.....",
            "......1......",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "flowing neighbours before holes",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            "......H......",
            "......7......",
            "....HFCZH....",
            "......1......",
            "......H......",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "blockers",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "......W......",
            ".....gCD.....",
            "......c......",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "blockers before holes",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            "......H......",
            "......W......",
            "....HgCDH....",
            "......c......",
            "......H......",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "other fluid",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "......o......",
            ".....oC.H....",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "path through flowing and falling",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            "......H......",
            "......Z......",
            "..H.1C.FH....",
            "......7......",
            "......H......",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "fluid floors",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "......B......",
            ".....BC.K....",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "path blocked by a waterlogged slab",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "......####...",
            "......C.WH...",
            "......####...",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "open field one far hole",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "......C......",
            ".............",
            ".............",
            "..........H..",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "corridor through S",
        [
            "#############",
            "#############",
            "#############",
            "#############",
            "#############",
            "#############",
            "######C.SH###",
            "#############",
            "#############",
            "#############",
            "#############",
            "#############",
            "#############",
        ],
    ),
    (
        "corridor through 7",
        [
            "#############",
            "#############",
            "#############",
            "#############",
            "#############",
            "#############",
            "######C.7H###",
            "#############",
            "#############",
            "#############",
            "#############",
            "#############",
            "#############",
        ],
    ),
    (
        "corridor through F",
        [
            "#############",
            "#############",
            "#############",
            "#############",
            "#############",
            "#############",
            "######C.FH###",
            "#############",
            "#############",
            "#############",
            "#############",
            "#############",
            "#############",
        ],
    ),
    (
        "corridor through Z",
        [
            "#############",
            "#############",
            "#############",
            "#############",
            "#############",
            "#############",
            "######C.ZH###",
            "#############",
            "#############",
            "#############",
            "#############",
            "#############",
            "#############",
        ],
    ),
    (
        "lower flowing neighbours",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "......6......",
            ".....6C5.....",
            "......5......",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "hole at +2,+3",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "......C......",
            ".............",
            ".............",
            "........H....",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "hole at +3,+2",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "......C......",
            ".............",
            ".........H...",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "hole at +1,+4",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "......C......",
            ".............",
            ".............",
            ".............",
            ".......H.....",
            ".............",
            ".............",
        ],
    ),
    (
        "hole at +4,+1",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "......C......",
            "..........H..",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "hole at -2,+3",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "......C......",
            ".............",
            ".............",
            "....H........",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "hole at -3,-2",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            "...H.........",
            ".............",
            "......C......",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "hole at +2,+2",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "......C......",
            ".............",
            "........H....",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "hole at +1,+3",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "......C......",
            ".............",
            ".............",
            ".......H.....",
            ".............",
            ".............",
            ".............",
        ],
    ),
    (
        "hole at -3,+1",
        [
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
            "......C......",
            "...H.........",
            ".............",
            ".............",
            ".............",
            ".............",
            ".............",
        ],
    ),
];

fn put(world: &Arc<World>, pos: &BlockPos, state: BlockStateId) {
    world.set_block_state(
        pos,
        state,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

/// The states a map's letters stand for, for one fluid.
struct Cells {
    source: BlockStateId,
    flowing_7: BlockStateId,
    flowing_1: BlockStateId,
    flowing_6: BlockStateId,
    flowing_5: BlockStateId,
    falling: BlockStateId,
    falling_source: BlockStateId,
    other: BlockStateId,
}

impl Cells {
    fn of<T: FlowingFluid>(
        fluid_impl: &T,
        fluid: &Fluid,
        source: BlockStateId,
        other: BlockStateId,
    ) -> Self {
        Self {
            source,
            flowing_7: fluid_impl
                .get_flowing(fluid, Level::L7, false)
                .to_state_id(fluid),
            flowing_1: fluid_impl
                .get_flowing(fluid, Level::L1, false)
                .to_state_id(fluid),
            flowing_6: fluid_impl
                .get_flowing(fluid, Level::L6, false)
                .to_state_id(fluid),
            flowing_5: fluid_impl
                .get_flowing(fluid, Level::L5, false)
                .to_state_id(fluid),
            falling: fluid_impl
                .get_flowing(fluid, Level::L7, true)
                .to_state_id(fluid),
            falling_source: fluid_impl.get_source(fluid, true).to_state_id(fluid),
            other,
        }
    }

    fn floor(&self, cell: char) -> BlockStateId {
        match cell {
            'H' => Block::AIR.default_state.id,
            'B' => self.source,
            'K' => self.falling_source,
            _ => Block::STONE.default_state.id,
        }
    }

    fn above(&self, cell: char) -> BlockStateId {
        match cell {
            '#' => Block::STONE.default_state.id,
            'S' | 'C' => self.source,
            '7' => self.flowing_7,
            '6' => self.flowing_6,
            '5' => self.flowing_5,
            '1' => self.flowing_1,
            'F' => self.falling,
            'Z' => self.falling_source,
            'o' => self.other,
            'W' => {
                let mut slab = WhiteWoolSlabLikeProperties::default(&Block::OAK_SLAB);
                slab.waterlogged = true;
                slab.to_state_id(&Block::OAK_SLAB)
            }
            'D' => Block::OAK_DOOR.default_state.id,
            'g' => Block::SHORT_GRASS.default_state.id,
            'c' => Block::WHITE_CARPET.default_state.id,
            _ => Block::AIR.default_state.id,
        }
    }
}

/// Lays the map with its north-west corner at (`x0`, 64, 0) and returns the centre.
fn lay(world: &Arc<World>, x0: i32, rows: &[&str; 13], cells: &Cells) -> BlockPos {
    let mut centre = BlockPos::new(x0 + 6, 64, 6);
    for (dz, row) in rows.iter().enumerate() {
        for (dx, cell) in row.chars().enumerate() {
            let (x, z) = (x0 + dx as i32, dz as i32);
            put(world, &BlockPos::new(x, 63, z), cells.floor(cell));
            put(world, &BlockPos::new(x, 64, z), cells.above(cell));
            if cell == 'C' {
                centre = BlockPos::new(x, 64, z);
            }
        }
    }
    centre
}

fn record<T: FlowingFluid + Sync>(
    world: &Arc<World>,
    fluid_impl: &T,
    fluid: &Fluid,
    centre: BlockPos,
) -> String {
    let (spread, count) = get_spread(fluid_impl, world, fluid, &centre);
    let chosen: Vec<String> = spread[..count]
        .iter()
        .map(|(direction, state)| format!("{direction:?}={state}"))
        .collect();
    let distances: Vec<String> = SIDES
        .iter()
        .map(|direction| {
            let side = centre.offset(direction.to_offset());
            let distance = get_in_flow_down_distance_iterative(
                fluid_impl,
                world,
                fluid,
                side,
                direction.opposite(),
            );
            format!("{direction:?}:{distance}")
        })
        .collect();
    format!(
        "spread [{}] distances {}",
        chosen.join(" "),
        distances.join(" ")
    )
}

fn load_area(world: &Arc<World>) {
    for cx in -1..=60 {
        for cz in -1..=1 {
            world.level.loaded_chunks.insert(
                Vector2::new(cx, cz),
                pumpkin_world::chunk::ChunkData::empty_sync(cx, cz),
            );
        }
    }
}

/// Records every map for one fluid in one world, 16 blocks apart.
fn record_all<T: FlowingFluid + Sync>(
    out: &mut String,
    label: &str,
    world: &Arc<World>,
    (fluid_impl, fluid): (&T, &Fluid),
    cells: &Cells,
) -> std::fmt::Result {
    for (index, (name, rows)) in MAPS.iter().enumerate() {
        let x0 = index as i32 * 16;
        let centre = lay(world, x0, rows, cells);
        let line = record(world, fluid_impl, fluid, centre);
        writeln!(out, "{label} {name}: {line}")?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn fluid_spread_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let worlds = server.worlds.load();
    let overworld = worlds[0].clone();
    let nether = worlds
        .iter()
        .find(|world| world.dimension.fast_lava)
        .cloned()
        .ok_or("no nether world")?;
    load_area(&overworld);
    load_area(&nether);

    let water = Block::WATER.default_state.id;
    let lava = Block::LAVA.default_state.id;
    let water_cells = Cells::of(&FlowingWater, &Fluid::FLOWING_WATER, water, lava);
    let lava_cells = Cells::of(&FlowingLava, &Fluid::FLOWING_LAVA, lava, water);
    let mut now = String::new();
    let water_fluid = (&FlowingWater, &Fluid::FLOWING_WATER);
    let lava_fluid = (&FlowingLava, &Fluid::FLOWING_LAVA);
    record_all(&mut now, "water", &overworld, water_fluid, &water_cells)?;
    // Lava after water in the same areas: every cell is rewritten by `lay`.
    record_all(&mut now, "lava", &overworld, lava_fluid, &lava_cells)?;
    record_all(&mut now, "nether lava", &nether, lava_fluid, &lava_cells)?;

    script_golden::bless_or_compare("fluid_spread.txt", &now)?;
    server.stop();
    Ok(())
}
