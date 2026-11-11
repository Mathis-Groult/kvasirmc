//! Tests of `BedrockClient::handle_inventory_action` over a real Bedrock player (see
//! `test_support::bedrock_player_with_tap`): a spectator only watches.

use std::sync::Arc;

use pumpkin_data::entity::EntityType;
use pumpkin_data::item::{Item, JavaToBedrockItemMapping};
use pumpkin_data::item_stack::ItemStack;
use pumpkin_protocol::bedrock::network_item::NetworkItemDescriptor;
use pumpkin_protocol::bedrock::server::inventory_transaction::{
    HandSlot, InventoryAction, SInventoryTransaction, TransactionData,
    UseItemOnEntityTransactionData,
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
