//! A record of what `BedrockClient::handle_inventory_action` does, made before the function was
//! split (`tests/golden/inventory_actions.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! One real Bedrock player per scene (survival, survival with half a food bar, creative,
//! spectator) stands on a small built area in its own chunks: a stone floor with a crafting
//! table, a chest, a lever, a note block, dirt, grass, an oak log and a water cauldron, next to a
//! cow and a zombie. It gets 250 seeded inventory transactions of every kind: normal and mismatch
//! ones with slot and world actions of every source and window, legacy slot drops with a legacy
//! request id, use-item on the built blocks, the floor and the air with every face and hand,
//! click-air, use-item on the cow, the zombie and missing entities with every action type, and
//! release. Before a third of them the held and offhand items change (blocks, food, armour,
//! tools, buckets, projectiles, a bow, a shield, a cow spawn egg, wheat), and the item the
//! client claims to hold is the held one, another one or none.
//!
//! After each transaction the decoded packets (net ids renamed by first appearance), the
//! inventory, the equipment, the selected slot, the item in use, health and food, the blocks of
//! the area, the entity count, the two mobs' health, the use and break statistics and the pearl cooldown
//! go into a running hash; a line every 50 transactions, and per scene the count of each kind.
//! Entities other than the two mobs are removed after each step (random places and speeds), and
//! plants grown by bone meal before it is recorded (random kinds).
//! `KVASIR_DUMP=<file>` appends each step's text to the file.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use pumpkin_data::data_component_impl::EquipmentSlot;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::{Item, JavaToBedrockItemMapping};
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::statistic::StatisticCategory;
use pumpkin_data::tag::Taggable;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_protocol::bedrock::network_item::NetworkItemDescriptor;
use pumpkin_protocol::bedrock::server::inventory_transaction::{
    HandSlot, InventoryAction, LegacySetItemSlot, MismatchTransactionData, NormalTransactionData,
    ReleaseItemTransactionData, SInventoryTransaction, TransactionData,
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

use super::item_stack_golden::{
    NetIds, Sequence, fnv, read_inventory_content, read_response, split_packet, stack_text,
};
use crate::entity::EntityBase;
use crate::entity::ai::test_support;
use crate::entity::player::Player;
use crate::entity::r#type::from_type;
use crate::net::ClientPlatform;
use crate::server::Server;
use crate::world::World;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const STEPS: u32 = 250;

/// The areas start this far from the origin, out of the world spawn's protection (the spawn
/// point is not settled when the first scene runs).
const FAR: i32 = 1024;

const SCENES: [(&str, GameMode); 4] = [
    ("survival", GameMode::Survival),
    ("survival_hungry", GameMode::Survival),
    ("creative", GameMode::Creative),
    ("spectator", GameMode::Spectator),
];

const HELD: [&Item; 24] = [
    &Item::STONE,
    &Item::OAK_PLANKS,
    &Item::TORCH,
    &Item::BREAD,
    &Item::GOLDEN_APPLE,
    &Item::APPLE,
    &Item::DIAMOND_HELMET,
    &Item::IRON_CHESTPLATE,
    &Item::DIAMOND_SWORD,
    &Item::DIAMOND_HOE,
    &Item::IRON_AXE,
    &Item::IRON_SHOVEL,
    &Item::BUCKET,
    &Item::WATER_BUCKET,
    &Item::FLINT_AND_STEEL,
    &Item::ENDER_PEARL,
    &Item::SNOWBALL,
    &Item::BOW,
    &Item::SHIELD,
    &Item::BONE_MEAL,
    &Item::STICK,
    &Item::ELYTRA,
    &Item::COW_SPAWN_EGG,
    &Item::WHEAT,
];

/// The built blocks: (x, z) offset from the scene's corner, all at y = 65 on the floor.
fn built() -> [((i32, i32), BlockStateId); 8] {
    [
        ((2, 2), Block::CRAFTING_TABLE.default_state.id),
        ((2, 4), Block::CHEST.default_state.id),
        ((4, 2), Block::LEVER.default_state.id),
        ((4, 4), Block::NOTE_BLOCK.default_state.id),
        ((6, 2), Block::DIRT.default_state.id),
        ((6, 4), Block::OAK_LOG.default_state.id),
        ((3, 6), Block::GRASS_BLOCK.default_state.id),
        ((7, 6), Block::WATER_CAULDRON.default_state.id),
    ]
}

struct Area {
    corner: i32,
    cow: i32,
    zombie: i32,
}

fn build_area(world: &Arc<World>, scene: usize) -> Area {
    let corner = FAR + scene as i32 * 32;
    for chunk_x in [corner / 16, corner / 16 + 1] {
        world.level.loaded_chunks.insert(
            Vector2::new(chunk_x, FAR / 16),
            pumpkin_world::chunk::ChunkData::empty_sync(chunk_x, FAR / 16),
        );
    }
    for x in 0..10 {
        for z in 0..10 {
            world.set_block_state(
                &BlockPos::new(corner + x, 64, FAR + z),
                Block::STONE.default_state.id,
                BlockFlags::NOTIFY_ALL,
            );
        }
    }
    for ((x, z), state) in built() {
        world.set_block_state(
            &BlockPos::new(corner + x, 65, FAR + z),
            state,
            BlockFlags::NOTIFY_ALL,
        );
    }
    let mob = |kind: &'static EntityType, x: f64, uuid: u128| {
        let entity = from_type(
            kind,
            Vector3::new(f64::from(corner) + x, 65.0, f64::from(FAR) + 8.5),
            world,
            uuid::Uuid::from_u128(uuid),
        );
        let id = entity.get_entity().entity_id;
        world.spawn_entity(entity);
        id
    };
    Area {
        corner,
        cow: mob(&EntityType::COW, 3.5, 0x00c0_0000 + scene as u128),
        zombie: mob(&EntityType::ZOMBIE, 6.5, 0x00d0_0000 + scene as u128),
    }
}

