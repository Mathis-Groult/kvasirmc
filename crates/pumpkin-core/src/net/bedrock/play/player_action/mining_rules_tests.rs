//! Bedrock players may break only what Java players may (card #84): a spectator, an adventure
//! player without a matching `can_break`, and a creative player holding a sword break nothing.

use std::sync::atomic::Ordering;

use pumpkin_data::Block;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_protocol::bedrock::server::player_action::{PlayerActionType, SPlayerAction};
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::codec::var_ulong::VarULong;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::{GameMode, Hand};
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use crate::entity::EntityBase;
use crate::entity::ai::test_support::{self, Identity};
use crate::net::ClientPlatform;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const DIGGER: Identity = Identity {
    name: "Digger",
    uuid: Uuid::from_u128(0x0000_00f5),
    entity_id: 9_501,
};

/// The block left after a `gamemode` player holding `held` starts destroying a `block`.
async fn left_after_start(
    gamemode: GameMode,
    held: Option<&'static Item>,
    block: &'static Block,
) -> Result<&'static str, Box<dyn std::error::Error>> {
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
    let (player, _tap) = test_support::bedrock_player_as(&world, gamemode, &DIGGER).await?;
    player.get_entity().set_pos(Vector3::new(200.5, 64.0, 10.5));
    player.get_entity().on_ground.store(true, Ordering::Relaxed);
    player.client_loaded.store(true, Ordering::Relaxed);
    if let Some(item) = held {
        player
            .inventory()
            .set_stack_in_hand(Hand::Right, ItemStack::new(1, item));
    }
    world.add_player(&player)?;
    let ClientPlatform::Bedrock(client) = player.client.as_ref() else {
        return Err("not a Bedrock player".into());
    };
    let packet = SPlayerAction {
        player_runtime_id: VarULong(u64::from(DIGGER.entity_id.unsigned_abs())),
        action: PlayerActionType::StartDestroyBlock,
        block_position: pos,
        result_pos: pos,
        face: VarInt(1),
    };
    client.handle_player_action(&player, &server, &packet);
    let left = world.get_block(&pos).name;
    server.stop();
    Ok(left)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_survival_player_breaks_a_torch() -> TestResult {
    assert_eq!(
        left_after_start(GameMode::Survival, None, &Block::TORCH).await?,
        "air"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_spectator_does_not_break_a_torch() -> TestResult {
    assert_eq!(
        left_after_start(GameMode::Spectator, None, &Block::TORCH).await?,
        "torch"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn an_adventure_player_does_not_break_a_torch() -> TestResult {
    assert_eq!(
        left_after_start(GameMode::Adventure, None, &Block::TORCH).await?,
        "torch"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_creative_player_with_a_sword_does_not_break_stone() -> TestResult {
    let left = left_after_start(
        GameMode::Creative,
        Some(&Item::DIAMOND_SWORD),
        &Block::STONE,
    )
    .await?;
    assert_eq!(left, "stone");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_creative_player_with_a_pickaxe_breaks_stone() -> TestResult {
    let left =
        left_after_start(GameMode::Creative, Some(&Item::IRON_PICKAXE), &Block::STONE).await?;
    assert_eq!(left, "air");
    Ok(())
}
