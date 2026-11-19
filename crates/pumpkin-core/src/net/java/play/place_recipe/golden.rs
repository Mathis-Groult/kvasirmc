//! A record of what `JavaClient::handle_place_recipe` does, made before the function was split
//! (`tests/golden/place_recipe.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene gives a fresh player (fixed identity) the scene's inventory and crafting-grid items,
//! opens a crafting table when the scene asks for one, then hands its client the scene's
//! place-recipe packets (recipe display id, use-max flag) in order. One line per scene: the grid
//! slots, the player's inventory, the items dropped nearby and the packets the player was sent
//! (length and FNV-1a of the bytes).
//! Not covered: plugins cancelling the recipe-book click (none is loaded); cooking recipes (the
//! handler returns at once for them, one scene shows it).

use std::num::NonZero;
use std::sync::Arc;

use pumpkin_data::Block;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::recipes::{CraftingRecipeTypes, RECIPES_COOKING, RECIPES_CRAFTING};
use pumpkin_inventory::Inventory;
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::java::server::play::SPlaceRecipe;
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use uuid::Uuid;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::EntityBase;
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::player::Player;
use crate::net::ClientPlatform;
use crate::net::java::PacketTap;
use crate::server::Server;
use crate::world::World;

const CRAFTER: Identity = Identity {
    name: "Crafter",
    uuid: Uuid::from_u128(0x0000_00fa),
    entity_id: 9_601,
};

/// Which recipe a packet names.
#[derive(Clone, Copy)]
enum Recipe {
    /// The first shaped or shapeless crafting recipe whose result is this item.
    Making(&'static str),
    /// The first transmute recipe.
    Transmute,
    /// The first cooking recipe (its display id comes after every crafting one).
    Cooking,
    /// A display id past every crafting and cooking recipe with no dynamic recipe behind it.
    Unknown,
}

#[derive(Clone, Copy)]
struct Scene {
    name: &'static str,
    /// The recipes clicked, in order, with the use-max flag.
    clicks: &'static [(Recipe, bool)],
    /// Stacks put in the inventory (main inventory slots in order).
    inventory: &'static [(&'static Item, u8)],
    /// Stacks put in the crafting grid first (grid slot, item, count).
    grid: &'static [(usize, &'static Item, u8)],
    /// A crafting table screen is open (3x3 grid) instead of the inventory's 2x2 grid.
    table: bool,
    /// The inventory's other slots are filled with stone first.
    full: bool,
}

const SCENE: Scene = Scene {
    name: "",
    clicks: &[],
    inventory: &[],
    grid: &[],
    table: false,
    full: false,
};

const PLANKS: Recipe = Recipe::Making("minecraft:oak_planks");
const CRAFTING_TABLE: Recipe = Recipe::Making("minecraft:crafting_table");
const CHEST: Recipe = Recipe::Making("minecraft:chest");
const STICK: Recipe = Recipe::Making("minecraft:stick");

const fn scene(name: &'static str, clicks: &'static [(Recipe, bool)]) -> Scene {
    Scene {
        name,
        clicks,
        ..SCENE
    }
}

fn scenes() -> Vec<Scene> {
    let mut scenes = grid_scenes();
    scenes.extend(table_scenes());
    scenes
}

