//! A record of what `BrushItem::use_on_block` does, made before the function was split
//! (`tests/golden/brush_use_on_block.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene lays a block into a loaded area of a real server's world (offset 0,0,0), puts a
//! fresh user next to it and clicks it with a brush as many times as the scene says, with a
//! sculk sensor four blocks north that listens for the block changes (`heard N` = ticks until it
//! reacted). A plugin handler records every `BlockBrushEvent` and cancels the ones the scene
//! asks for. One line per click: the result, the block before and after, the stack, the user's
//! use state, the block entity, the item entities near the block, the event log, the user's
//! packets (length and id; the sound seed is random) and the non-air blocks around.
//! Scenes: every dusted stage of sand and gravel, blocks the brush does nothing to, creative
//! mode, a worn brush that breaks, other faces, a user who is not in the world, a cancelling
//! plugin, and every kind of loot (a stored item, an empty one, a loot table with and without
//! seed, an unknown table, no block entity). Random loot is recorded as "in the expected
//! table" so the file does not depend on the dice.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::num::NonZero;
use std::sync::{Arc, Mutex};

use pumpkin_data::block_properties::{
    SculkSensorLikeProperties, SculkSensorPhase, SuspiciousSandLikeProperties,
};
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::{Block, BlockDirection, BlockStateId};
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use super::BrushItem;
use crate::block::entities::brushable_block::BrushableBlockBlockEntity;
use crate::block::registry::BlockActionResult;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::EntityBase;
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::item::ItemEntity;
use crate::item::ItemBehaviour;
use crate::net::java::PacketTap;
use crate::plugin::api::events::block::block_brush::BlockBrushEvent;
use crate::plugin::api::events::{Cancellable, EventPriority};
use crate::plugin::{BoxFuture, EventHandler};
use crate::server::Server;
use crate::world::World;
use crate::world::loot::LootContextParameters;

const USER: Identity = Identity {
    name: "Brusher",
    uuid: Uuid::from_u128(0x0000_00b1),
    entity_id: 9_501,
};

/// What the block entity of a suspicious block holds.
#[derive(Clone, Copy)]
struct Stored {
    item: Option<(&'static Item, u8)>,
    table: Option<&'static str>,
    seed: i64,
}

struct Scene {
    name: String,
    gamemode: GameMode,
    worn: bool,
    face: BlockDirection,
    block: &'static Block,
    dusted: u8,
    clicks: usize,
    stored: Option<Stored>,
    cancel: bool,
    listed: bool,
    /// The loot table a random drop must come from (recorded as a yes/no, not the item).
    random_from: Option<&'static str>,
}

impl Scene {
    fn new(name: impl Into<String>, block: &'static Block, dusted: u8) -> Self {
        Self {
            name: name.into(),
            gamemode: GameMode::Survival,
            worn: false,
            face: BlockDirection::Up,
            block,
            dusted,
            clicks: 1,
            stored: None,
            cancel: false,
            listed: true,
            random_from: None,
        }
    }

    fn sand(name: impl Into<String>, dusted: u8) -> Self {
        Self::new(name, &Block::SUSPICIOUS_SAND, dusted)
    }

    fn gravel(name: impl Into<String>, dusted: u8) -> Self {
        Self::new(name, &Block::SUSPICIOUS_GRAVEL, dusted)
    }

