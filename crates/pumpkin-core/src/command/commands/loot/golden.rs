//! A record of `/loot` run through the real dispatcher (`tests/golden/loot_command.txt`; rewrite
//! it on purpose with `KVASIR_BLESS=1`), made before `LootExecutor::execute` and
//! `replace_entity_slots` were split.
//!
//! Every case runs in its own area of loaded empty chunks: a chest (stone in slot 0, a full stack
//! of cobblestone in slot 2), a furnace, blocks to mine, an op player standing in the area with
//! shears in the selected slot, a diamond pickaxe in the off hand and an inventory that is full
//! apart from one empty and one nearly full slot, and a zombie. Loot rolls use a pinned seed in
//! test builds (`loot_seed`). One line per case: the command (relative to the area), its result
//! (count or error text), then the chest, furnace, player inventory, ender chest and zombie
//! equipment slots that are not empty, and the item entities the case left in the area (block
//! position and stack). A case that does not return within the deadline is recorded as `HANG`.
//!
//! Appended after the first mutation run (`MOB_CASES`): a tall grass plant to mine (its table
//! checks the block above through the loot position and world, and the mined block's state),
//! mobs spawned for one case only (cows burning or not, mobs whose drops need a player kill),
//! loot that equips armour and weapons, and at the end of each line the attribute values of
//! the player and the zombie before and after (they show which equipment slot an item went to).
//! `SETUP_CASES` turn one biome cell into a bamboo jungle first: the fishing junk and abandoned
//! camp tables check the biome at the loot position.

use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use pumpkin_data::Block;
use pumpkin_data::attributes::Attributes;
use pumpkin_data::block_properties::{DoubleBlockHalf, LargeFernProperties};
use pumpkin_data::data_component_impl::EquipmentSlot;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_inventory::Inventory;
use pumpkin_util::GameMode;
use pumpkin_util::PermissionLvl;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::text::TextComponent;
use pumpkin_world::world::BlockFlags;

use crate::command::{CommandSender, CommandSource};
use crate::entity::ai::test_support;
use crate::entity::living::LivingEntity;
use crate::entity::player::Player;
use crate::entity::{Entity, EntityBase};
use crate::server::Server;
use crate::world::World;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const CHEST: (i32, i32, i32) = (4, 61, 4);
const FURNACE: (i32, i32, i32) = (6, 61, 4);
const STONE: (i32, i32, i32) = (8, 61, 4);
const GLASS: (i32, i32, i32) = (9, 61, 4);
const DIAMOND_ORE: (i32, i32, i32) = (10, 61, 4);
const GRAVEL: (i32, i32, i32) = (11, 61, 4);
const SHORT_GRASS: (i32, i32, i32) = (12, 61, 4);
const OAK_LOG: (i32, i32, i32) = (13, 61, 4);
const EMPTY: (i32, i32, i32) = (14, 61, 4);
const TALL_GRASS: (i32, i32, i32) = (15, 61, 4);
const TALL_GRASS_TOP: (i32, i32, i32) = (15, 62, 4);
const SPAWN: (i32, i32, i32) = (20, 62, 8);
const PLAYER_AT: (i32, i32, i32) = (24, 61, 8);
const ZOMBIE_AT: (i32, i32, i32) = (28, 61, 8);

const ZOMBIE: &str = "@e[type=minecraft:zombie,limit=1,sort=nearest]";

