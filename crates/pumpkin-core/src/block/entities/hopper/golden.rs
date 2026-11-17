//! A record of the hopper pulling items in (`tests/golden/hopper_suck.txt`; rewrite it on
//! purpose with `KVASIR_BLESS=1`), made before `suck_in_items` was split.
//!
//! Each scene places a hopper (facing down over air, so it never ejects) with its block entity
//! in the world, something above it (a chest or furnace with items, a block, nothing) and item
//! entities above it or in its bowl, then ticks it through the world's own block entity. A line
//! is written whenever anything changed: the hopper's slots and cooldown, the source's slots,
//! the item entities left in the scene (block position, count, removed), the experience orbs.

use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use pumpkin_data::block_properties::HopperLikeProperties;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::recipes::{CookingRecipeKind, get_cooking_recipe_with_ingredient};
use pumpkin_data::{Block, BlockStateId};
use pumpkin_inventory::Inventory;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::world::BlockFlags;

use super::HopperBlockEntity;
use crate::block::entities::BlockEntity;
use crate::block::entities::chest::ChestBlockEntity;
use crate::block::entities::furnace::FurnaceBlockEntity;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::Entity;
use crate::entity::ai::test_support;
use crate::entity::item::ItemEntity;
use crate::world::World;

type Stacks = &'static [(usize, &'static Item, u8)];

/// What sits in the block above the hopper.
#[derive(Clone, Copy)]
enum Above {
    Nothing,
    Chest(Stacks),
    /// Slots, and whether a smelted raw iron is on record (experience to hand out).
    Furnace(Stacks, bool),
    Block(&'static Block),
}

struct Scene {
    name: &'static str,
    above: Above,
    hopper: Stacks,
    /// Item entities: (y offset from the hopper's base, item, count).
    drops: &'static [(f64, &'static Item, u8)],
    enabled: bool,
    cooldown: i32,
    ticks: u32,
}

const fn scene(name: &'static str, above: Above, hopper: Stacks, ticks: u32) -> Scene {
    Scene {
        name,
        above,
        hopper,
        drops: &[],
        enabled: true,
        cooldown: -1,
        ticks,
    }
}

const fn with_drops(mut s: Scene, drops: &'static [(f64, &'static Item, u8)]) -> Scene {
    s.drops = drops;
    s
}

const fn disabled(mut s: Scene) -> Scene {
    s.enabled = false;
    s
}

const fn with_cooldown(mut s: Scene, cooldown: i32) -> Scene {
    s.cooldown = cooldown;
    s
}

const FULL_OF_STONE: Stacks = &[
    (0, &Item::STONE, 64),
    (1, &Item::STONE, 64),
    (2, &Item::STONE, 64),
    (3, &Item::STONE, 64),
    (4, &Item::STONE, 64),
];
const NO_ROOM_FOR_DIRT: Stacks = &[
    (0, &Item::STONE, 64),
    (1, &Item::STONE, 64),
    (2, &Item::STONE, 64),
    (3, &Item::STONE, 64),
    (4, &Item::COBBLESTONE, 10),
];
const ONE_DIRT_SHORT: Stacks = &[
    (0, &Item::STONE, 64),
    (1, &Item::STONE, 64),
    (2, &Item::STONE, 64),
    (3, &Item::STONE, 64),
    (4, &Item::DIRT, 63),
];