fn grid_scenes() -> Vec<Scene> {
    vec![
        Scene {
            inventory: &[(&Item::OAK_LOG, 3)],
            ..scene("planks once", &[(PLANKS, false)])
        },
        Scene {
            inventory: &[(&Item::OAK_LOG, 3)],
            ..scene("planks twice", &[(PLANKS, false), (PLANKS, false)])
        },
        Scene {
            inventory: &[(&Item::OAK_LOG, 3)],
            ..scene(
                "planks four times",
                &[
                    (PLANKS, false),
                    (PLANKS, false),
                    (PLANKS, false),
                    (PLANKS, false),
                ],
            )
        },
        Scene {
            inventory: &[(&Item::OAK_LOG, 20), (&Item::OAK_LOG, 30)],
            ..scene("planks, use max", &[(PLANKS, true)])
        },
        Scene {
            inventory: &[(&Item::BIRCH_LOG, 3)],
            ..scene("planks without oak logs", &[(PLANKS, false)])
        },
        Scene {
            inventory: &[(&Item::BIRCH_LOG, 3)],
            ..scene("planks without oak logs, use max", &[(PLANKS, true)])
        },
        Scene {
            inventory: &[(&Item::OAK_PLANKS, 9)],
            ..scene("crafting table", &[(CRAFTING_TABLE, false)])
        },
        Scene {
            inventory: &[(&Item::OAK_PLANKS, 3)],
            ..scene(
                "crafting table with three planks",
                &[(CRAFTING_TABLE, false)],
            )
        },
        Scene {
            inventory: &[(&Item::OAK_PLANKS, 9)],
            ..scene("crafting table, use max", &[(CRAFTING_TABLE, true)])
        },
        Scene {
            inventory: &[(&Item::OAK_PLANKS, 9)],
            ..scene("stick", &[(STICK, false)])
        },
        Scene {
            inventory: &[(&Item::OAK_PLANKS, 16)],
            ..scene("chest in the 2x2 grid", &[(CHEST, false)])
        },
        Scene {
            inventory: &[(&Item::OAK_LOG, 3)],
            grid: &[(0, &Item::DIRT, 2), (3, &Item::STONE, 1)],
            ..scene("planks over other items in the grid", &[(PLANKS, false)])
        },
        Scene {
            inventory: &[(&Item::OAK_LOG, 3)],
            grid: &[(1, &Item::OAK_LOG, 2)],
            ..scene("planks with a log in another slot", &[(PLANKS, false)])
        },
        Scene {
            inventory: &[(&Item::OAK_LOG, 3)],
            grid: &[(0, &Item::DIRT, 2)],
            full: true,
            ..scene("planks over dirt, inventory full", &[(PLANKS, false)])
        },
        Scene {
            inventory: &[(&Item::OAK_PLANKS, 9)],
            ..scene("planks then stick", &[(PLANKS, false), (STICK, false)])
        },
        Scene {
            inventory: &[(&Item::OAK_LOG, 3)],
            ..scene("cooking recipe", &[(Recipe::Cooking, false)])
        },
        Scene {
            inventory: &[(&Item::OAK_LOG, 3)],
            ..scene("unknown recipe", &[(Recipe::Unknown, false)])
        },
        Scene {
            inventory: &[(&Item::WHITE_SHULKER_BOX, 1), (&Item::RED_DYE, 2)],
            ..scene("transmute", &[(Recipe::Transmute, false)])
        },
    ]
}

fn table_scenes() -> Vec<Scene> {
    vec![
        Scene {
            table: true,
            inventory: &[(&Item::OAK_PLANKS, 16)],
            ..scene("chest on a table", &[(CHEST, false)])
        },
        Scene {
            table: true,
            inventory: &[(&Item::OAK_PLANKS, 64)],
            ..scene("chest on a table, use max", &[(CHEST, true)])
        },
        Scene {
            table: true,
            inventory: &[(&Item::OAK_PLANKS, 20)],
            ..scene("chest on a table twice", &[(CHEST, false), (CHEST, false)])
        },
        Scene {
            table: true,
            inventory: &[(&Item::OAK_LOG, 3)],
            ..scene("planks on a table", &[(PLANKS, false)])
        },
        Scene {
            table: true,
            inventory: &[(&Item::OAK_PLANKS, 9)],
            ..scene("crafting table on a table", &[(CRAFTING_TABLE, false)])
        },
        Scene {
            table: true,
            inventory: &[(&Item::WHITE_SHULKER_BOX, 1), (&Item::RED_DYE, 2)],
            ..scene("transmute on a table", &[(Recipe::Transmute, false)])
        },
    ]
}

/// Whether the recipe book shows the recipe (display ids skip these).
const fn shown(recipe: &CraftingRecipeTypes) -> bool {
    !matches!(
        recipe,
        CraftingRecipeTypes::CraftingSpecial | CraftingRecipeTypes::CraftingDecoratedPot { .. }
    )
}