/// Placeholders: `{c}` chest, `{f}` furnace, `{e}` an empty position, `{p}` the spawn point,
/// `{stone}` `{glass}` `{ore}` `{gravel}` `{grass}` `{log}` the blocks to mine, `{z}` the zombie.
const CASES: &[&str] = &[
    // Sources (dropped at the spawn point).
    "loot spawn {p} loot minecraft:blocks/diamond_block",
    "loot spawn {p} loot blocks/diamond_block",
    "loot spawn {p} loot minecraft:no_such_table",
    "loot spawn {p} loot minecraft:chests/simple_dungeon",
    "loot spawn {p} loot minecraft:entities/zombie",
    "loot spawn {p} fish minecraft:gameplay/fishing {stone}",
    "loot spawn {p} fish gameplay/fishing {stone} minecraft:fishing_rod",
    "loot spawn {p} fish minecraft:gameplay/fishing {stone} mainhand",
    "loot spawn {p} fish minecraft:gameplay/fishing {stone} offhand",
    "loot spawn {p} fish minecraft:no_such_table {stone}",
    "loot spawn {p} mine {stone}",
    "loot spawn {p} mine {stone} minecraft:wooden_pickaxe",
    "loot spawn {p} mine {stone} mainhand",
    "loot spawn {p} mine {stone} offhand",
    "loot spawn {p} mine {glass}",
    "loot spawn {p} mine {glass} minecraft:diamond_pickaxe[minecraft:enchantments={\"minecraft:silk_touch\":1}]",
    "loot spawn {p} mine {ore} offhand",
    "loot spawn {p} mine {ore} minecraft:diamond_pickaxe[minecraft:enchantments={\"minecraft:fortune\":3}]",
    "loot spawn {p} mine {gravel}",
    "loot spawn {p} mine {grass}",
    "loot spawn {p} mine {grass} mainhand",
    "loot spawn {p} mine {log}",
    "loot spawn {p} mine {e}",
    "loot spawn {p} kill {z}",
    "loot spawn {p} kill @s",
    "loot spawn {p} kill @e[type=minecraft:cow]",
    // Targets.
    "loot give @s loot minecraft:blocks/diamond_block",
    "loot give @s loot minecraft:chests/simple_dungeon",
    "loot give @s mine {stone}",
    "loot give @s kill {z}",
    "loot insert {c} loot minecraft:blocks/diamond_block",
    "loot insert {c} loot minecraft:blocks/stone",
    "loot insert {c} loot minecraft:chests/simple_dungeon",
    "loot insert {f} loot minecraft:chests/simple_dungeon",
    "loot insert {e} loot minecraft:blocks/diamond_block",
    "loot insert {stone} loot minecraft:blocks/diamond_block",
    "loot replace block {c} container.0 loot minecraft:blocks/diamond_block",
    "loot replace block {c} container.5 2 loot minecraft:blocks/diamond_block",
    "loot replace block {c} container.1 loot minecraft:chests/simple_dungeon",
    "loot replace block {c} container.25 4 loot minecraft:chests/simple_dungeon",
    "loot replace block {c} container.26 loot minecraft:blocks/diamond_block",
    "loot replace block {c} container.2 0 loot minecraft:blocks/diamond_block",
    "loot replace block {f} container.1 3 loot minecraft:chests/simple_dungeon",
    "loot replace block {f} container.3 loot minecraft:blocks/diamond_block",
    "loot replace block {e} container.0 loot minecraft:blocks/diamond_block",
    "loot replace entity @s weapon.mainhand loot minecraft:blocks/diamond_block",
    "loot replace entity @s weapon.offhand loot minecraft:blocks/diamond_block",
    "loot replace entity @s armor.head loot minecraft:blocks/diamond_block",
    "loot replace entity @s armor.feet 4 loot minecraft:chests/simple_dungeon",
    "loot replace entity @s hotbar.3 2 loot minecraft:chests/simple_dungeon",
    "loot replace entity @s inventory.26 3 loot minecraft:chests/simple_dungeon",
    "loot replace entity @s enderchest.0 3 loot minecraft:chests/simple_dungeon",
    "loot replace entity @s enderchest.26 2 loot minecraft:blocks/diamond_block",
    "loot replace entity @s container.0 loot minecraft:blocks/diamond_block",
    "loot replace entity @s horse.saddle loot minecraft:blocks/diamond_block",
    "loot replace entity @s armor.body loot minecraft:blocks/diamond_block",
    "loot replace entity {z} weapon.mainhand loot minecraft:blocks/diamond_block",
    "loot replace entity {z} weapon.offhand loot minecraft:blocks/diamond_block",
    "loot replace entity {z} armor.feet 4 loot minecraft:chests/simple_dungeon",
    "loot replace entity {z} armor.body loot minecraft:blocks/diamond_block",
    "loot replace entity {z} saddle loot minecraft:blocks/diamond_block",
    "loot replace entity {z} container.0 loot minecraft:blocks/diamond_block",
    "loot replace entity {z} weapon.mainhand 0 loot minecraft:blocks/diamond_block",
    "loot replace entity @e[type=minecraft:cow] weapon.mainhand loot minecraft:blocks/diamond_block",
];

