//! Tests of `BedrockClient::handle_item_stack_request` over a real Bedrock player (see
//! `test_support::bedrock_player_with_tap`).

use std::sync::Arc;

use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_protocol::bedrock::network_item::{ContainerName, FullContainerName};
use pumpkin_protocol::bedrock::server::item_stack_request::{
    ItemStackRequest, ItemStackRequestAction, ItemStackRequestSlotInfo, SItemStackRequest,
};
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_util::GameMode;

use crate::entity::ai::test_support;
use crate::entity::player::Player;
use crate::net::ClientPlatform;
use crate::net::bedrock::BedrockClient;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Reads one `VarUInt` from the front of `bytes`: (value, bytes used).
fn var_uint(bytes: &[u8]) -> (u32, usize) {
    let mut value = 0u32;
    for (index, byte) in bytes.iter().take(5).enumerate() {
        value |= u32::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 == 0 {
            return (value, index + 1);
        }
    }
    (value, bytes.len().min(5))
}

/// The game packet id of a queued Bedrock packet: after its length, the low 10 bits of the
/// header.
fn packet_id(packet: &[u8]) -> u32 {
    let (_, length_size) = var_uint(packet);
    let (header, _) = var_uint(&packet[length_size..]);
    header & 0x3ff
}

fn slot(container: ContainerName, slot_id: u8) -> ItemStackRequestSlotInfo {
    ItemStackRequestSlotInfo {
        container_name: FullContainerName {
            container_name: container,
            dynamic_id: None,
        },
        slot_id,
        stack_id: 0,
    }
}

fn bedrock(player: &Player) -> Option<&Arc<BedrockClient>> {
    match player.client.as_ref() {
        ClientPlatform::Bedrock(client) => Some(client),
        ClientPlatform::Java(_) => None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn harness_answers_a_take_to_the_cursor() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server
        .worlds
        .load()
        .first()
        .cloned()
        .ok_or("the server has no world")?;
    let (player, mut tap) =
        test_support::bedrock_player_with_tap(&world, GameMode::Survival).await?;
    player
        .inventory
        .main_inventory
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)[0] = ItemStack::new(16, &Item::STONE);
    let _ = tap.drain();

    let client = bedrock(&player).ok_or("not a Bedrock player")?;
    client.handle_item_stack_request(
        &player,
        SItemStackRequest {
            requests: vec![ItemStackRequest {
                request_id: VarInt(-1),
                actions: vec![ItemStackRequestAction::Take {
                    count: 5,
                    source: slot(ContainerName::HotBar, 0),
                    destination: slot(ContainerName::Cursor, 0),
                }],
                filter_strings: Vec::new(),
                filter_cause: 0,
            }],
        },
    );
    let ids: Vec<u32> = tap.drain().iter().map(|p| packet_id(p)).collect();
    let cursor = player
        .current_screen_handler
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let cursor = cursor
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get_behaviour()
        .cursor_stack
        .lock()
        .map(|stack| (stack.item.registry_key, stack.item_count))
        .map_err(|_| "cursor lock poisoned")?;
    server.stop();
    // ItemStackResponse, then InventoryContent (the request changed the inventory).
    assert_eq!(ids, [148, 49]);
    assert_eq!(cursor, ("stone", 5));
    Ok(())
}
