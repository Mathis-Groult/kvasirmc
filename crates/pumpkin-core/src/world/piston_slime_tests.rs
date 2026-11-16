//! A sticky piston moving an L of slime blocks (card #52): every slime block must arrive,
//! none may vanish or turn into an item, both when pushed and when pulled back.

use std::sync::Arc;

use pumpkin_data::{Block, BlockDirection, FacingExt};
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::world::BlockFlags;

use crate::block::blocks::piston::piston::PistonProps;
use crate::entity::ai::test_support;
use crate::server::Server;
use crate::world::World;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const Y: i32 = 64;

fn at(x: i32, y: i32) -> BlockPos {
    BlockPos(Vector3::new(x, y, 8))
}

fn put(world: &Arc<World>, pos: BlockPos, state: pumpkin_data::BlockStateId) {
    world.set_block_state(
        &pos,
        state,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

/// The L: three slime blocks in front of the piston and two more up from the last one.
fn l_shape(x0: i32) -> Vec<BlockPos> {
    vec![
        at(x0, Y),
        at(x0 + 1, Y),
        at(x0 + 2, Y),
        at(x0 + 2, Y + 1),
        at(x0 + 2, Y + 2),
    ]
}

fn layout(world: &Arc<World>) -> String {
    let mut rows = Vec::new();
    for y in (Y..=Y + 2).rev() {
        let row: String = (1..=8)
            .map(|x| {
                let block = world.get_block(&at(x, y));
                if block == &Block::SLIME_BLOCK {
                    'S'
                } else if block == &Block::MOVING_PISTON {
                    'm'
                } else if block == &Block::STICKY_PISTON {
                    'P'
                } else if block == &Block::PISTON_HEAD {
                    'h'
                } else if block == &Block::REDSTONE_BLOCK {
                    'R'
                } else if block.default_state.is_air() {
                    '.'
                } else {
                    '?'
                }
            })
            .collect();
        rows.push(row);
    }
    rows.join("/")
}

fn run_ticks(world: &Arc<World>, server: &Arc<Server>, ticks: usize) {
    for _ in 0..ticks {
        world.tick(server);
        let chunks: rustc_hash::FxHashSet<Vector2<i32>> =
            std::iter::once(Vector2::new(0, 0)).collect();
        let entities = world.collect_block_entities(&Arc::new(chunks));
        world.tick_block_entities(&entities);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sticky_piston_moves_an_l_of_slime_blocks_without_losing_any() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server
        .worlds
        .load()
        .first()
        .cloned()
        .ok_or("the server has no world")?;
    world.level.loaded_chunks.insert(
        Vector2::new(0, 0),
        pumpkin_world::chunk::ChunkData::empty_sync(0, 0),
    );
    let mut props = PistonProps::default(&Block::STICKY_PISTON);
    props.facing = BlockDirection::East.to_facing();
    put(&world, at(2, Y), props.to_state_id(&Block::STICKY_PISTON));
    for pos in l_shape(3) {
        put(&world, pos, Block::SLIME_BLOCK.default_state.id);
    }
    let start = layout(&world);

    world.set_block_state(
        &at(1, Y),
        Block::REDSTONE_BLOCK.default_state.id,
        BlockFlags::NOTIFY_ALL,
    );
    run_ticks(&world, &server, 6);
    let pushed = layout(&world);
    let missing: Vec<_> = l_shape(4)
        .into_iter()
        .filter(|pos| world.get_block(pos) != &Block::SLIME_BLOCK)
        .collect();
    assert!(
        missing.is_empty(),
        "after the push {missing:?} are not slime: {start} -> {pushed}"
    );

    world.set_block_state(
        &at(1, Y),
        Block::AIR.default_state.id,
        BlockFlags::NOTIFY_ALL,
    );
    run_ticks(&world, &server, 6);
    let pulled = layout(&world);
    let missing: Vec<_> = l_shape(3)
        .into_iter()
        .filter(|pos| world.get_block(pos) != &Block::SLIME_BLOCK)
        .collect();
    assert!(
        missing.is_empty(),
        "after the pull {missing:?} are not slime: {start} -> {pushed} -> {pulled}"
    );
    server.stop();
    Ok(())
}
