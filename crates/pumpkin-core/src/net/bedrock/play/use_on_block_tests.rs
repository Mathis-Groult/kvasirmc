//! A Bedrock player's block use works on the stack in the hand, as on Java (card #91): the
//! block's own use (`use_with_item`) consumes from the hand instead of from a copy.

use pumpkin_data::Block;
use pumpkin_data::block_properties::RespawnAnchorLikeProperties;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_protocol::bedrock::network_item::NetworkItemDescriptor;
use pumpkin_protocol::bedrock::server::inventory_transaction::{
    HandSlot, SInventoryTransaction, TransactionData, UseItemTransactionData,
};
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::codec::var_uint::VarUInt;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::{GameMode, Hand};
use pumpkin_world::world::BlockFlags;

use crate::command::commands::script_golden::TestResult;
use crate::entity::EntityBase;
use crate::entity::ai::test_support;
use crate::net::ClientPlatform;

#[tokio::test(flavor = "multi_thread")]
async fn charging_a_respawn_anchor_uses_up_one_glowstone() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let pos = BlockPos::new(200, 64, 8);
    let chunk = pos.chunk_position();
    for dx in -1..=1 {
        for dz in -1..=1 {
            let at = Vector2::new(chunk.x + dx, chunk.y + dz);
            world
                .level
                .loaded_chunks
                .insert(at, pumpkin_world::chunk::ChunkData::empty_sync(at.x, at.y));
        }
    }
    let flags = BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK;
    world.set_block_state(&pos.down(), Block::STONE.default_state.id, flags);
    let anchor = RespawnAnchorLikeProperties { charges: 0 }.to_state_id(&Block::RESPAWN_ANCHOR);
    world.set_block_state(&pos, anchor, flags);
    let (player, mut tap) =
        test_support::bedrock_player_with_tap(&world, GameMode::Survival).await?;
    player.get_entity().set_pos(Vector3::new(200.5, 64.0, 10.5));
    player
        .inventory()
        .set_stack_in_hand(Hand::Right, ItemStack::new(3, &Item::GLOWSTONE));
    let ClientPlatform::Bedrock(client) = player.client.as_ref() else {
        return Err("not a Bedrock player".into());
    };
    tap.drain();
    let packet = SInventoryTransaction {
        legacy_request_id: VarInt(0),
        legacy_set_item_slots: Vec::new(),
        has_value: false,
        actions: Vec::new(),
        transaction_type: VarUInt(2),
        transaction_data: TransactionData::UseItem(UseItemTransactionData {
            action_type: VarInt(0),
            trigger_type: 1,
            block_position: pos,
            block_face: 1,
            hot_bar_slot: VarInt(player.inventory.get_selected_slot().into()),
            hand: HandSlot::Mainhand,
            item_in_hand: NetworkItemDescriptor::default(),
            player_position: Vector3::new(200.5, 65.62, 10.5),
            click_position: Vector3::new(0.5, 1.0, 0.5),
            block_runtime_id: VarUInt(0),
            client_prediction: 1,
            client_cooldown_state: 0,
        }),
    };
    client.handle_inventory_action(&player, packet);
    let charges =
        RespawnAnchorLikeProperties::from_state_id(world.get_block_state_id(&pos)).charges;
    let left = player.inventory().held_item().item_count;
    world.remove_player(&player, false).await;
    server.stop();
    assert_eq!(charges, 1, "the anchor was not charged");
    assert_eq!(left, 2, "glowstone left in the hand (vanilla 2)");
    Ok(())
}
