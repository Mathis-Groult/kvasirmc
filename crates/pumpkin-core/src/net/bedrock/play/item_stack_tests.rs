//! Tests of `BedrockClient::handle_item_stack_request` over a real Bedrock player (see
//! `test_support::bedrock_player_with_tap`).

use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};

use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_inventory::screen_handler::ScreenHandler;
use pumpkin_inventory::stonecutter_screen_handler::StonecutterScreenHandler;
use pumpkin_protocol::bedrock::network_item::{ContainerName, FullContainerName};
use pumpkin_protocol::bedrock::server::item_stack_request::{
    ItemStackRequest, ItemStackRequestAction, ItemStackRequestSlotInfo, SItemStackRequest,
};
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_util::GameMode;
use pumpkin_util::text::TextComponent;

use crate::entity::ai::test_support;
use crate::entity::player::Player;
use crate::net::ClientPlatform;
use crate::net::bedrock::BedrockClient;
use crate::net::bedrock::play::item_stack_request::map_bedrock_container_slot;

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

fn request(action: ItemStackRequestAction) -> SItemStackRequest {
    SItemStackRequest {
        requests: vec![ItemStackRequest {
            request_id: VarInt(-1),
            actions: vec![action],
            filter_strings: Vec::new(),
            filter_cause: 0,
        }],
    }
}

/// The names of the requests whose handling panicked.
fn panicking(
    client: &BedrockClient,
    player: &Arc<Player>,
    cases: Vec<(&'static str, ItemStackRequestAction)>,
) -> Vec<&'static str> {
    let mut panicked = Vec::new();
    for (name, action) in cases {
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
            client.handle_item_stack_request(player, request(action));
        }));
        if outcome.is_err() {
            panicked.push(name);
        }
    }
    panicked
}

/// Card #26: a client names any slot id; one past the end of the open screen used to index past
/// its slot list, and the panic stopped the server (pumpkin's panic hook). Such a slot now maps
/// nowhere.
#[tokio::test(flavor = "multi_thread")]
async fn slots_past_the_screen_do_not_panic() -> TestResult {
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
    let client = bedrock(&player).ok_or("not a Bedrock player")?;

    // The player's own screen: 46 slots, the hotbar from 36.
    let mut panicked = panicking(
        client,
        &player,
        vec![
            (
                "take from hotbar 12",
                ItemStackRequestAction::Take {
                    count: 5,
                    source: slot(ContainerName::HotBar, 12),
                    destination: slot(ContainerName::Cursor, 0),
                },
            ),
            (
                "swap with hotbar 15",
                ItemStackRequestAction::Swap {
                    slot1: slot(ContainerName::HotBar, 0),
                    slot2: slot(ContainerName::HotBar, 15),
                },
            ),
            (
                "drop from hotbar 30",
                ItemStackRequestAction::Drop {
                    count: 1,
                    source: slot(ContainerName::HotBar, 30),
                    randomly: false,
                },
            ),
            (
                "consume hotbar 99",
                ItemStackRequestAction::Consume {
                    count: 1,
                    source: slot(ContainerName::HotBar, 99),
                },
            ),
        ],
    );

    // A stonecutter: 2 + 36 slots, so the offhand (45) is past the end.
    let cutter: Arc<Mutex<dyn ScreenHandler>> = Arc::new(Mutex::new(
        StonecutterScreenHandler::new(1, &player.inventory, None),
    ));
    player.open_handled_screen_direct(cutter, &TextComponent::text("test"));
    panicked.extend(panicking(
        client,
        &player,
        vec![(
            "take into the offhand of a stonecutter",
            ItemStackRequestAction::Take {
                count: 1,
                source: slot(ContainerName::HotBar, 0),
                destination: slot(ContainerName::Offhand, 0),
            },
        )],
    ));
    let mapped = {
        let handler = player
            .current_screen_handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let handler = handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        [
            map_bedrock_container_slot(&*handler, ContainerName::Offhand, 0),
            map_bedrock_container_slot(&*handler, ContainerName::HotBar, 8),
            map_bedrock_container_slot(&*handler, ContainerName::HotBar, 9),
        ]
    };
    let _ = tap.drain();
    server.stop();
    assert_eq!(panicked, Vec::<&str>::new());
    // Hotbar 8 is the last screen slot (2 + 27 + 8 = 37); hotbar 9 and the offhand are past it.
    assert_eq!(mapped, [None, Some(37), None]);
    Ok(())
}