/// A case with mobs spawned for it alone, beside the zombie: (entity type, fire ticks to set;
/// `None` leaves the new entity's own value).
type Mobs = &'static [(&'static EntityType, Option<i32>)];

const NO_MOBS: Mobs = &[];

/// Cases appended after the first mutation run; `{tall}` and `{tall_top}` are the two halves of
/// the tall grass, `{mob}` the nearest mob of the case's first type.
const MOB_CASES: &[(&str, Mobs)] = &[
    ("loot spawn {p} mine {tall} mainhand", NO_MOBS),
    ("loot spawn {p} mine {tall}", NO_MOBS),
    ("loot spawn {p} mine {tall} offhand", NO_MOBS),
    ("loot spawn {p} mine {tall_top} mainhand", NO_MOBS),
    ("loot spawn {p} kill {mob}", &[(&EntityType::COW, None)]),
    (
        "loot spawn {p} kill {mob}",
        &[(&EntityType::COW, Some(100))],
    ),
    (
        "loot spawn {p} kill @e[type=minecraft:cow,distance=..30]",
        &[(&EntityType::COW, None), (&EntityType::COW, Some(100))],
    ),
    ("loot spawn {p} kill {mob}", &[(&EntityType::BLAZE, None)]),
    ("loot spawn {p} kill {mob}", &[(&EntityType::EVOKER, None)]),
    ("loot spawn {p} kill {mob}", &[(&EntityType::RABBIT, None)]),
    (
        "loot spawn {p} kill {mob}",
        &[(&EntityType::WITHER_SKELETON, None)],
    ),
    ("loot give @s kill {mob}", &[(&EntityType::BLAZE, None)]),
    (
        "loot replace entity {z} armor.feet 4 loot minecraft:blocks/diamond_block",
        NO_MOBS,
    ),
    (
        "loot replace entity {z} armor.feet 4 loot minecraft:equipment/trial_chamber",
        NO_MOBS,
    ),
    (
        "loot replace entity {z} weapon.mainhand loot minecraft:equipment/trial_chamber_melee",
        NO_MOBS,
    ),
    (
        "loot replace entity @s armor.feet 4 loot minecraft:equipment/trial_chamber",
        NO_MOBS,
    ),
    (
        "loot replace entity @s armor.feet loot minecraft:equipment/trial_chamber",
        NO_MOBS,
    ),
    (
        "loot replace entity @s armor.legs loot minecraft:equipment/trial_chamber",
        NO_MOBS,
    ),
    (
        "loot replace entity @s armor.chest loot minecraft:equipment/trial_chamber",
        NO_MOBS,
    ),
    (
        "loot replace entity @s armor.head loot minecraft:equipment/trial_chamber",
        NO_MOBS,
    ),
    (
        "loot replace entity @s hotbar.0 4 loot minecraft:equipment/trial_chamber",
        NO_MOBS,
    ),
    (
        "loot replace entity @s hotbar.1 4 loot minecraft:equipment/trial_chamber",
        NO_MOBS,
    ),
    (
        "loot replace entity @s weapon.mainhand loot minecraft:equipment/trial_chamber_melee",
        NO_MOBS,
    ),
    (
        "loot replace entity @s weapon.offhand loot minecraft:equipment/trial_chamber_melee",
        NO_MOBS,
    ),
    (
        "loot replace entity @s hotbar.1 loot minecraft:equipment/trial_chamber_melee",
        NO_MOBS,
    ),
    (
        "loot replace entity @s armor.feet 4 loot minecraft:chests/village/village_armorer",
        NO_MOBS,
    ),
    (
        "loot replace entity @s hotbar.1 4 loot minecraft:chests/village/village_armorer",
        NO_MOBS,
    ),
    (SELECTS_SLOT_1, NO_MOBS),
    ("loot spawn {p} kill {mob}", &[(&EntityType::COW, Some(0))]),
    ("loot spawn {p} kill {mob}", &[(&EntityType::SHEEP, None)]),
];