const SCENES: &[Scene] = &[
    scene("nothing above", Above::Nothing, &[], 20),
    scene(
        "chest above",
        Above::Chest(&[(3, &Item::COBBLESTONE, 3), (7, &Item::DIRT, 1)]),
        &[],
        50,
    ),
    scene("empty chest above", Above::Chest(&[]), &[], 20),
    with_drops(
        scene("empty chest above, items on it", Above::Chest(&[]), &[], 20),
        &[(2.0, &Item::DIRT, 2)],
    ),
    scene(
        "chest above, hopper full",
        Above::Chest(&[(0, &Item::DIRT, 5)]),
        FULL_OF_STONE,
        20,
    ),
    scene(
        "chest above, no room for its item",
        Above::Chest(&[(0, &Item::DIRT, 5), (1, &Item::COBBLESTONE, 2)]),
        NO_ROOM_FOR_DIRT,
        30,
    ),
    scene(
        "chest above, one item short of full",
        Above::Chest(&[(0, &Item::DIRT, 5)]),
        ONE_DIRT_SHORT,
        30,
    ),
    scene(
        "furnace output with experience",
        Above::Furnace(&[(2, &Item::IRON_INGOT, 2)], true),
        &[],
        30,
    ),
    scene(
        "furnace input and fuel only",
        Above::Furnace(&[(0, &Item::RAW_IRON, 2), (1, &Item::COAL, 1)], false),
        &[],
        30,
    ),
    scene(
        "furnace output without experience",
        Above::Furnace(&[(2, &Item::IRON_INGOT, 1)], false),
        &[],
        20,
    ),
    with_drops(
        scene("item stack above", Above::Nothing, &[], 40),
        &[(1.2, &Item::DIRT, 3)],
    ),
    with_drops(
        scene("single item above", Above::Nothing, &[], 20),
        &[(1.2, &Item::DIRT, 1)],
    ),
    with_drops(
        scene("two stacks above", Above::Nothing, &[], 40),
        &[(1.2, &Item::DIRT, 2), (1.5, &Item::COBBLESTONE, 2)],
    ),
    with_drops(
        scene("item above, hopper full", Above::Nothing, FULL_OF_STONE, 20),
        &[(1.2, &Item::DIRT, 2)],
    ),
    with_drops(
        scene(
            "item above, no room for it",
            Above::Nothing,
            NO_ROOM_FOR_DIRT,
            20,
        ),
        &[(1.2, &Item::DIRT, 2)],
    ),
    with_drops(
        scene("item in the bowl", Above::Nothing, &[], 20),
        &[(0.7, &Item::DIRT, 2)],
    ),
    with_drops(
        scene("item high above", Above::Nothing, &[], 20),
        &[(2.5, &Item::DIRT, 2)],
    ),
    with_drops(
        scene(
            "stone above, item in it",
            Above::Block(&Block::STONE),
            &[],
            20,
        ),
        &[(1.2, &Item::DIRT, 2)],
    ),
    with_drops(
        scene(
            "beehive above, item in it",
            Above::Block(&Block::BEEHIVE),
            &[],
            20,
        ),
        &[(1.2, &Item::DIRT, 2)],
    ),
    with_drops(
        scene(
            "slab above, item on it",
            Above::Block(&Block::STONE_SLAB),
            &[],
            20,
        ),
        &[(1.2, &Item::DIRT, 2)],
    ),
    with_drops(
        disabled(scene(
            "disabled",
            Above::Chest(&[(0, &Item::DIRT, 5)]),
            &[],
            20,
        )),
        &[(1.2, &Item::DIRT, 2)],
    ),
    with_cooldown(
        scene(
            "preset cooldown",
            Above::Chest(&[(0, &Item::DIRT, 5)]),
            &[],
            20,
        ),
        5,
    ),
];