    fn state(&self) -> BlockStateId {
        if self.block == &Block::SUSPICIOUS_SAND || self.block == &Block::SUSPICIOUS_GRAVEL {
            let mut props =
                SuspiciousSandLikeProperties::from_state_id(self.block.default_state.id);
            props.dusted = self.dusted;
            props.to_state_id(self.block)
        } else {
            self.block.default_state.id
        }
    }
}

const NONE: Stored = Stored {
    item: None,
    table: None,
    seed: 0,
};

fn stage_scenes() -> Vec<Scene> {
    let mut scenes = Vec::new();
    for dusted in 0..=2 {
        scenes.push(Scene::sand(format!("sand dusted {dusted}"), dusted));
        scenes.push(Scene::gravel(format!("gravel dusted {dusted}"), dusted));
    }
    scenes.push(Scene {
        clicks: 4,
        random_from: Some("minecraft:archaeology/desert_pyramid"),
        ..Scene::sand("sand walked through all stages", 0)
    });
    scenes.push(Scene {
        clicks: 4,
        random_from: Some("minecraft:archaeology/trail_ruins_common"),
        ..Scene::gravel("gravel walked through all stages", 0)
    });
    for (face, name) in [
        (BlockDirection::Down, "down"),
        (BlockDirection::North, "north"),
        (BlockDirection::West, "west"),
    ] {
        scenes.push(Scene {
            face,
            ..Scene::sand(format!("sand dusted 1, {name} face"), 1)
        });
    }
    scenes.push(Scene {
        gamemode: GameMode::Creative,
        ..Scene::gravel("creative gravel dusted 0", 0)
    });
    scenes.push(Scene {
        gamemode: GameMode::Adventure,
        ..Scene::sand("adventure sand dusted 2", 2)
    });
    scenes.push(Scene {
        worn: true,
        ..Scene::sand("worn brush, sand dusted 1", 1)
    });
    scenes.push(Scene {
        listed: false,
        ..Scene::sand("user not in the world, sand dusted 1", 1)
    });
    scenes
}

fn plain_scenes() -> Vec<Scene> {
    let mut scenes = Vec::new();
    for block in [
        &Block::STONE,
        &Block::SAND,
        &Block::GRAVEL,
        &Block::DIRT,
        &Block::AIR,
    ] {
        scenes.push(Scene::new(format!("brush on {}", block.name), block, 0));
    }
    scenes.push(Scene {
        gamemode: GameMode::Creative,
        face: BlockDirection::East,
        ..Scene::new("creative brush on stone, east face", &Block::STONE, 0)
    });
    scenes.push(Scene {
        clicks: 3,
        ..Scene::new("brush on stone three times", &Block::STONE, 0)
    });
    scenes.push(Scene {
        cancel: true,
        ..Scene::new("cancelling plugin, brush on stone", &Block::STONE, 0)
    });
    scenes
}

fn plugin_scenes() -> Vec<Scene> {
    vec![
        Scene {
            cancel: true,
            ..Scene::sand("cancelled, sand dusted 0", 0)
        },
        Scene {
            cancel: true,
            clicks: 2,
            ..Scene::gravel("cancelled, gravel dusted 2", 2)
        },
        Scene {
            cancel: true,
            stored: Some(Stored {
                item: Some((&Item::DIAMOND, 1)),
                ..NONE
            }),
            ..Scene::sand("cancelled, sand dusted 3 with a stored diamond", 3)
        },
        Scene {
            cancel: true,
            gamemode: GameMode::Creative,
            ..Scene::gravel("cancelled, creative gravel dusted 3", 3)
        },
    ]
}

fn loot_scenes() -> Vec<Scene> {
    let mut scenes = Vec::new();
    let desert = Some("minecraft:archaeology/desert_pyramid");
    let trail = Some("minecraft:archaeology/trail_ruins_common");
    // Without a block entity the default table of the block decides; random, so repeat it.
    for round in 1..=6 {
        scenes.push(Scene {
            random_from: desert,
            ..Scene::sand(format!("sand dusted 3, no block entity, round {round}"), 3)
        });
        scenes.push(Scene {
            random_from: trail,
            ..Scene::gravel(
                format!("gravel dusted 3, no block entity, round {round}"),
                3,
            )
        });
    }
    for (what, item) in [
        ("a stored diamond", Some((&Item::DIAMOND, 1))),
        ("a stored stack of 3 bricks", Some((&Item::BRICK, 3))),
    ] {
        scenes.push(Scene {
            stored: Some(Stored { item, ..NONE }),
            ..Scene::sand(format!("sand dusted 3 with {what}"), 3)
        });
        scenes.push(Scene {
            stored: Some(Stored { item, ..NONE }),
            ..Scene::gravel(format!("gravel dusted 3 with {what}"), 3)
        });
    }
    scenes.push(Scene {
        stored: Some(Stored {
            item: Some((&Item::DIAMOND, 1)),
            table: Some("minecraft:archaeology/desert_well"),
            seed: 5,
        }),
        ..Scene::sand("sand dusted 3 with a stored diamond and a loot table", 3)
    });
    scenes.push(Scene {
        stored: Some(Stored {
            item: Some((&Item::DIAMOND, 0)),
            table: Some("minecraft:archaeology/desert_well"),
            seed: 5,
        }),
        ..Scene::sand(
            "sand dusted 3 with an empty stored stack and a loot table",
            3,
        )
    });
    scenes.push(Scene {
        stored: Some(Stored {
            item: Some((&Item::DIAMOND, 0)),
            ..NONE
        }),
        random_from: desert,
        ..Scene::sand("sand dusted 3 with an empty stored stack only", 3)
    });
    scenes
}

fn table_scenes() -> Vec<Scene> {
    let mut scenes = Vec::new();
    for (block_name, sand) in [("sand", true), ("gravel", false)] {
        let make = |name: String, dusted: u8| {
            if sand {
                Scene::sand(name, dusted)
            } else {
                Scene::gravel(name, dusted)
            }
        };
        for (table, seed) in [
            ("minecraft:archaeology/desert_well", 5),
            ("minecraft:archaeology/desert_well", 6),
            ("minecraft:archaeology/trail_ruins_rare", 7),
            ("archaeology/trail_ruins_rare", 9),
        ] {
            scenes.push(Scene {
                stored: Some(Stored {
                    item: None,
                    table: Some(table),
                    seed,
                }),
                ..make(
                    format!("{block_name} dusted 3, table {table} seed {seed}"),
                    3,
                )
            });
        }
        scenes.push(Scene {
            stored: Some(Stored {
                item: None,
                table: Some("minecraft:archaeology/desert_well"),
                seed: 0,
            }),
            random_from: Some("minecraft:archaeology/desert_well"),
            ..make(
                format!("{block_name} dusted 3, table desert_well, no seed"),
                3,
            )
        });
        scenes.push(Scene {
            stored: Some(Stored {
                item: None,
                table: Some("minecraft:no_such_table"),
                seed: 3,
            }),
            random_from: Some(if sand {
                "minecraft:archaeology/desert_pyramid"
            } else {
                "minecraft:archaeology/trail_ruins_common"
            }),
            ..make(format!("{block_name} dusted 3, unknown table"), 3)
        });
        scenes.push(Scene {
            stored: Some(NONE),
            random_from: Some(if sand {
                "minecraft:archaeology/desert_pyramid"
            } else {
                "minecraft:archaeology/trail_ruins_common"
            }),
            ..make(format!("{block_name} dusted 3, empty block entity"), 3)
        });
    }
    scenes
}

fn wear_scenes() -> Vec<Scene> {
    vec![
        Scene {
            gamemode: GameMode::Creative,
            stored: Some(Stored {
                item: Some((&Item::EMERALD, 2)),
                ..NONE
            }),
            ..Scene::gravel("creative gravel dusted 3 with 2 emeralds", 3)
        },
        Scene {
            worn: true,
            stored: Some(Stored {
                item: Some((&Item::EMERALD, 2)),
                ..NONE
            }),
            ..Scene::sand("worn brush, sand dusted 3 with 2 emeralds", 3)
        },
        Scene {
            worn: true,
            gamemode: GameMode::Creative,
            stored: Some(Stored {
                item: Some((&Item::EMERALD, 2)),
                ..NONE
            }),
            ..Scene::sand("worn creative brush, sand dusted 3 with 2 emeralds", 3)
        },
        Scene {
            face: BlockDirection::South,
            stored: Some(Stored {
                item: Some((&Item::DIAMOND, 1)),
                ..NONE
            }),
            ..Scene::gravel("gravel dusted 3 with a diamond, south face", 3)
        },
        Scene {
            listed: false,
            stored: Some(Stored {
                item: Some((&Item::DIAMOND, 1)),
                ..NONE
            }),
            ..Scene::sand("user not in the world, sand dusted 3 with a diamond", 3)
        },
    ]
}

fn scenes() -> Vec<Scene> {
    let mut all = stage_scenes();
    all.extend(plain_scenes());
    all.extend(plugin_scenes());
    all.extend(loot_scenes());
    all.extend(table_scenes());
    all.extend(wear_scenes());
    all
}

const SPACING: i32 = 8;

const fn position_of(index: usize) -> BlockPos {
    BlockPos::new(index as i32 * SPACING + 200, 64, 8)
}

/// Remembers every `BlockBrushEvent` and cancels the ones at the position it is told to.
#[derive(Default)]
struct Spy {
    log: Mutex<Vec<String>>,
    cancel_at: Mutex<Option<BlockPos>>,
}

impl EventHandler<BlockBrushEvent> for Spy {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut BlockBrushEvent,
    ) -> BoxFuture<'a, ()> {
        let cancel = *self
            .cancel_at
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            == Some(event.block_pos);
        self.log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(format!(
                "event {},{},{} {} by {} cancel {cancel}",
                event.block_pos.0.x,
                event.block_pos.0.y,
                event.block_pos.0.z,
                stack_text(&event.item),
                event.player.gameprofile.name,
            ));
        if cancel {
            event.set_cancelled(true);
        }
        Box::pin(async {})
    }
}