/// Cases that first run a setup command in the area (its result ends the line). `{fish_at}` is a
/// block whose biome cell alone is turned into a bamboo jungle by `{jungle_fish}`; `{jungle_player}`
/// does the same to the cell the player stands in. Each runs once without the biome as a control.
const SETUP_CASES: &[(&str, &str)] = &[
    (
        "",
        "loot spawn {p} fish minecraft:gameplay/fishing/junk {fish_at}",
    ),
    (
        "fillbiome {jungle_fish} minecraft:bamboo_jungle",
        "loot spawn {p} fish minecraft:gameplay/fishing/junk {fish_at}",
    ),
    (
        "",
        "loot spawn {p} loot minecraft:chests/abandoned_camp_common_chest",
    ),
    (
        "fillbiome {jungle_player} minecraft:bamboo_jungle",
        "loot spawn {p} loot minecraft:chests/abandoned_camp_common_chest",
    ),
];

/// The one case whose player has hotbar slot 1 selected, so the sword the table drops second
/// lands in the main hand.
const SELECTS_SLOT_1: &str =
    "loot replace entity @s hotbar.0 2 loot minecraft:equipment/trial_chamber_melee";

fn at(x0: i32, (x, y, z): (i32, i32, i32)) -> BlockPos {
    BlockPos::new(x0 + x, y, z)
}

fn place(world: &Arc<World>, pos: BlockPos, block: &Block) {
    world.set_block_state(&pos, block.default_state.id, BlockFlags::NOTIFY_ALL);
}

fn stack_text(stack: &ItemStack) -> String {
    format!("{}x{}", stack.item_count, stack.item.registry_key)
}

/// The area's blocks, chest and furnace; returns its x origin.
fn build_area(world: &Arc<World>, index: usize) -> i32 {
    let chunk_x = index as i32 * 4;
    for cx in chunk_x..chunk_x + 4 {
        for cz in -1..3 {
            world.level.loaded_chunks.insert(
                Vector2::new(cx, cz),
                pumpkin_world::chunk::ChunkData::empty_sync(cx, cz),
            );
        }
    }
    let x0 = chunk_x * 16;
    for x in 0..40 {
        for z in 0..16 {
            world.set_block_state(
                &BlockPos::new(x0 + x, 60, z),
                Block::STONE.default_state.id,
                BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
            );
        }
    }
    place(world, at(x0, CHEST), &Block::CHEST);
    if let Some(entity) = world.get_block_entity(&at(x0, CHEST))
        && let Some(inventory) = entity.get_inventory()
    {
        inventory.set_stack(0, ItemStack::new(10, &Item::STONE));
        inventory.set_stack(2, ItemStack::new(64, &Item::COBBLESTONE));
    }
    place(world, at(x0, FURNACE), &Block::FURNACE);
    place(world, at(x0, STONE), &Block::STONE);
    place(world, at(x0, GLASS), &Block::GLASS);
    place(world, at(x0, DIAMOND_ORE), &Block::DIAMOND_ORE);
    place(world, at(x0, GRAVEL), &Block::GRAVEL);
    // Short grass needs a grass block under it, or it pops off and drops seeds at random.
    let (gx, _, gz) = SHORT_GRASS;
    place(world, BlockPos::new(x0 + gx, 60, gz), &Block::GRASS_BLOCK);
    place(world, at(x0, SHORT_GRASS), &Block::SHORT_GRASS);
    place(world, at(x0, OAK_LOG), &Block::OAK_LOG);
    x0
}

