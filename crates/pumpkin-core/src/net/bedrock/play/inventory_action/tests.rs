//! Tests of `BedrockClient::handle_inventory_action` over a real Bedrock player (see
//! `test_support::bedrock_player_with_tap`): the server keeps its own inventory as the truth,
//! and a spectator only watches.

use std::sync::Arc;

use pumpkin_data::entity::EntityType;
use pumpkin_data::item::{Item, JavaToBedrockItemMapping};
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::statistic::StatisticCategory;
use pumpkin_protocol::bedrock::network_item::NetworkItemDescriptor;
use pumpkin_protocol::bedrock::server::inventory_transaction::{
    HandSlot, InventoryAction, NormalTransactionData, SInventoryTransaction, TransactionData,
    UseItemOnEntityTransactionData, UseItemTransactionData,
};
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::codec::var_uint::VarUInt;
use pumpkin_protocol::codec::var_ulong::VarULong;
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::world::BlockFlags;

use crate::entity::EntityBase;
use crate::entity::ai::test_support;
use crate::entity::player::Player;
use crate::entity::r#type::from_type;
use crate::net::ClientPlatform;
use crate::net::bedrock::BedrockClient;
use crate::server::Server;
use crate::world::World;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Inventory source `World` (items the client says it threw on the ground).
const SOURCE_WORLD: u32 = 2;
/// Inventory source `Container` with window 0 (the player's own inventory).
const SOURCE_CONTAINER: u32 = 0;

struct Scene {
    _folder: tempfile::TempDir,
    server: Arc<Server>,
    world: Arc<World>,
    player: Arc<Player>,
    _tap: crate::net::bedrock::BedrockPacketTap,
}

impl Scene {
    async fn new(gamemode: GameMode) -> Result<Self, Box<dyn std::error::Error>> {
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
        for x in 0..8 {
            for z in 0..8 {
                world.set_block_state(
                    &BlockPos::new(x, 64, z),
                    pumpkin_data::Block::STONE.default_state.id,
                    BlockFlags::NOTIFY_ALL,
                );
            }
        }
        let (player, tap) = test_support::bedrock_player_with_tap(&world, gamemode).await?;
        player.get_entity().set_pos(Vector3::new(4.5, 65.0, 4.5));
        Ok(Self {
            _folder: folder,
            server,
            world,
            player,
            _tap: tap,
        })
    }

    fn client(&self) -> Result<&BedrockClient, Box<dyn std::error::Error>> {
        match self.player.client.as_ref() {
            ClientPlatform::Bedrock(client) => Ok(client),
            ClientPlatform::Java(_) => Err("not a Bedrock player".into()),
        }
    }

    fn send(&self, data: TransactionData, actions: Vec<InventoryAction>) -> TestResult {
        self.client()?.handle_inventory_action(
            &self.player,
            SInventoryTransaction {
                legacy_request_id: VarInt(0),
                legacy_set_item_slots: Vec::new(),
                has_value: !actions.is_empty(),
                actions,
                transaction_type: VarUInt(0),
                transaction_data: data,
            },
        );
        Ok(())
    }

    /// Item entities (and anything else) that are not the player.
    fn entity_count(&self) -> usize {
        self.world.entities.load().len()
    }

    fn hotbar(&self, slot: usize) -> String {
        let inventory = self
            .player
            .inventory
            .main_inventory
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let stack = &inventory[slot];
        if stack.is_empty() {
            "empty".to_string()
        } else {
            format!("{} x{}", stack.item.registry_key, stack.item_count)
        }
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        self.server.stop();
    }
}

fn descriptor(stack: &ItemStack) -> NetworkItemDescriptor {
    JavaToBedrockItemMapping::from_java_item_id(stack.item.id).map_or_else(
        NetworkItemDescriptor::default,
        |mapping| NetworkItemDescriptor {
            id: VarInt(i32::from(mapping.bedrock_item.id)),
            stack_size: u16::from(stack.item_count),
            aux_value: VarUInt(mapping.bedrock_data),
            ..NetworkItemDescriptor::default()
        },
    )
}

fn diamonds() -> NetworkItemDescriptor {
    descriptor(&ItemStack::new(64, &Item::DIAMOND))
}

fn use_item(action_type: i32, item_in_hand: NetworkItemDescriptor) -> TransactionData {
    TransactionData::UseItem(UseItemTransactionData {
        action_type: VarInt(action_type),
        trigger_type: 1,
        block_position: BlockPos::new(4, 64, 6),
        block_face: 1,
        hot_bar_slot: VarInt(0),
        hand: HandSlot::Mainhand,
        item_in_hand,
        player_position: Vector3::new(4.5, 66.62, 4.5),
        click_position: Vector3::new(0.5, 1.0, 0.5),
        block_runtime_id: VarUInt(0),
        client_prediction: 1,
        client_cooldown_state: 0,
    })
}