// ---------------------------------------------------------------------------------------
// Transactions

fn descriptor(stack: &ItemStack) -> NetworkItemDescriptor {
    if stack.is_empty() {
        return NetworkItemDescriptor::default();
    }
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

fn random_stack(sequence: &mut Sequence) -> ItemStack {
    let item = sequence.pick(&HELD);
    let limit = u64::from(ItemStack::new(1, item).get_max_stack_size());
    ItemStack::new((1 + sequence.next(limit)) as u8, item)
}

fn random_descriptor(sequence: &mut Sequence) -> NetworkItemDescriptor {
    if sequence.chance(4) {
        NetworkItemDescriptor::default()
    } else {
        descriptor(&random_stack(sequence))
    }
}

fn random_action(sequence: &mut Sequence) -> InventoryAction {
    let source_type = sequence.pick(&[0, 0, 0, 2, 2, 3, 99_999, 7]);
    let window_id = sequence.pick(&[Some(0), Some(0), Some(119), Some(120), Some(124), None]);
    let inventory_slot = if sequence.chance(3) {
        sequence.pick(&[9, 20, 35, 36, 40, 1000])
    } else {
        sequence.next(9) as u32
    };
    InventoryAction {
        source_type,
        window_id,
        source_flags: Some(0),
        inventory_slot,
        old_item: random_descriptor(sequence),
        new_item: random_descriptor(sequence),
    }
}

fn legacy_slots(sequence: &mut Sequence) -> Vec<LegacySetItemSlot> {
    (0..=sequence.next(2))
        .map(|_| LegacySetItemSlot {
            container_id: sequence.pick(&[28, 29, 6, 120, 34, 119, 7]),
            slots: (0..=sequence.next(2))
                .map(|_| sequence.pick(&[0, 1, 2, 3, 8, 9, 20, 35, 40]))
                .collect(),
        })
        .collect()
}

/// The item the client says it holds: the held one, another one, or none.
fn claimed(player: &Player, sequence: &mut Sequence) -> NetworkItemDescriptor {
    match sequence.next(3) {
        0 => NetworkItemDescriptor::default(),
        1 => descriptor(&player.inventory.held_item()),
        _ => descriptor(&random_stack(sequence)),
    }
}

fn target_block(area: &Area, sequence: &mut Sequence) -> BlockPos {
    let corner = area.corner;
    match sequence.next(4) {
        0 | 1 => {
            let ((x, z), _) = sequence.pick(&built());
            BlockPos::new(corner + x, 65, FAR + z)
        }
        2 => BlockPos::new(
            corner + 1 + sequence.next(8) as i32,
            64,
            FAR + sequence.next(8) as i32,
        ),
        _ => BlockPos::new(corner + 5, 66 + sequence.next(2) as i32, FAR + 5),
    }
}

fn use_item(
    player: &Player,
    area: &Area,
    sequence: &mut Sequence,
    action_type: i32,
) -> UseItemTransactionData {
    let block_position = target_block(area, sequence);
    let click = sequence.pick(&[(0.5, 1.0, 0.5), (0.5, 0.5, 0.0), (0.2, 0.0, 0.7)]);
    UseItemTransactionData {
        action_type: VarInt(action_type),
        trigger_type: 1,
        block_position,
        block_face: sequence.next(7) as u8,
        hot_bar_slot: VarInt(player.inventory.get_selected_slot().into()),
        hand: if sequence.chance(4) {
            HandSlot::Offhand
        } else {
            HandSlot::Mainhand
        },
        item_in_hand: claimed(player, sequence),
        player_position: Vector3::new(area.corner as f32 + 4.5, 66.62, FAR as f32 + 6.5),
        click_position: Vector3::new(click.0, click.1, click.2),
        block_runtime_id: VarUInt(0),
        client_prediction: 1,
        client_cooldown_state: 0,
    }
}

fn use_on_entity(
    player: &Player,
    area: &Area,
    sequence: &mut Sequence,
) -> UseItemOnEntityTransactionData {
    let target = match sequence.next(5) {
        0 | 1 => area.cow as u64,
        2 | 3 => area.zombie as u64,
        _ => sequence.pick(&[999_999, u64::from(u32::MAX) + 7]),
    };
    UseItemOnEntityTransactionData {
        target_entity_runtime_id: VarULong(target),
        action_type: VarInt(sequence.pick(&[0, 0, 1, 1, 2, 3])),
        hot_bar_slot: VarInt(player.inventory.get_selected_slot().into()),
        hand: HandSlot::Mainhand,
        item_in_hand: claimed(player, sequence),
        player_position: Vector3::new(area.corner as f32 + 4.5, 66.62, FAR as f32 + 6.5),
        click_position: Vector3::new(0.0, 0.0, 0.0),
    }
}

fn release(player: &Player, sequence: &mut Sequence) -> ReleaseItemTransactionData {
    ReleaseItemTransactionData {
        action_type: VarInt(0),
        hot_bar_slot: VarInt(player.inventory.get_selected_slot().into()),
        item_in_hand: claimed(player, sequence),
        head_position: Vector3::new(0.0, 0.0, 0.0),
        hand: HandSlot::Mainhand,
    }
}

/// A seeded transaction and its kind's name.
fn transaction(
    player: &Player,
    area: &Area,
    sequence: &mut Sequence,
) -> (&'static str, SInventoryTransaction) {
    let (kind, data) = match sequence.next(10) {
        0 => ("normal", TransactionData::Normal(NormalTransactionData)),
        1 => (
            "mismatch",
            TransactionData::Mismatch(MismatchTransactionData),
        ),
        2..=4 => (
            "use_on_block",
            TransactionData::UseItem(use_item(player, area, sequence, 0)),
        ),
        5 | 6 => (
            "use_in_air",
            TransactionData::UseItem(use_item(player, area, sequence, 1)),
        ),
        7 => (
            "use_other",
            TransactionData::UseItem(use_item(player, area, sequence, 2)),
        ),
        8 => (
            "use_on_entity",
            TransactionData::UseItemOnEntity(use_on_entity(player, area, sequence)),
        ),
        _ => (
            "release",
            TransactionData::ReleaseItem(release(player, sequence)),
        ),
    };
    let legacy = sequence.chance(6);
    let actions = if legacy && sequence.chance(2) {
        Vec::new()
    } else {
        (0..sequence.next(3))
            .map(|_| random_action(sequence))
            .collect()
    };
    let packet = SInventoryTransaction {
        legacy_request_id: VarInt(if legacy { -7 } else { 0 }),
        legacy_set_item_slots: if legacy {
            legacy_slots(sequence)
        } else {
            Vec::new()
        },
        has_value: !actions.is_empty(),
        actions,
        transaction_type: VarUInt(0),
        transaction_data: data,
    };
    (kind, packet)
}

/// Before a third of the transactions: a new held item (and now and then a new offhand item and
/// another selected slot).
fn change_hands(player: &Player, sequence: &mut Sequence) {
    if !sequence.chance(3) {
        return;
    }
    if sequence.chance(5) {
        player.inventory.set_selected_slot(sequence.next(2) as u8);
    }
    let stack = if sequence.chance(8) {
        ItemStack::EMPTY.clone()
    } else {
        random_stack(sequence)
    };
    player.inventory.set_held_item(stack);
    if sequence.chance(4) {
        player
            .inventory
            .set_stack_in_hand(pumpkin_util::Hand::Left, random_stack(sequence));
    }
}

// ---------------------------------------------------------------------------------------
// State

fn equipment_text(player: &Player) -> String {
    let equipment = player
        .inventory
        .entity_equipment
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    [
        EquipmentSlot::HEAD,
        EquipmentSlot::CHEST,
        EquipmentSlot::LEGS,
        EquipmentSlot::FEET,
        EquipmentSlot::OFF_HAND,
    ]
    .iter()
    .map(|slot| stack_text(&equipment.get(slot)))
    .collect::<Vec<_>>()
    .join(";")
}

fn player_text(player: &Player) -> String {
    let mut text = String::new();
    for stack in player
        .inventory
        .main_inventory
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
    {
        text.push_str(&stack_text(stack));
        text.push(';');
    }
    let in_use = player
        .living_entity
        .item_in_use
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .map_or_else(|| "-".to_string(), stack_text);
    let used: Vec<i32> = HELD
        .iter()
        .map(|item| player.get_stat(StatisticCategory::Used, i32::from(item.id)))
        .collect();
    let broken: Vec<i32> = HELD
        .iter()
        .map(|item| player.get_stat(StatisticCategory::Broken, i32::from(item.id)))
        .collect();
    let _ = write!(
        text,
        " eq {} sel {} use {in_use} t {} hp {} food {} used {used:?} broken {broken:?} pearl {}",
        equipment_text(player),
        player.inventory.get_selected_slot(),
        player.living_entity.item_use_time.load(Ordering::Relaxed),
        player.living_entity.health.load(),
        player.hunger_manager.level.load(),
        player.is_on_cooldown("ender_pearl"),
    );
    text
}

fn world_text(world: &Arc<World>, area: &Area) -> String {
    let mut blocks = String::new();
    for x in 0..10 {
        for y in 64..68 {
            for z in 0..10 {
                let _ = write!(
                    blocks,
                    "{},",
                    world.get_block_state_id(&BlockPos::new(area.corner + x, y, FAR + z))
                );
            }
        }
    }
    let health = |id: i32| {
        world.get_entity_by_id(id).map_or_else(
            || "gone".to_string(),
            |entity| {
                entity.get_living_entity().map_or_else(
                    || "?".to_string(),
                    |living| living.health.load().to_string(),
                )
            },
        )
    };
    format!(
        "blocks {:016x} entities {} cow {} zombie {}",
        fnv(0xcbf2_9ce4_8422_2325, blocks.as_bytes()),
        world.entities.load().len(),
        health(area.cow),
        health(area.zombie)
    )
}

fn packets_text(tap: &mut crate::net::bedrock::BedrockPacketTap, ids: &mut NetIds) -> String {
    let mut text = String::new();
    for packet in tap.drain() {
        let Some((id, payload)) = split_packet(&packet) else {
            text.push_str("unreadable;");
            continue;
        };
        match id {
            148 => {
                let response = read_response(payload, ids);
                text.push_str(response.as_ref().map_or("bad response", |(t, _)| t));
            }
            49 => {
                let content = read_inventory_content(payload, ids);
                text.push_str(content.as_deref().unwrap_or("bad content"));
            }
            _ => {
                let _ = write!(text, "p{id};");
            }
        }
    }
    text
}

/// Plants grown by bone meal on the grass block leave the world before each step is recorded:
/// their kind is random (`rand::rng()`: short grass, a flower, tall grass), and no held item
/// places a plant. Their block updates are only sent on a tick, which the scenes never run.
fn clear_grown_plants(world: &Arc<World>, area: &Area) {
    for x in 0..10 {
        for y in 65..68 {
            for z in 0..10 {
                let position = BlockPos::new(area.corner + x, y, FAR + z);
                let block = world.get_block(&position);
                if block == &Block::SHORT_GRASS
                    || block == &Block::TALL_GRASS
                    || block.has_tag(&pumpkin_data::tag::Block::MINECRAFT_FLOWERS)
                {
                    world.set_block_state(
                        &position,
                        Block::AIR.default_state.id,
                        BlockFlags::FORCE_STATE | BlockFlags::SKIP_DROPS,
                    );
                }
            }
        }
    }
}

/// Dropped items and thrown projectiles leave the world after each step: their places and
/// speeds are random, and a later block placement checks for entities in the way.
fn clear_spawned(world: &Arc<World>, area: &Area) {
    let spawned: Vec<Arc<dyn EntityBase>> = world
        .entities
        .load()
        .iter()
        .filter(|entity| ![area.cow, area.zombie].contains(&entity.get_entity().entity_id))
        .cloned()
        .collect();
    for entity in spawned {
        world.remove_entity(entity.as_ref());
    }
}

fn dump(scene: &str, step: u32, kind: &str, text: &str) {
    if let Some(dump) = std::env::var_os("KVASIR_DUMP") {
        use std::io::Write as _;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(dump)
        {
            let _ = writeln!(file, "{scene} {step} {kind} {text}");
        }
    }
}

// ---------------------------------------------------------------------------------------
// The run

async fn run_scene(
    server: &Arc<Server>,
    index: usize,
    (name, gamemode): (&'static str, GameMode),
) -> Result<String, String> {
    let world = server
        .worlds
        .load()
        .first()
        .cloned()
        .ok_or("the server has no world")?;
    let area = build_area(&world, index);
    let (player, mut tap) = test_support::bedrock_player_with_tap(&world, gamemode).await?;
    let ClientPlatform::Bedrock(client) = player.client.as_ref() else {
        return Err("not a Bedrock player".to_string());
    };
    player.get_entity().set_pos(Vector3::new(
        f64::from(area.corner) + 4.5,
        65.0,
        f64::from(FAR) + 6.5,
    ));
    if name == "survival_hungry" {
        player.hunger_manager.level.store(10);
    }
    let mut sequence = Sequence(0x1a7e + index as u64);
    tap.drain();

    let mut report = String::new();
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut ids = NetIds::default();
    let mut kinds: BTreeMap<&'static str, u32> = BTreeMap::new();
    for step in 1..=STEPS {
        change_hands(&player, &mut sequence);
        let (kind, packet) = transaction(&player, &area, &mut sequence);
        *kinds.entry(kind).or_default() += 1;
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
            client.handle_inventory_action(&player, packet);
        }));
        let packets = if outcome.is_err() {
            *kinds.entry("panic").or_default() += 1;
            "panic".to_string()
        } else {
            packets_text(&mut tap, &mut ids)
        };
        clear_grown_plants(&world, &area);
        let text = format!(
            "{packets} | {} | {}",
            player_text(&player),
            world_text(&world, &area)
        );
        dump(name, step, kind, &text);
        clear_spawned(&world, &area);
        hash = fnv(hash, text.as_bytes());
        if step % 50 == 0 {
            let _ = writeln!(report, "{name} steps {}-{step}: {hash:016x}", step - 49);
        }
    }
    let _ = writeln!(report, "{name} kinds: {kinds:?}");
    report.push_str(&on_cow_steps(
        &player,
        client,
        &mut tap,
        &mut ids,
        (name, &world, &area),
    ));
    Ok(report)
}

