//! Powered shelves link into chains (vanilla 26.3 `ShelfBlock` + `SideChainPartBlock`), driven the
//! way a world drives them: a redstone block set under a shelf updates its neighbours.

use pumpkin_data::Block;
use pumpkin_data::block_properties::{
    AcaciaShelfLikeProperties as ShelfProperties, HorizontalFacing, SideChainPart,
};
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_world::world::BlockFlags;
use std::sync::Arc;

use crate::command::commands::script_golden::TestResult;
use crate::entity::ai::test_support;
use crate::world::World;

use SideChainPart::{Center, Left, Right, Unconnected};

const X: i32 = 300;
const Y: i32 = 100;

/// A row z-line `z`: shelf `i` stands at x = 300 + i, its redstone block one below.
fn shelf_at(i: i32, z: i32) -> BlockPos {
    BlockPos::new(X + i, Y, z)
}

fn put_shelf(world: &Arc<World>, i: i32, z: i32, facing: HorizontalFacing) {
    let mut props = ShelfProperties::default(&Block::OAK_SHELF);
    props.facing = facing;
    world.set_block_state(
        &shelf_at(i, z),
        props.to_state_id(&Block::OAK_SHELF),
        BlockFlags::NOTIFY_ALL,
    );
}

fn set_power(world: &Arc<World>, i: i32, z: i32, on: bool) {
    let below = BlockPos::new(X + i, Y - 1, z);
    let block = if on {
        &Block::REDSTONE_BLOCK
    } else {
        &Block::AIR
    };
    world.set_block_state(&below, block.default_state.id, BlockFlags::NOTIFY_ALL);
}

/// "powered/part" of each shelf in the row, e.g. `[R, C, L]`; `-` for an unpowered shelf and `.`
/// for no shelf.
fn row(world: &Arc<World>, z: i32, len: i32) -> String {
    let cells: Vec<&str> = (0..len)
        .map(|i| {
            let (block, id) = world.get_block_and_state_id(&shelf_at(i, z));
            if block.id != Block::OAK_SHELF.id {
                return ".";
            }
            let props = ShelfProperties::from_state_id(id);
            match (props.powered, props.side_chain) {
                (false, _) => "-",
                (true, Unconnected) => "U",
                (true, Right) => "R",
                (true, Center) => "C",
                (true, Left) => "L",
            }
        })
        .collect();
    format!("[{}]", cells.join(" "))
}

async fn with_world<T>(run: impl FnOnce(&Arc<World>) -> T) -> Result<T, String> {
    let folder = tempfile::tempdir().map_err(|e| e.to_string())?;
    let server = test_support::server(&folder)
        .await
        .map_err(|e| e.to_string())?;
    let world = server.worlds.load()[0].clone();
    for dx in -2..=2 {
        for dz in -2..=2 {
            let at = Vector2::new(X.div_euclid(16) + dx, dz);
            world
                .level
                .loaded_chunks
                .insert(at, pumpkin_world::chunk::ChunkData::empty_sync(at.x, at.y));
        }
    }
    let out = run(&world);
    server.stop();
    Ok(out)
}

#[tokio::test(flavor = "multi_thread")]
async fn shelves_powered_one_by_one_form_a_chain() -> TestResult {
    let seen = with_world(|world| {
        let mut seen = Vec::new();
        for i in 0..3 {
            put_shelf(world, i, 0, HorizontalFacing::North);
        }
        seen.push(row(world, 0, 3));
        for i in 0..3 {
            set_power(world, i, 0, true);
            seen.push(row(world, 0, 3));
        }
        seen
    })
    .await?;
    assert_eq!(
        seen,
        ["[- - -]", "[U - -]", "[R L -]", "[R C L]"],
        "a shelf takes its part from the powered shelves beside it"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn the_middle_shelf_powered_last_joins_both_sides() -> TestResult {
    let seen = with_world(|world| {
        for i in 0..3 {
            put_shelf(world, i, 4, HorizontalFacing::North);
        }
        set_power(world, 2, 4, true);
        set_power(world, 0, 4, true);
        let before = row(world, 4, 3);
        set_power(world, 1, 4, true);
        (before, row(world, 4, 3))
    })
    .await?;
    assert_eq!(seen, ("[U - U]".to_string(), "[R C L]".to_string()));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fourth_shelf_does_not_join_a_full_chain() -> TestResult {
    let seen = with_world(|world| {
        for i in 0..4 {
            put_shelf(world, i, 8, HorizontalFacing::North);
            set_power(world, i, 8, true);
        }
        row(world, 8, 4)
    })
    .await?;
    assert_eq!(seen, "[R C L U]");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn shelves_of_another_facing_stay_apart() -> TestResult {
    let seen = with_world(|world| {
        put_shelf(world, 0, 12, HorizontalFacing::North);
        put_shelf(world, 1, 12, HorizontalFacing::South);
        set_power(world, 0, 12, true);
        set_power(world, 1, 12, true);
        row(world, 12, 2)
    })
    .await?;
    assert_eq!(seen, "[U U]");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn losing_power_or_breaking_a_shelf_unlinks_its_neighbours() -> TestResult {
    let seen = with_world(|world| {
        for i in 0..3 {
            put_shelf(world, i, 16, HorizontalFacing::North);
            set_power(world, i, 16, true);
        }
        let full = row(world, 16, 3);
        set_power(world, 1, 16, false);
        let unpowered = row(world, 16, 3);
        set_power(world, 1, 16, true);
        let relinked = row(world, 16, 3);
        world.set_block_state(
            &shelf_at(1, 16),
            Block::AIR.default_state.id,
            BlockFlags::NOTIFY_ALL,
        );
        (full, unpowered, relinked, row(world, 16, 3))
    })
    .await?;
    assert_eq!(
        seen,
        (
            "[R C L]".to_string(),
            "[U - U]".to_string(),
            "[R C L]".to_string(),
            "[U . U]".to_string()
        )
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn shelves_facing_east_in_an_x_row_do_not_link() -> TestResult {
    let seen = with_world(|world| {
        for i in 0..2 {
            put_shelf(world, i, 20, HorizontalFacing::East);
            set_power(world, i, 20, true);
        }
        row(world, 20, 2)
    })
    .await?;
    // East-facing shelves line up along z, not x: the two neighbours in x are not connectable.
    assert_eq!(seen, "[U U]");
    Ok(())
}