/// A two-block tall grass on a grass block (both halves forced, no neighbour updates).
fn place_tall_grass(world: &Arc<World>, x0: i32) {
    let flags = BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK;
    let (x, _, z) = TALL_GRASS;
    world.set_block_state(
        &BlockPos::new(x0 + x, 60, z),
        Block::GRASS_BLOCK.default_state.id,
        flags,
    );
    for (pos, half) in [
        (TALL_GRASS, DoubleBlockHalf::Lower),
        (TALL_GRASS_TOP, DoubleBlockHalf::Upper),
    ] {
        let state = LargeFernProperties { half }.to_state_id(&Block::TALL_GRASS);
        world.set_block_state(&at(x0, pos), state, flags);
    }
}

/// The case's own mobs, two blocks apart beside the zombie.
fn add_mobs(world: &Arc<World>, x0: i32, mobs: Mobs) {
    let (x, y, z) = ZOMBIE_AT;
    for (index, (kind, fire_ticks)) in mobs.iter().enumerate() {
        let mob = Arc::new(LivingEntity::new(Entity::new(
            Arc::clone(world),
            Vector3::new(
                f64::from(x0 + x) + 0.5,
                f64::from(y),
                f64::from(z + 2 + 2 * index as i32) + 0.5,
            ),
            kind,
        )));
        if let Some(ticks) = fire_ticks {
            mob.entity.fire_ticks.store(*ticks, Ordering::Relaxed);
        }
        world.spawn_entity(mob);
    }
}

/// Attribute values that equipment changes, for the player and the zombie.
fn attributes_text(player: &Arc<Player>, zombie: &Arc<LivingEntity>) -> String {
    let mut text = String::new();
    for (who, living) in [
        ("player", &player.living_entity),
        ("zombie", zombie.as_ref()),
    ] {
        let values: Vec<String> = [
            &Attributes::ARMOR,
            &Attributes::ARMOR_TOUGHNESS,
            &Attributes::ATTACK_DAMAGE,
            &Attributes::ATTACK_SPEED,
            &Attributes::KNOCKBACK_RESISTANCE,
        ]
        .iter()
        .map(|attribute| format!("{}", living.get_attribute_value(attribute)))
        .collect();
        let _ = write!(text, "{who}[{}]", values.join(","));
    }
    text
}

/// An op player in the area: shears selected, a diamond pickaxe in the off hand, slots 1-35
/// full of dirt except slot 5 (empty) and slot 6 (63 stone).
fn add_player(world: &Arc<World>, x0: i32) -> Arc<Player> {
    let player = test_support::java_player(world, GameMode::Survival);
    player.permission_lvl.store(PermissionLvl::Four);
    let (x, y, z) = PLAYER_AT;
    player.living_entity.entity.set_pos(Vector3::new(
        f64::from(x0 + x) + 0.5,
        f64::from(y),
        f64::from(z) + 0.5,
    ));
    let inventory = player.inventory();
    inventory.set_selected_slot(0);
    inventory.set_stack(0, ItemStack::new(1, &Item::SHEARS));
    for slot in 1..36 {
        match slot {
            5 => {}
            6 => inventory.set_stack(slot, ItemStack::new(63, &Item::STONE)),
            _ => inventory.set_stack(slot, ItemStack::new(64, &Item::DIRT)),
        }
    }
    inventory.set_stack(40, ItemStack::new(1, &Item::DIAMOND_PICKAXE));
    let _ = world.add_player(&player);
    player
}

fn add_zombie(world: &Arc<World>, x0: i32) -> Arc<LivingEntity> {
    let (x, y, z) = ZOMBIE_AT;
    let zombie = Arc::new(LivingEntity::new(Entity::new(
        Arc::clone(world),
        Vector3::new(f64::from(x0 + x) + 0.5, f64::from(y), f64::from(z) + 0.5),
        &EntityType::ZOMBIE,
    )));
    world.spawn_entity(zombie.clone());
    zombie
}