fn on_entity(target: i32, action_type: i32, item: &ItemStack) -> TransactionData {
    TransactionData::UseItemOnEntity(UseItemOnEntityTransactionData {
        target_entity_runtime_id: VarULong(target as u64),
        action_type: VarInt(action_type),
        hot_bar_slot: VarInt(0),
        hand: HandSlot::Mainhand,
        item_in_hand: descriptor(item),
        player_position: Vector3::new(4.5, 66.62, 4.5),
        click_position: Vector3::new(0.0, 0.0, 0.0),
    })
}

/// A client that writes 64 diamonds into its empty hotbar gets nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_slot_the_client_writes_keeps_the_server_stack() -> TestResult {
    let scene = Scene::new(GameMode::Survival).await?;
    let write = InventoryAction {
        source_type: SOURCE_CONTAINER,
        window_id: Some(0),
        source_flags: Some(0),
        inventory_slot: 0,
        old_item: NetworkItemDescriptor::default(),
        new_item: diamonds(),
    };
    scene.send(TransactionData::Normal(NormalTransactionData), vec![write])?;
    assert_eq!(scene.hotbar(0), "empty");
    Ok(())
}

/// A client that says it threw 64 diamonds it never had on the ground drops nothing.
#[tokio::test(flavor = "multi_thread")]
async fn items_thrown_from_nothing_are_not_dropped() -> TestResult {
    let scene = Scene::new(GameMode::Survival).await?;
    let before = scene.entity_count();
    let throw = InventoryAction {
        source_type: SOURCE_WORLD,
        window_id: None,
        source_flags: Some(0),
        inventory_slot: 0,
        old_item: NetworkItemDescriptor::default(),
        new_item: diamonds(),
    };
    scene.send(TransactionData::Normal(NormalTransactionData), vec![throw])?;
    assert_eq!(
        scene.entity_count(),
        before,
        "an item appeared in the world"
    );
    Ok(())
}

/// Clicking a block with an empty hand while claiming a stone places nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_claimed_block_in_hand_is_not_placed() -> TestResult {
    let scene = Scene::new(GameMode::Survival).await?;
    let stone = descriptor(&ItemStack::new(64, &Item::STONE));
    scene.send(use_item(0, stone), Vec::new())?;
    let above = scene.world.get_block_state_id(&BlockPos::new(4, 65, 6));
    assert_eq!(
        above,
        pumpkin_data::Block::AIR.default_state.id,
        "a block was placed"
    );
    assert_eq!(scene.hotbar(0), "empty");
    Ok(())
}

/// Clicking the air with an empty hand while claiming a stack of diamonds leaves the hand empty
/// and counts no use.
#[tokio::test(flavor = "multi_thread")]
async fn a_claimed_item_in_hand_is_not_given() -> TestResult {
    let scene = Scene::new(GameMode::Survival).await?;
    scene.send(use_item(1, diamonds()), Vec::new())?;
    assert_eq!(scene.hotbar(0), "empty");
    let used = scene
        .player
        .get_stat(StatisticCategory::Used, i32::from(Item::DIAMOND.id));
    assert_eq!(
        used, 0,
        "a use of an item the player does not hold was counted"
    );
    Ok(())
}

/// A spectator only watches: hitting a cow or using a spawn egg on it does nothing (the Java
/// path turns the click into a camera switch before any attack or interaction).
#[tokio::test(flavor = "multi_thread")]
async fn a_spectator_neither_hits_nor_uses_items_on_entities() -> TestResult {
    let scene = Scene::new(GameMode::Spectator).await?;
    let cow: Arc<dyn EntityBase> = from_type(
        &EntityType::COW,
        Vector3::new(5.5, 65.0, 4.5),
        &scene.world,
        uuid::Uuid::from_u128(0xc0),
    );
    let cow_id = cow.get_entity().entity_id;
    scene.world.spawn_entity(cow.clone());
    let health = || cow.get_living_entity().map(|living| living.health.load());
    let before = (health(), scene.entity_count());

    let sword = ItemStack::new(1, &Item::DIAMOND_SWORD);
    scene.player.inventory.set_held_item(sword.clone());
    scene.send(on_entity(cow_id, 1, &sword), Vec::new())?;
    let egg = ItemStack::new(1, &Item::COW_SPAWN_EGG);
    scene.player.inventory.set_held_item(egg.clone());
    scene.send(on_entity(cow_id, 0, &egg), Vec::new())?;

    assert_eq!(
        (health(), scene.entity_count()),
        before,
        "the spectator hurt the cow or spawned a calf"
    );
    assert_eq!(scene.hotbar(0), "cow_spawn_egg x1");
    Ok(())
}
