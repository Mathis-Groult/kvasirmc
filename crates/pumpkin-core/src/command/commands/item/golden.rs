//! A record of `/item replace entity` run through the shared script harness
//! (`tests/golden/item_command.txt`; rewrite it on purpose with `KVASIR_BLESS=1`), made before
//! `EntityReplaceExecutor::execute` was split.
//!
//! The four script players (see `script_golden`; `GoldB` has hotbar slot 3 selected) plus a
//! zombie and a horse with fixed entity ids and UUIDs (the feedback names a single target with
//! its UUID in the hover text). Per step the command, its result, every packet each
//! player was sent (only their number for the Bedrock player, see `packets_bedrock_counted`),
//! then every player's non-empty inventory and ender chest slots and each mob's
//! non-empty equipment.

use std::fmt::Write;
use std::sync::Arc;

use pumpkin_data::data_component_impl::EquipmentSlot;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_inventory::Inventory;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;

use crate::command::commands::script_golden::{self, Online, TestResult};
use crate::entity::Entity;
use crate::entity::ai::test_support;
use crate::entity::living::LivingEntity;
use crate::world::World;

const STEPS: &[&str] = &[
    // Player slots.
    "item replace entity GoldA hotbar.0 with minecraft:diamond",
    "item replace entity GoldA hotbar.8 with minecraft:diamond 5",
    "item replace entity GoldA inventory.0 with minecraft:emerald 64",
    "item replace entity GoldA inventory.26 with minecraft:emerald 99",
    "item replace entity GoldA container.35 with minecraft:stone",
    "item replace entity GoldA container.36 with minecraft:stone",
    "item replace entity GoldA weapon.mainhand with minecraft:iron_sword",
    "item replace entity GoldB weapon.mainhand with minecraft:iron_sword",
    "item replace entity GoldB weapon.offhand with minecraft:shield",
    "item replace entity GoldB armor.feet with minecraft:iron_boots",
    "item replace entity GoldB armor.legs with minecraft:iron_leggings",
    "item replace entity GoldB armor.chest with minecraft:iron_chestplate",
    "item replace entity GoldB armor.head with minecraft:iron_helmet",
    "item replace entity GoldB armor.body with minecraft:saddle",
    "item replace entity GoldB saddle with minecraft:saddle",
    "item replace entity GoldB enderchest.0 with minecraft:gold_ingot",
    "item replace entity GoldB enderchest.26 with minecraft:gold_ingot 3",
    "item replace entity GoldB player.cursor with minecraft:gold_ingot",
    "item replace entity GoldB player.crafting.0 with minecraft:gold_ingot",
    "item replace entity GoldB horse.0 with minecraft:gold_ingot",
    "item replace entity GoldB villager.0 with minecraft:gold_ingot",
    "item replace entity GoldB hotbar.3 with minecraft:air",
    "item replace entity GoldD hotbar.1 with minecraft:diamond_sword[minecraft:damage=5]",
    "item replace entity GoldD armor.head with minecraft:carved_pumpkin",
    // Several players.
    "item replace entity @a hotbar.4 with minecraft:apple 2",
    "item replace entity @a armor.body with minecraft:apple",
    "item replace entity @a[name=!GoldA] enderchest.5 with minecraft:apple",
    // Mobs.
    "item replace entity @e[type=minecraft:zombie] weapon.mainhand with minecraft:iron_axe",
    "item replace entity @e[type=minecraft:zombie] weapon.offhand with minecraft:torch",
    "item replace entity @e[type=minecraft:zombie] armor.feet with minecraft:golden_boots",
    "item replace entity @e[type=minecraft:zombie] armor.legs with minecraft:golden_leggings",
    "item replace entity @e[type=minecraft:zombie] armor.chest with minecraft:golden_chestplate",
    "item replace entity @e[type=minecraft:zombie] armor.head with minecraft:golden_helmet",
    "item replace entity @e[type=minecraft:horse] armor.body with minecraft:iron_horse_armor",
    "item replace entity @e[type=minecraft:horse] saddle with minecraft:saddle",
    "item replace entity @e[type=minecraft:zombie] hotbar.0 with minecraft:stone",
    "item replace entity @e[type=minecraft:zombie] enderchest.0 with minecraft:stone",
    "item replace entity @e[type=minecraft:zombie] horse.0 with minecraft:stone",
    // Everything at once.
    "item replace entity @e armor.body with minecraft:leather_horse_armor",
    "item replace entity @e weapon.mainhand with minecraft:stick 7",
    "item replace entity @e[type=!minecraft:player] armor.head with minecraft:skeleton_skull",
    "item replace entity @e enderchest.1 with minecraft:stick",
    // Failures.
    "item replace entity @e[type=minecraft:cow] hotbar.0 with minecraft:stone",
    "item replace entity GoldA hotbar.9 with minecraft:stone",
    "item replace entity GoldA nowhere with minecraft:stone",
    "item replace entity GoldA hotbar.0 with minecraft:stone 0",
    "item replace entity GoldA hotbar.0 with minecraft:stone 100",
    "item replace entity GoldA hotbar.0 with minecraft:no_such_item",
    "item replace entity GoldA hotbar.0",
];