fn put(world: &Arc<World>, pos: &BlockPos, state: BlockStateId) {
    world.set_block_state(
        pos,
        state,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

const fn result_text(result: &BlockActionResult) -> &'static str {
    match result {
        BlockActionResult::Success => "Success",
        BlockActionResult::SuccessServer => "SuccessServer",
        BlockActionResult::Consume => "Consume",
        BlockActionResult::Fail => "Fail",
        BlockActionResult::Pass => "Pass",
        BlockActionResult::PassToDefaultBlockAction => "PassToDefaultBlockAction",
    }
}

fn stack_text(stack: &ItemStack) -> String {
    if stack.is_empty() {
        return "-".to_string();
    }
    format!(
        "{}x{} dmg {}",
        stack.item.registry_key,
        stack.item_count,
        stack.get_damage()
    )
}

/// Packet ids whose bytes are random like in vanilla: the sound packet (seed).
const RANDOM_PACKETS: [u8; 1] = [119];
/// The sound's seed is its last 8 bytes; the rest (sound id, position, volume) is hashed.
const SEED_BYTES: usize = 8;

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn packets(tap: &mut PacketTap) -> String {
    tap.drain()
        .iter()
        .map(|p| match p.first() {
            Some(id) if RANDOM_PACKETS.contains(id) => {
                format!(
                    "{}:id{id}:{:08x}",
                    p.len(),
                    fnv(&p[..p.len() - SEED_BYTES]) as u32
                )
            }
            _ => format!("{}:{:08x}", p.len(), fnv(p) as u32),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Ticks the sensor's block entity until it is no longer inactive (at most 40 ticks).
fn heard(world: &Arc<World>, sensor: BlockPos) -> String {
    for tick in 1..=40 {
        world
            .level_time
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .world_age += 1;
        if let Some(entity) = world.get_block_entity(&sensor) {
            entity.tick(world);
        }
        let id = world.get_block_state(&sensor).id;
        if SculkSensorLikeProperties::from_state_id(id).sculk_sensor_phase
            != SculkSensorPhase::Inactive
        {
            return format!("heard {tick}");
        }
    }
    "deaf".to_string()
}

fn region_text(world: &World, pos: BlockPos) -> String {
    let mut text = String::new();
    for dx in -2..=2 {
        for dy in -1..=2 {
            for dz in -2..=2 {
                let at = BlockPos::new(pos.0.x + dx, pos.0.y + dy, pos.0.z + dz);
                let block = world.get_block(&at);
                if block != &Block::AIR {
                    let _ = write!(
                        text,
                        " {dx},{dy},{dz}:{}#{}",
                        block.name,
                        world.get_block_state_id(&at).as_u16()
                    );
                }
            }
        }
    }
    text
}

/// What the block entity laid at `pos` holds now (the scene keeps a handle, so it can be read
/// after the block was replaced), and whether the world still has one there.
fn entity_text(world: &World, pos: BlockPos, held: Option<&BrushableBlockBlockEntity>) -> String {
    let in_world = world.get_block_entity(&pos).is_some();
    let Some(brushable) = held else {
        return format!("none (world {in_world})");
    };
    let item = brushable
        .item
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .map_or_else(|| "none".to_string(), stack_text);
    let table = brushable
        .loot_table
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let seed = *brushable
        .loot_table_seed
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    format!("item {item} table {table:?} seed {seed} (world {in_world})")
}

/// Every item the table can give, found by rolling it with many seeds.
fn possible_items(world: &World, key: &str) -> BTreeSet<&'static str> {
    let mut found = BTreeSet::new();
    let Some(table) = world.get_loot_table(key) else {
        return found;
    };
    for seed in 0..400i64 {
        let params = LootContextParameters {
            world: Some(world),
            ..Default::default()
        };
        for stack in table.generate_loot_with_context(seed, &params) {
            found.insert(stack.item.registry_key);
        }
    }
    found
}

/// The item entities near `pos`: item, count and where they are (relative to the block).
fn drops_text(world: &Arc<World>, pos: BlockPos, random_from: Option<&str>) -> String {
    let mut text = String::new();
    for entity in world.entities.load().iter() {
        let Some(item) = entity.cast_any().downcast_ref::<ItemEntity>() else {
            continue;
        };
        let at = entity.get_entity().pos.load();
        let (dx, dy, dz) = (
            at.x - f64::from(pos.0.x),
            at.y - f64::from(pos.0.y),
            at.z - f64::from(pos.0.z),
        );
        if dx.abs() > 4.0 || dz.abs() > 4.0 || dy.abs() > 4.0 {
            continue;
        }
        let stack = item
            .get_item_stack()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let what = match random_from {
            Some(table) => format!(
                "x{} from {table}: {}",
                stack.item_count,
                possible_items(world, table).contains(stack.item.registry_key)
            ),
            None => stack_text(&stack),
        };
        let _ = write!(text, " [{what} at {dx},{dy},{dz}]");
    }
    if text.is_empty() {
        "none".to_string()
    } else {
        text.trim_start().to_string()
    }
}

fn lay(world: &Arc<World>, pos: BlockPos, scene: &Scene) -> Option<Arc<BrushableBlockBlockEntity>> {
    put(world, &pos, scene.state());
    let stored = scene.stored?;
    let entity = Arc::new(BrushableBlockBlockEntity {
        position: pos,
        item: Mutex::new(stored.item.map(|(item, count)| ItemStack::new(count, item))),
        hits: Mutex::new(0),
        hit_direction: Mutex::new(None),
        loot_table: Mutex::new(stored.table.map(ToString::to_string)),
        loot_table_seed: Mutex::new(stored.seed),
    });
    world.add_block_entity(entity.clone());
    Some(entity)
}

fn use_text(user: &crate::entity::player::Player) -> String {
    let living = &user.living_entity;
    let in_use = living
        .item_in_use
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .map_or_else(|| "-".to_string(), stack_text);
    let hand = *living
        .active_hand
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    format!(
        "{} {hand:?} {in_use}",
        living
            .item_use_time
            .load(std::sync::atomic::Ordering::Relaxed)
    )
}

fn run(
    server: &Arc<Server>,
    world: &Arc<World>,
    spy: &Spy,
    index: usize,
    scene: &Scene,
) -> Result<String, String> {
    let pos = position_of(index);
    let held = lay(world, pos, scene);
    let sensor = BlockPos::new(pos.0.x, pos.0.y, pos.0.z - 4);
    world.set_block_state(
        &sensor,
        Block::SCULK_SENSOR.default_state.id,
        BlockFlags::NOTIFY_ALL,
    );
    let (user, mut tap) =
        test_support::java_player_as(world, scene.gamemode, &USER).ok_or("no Java packet tap")?;
    user.get_entity().set_pos(Vector3::new(
        f64::from(pos.0.x) + 0.5,
        f64::from(pos.0.y),
        f64::from(pos.0.z) + 2.5,
    ));
    user.watched_section.store(Cylindrical::new(
        pos.chunk_position(),
        NonZero::<u8>::MIN.saturating_add(1),
    ));
    if scene.listed {
        world.add_player(&user)?;
    }
    drop(packets(&mut tap));
    *spy.cancel_at
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = scene.cancel.then_some(pos);
    let mut stack = ItemStack::new(1, &Item::BRUSH);
    if scene.worn
        && let Some(max) = stack.get_max_damage()
    {
        stack.set_damage(max - 1);
    }
    let mut out = String::new();
    for click in 1..=scene.clicks {
        let block = world.get_block(&pos);
        let before = world.get_block_state_id(&pos).as_u16();
        let result = BrushItem.use_on_block(
            &mut stack,
            &user,
            pos,
            scene.face,
            Vector3::new(0.5, 0.5, 0.5),
            block,
            server,
        );
        let sent = packets(&mut tap);
        let events = std::mem::take(
            &mut *spy
                .log
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        let _ = writeln!(
            out,
            "{} #{click}: {} | {}#{before} -> {}#{} | stack {} | use {} | entity {} | drops {} | {} | {events:?} | sent [{sent}] |{}",
            scene.name,
            result_text(&result),
            block.name,
            world.get_block(&pos).name,
            world.get_block_state_id(&pos).as_u16(),
            stack_text(&stack),
            use_text(&user),
            entity_text(world, pos, held.as_deref()),
            drops_text(world, pos, scene.random_from),
            heard(world, sensor),
            region_text(world, pos),
        );
    }
    Ok(out)
}

#[tokio::test(flavor = "multi_thread")]
async fn brush_use_on_block_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let spy = Arc::new(Spy::default());
    server.plugin_manager.register::<BlockBrushEvent, _>(
        Arc::clone(&spy),
        EventPriority::Normal,
        true,
    );
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
        out.push_str(&run(&server, &world, &spy, index, scene)?);
        for player in world.players.load().iter() {
            world.remove_player(player, false).await;
        }
    }
    server.stop();
    script_golden::bless_or_compare("brush_use_on_block.txt", &out)
}