fn inventory_text(inventory: Option<Arc<dyn pumpkin_inventory::Inventory>>) -> String {
    let Some(inventory) = inventory else {
        return "none".to_owned();
    };
    let mut slots = Vec::new();
    for slot in 0..inventory.size() {
        let stack = inventory.get_stack(slot);
        if !stack.is_empty() {
            slots.push(format!("{slot}={}", stack_text(&stack)));
        }
    }
    slots.join(" ")
}

fn record(world: &Arc<World>, x0: i32, player: &Arc<Player>, zombie: &Arc<LivingEntity>) -> String {
    let container = |pos| {
        world
            .get_block_entity(&at(x0, pos))
            .and_then(crate::block::entities::BlockEntity::get_inventory)
    };
    let chest = inventory_text(container(CHEST));
    let furnace = inventory_text(container(FURNACE));
    let mut held = Vec::new();
    let inventory = player.inventory();
    for slot in 0..inventory.size() {
        let stack = inventory.get_stack(slot);
        let default = match slot {
            0 => ItemStack::new(1, &Item::SHEARS),
            5 | 36..=39 | 41.. => ItemStack::EMPTY.clone(),
            6 => ItemStack::new(63, &Item::STONE),
            40 => ItemStack::new(1, &Item::DIAMOND_PICKAXE),
            _ => ItemStack::new(64, &Item::DIRT),
        };
        if stack_text(&stack) != stack_text(&default) && !(stack.is_empty() && default.is_empty()) {
            held.push(format!("{slot}={}", stack_text(&stack)));
        }
    }
    let mut ender = Vec::new();
    for slot in 0..player.ender_chest_inventory.size() {
        let stack = player.ender_chest_inventory.get_stack(slot);
        if !stack.is_empty() {
            ender.push(format!("{slot}={}", stack_text(&stack)));
        }
    }
    let mut equipment = Vec::new();
    {
        let worn = zombie
            .entity_equipment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (name, slot) in [
            ("mainhand", EquipmentSlot::MAIN_HAND),
            ("offhand", EquipmentSlot::OFF_HAND),
            ("feet", EquipmentSlot::FEET),
            ("legs", EquipmentSlot::LEGS),
            ("chest", EquipmentSlot::CHEST),
            ("head", EquipmentSlot::HEAD),
            ("body", EquipmentSlot::BODY),
            ("saddle", EquipmentSlot::SADDLE),
        ] {
            let stack = worn.get(&slot);
            if !stack.is_empty() {
                equipment.push(format!("{name}={}", stack_text(&stack)));
            }
        }
    }
    let mut items = Vec::new();
    for entity in world.entities.load().iter() {
        let base = entity.get_entity();
        let pos = base.pos.load();
        if pos.x < f64::from(x0) || pos.x >= f64::from(x0 + 64) {
            continue;
        }
        if let Some(item) = entity.get_item_entity() {
            let stack = item
                .get_item_stack()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            items.push(format!(
                "{},{},{}:{}",
                pos.x.floor() as i32 - x0,
                pos.y.floor() as i32,
                pos.z.floor() as i32,
                stack_text(&stack)
            ));
        }
    }
    items.sort();
    format!(
        "chest[{chest}] furnace[{furnace}] player[{}] ender[{}] zombie[{}] items[{}]",
        held.join(" "),
        ender.join(" "),
        equipment.join(" "),
        items.join(" ")
    )
}

fn run(server: &Arc<Server>, world: &Arc<World>, player: &Arc<Player>, command: &str) -> String {
    let source = CommandSource::new(
        CommandSender::Player(player.clone()),
        world.clone(),
        Some(player.clone() as Arc<dyn EntityBase>),
        player.living_entity.entity.pos.load(),
        Vector2::new(0.0, 0.0),
        "Golden".to_string(),
        TextComponent::text("Golden"),
        server.clone(),
    )
    .with_silent();
    let server = server.clone();
    let command = command.to_owned();
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let dispatcher = server.command_dispatcher.load();
        let result = dispatcher.execute_input(&command, &source);
        let _ = send.send(match result {
            Ok(count) => format!("ok {count}"),
            Err(error) => format!("err {}", error.message.get_text()),
        });
    });
    receive
        // Generous: the 32768-block cases take over 20 s in a debug build while the whole
        // suite runs, and a deadlock still shows as `HANG`.
        .recv_timeout(Duration::from_secs(120))
        .unwrap_or_else(|_| "HANG".to_owned())
}