fn display_id(recipe: Recipe) -> i32 {
    let crafting = RECIPES_CRAFTING.iter().filter(|r| shown(r));
    let found = match recipe {
        Recipe::Making(result) => crafting.enumerate().find_map(|(index, r)| match r {
            CraftingRecipeTypes::CraftingShaped { result: made, .. }
            | CraftingRecipeTypes::CraftingShapeless { result: made, .. }
                if made.id == result =>
            {
                Some(index)
            }
            _ => None,
        }),
        Recipe::Transmute => crafting.enumerate().find_map(|(index, r)| {
            matches!(r, CraftingRecipeTypes::CraftingTransmute { .. }).then_some(index)
        }),
        Recipe::Cooking => Some(RECIPES_CRAFTING.iter().filter(|r| shown(r)).count()),
        Recipe::Unknown => {
            Some(RECIPES_CRAFTING.iter().filter(|r| shown(r)).count() + RECIPES_COOKING.len() + 500)
        }
    };
    found.map_or(-1, |index| index as i32)
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn packets(tap: &mut PacketTap) -> String {
    tap.drain()
        .iter()
        .map(|p| format!("{}:{:08x}", p.len(), fnv(p) as u32))
        .collect::<Vec<_>>()
        .join(" ")
}

fn stack_text(stack: &ItemStack) -> String {
    format!("{}x{}", stack.item.registry_key, stack.item_count)
}

/// The non-empty slots of `inventory` as `slot:item xcount`.
fn slots_text(inventory: &dyn Inventory) -> String {
    (0..inventory.size())
        .filter_map(|slot| {
            let stack = inventory.get_stack(slot);
            (!stack.is_empty()).then(|| format!("{slot}:{}", stack_text(&stack)))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The crafting grid of the player's open screen (slot 1 of a crafting screen is the first
/// grid slot).
fn grid_text(player: &Player) -> String {
    let handler = player
        .current_screen_handler
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let handler = handler
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let grid = handler.get_behaviour().slots[1].get_inventory();
    slots_text(grid.as_ref())
}

/// Items lying within eight blocks of `at`, sorted.
fn items_near(world: &World, at: Vector3<f64>) -> Vec<String> {
    let mut found: Vec<String> = world
        .entities
        .load()
        .iter()
        .filter_map(|entity| {
            let pos = entity.get_entity().pos.load();
            let item = entity.get_item_entity()?;
            ((pos.x - at.x).abs() < 8.0 && (pos.z - at.z).abs() < 8.0).then(|| {
                stack_text(
                    &item
                        .get_item_stack()
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner),
                )
            })
        })
        .collect();
    found.sort();
    found
}

/// Gives the crafter the scene's inventory, opens its table and fills its grid.
fn prepare(
    server: &Arc<Server>,
    world: &Arc<World>,
    crafter: &Arc<Player>,
    scene: &Scene,
    pos: BlockPos,
) {
    for (slot, (item, count)) in scene.inventory.iter().enumerate() {
        crafter
            .inventory
            .set_stack(slot, ItemStack::new(*count, item));
    }
    if scene.full {
        for slot in scene.inventory.len()..36 {
            crafter
                .inventory
                .set_stack(slot, ItemStack::new(64, &Item::STONE));
        }
    }
    if scene.table
        && let Some(factory) = server.block_registry.get_screen_handler_factory(
            &Block::CRAFTING_TABLE,
            crafter,
            &pos,
            server,
            world,
        )
    {
        crafter.open_handled_screen(factory.as_ref(), Some(pos));
    }
    let handler = crafter
        .current_screen_handler
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let grid = handler
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get_behaviour()
        .slots[1]
        .get_inventory();
    for (slot, item, count) in scene.grid {
        grid.set_stack(*slot, ItemStack::new(*count, item));
    }
}

fn run(
    server: &Arc<Server>,
    world: &Arc<World>,
    index: usize,
    scene: &Scene,
) -> Result<String, String> {
    let pos = BlockPos::new(index as i32 * 16 + 200, 64, 8);
    let at = Vector3::new(f64::from(pos.0.x) + 0.5, 64.0, f64::from(pos.0.z) + 2.5);
    let (crafter, mut tap) = test_support::java_player_as(world, GameMode::Survival, &CRAFTER)
        .ok_or("no Java packet tap")?;
    crafter.get_entity().set_pos(at);
    crafter.watched_section.store(Cylindrical::new(
        pos.chunk_position(),
        NonZero::<u8>::MIN.saturating_add(1),
    ));
    world.add_player(&crafter)?;
    prepare(server, world, &crafter, scene, pos);
    let ClientPlatform::Java(client) = crafter.client.as_ref() else {
        return Err("not a Java player".into());
    };
    drop(packets(&mut tap));
    let mut ids = Vec::new();
    for (recipe, use_max) in scene.clicks {
        let id = display_id(*recipe);
        ids.push(id);
        let packet = SPlaceRecipe {
            container_id: 0,
            recipe_display_id: VarInt(id),
            use_max_items: *use_max,
        };
        client.handle_place_recipe(server, &crafter, &packet);
    }
    Ok(format!(
        "{} {ids:?}: grid [{}] | inventory [{}] | dropped {:?} | sent [{}]\n",
        scene.name,
        grid_text(&crafter),
        slots_text(crafter.inventory.as_ref()),
        items_near(world, at),
        packets(&mut tap),
    ))
}

#[tokio::test(flavor = "multi_thread")]
async fn place_recipe_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let scenes = scenes();
    for index in 0..scenes.len() {
        let chunk = BlockPos::new(index as i32 * 16 + 200, 64, 8).chunk_position();
        for dx in -1..=1 {
            for dz in -1..=1 {
                let at = Vector2::new(chunk.x + dx, chunk.y + dz);
                world
                    .level
                    .loaded_chunks
                    .insert(at, pumpkin_world::chunk::ChunkData::empty_sync(at.x, at.y));
            }
        }
    }
    let mut out = String::new();
    for (index, scene) in scenes.iter().enumerate() {
        out.push_str(&run(&server, &world, index, scene)?);
        for player in world.players.load().iter() {
            world.remove_player(player, false).await;
        }
    }
    server.stop();
    script_golden::bless_or_compare("place_recipe.txt", &out)
}