fn put(world: &Arc<World>, pos: &BlockPos, state: BlockStateId) {
    world.set_block_state(
        pos,
        state,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

fn stacks_text(inventory: &dyn Inventory) -> String {
    let mut slots = Vec::new();
    for slot in 0..inventory.size() {
        let stack = inventory.get_stack(slot);
        if !stack.is_empty() {
            slots.push(format!(
                "{slot}:{}x{}",
                stack.get_item().registry_key,
                stack.item_count
            ));
        }
    }
    slots.join(" ")
}

fn entities_text(world: &World, x0: i32) -> String {
    let mut found = Vec::new();
    for entity in world.entities.load().iter() {
        let base = entity.get_entity();
        let pos = base.pos.load();
        if pos.x < f64::from(x0) || pos.x >= f64::from(x0 + 16) {
            continue;
        }
        let removed = base.removed.load(Ordering::Relaxed);
        let kind = entity.get_item_entity().map_or_else(
            || base.entity_type.resource_name.to_string(),
            |item| {
                let stack = item
                    .get_item_stack()
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone();
                format!("{}x{}", stack.get_item().registry_key, stack.item_count)
            },
        );
        found.push(format!(
            "{}@{}{}",
            kind,
            (pos.y * 16.0).floor() as i32,
            if removed { "(removed)" } else { "" }
        ));
    }
    found.sort();
    found.join(" ")
}

fn place_above(world: &Arc<World>, pos: BlockPos, above: Above) -> Option<Arc<dyn Inventory>> {
    match above {
        Above::Nothing => None,
        Above::Block(block) => {
            put(world, &pos, block.default_state.id);
            None
        }
        Above::Chest(stacks) => {
            put(world, &pos, Block::CHEST.default_state.id);
            let chest = Arc::new(ChestBlockEntity::new(pos));
            for (slot, item, count) in stacks {
                chest.set_stack(*slot, ItemStack::new(*count, item));
            }
            world.add_block_entity(chest.clone());
            Some(chest)
        }
        Above::Furnace(stacks, experience) => {
            put(world, &pos, Block::FURNACE.default_state.id);
            let furnace = Arc::new(FurnaceBlockEntity::new(pos));
            for (slot, item, count) in stacks {
                furnace.set_stack(*slot, ItemStack::new(*count, item));
            }
            if experience
                && let Some(recipe) =
                    get_cooking_recipe_with_ingredient(&Item::RAW_IRON, CookingRecipeKind::Smelting)
                && let Ok(mut used) = furnace.recipes_used.lock()
            {
                used.insert(recipe.recipe_id.to_string(), 3);
            }
            world.add_block_entity(furnace.clone());
            Some(furnace)
        }
    }
}

fn run(out: &mut String, world: &Arc<World>, index: usize, scene: &Scene) -> std::fmt::Result {
    let x0 = index as i32 * 16;
    let position = BlockPos::new(x0 + 8, 64, 8);
    let mut props = HopperLikeProperties::default(&Block::HOPPER);
    props.enabled = scene.enabled;
    put(world, &position, props.to_state_id(&Block::HOPPER));
    let hopper = Arc::new(HopperBlockEntity::new(position, props.facing));
    for (slot, item, count) in scene.hopper {
        hopper.set_stack(*slot, ItemStack::new(*count, item));
    }
    hopper
        .cooldown_time
        .store(scene.cooldown, Ordering::Relaxed);
    world.add_block_entity(hopper.clone());
    let source = place_above(world, position.up(), scene.above);
    for (dy, item, count) in scene.drops {
        let at = Vector3::new(f64::from(x0) + 8.5, 64.0 + dy, 8.5);
        let entity = Entity::new(world.clone(), at, &EntityType::ITEM);
        world.spawn_entity(Arc::new(ItemEntity::new(
            entity,
            ItemStack::new(*count, item),
        )));
    }
    let state = || {
        format!(
            "hopper [{}] cooldown {} source [{}] entities [{}]",
            stacks_text(hopper.as_ref()),
            hopper.cooldown_time.load(Ordering::Relaxed),
            source
                .as_ref()
                .map(|s| stacks_text(s.as_ref()))
                .unwrap_or_default(),
            entities_text(world, x0)
        )
    };
    let mut last = state();
    writeln!(out, "{}: start {last}", scene.name)?;
    for tick in 1..=scene.ticks {
        hopper.tick(world);
        let now = state();
        if now != last {
            writeln!(out, "{}: tick {tick} {now}", scene.name)?;
            last = now;
        }
    }
    writeln!(out, "{}: end", scene.name)
}

#[tokio::test(flavor = "multi_thread")]
async fn hopper_suck_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    for cx in -1..=SCENES.len() as i32 {
        for cz in -1..=1 {
            world.level.loaded_chunks.insert(
                Vector2::new(cx, cz),
                pumpkin_world::chunk::ChunkData::empty_sync(cx, cz),
            );
        }
    }
    let mut now = String::new();
    for (index, scene) in SCENES.iter().enumerate() {
        run(&mut now, &world, index, scene)?;
    }
    script_golden::bless_or_compare("hopper_suck.txt", &now)?;
    server.stop();
    Ok(())
}