/// The command with every placeholder replaced by absolute coordinates in the area.
fn expand(case: &str, x0: i32) -> String {
    let pos = |(x, y, z): (i32, i32, i32)| format!("{} {y} {z}", x0 + x);
    case.replace("{c}", &pos(CHEST))
        .replace("{f}", &pos(FURNACE))
        .replace("{e}", &pos(EMPTY))
        .replace("{p}", &pos(SPAWN))
        .replace("{stone}", &pos(STONE))
        .replace("{glass}", &pos(GLASS))
        .replace("{ore}", &pos(DIAMOND_ORE))
        .replace("{gravel}", &pos(GRAVEL))
        .replace("{grass}", &pos(SHORT_GRASS))
        .replace("{log}", &pos(OAK_LOG))
        .replace("{z}", ZOMBIE)
        .replace("{tall_top}", &pos(TALL_GRASS_TOP))
        .replace("{fish_at}", &format!("{} 64 4", x0 + 8))
        .replace(
            "{jungle_fish}",
            &format!("{} 64 4 {} 67 7", x0 + 8, x0 + 11),
        )
        .replace(
            "{jungle_player}",
            &format!("{} 60 8 {} 63 11", x0 + 24, x0 + 27),
        )
        .replace("{tall}", &pos(TALL_GRASS))
}

#[tokio::test(flavor = "multi_thread")]
async fn loot_command_gives_the_same_items() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();

    let mut now = String::new();
    for (index, case) in CASES.iter().enumerate() {
        let x0 = build_area(&world, index + 1);
        let player = add_player(&world, x0);
        let zombie = add_zombie(&world, x0);
        let before = record(&world, x0, &player, &zombie);
        let result = run(&server, &world, &player, &expand(case, x0));
        let after = record(&world, x0, &player, &zombie);
        writeln!(now, "{case} -> {result}; before {before}; after {after}")?;
        world.remove_player(&player, false).await;
    }
    let appended = MOB_CASES
        .iter()
        .map(|(case, mobs)| ("", *case, *mobs))
        .chain(
            SETUP_CASES
                .iter()
                .map(|(setup, case)| (*setup, *case, NO_MOBS)),
        );
    for (index, (setup, case, mobs)) in appended.enumerate() {
        let x0 = build_area(&world, CASES.len() + index + 1);
        place_tall_grass(&world, x0);
        let player = add_player(&world, x0);
        if case == SELECTS_SLOT_1 {
            player.inventory().set_selected_slot(1);
        }
        let zombie = add_zombie(&world, x0);
        add_mobs(&world, x0, mobs);
        let command = match mobs.first() {
            Some((kind, _)) => case.replace(
                "{mob}",
                &format!(
                    "@e[type=minecraft:{},limit=1,sort=nearest]",
                    kind.resource_name
                ),
            ),
            None => case.to_owned(),
        };
        let setup_result = if setup.is_empty() {
            String::new()
        } else {
            format!(
                "; setup {}",
                run(&server, &world, &player, &expand(setup, x0))
            )
        };
        let attributes_before = attributes_text(&player, &zombie);
        let before = record(&world, x0, &player, &zombie);
        let result = run(&server, &world, &player, &expand(&command, x0));
        let after = record(&world, x0, &player, &zombie);
        let attributes_after = attributes_text(&player, &zombie);
        writeln!(
            now,
            "{case} -> {result}; before {before}; after {after}; attributes {attributes_before} -> {attributes_after}{setup_result}"
        )?;
        world.remove_player(&player, false).await;
    }

    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/loot_command.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &now)?;
    } else {
        assert_eq!(std::fs::read_to_string(&path)?, now);
    }
    server.stop();
    Ok(())
}
