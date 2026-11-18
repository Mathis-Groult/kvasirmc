//! `World::break_block` leaves the fluid the block held, like vanilla 26.3 `Level.destroyBlock`
//! (`setBlock(pos, getFluidState(pos).createLegacyBlock(), ..)`; card #77).

use std::sync::Arc;

use pumpkin_data::block_properties::{WaterLikeProperties, WhiteWoolSlabLikeProperties};
use pumpkin_data::{Block, BlockStateId};
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_world::world::BlockFlags;

use crate::command::commands::script_golden::TestResult;
use crate::entity::ai::test_support;
use crate::world::World;

const AT: BlockPos = BlockPos::new(8, 64, 8);

fn waterlogged_slab() -> BlockStateId {
    let mut slab = WhiteWoolSlabLikeProperties::default(&Block::OAK_SLAB);
    slab.waterlogged = true;
    slab.to_state_id(&Block::OAK_SLAB)
}

fn water(level: u8) -> BlockStateId {
    WaterLikeProperties { level }.to_state_id(&Block::WATER)
}

/// Places `state` on stone in an empty loaded chunk, breaks it with no cause and returns the
/// state left there.
fn left_after_break(world: &Arc<World>, state: BlockStateId) -> BlockStateId {
    let flags = BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK;
    world.set_block_state(&AT.down(), Block::STONE.default_state.id, flags);
    world.set_block_state(&AT, state, flags);
    world.break_block(&AT, None, BlockFlags::NOTIFY_ALL | BlockFlags::SKIP_DROPS);
    world.get_block_state_id(&AT)
}

#[tokio::test(flavor = "multi_thread")]
async fn breaking_a_block_leaves_the_fluid_it_held() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let chunk = Vector2::new(0, 0);
    world.level.loaded_chunks.insert(
        chunk,
        pumpkin_world::chunk::ChunkData::empty_sync(chunk.x, chunk.y),
    );
    let cases = [
        ("waterlogged slab", waterlogged_slab(), water(0)),
        ("kelp", Block::KELP.default_state.id, water(0)),
        ("seagrass", Block::SEAGRASS.default_state.id, water(0)),
        ("water source", water(0), water(0)),
        ("flowing water level 3", water(3), water(3)),
        (
            "stone",
            Block::STONE.default_state.id,
            Block::AIR.default_state.id,
        ),
    ];
    let mut wrong = Vec::new();
    for (name, placed, expected) in cases {
        let left = left_after_break(&world, placed);
        if left != expected {
            wrong.push(format!(
                "{name}: left {} ({:?}), expected {} ({:?})",
                Block::from_state_id(left).name,
                left,
                Block::from_state_id(expected).name,
                expected
            ));
        }
    }
    server.stop();
    if wrong.is_empty() {
        Ok(())
    } else {
        Err(wrong.join("; ").into())
    }
}
