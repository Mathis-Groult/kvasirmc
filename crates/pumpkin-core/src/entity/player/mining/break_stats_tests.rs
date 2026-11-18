//! Block-break statistics as vanilla counts them (card #85): `Mined` only when the block was
//! broken with what harvests it, `Used` only for an item with a tool component.

use std::sync::atomic::Ordering;

use pumpkin_data::Block;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_protocol::bedrock::server::player_action::{PlayerActionType, SPlayerAction};
use pumpkin_protocol::codec::var_int::VarInt as BedrockVarInt;
use pumpkin_protocol::codec::var_ulong::VarULong;
use pumpkin_protocol::java::server::play::SPlayerAction as SJavaPlayerAction;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::{GameMode, Hand};
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use crate::entity::EntityBase;
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::player::statistics::StatisticCategory;
use crate::net::ClientPlatform;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const DIGGER: Identity = Identity {
    name: "StatDigger",
    uuid: Uuid::from_u128(0x0000_00f6),
    entity_id: 9_601,
};

#[derive(Clone, Copy)]
enum Platform {
    Java,
    Bedrock,
}

/// `(used, mined)` after a survival player on `platform` holding `held` digs a `block` until it
/// breaks (start; then, unless instant, 200 ticks and a finish or predicted break).
async fn stats_after_break(
    platform: Platform,
    held: Option<&'static Item>,
    block: &'static Block,
) -> Result<(i32, i32), Box<dyn std::error::Error>> {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    for dx in -1..=1 {
        for dz in -1..=1 {
            let at = Vector2::new(12 + dx, dz);
            world
                .level
                .loaded_chunks
                .insert(at, pumpkin_world::chunk::ChunkData::empty_sync(at.x, at.y));
        }
    }
    // Far from the world spawn: spawn protection would refuse every break.
    let pos = BlockPos::new(200, 64, 8);
    world.set_block_state(
        &pos,
        block.default_state.id,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
    let player = match platform {
        Platform::Java => {
            test_support::java_player_as(&world, GameMode::Survival, &DIGGER)
                .ok_or("no Java packet tap")?
                .0
        }
        Platform::Bedrock => {
            test_support::bedrock_player_as(&world, GameMode::Survival, &DIGGER)
                .await?
                .0
        }
    };
    player.get_entity().set_pos(Vector3::new(200.5, 64.0, 10.5));
    player.get_entity().on_ground.store(true, Ordering::Relaxed);
    player.client_loaded.store(true, Ordering::Relaxed);
    if let Some(item) = held {
        player
            .inventory()
            .set_stack_in_hand(Hand::Right, ItemStack::new(1, item));
    }
    world.add_player(&player)?;
    match player.client.as_ref() {
        ClientPlatform::Java(client) => {
            for (sequence, status) in [0, 2].into_iter().enumerate() {
                if world.get_block(&pos) == &Block::AIR {
                    break;
                }
                player.tick_counter.fetch_add(200, Ordering::Relaxed);
                let packet = SJavaPlayerAction {
                    status: pumpkin_protocol::codec::var_int::VarInt(status),
                    position: pos,
                    face: 1,
                    sequence: pumpkin_protocol::codec::var_int::VarInt(sequence as i32 + 1),
                };
                client.handle_player_action(&player, &packet, &server);
            }
        }
        ClientPlatform::Bedrock(client) => {
            for action in [
                PlayerActionType::StartDestroyBlock,
                PlayerActionType::PredictDestroyBlock,
            ] {
                if world.get_block(&pos) == &Block::AIR {
                    break;
                }
                player.tick_counter.fetch_add(200, Ordering::Relaxed);
                let packet = SPlayerAction {
                    player_runtime_id: VarULong(u64::from(DIGGER.entity_id.unsigned_abs())),
                    action,
                    block_position: pos,
                    result_pos: pos,
                    face: BedrockVarInt(1),
                };
                client.handle_player_action(&player, &server, &packet);
            }
        }
    }
    let broken = world.get_block(&pos) == &Block::AIR;
    let stats = player
        .stats
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let used = held.map_or(0, |item| stats.get(StatisticCategory::Used, item.id as i32))
        + stats.get(StatisticCategory::Used, Item::AIR.id as i32);
    let mined = stats.get(StatisticCategory::Mined, block.id.as_u16() as i32);
    drop(stats);
    server.stop();
    if !broken {
        return Err(format!("{} was not broken", block.name).into());
    }
    Ok((used, mined))
}

#[tokio::test(flavor = "multi_thread")]
async fn bedrock_stone_by_hand_counts_nothing() -> TestResult {
    assert_eq!(
        stats_after_break(Platform::Bedrock, None, &Block::STONE).await?,
        (0, 0)
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn bedrock_torch_by_hand_counts_mined_only() -> TestResult {
    assert_eq!(
        stats_after_break(Platform::Bedrock, None, &Block::TORCH).await?,
        (0, 1)
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn bedrock_stone_with_a_pickaxe_counts_both() -> TestResult {
    assert_eq!(
        stats_after_break(Platform::Bedrock, Some(&Item::IRON_PICKAXE), &Block::STONE).await?,
        (1, 1)
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn bedrock_torch_holding_an_apple_counts_mined_only() -> TestResult {
    assert_eq!(
        stats_after_break(Platform::Bedrock, Some(&Item::APPLE), &Block::TORCH).await?,
        (0, 1)
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn java_stone_by_hand_counts_nothing() -> TestResult {
    assert_eq!(
        stats_after_break(Platform::Java, None, &Block::STONE).await?,
        (0, 0)
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn java_torch_by_hand_counts_mined_only() -> TestResult {
    assert_eq!(
        stats_after_break(Platform::Java, None, &Block::TORCH).await?,
        (0, 1)
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn java_stone_with_a_pickaxe_counts_both() -> TestResult {
    assert_eq!(
        stats_after_break(Platform::Java, Some(&Item::IRON_PICKAXE), &Block::STONE).await?,
        (1, 1)
    );
    Ok(())
}