/// Fixed last steps: one spawn egg, then one wheat, used on the cow. The egg is not handled by
/// the cow, so the item's own entity use spawns a calf; the wheat is eaten by the cow and empties
/// the stack, which is not a break.
fn on_cow_steps(
    player: &Arc<Player>,
    client: &crate::net::bedrock::BedrockClient,
    tap: &mut crate::net::bedrock::BedrockPacketTap,
    ids: &mut NetIds,
    (name, world, area): (&str, &Arc<World>, &Area),
) -> String {
    let mut report = String::new();
    for (label, item) in [("egg", &Item::COW_SPAWN_EGG), ("wheat", &Item::WHEAT)] {
        player.inventory.set_held_item(ItemStack::new(1, item));
        let held = player.inventory.held_item();
        let packet = SInventoryTransaction {
            legacy_request_id: VarInt(0),
            legacy_set_item_slots: Vec::new(),
            has_value: false,
            actions: Vec::new(),
            transaction_type: VarUInt(0),
            transaction_data: TransactionData::UseItemOnEntity(UseItemOnEntityTransactionData {
                target_entity_runtime_id: VarULong(area.cow as u64),
                action_type: VarInt(0),
                hot_bar_slot: VarInt(player.inventory.get_selected_slot().into()),
                hand: HandSlot::Mainhand,
                item_in_hand: descriptor(&held),
                player_position: Vector3::new(area.corner as f32 + 4.5, 66.62, FAR as f32 + 6.5),
                click_position: Vector3::new(0.0, 0.0, 0.0),
            }),
        };
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
            client.handle_inventory_action(player, packet);
        }));
        let packets = if outcome.is_err() {
            "panic".to_string()
        } else {
            packets_text(tap, ids)
        };
        let text = format!(
            "{packets} | {} | {}",
            player_text(player),
            world_text(world, area)
        );
        dump(name, 0, label, &text);
        clear_spawned(world, area);
        let _ = writeln!(
            report,
            "{name} {label} on cow: {:016x}",
            fnv(0xcbf2_9ce4_8422_2325, text.as_bytes())
        );
    }
    report
}

#[tokio::test(flavor = "multi_thread")]
async fn inventory_actions() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let mut now = String::new();
    for (index, scene) in SCENES.iter().enumerate() {
        now.push_str(&run_scene(&server, index, *scene).await?);
    }
    server.stop();

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("inventory_actions.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, now);
    Ok(())
}
