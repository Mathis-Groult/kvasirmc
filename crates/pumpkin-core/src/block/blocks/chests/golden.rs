//! A record of what `get_chest_screen_handler_factory` does, made before it was split
//! (`tests/golden/chest_screen_factory.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene lays a chest (type, facing, a partner of the other type on the connected side or
//! none, a solid roof over either half, deferred loot tables, items) in a loaded area of a real
//! server's world and asks for the screen factory as the scene's user (game mode). One line per
//! scene: whether a factory came back, the screen handler it makes (the chest slots, in order),
//! the contents and pending loot of both halves afterwards and the advancements whose
//! progress changed.

use std::fmt::Write as _;
use std::sync::Arc;

use pumpkin_data::block_properties::{ChestLikeProperties, ChestType, HorizontalFacing};
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use super::get_chest_screen_handler_factory;
use crate::block::GetScreenHandlerFactoryArgs;
use crate::block::entities::chest::ChestBlockEntity;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::player::Player;
use crate::server::Server;
use crate::world::World;
use pumpkin_inventory::screen_handler::ScreenHandlerFactory;

const USER: Identity = Identity {
    name: "Opener",
    uuid: Uuid::from_u128(0x0000_00b1),
    entity_id: 9_501,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Partner {
    None,
    /// A chest block of the other type with its block entity.
    Chest,
    /// The chest block without a block entity.
    BlockOnly,
}

#[derive(Clone)]
struct Scene {
    name: String,
    block: &'static Block,
    facing: HorizontalFacing,
    r#type: ChestType,
    partner: Partner,
    roof_first: bool,
    roof_second: bool,
    /// Pending loot table on the clicked half / the partner.
    loot_first: bool,
    loot_second: bool,
    filled: bool,
    gamemode: GameMode,
    /// The clicked half has no block entity.
    no_entity: bool,
}

fn scene(name: impl Into<String>, r#type: ChestType) -> Scene {
    Scene {
        name: name.into(),
        block: &Block::CHEST,
        facing: HorizontalFacing::North,
        r#type,
        partner: Partner::None,
        roof_first: false,
        roof_second: false,
        loot_first: false,
        loot_second: false,
        filled: true,
        gamemode: GameMode::Survival,
        no_entity: false,
    }
}

fn double(name: impl Into<String>, r#type: ChestType) -> Scene {
    Scene {
        partner: Partner::Chest,
        ..scene(name, r#type)
    }
}

fn scenes() -> Vec<Scene> {
    let mut scenes = single_scenes();
    double_scenes(&mut scenes);
    scenes
}

fn single_scenes() -> Vec<Scene> {
    vec![
        scene("single chest", ChestType::Single),
        Scene {
            filled: false,
            ..scene("single empty chest", ChestType::Single)
        },
        Scene {
            block: &Block::TRAPPED_CHEST,
            ..scene("single trapped chest", ChestType::Single)
        },
        Scene {
            roof_first: true,
            ..scene("single chest under stone", ChestType::Single)
        },
        Scene {
            no_entity: true,
            ..scene("single chest without block entity", ChestType::Single)
        },
        Scene {
            loot_first: true,
            ..scene("single chest with loot", ChestType::Single)
        },
        Scene {
            loot_first: true,
            filled: false,
            ..scene("single empty chest with loot", ChestType::Single)
        },
        Scene {
            loot_first: true,
            gamemode: GameMode::Spectator,
            ..scene("single chest with loot, spectator", ChestType::Single)
        },
        Scene {
            gamemode: GameMode::Spectator,
            ..scene("single chest, spectator", ChestType::Single)
        },
        Scene {
            loot_first: true,
            gamemode: GameMode::Creative,
            ..scene("single chest with loot, creative", ChestType::Single)
        },
        Scene {
            partner: Partner::Chest,
            ..scene("single chest beside a chest", ChestType::Single)
        },
    ]
}

fn double_scenes(scenes: &mut Vec<Scene>) {
    for (r#type, name) in [(ChestType::Left, "left"), (ChestType::Right, "right")] {
        let named = |suffix: &str| format!("{name} half of a double chest{suffix}");
        scenes.extend([
            double(named(""), r#type),
            Scene {
                roof_first: true,
                ..double(named(", roof over the clicked half"), r#type)
            },
            Scene {
                roof_second: true,
                ..double(named(", roof over the partner"), r#type)
            },
            Scene {
                loot_first: true,
                ..double(named(", loot in the clicked half"), r#type)
            },
            Scene {
                loot_second: true,
                ..double(named(", loot in the partner"), r#type)
            },
            Scene {
                loot_first: true,
                loot_second: true,
                ..double(named(", loot in both"), r#type)
            },
            Scene {
                loot_first: true,
                loot_second: true,
                gamemode: GameMode::Spectator,
                ..double(named(", loot in both, spectator"), r#type)
            },
            Scene {
                loot_second: true,
                gamemode: GameMode::Spectator,
                ..double(named(", loot in the partner, spectator"), r#type)
            },
            Scene {
                partner: Partner::None,
                ..double(named(", no partner"), r#type)
            },
            Scene {
                partner: Partner::BlockOnly,
                ..double(named(", partner without block entity"), r#type)
            },
            Scene {
                filled: false,
                ..double(named(", both empty"), r#type)
            },
            Scene {
                no_entity: true,
                ..double(named(", clicked half without block entity"), r#type)
            },
            Scene {
                facing: HorizontalFacing::South,
                ..double(named(" facing south"), r#type)
            },
            Scene {
                facing: HorizontalFacing::East,
                ..double(named(" facing east"), r#type)
            },
            Scene {
                facing: HorizontalFacing::West,
                ..double(named(" facing west"), r#type)
            },
        ]);
    }
}

/// Offset to the connected half: clockwise of the facing for a left half (vanilla
/// `ChestBlock.getConnectedDirection`).
const fn connected(facing: HorizontalFacing, r#type: ChestType) -> (i32, i32) {
    let clockwise = [(1, 0), (0, 1), (-1, 0), (0, -1)];
    let at = match facing {
        HorizontalFacing::North => 0,
        HorizontalFacing::East => 1,
        HorizontalFacing::South => 2,
        HorizontalFacing::West => 3,
    };
    match r#type {
        ChestType::Left => clockwise[at],
        _ => clockwise[(at + 2) % 4],
    }
}

fn chest_state(block: &Block, facing: HorizontalFacing, r#type: ChestType) -> BlockStateId {
    ChestLikeProperties {
        facing,
        r#type,
        waterlogged: false,
    }
    .to_state_id(block)
}

const fn position_of(index: usize) -> BlockPos {
    BlockPos::new(index as i32 * 8 + 200, 64, 8)
}

fn put(world: &Arc<World>, pos: &BlockPos, state: BlockStateId) {
    world.set_block_state(
        pos,
        state,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

fn entity(
    world: &Arc<World>,
    pos: BlockPos,
    loot: Option<(&str, i64)>,
    filled: bool,
    base: u8,
) -> Arc<ChestBlockEntity> {
    let mut chest = ChestBlockEntity::new(pos);
    if let Some((key, seed)) = loot {
        *chest
            .loot_table
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(key.to_string());
        chest.loot_table_seed = seed;
    }
    if filled {
        let mut items = chest
            .items
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        items[0] = ItemStack::new(base, &Item::DIAMOND);
        items[5] = ItemStack::new(base + 1, &Item::STICK);
        items[26] = ItemStack::new(2, &Item::BREAD);
    }
    let chest = Arc::new(chest);
    world.add_block_entity(chest.clone());
    chest
}

fn items_text(chest: &ChestBlockEntity) -> String {
    let items = chest
        .items
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut text = String::new();
    for (slot, stack) in items.iter().enumerate() {
        if !stack.is_empty() {
            let _ = write!(
                text,
                " {slot}:{}x{}",
                stack.item.registry_key, stack.item_count
            );
        }
    }
    let loot = chest
        .loot_table
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_some();
    format!(
        "[{}{}]",
        text.trim_start(),
        if loot { " +loot" } else { "" }
    )
}

/// What the factory gives: nothing, a factory the user cannot open, or the chest slots of its screen.
fn screen_text(factory: Option<Box<dyn ScreenHandlerFactory>>, user: &Arc<Player>) -> String {
    let mut out = String::new();
    match factory {
        None => out.push_str("no factory"),
        Some(factory) => match factory.create_screen_handler(1, user.inventory(), user.as_ref()) {
            None => out.push_str("factory, no screen"),
            Some(handler) => {
                let handler = handler
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let slots = &handler.get_behaviour().slots;
                let chest_slots = slots.len().saturating_sub(36);
                let _ = write!(out, "screen {chest_slots} slots [");
                let mut shown = Vec::new();
                for (slot, entry) in slots.iter().take(chest_slots).enumerate() {
                    let stack = entry.get_stack();
                    if !stack.is_empty() {
                        shown.push(format!(
                            "{slot}:{}x{}",
                            stack.item.registry_key, stack.item_count
                        ));
                    }
                }
                out.push_str(&shown.join(" "));
                out.push(']');
            }
        },
    }
    out
}

fn run(
    server: &Arc<Server>,
    world: &Arc<World>,
    index: usize,
    scene: &Scene,
) -> Result<String, String> {
    let pos = position_of(index);
    let (dx, dz) = connected(scene.facing, scene.r#type);
    let second_pos = BlockPos::new(pos.0.x + dx, pos.0.y, pos.0.z + dz);
    let opposite = match scene.r#type {
        ChestType::Left => ChestType::Right,
        _ => ChestType::Left,
    };
    put(
        world,
        &pos,
        chest_state(scene.block, scene.facing, scene.r#type),
    );
    let loot = |on: bool, seed: i64| on.then_some(("minecraft:chests/bastion_other", seed));
    let first =
        (!scene.no_entity).then(|| entity(world, pos, loot(scene.loot_first, 7), scene.filled, 3));
    let second = if scene.partner == Partner::None {
        None
    } else {
        put(
            world,
            &second_pos,
            chest_state(scene.block, scene.facing, opposite),
        );
        (scene.partner == Partner::Chest).then(|| {
            entity(
                world,
                second_pos,
                loot(scene.loot_second, 11),
                scene.filled,
                5,
            )
        })
    };
    if scene.roof_first {
        put(world, &pos.up(), Block::STONE.default_state.id);
    }
    if scene.roof_second {
        put(world, &second_pos.up(), Block::STONE.default_state.id);
    }
    let (user, _tap) =
        test_support::java_player_as(world, scene.gamemode, &USER).ok_or("no Java packet tap")?;
    user.advancements
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .player = Arc::downgrade(&user);
    world.add_player(&user)?;
    let factory = get_chest_screen_handler_factory(GetScreenHandlerFactoryArgs {
        server,
        world,
        block: scene.block,
        position: &pos,
        player: &user,
    });
    let mut line = format!("{}: {}", scene.name, screen_text(factory, &user));
    let text = |chest: &Option<Arc<ChestBlockEntity>>| {
        chest.as_ref().map_or("-".to_string(), |c| items_text(c))
    };
    let mut changed: Vec<String> = user
        .advancements
        .try_lock()
        .map(|advancements| {
            advancements
                .progress_changed
                .iter()
                .map(|advancement| advancement.id.to_string())
                .collect()
        })
        .unwrap_or_default();
    changed.sort();
    let _ = writeln!(
        line,
        " | first {} second {} | advancements {changed:?}",
        text(&first),
        text(&second)
    );
    Ok(line)
}

#[tokio::test(flavor = "multi_thread")]
async fn chest_screen_factory_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let scenes = scenes();
    for index in 0..scenes.len() {
        let chunk = position_of(index).chunk_position();
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
    script_golden::bless_or_compare("chest_screen_factory.txt", &out)
}