fn stack_text(stack: &ItemStack) -> String {
    format!("{}x{}", stack.item_count, stack.item.registry_key)
}

fn slots_text(inventory: &dyn Inventory) -> String {
    let mut slots = Vec::new();
    for slot in 0..inventory.size() {
        let stack = inventory.get_stack(slot);
        if !stack.is_empty() {
            slots.push(format!("{slot}={}", stack_text(&stack)));
        }
    }
    slots.join(",")
}

const EQUIPMENT: [(&str, EquipmentSlot); 8] = [
    ("mainhand", EquipmentSlot::MAIN_HAND),
    ("offhand", EquipmentSlot::OFF_HAND),
    ("feet", EquipmentSlot::FEET),
    ("legs", EquipmentSlot::LEGS),
    ("chest", EquipmentSlot::CHEST),
    ("head", EquipmentSlot::HEAD),
    ("body", EquipmentSlot::BODY),
    ("saddle", EquipmentSlot::SADDLE),
];

fn equipment_text(living: &LivingEntity) -> String {
    let worn = living
        .entity_equipment
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut slots = Vec::new();
    for (name, slot) in EQUIPMENT {
        let stack = worn.get(&slot);
        if !stack.is_empty() {
            slots.push(format!("{name}={}", stack_text(&stack)));
        }
    }
    slots.join(",")
}

fn state(online: &[Online], mobs: &[(&str, Arc<LivingEntity>)]) -> String {
    let mut text = String::new();
    for who in online {
        let _ = write!(
            text,
            " {}[{}|ender {}|worn {}]",
            who.name,
            slots_text(who.player.inventory().as_ref()),
            slots_text(who.player.ender_chest_inventory.as_ref()),
            equipment_text(&who.player.living_entity)
        );
    }
    for (name, mob) in mobs {
        let _ = write!(text, " {name}[{}]", equipment_text(mob));
    }
    text
}

fn spawn_mob(world: &Arc<World>, kind: &'static EntityType, entity_id: i32) -> Arc<LivingEntity> {
    let mut entity = Entity::new(Arc::clone(world), Vector3::new(8.5, 64.0, 8.5), kind);
    entity.entity_id = entity_id;
    entity.entity_uuid = uuid::Uuid::from_u128(0x0b05_5ba0_0001_0000 + entity_id as u128);
    let mob = Arc::new(LivingEntity::new(entity));
    world.spawn_entity(mob.clone());
    mob
}

#[tokio::test(flavor = "multi_thread")]
async fn item_command_changes_the_same_slots() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    for cx in -1..2 {
        for cz in -1..2 {
            world.level.loaded_chunks.insert(
                Vector2::new(cx, cz),
                pumpkin_world::chunk::ChunkData::empty_sync(cx, cz),
            );
        }
    }
    let mobs = [
        ("zombie", spawn_mob(&world, &EntityType::ZOMBIE, 71_101)),
        ("horse", spawn_mob(&world, &EntityType::HORSE, 71_102)),
    ];
    let mut online = script_golden::join(&world).await?;
    online[1].player.inventory().set_selected_slot(3);
    let source_player = online[0].player.clone();

    let mut now = String::new();
    writeln!(now, "start;{}", state(&online, &mobs))?;
    for step in STEPS {
        let result = script_golden::run(&server, &source_player, step);
        let sent = script_golden::packets_bedrock_counted(&mut online);
        writeln!(now, "{step} -> {result};{sent};{}", state(&online, &mobs))?;
    }

    script_golden::bless_or_compare("item_command.txt", &now)?;
    script_golden::finish(&server, &world, &online).await;
    Ok(())
}

/// `player.cursor` and `player.crafting.0-3` (vanilla `Player.getSlot` 499 and 500-503) reach
/// the carried item of the open menu and the cells of the inventory menu's 2x2 grid.
#[tokio::test(flavor = "multi_thread")]
async fn item_command_reaches_the_cursor_and_the_crafting_grid() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let online = script_golden::join(&world).await?;
    let source = online[0].player.clone();
    let target = online[1].player.clone();

    for step in [
        "item replace entity GoldB player.cursor with minecraft:gold_ingot 5",
        "item replace entity GoldB player.crafting.2 with minecraft:diamond 3",
    ] {
        let result = script_golden::run(&server, &source, step);
        assert!(result.starts_with("ok 1"), "{step}: {result}");
    }

    let carried = {
        let handler = target
            .current_screen_handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let handler = handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let stack = handler
            .get_behaviour()
            .cursor_stack
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        (stack.item.registry_key, stack.item_count)
    };
    assert_eq!(carried, ("gold_ingot", 5));

    let grid: Vec<_> = {
        let handler = target
            .player_screen_handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (1..=4)
            .map(|slot| {
                let stack = handler.get_slot(slot).get_cloned_stack();
                (stack.item.registry_key, stack.item_count)
            })
            .collect()
    };
    assert_eq!(
        grid,
        [("air", 0), ("air", 0), ("diamond", 3), ("air", 0)],
        "crafting.2 is the third cell"
    );
    script_golden::finish(&server, &world, &online).await;
    Ok(())
}
