//! A record of what `World::break_block` does, made before the function was split
//! (`tests/golden/break_block.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene places one block (stone, dirt, glass, a waterlogged slab, fire, air, a chest or a
//! shulker box holding items, an empty shulker box) in a loaded area of a real server's world,
//! far from spawn or inside / just outside spawn protection, and breaks it with no cause, a
//! survival, creative or operator player, with or without `SKIP_DROPS`. One line per scene: the
//! returned state, the block left, the items dropped in the area (name x count, sorted), and the
//! world event and system chat packets the breaking player and a second, watching player got.

use std::fmt::Write;
use std::num::NonZero;
use std::sync::Arc;

use pumpkin_data::block_properties::WhiteWoolSlabLikeProperties;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::packet::CURRENT_MC_VERSION;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_inventory::Inventory;
use pumpkin_protocol::java::client::play::{CSystemChatMessage, CWorldEvent};
use pumpkin_protocol::packet::MultiVersionJavaPacket;
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::permission::PermissionLvl;
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use pumpkin_world::world::BlockFlags;

use crate::block::entities::chest::ChestBlockEntity;
use crate::block::entities::shulker_box::ShulkerBoxBlockEntity;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::EntityBase;
use crate::entity::ai::test_support;
use crate::entity::player::Player;
use crate::net::java::PacketTap;
use crate::world::World;

#[derive(Clone, Copy, Debug)]
enum Kind {
    Stone,
    Dirt,
    Glass,
    WaterloggedSlab,
    Fire,
    Air,
    FullChest,
    FullShulker,
    EmptyShulker,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cause {
    Nobody,
    Survival,
    Creative,
    Operator,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    Far,
    Spawn,
    PastProtection,
}

struct Scene {
    kind: Kind,
    cause: Cause,
    skip_drops: bool,
    place: Place,
}

const fn far(kind: Kind, cause: Cause, skip_drops: bool) -> Scene {
    Scene {
        kind,
        cause,
        skip_drops,
        place: Place::Far,
    }
}

const fn at(mut scene: Scene, place: Place) -> Scene {
    scene.place = place;
    scene
}

const SCENES: &[Scene] = &[
    far(Kind::Stone, Cause::Nobody, false),
    far(Kind::Stone, Cause::Survival, false),
    far(Kind::Stone, Cause::Creative, false),
    far(Kind::Stone, Cause::Survival, true),
    far(Kind::Dirt, Cause::Survival, false),
    far(Kind::Glass, Cause::Survival, false),
    far(Kind::WaterloggedSlab, Cause::Survival, false),
    far(Kind::WaterloggedSlab, Cause::Nobody, true),
    far(Kind::Fire, Cause::Survival, false),
    far(Kind::Air, Cause::Survival, false),
    far(Kind::Air, Cause::Nobody, false),
    far(Kind::FullChest, Cause::Survival, false),
    far(Kind::FullChest, Cause::Creative, true),
    far(Kind::FullShulker, Cause::Survival, false),
    far(Kind::FullShulker, Cause::Creative, true),
    far(Kind::FullShulker, Cause::Survival, true),
    far(Kind::FullShulker, Cause::Nobody, true),
    far(Kind::EmptyShulker, Cause::Creative, true),
    at(far(Kind::Stone, Cause::Survival, false), Place::Spawn),
    at(far(Kind::Stone, Cause::Operator, false), Place::Spawn),
    at(far(Kind::Stone, Cause::Nobody, false), Place::Spawn),
    at(
        far(Kind::Stone, Cause::Survival, false),
        Place::PastProtection,
    ),
];

fn put(world: &Arc<World>, pos: &BlockPos, state: BlockStateId) {
    world.set_block_state(
        pos,
        state,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

fn lay(world: &Arc<World>, pos: BlockPos, kind: Kind) {
    let state = match kind {
        Kind::Stone => Block::STONE.default_state.id,
        Kind::Dirt => Block::DIRT.default_state.id,
        Kind::Glass => Block::GLASS.default_state.id,
        Kind::WaterloggedSlab => {
            let mut slab = WhiteWoolSlabLikeProperties::default(&Block::OAK_SLAB);
            slab.waterlogged = true;
            slab.to_state_id(&Block::OAK_SLAB)
        }
        Kind::Fire => Block::FIRE.default_state.id,
        Kind::Air => Block::AIR.default_state.id,
        Kind::FullChest => Block::CHEST.default_state.id,
        Kind::FullShulker | Kind::EmptyShulker => Block::SHULKER_BOX.default_state.id,
    };
    put(world, &pos.down(), Block::STONE.default_state.id);
    put(world, &pos, state);
    match kind {
        Kind::FullChest => {
            let chest = Arc::new(ChestBlockEntity::new(pos));
            chest.set_stack(0, ItemStack::new(5, &Item::APPLE));
            chest.set_stack(4, ItemStack::new(2, &Item::DIAMOND));
            world.add_block_entity(chest);
        }
        Kind::FullShulker | Kind::EmptyShulker => {
            let shulker = Arc::new(ShulkerBoxBlockEntity::new(pos));
            if matches!(kind, Kind::FullShulker) {
                shulker.set_stack(0, ItemStack::new(3, &Item::APPLE));
            }
            world.add_block_entity(shulker);
        }
        _ => {}
    }
}

fn var_int(bytes: &[u8], mut at: usize) -> Option<(i32, usize)> {
    let mut value = 0i32;
    for shift in (0..35).step_by(7) {
        let byte = *bytes.get(at)?;
        at += 1;
        value |= i32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some((value, at));
        }
    }
    None
}

/// World events as hex, system chat messages as a count.
fn packets(tap: &mut PacketTap) -> String {
    let event = CWorldEvent::to_id(CURRENT_MC_VERSION);
    let chat = CSystemChatMessage::to_id(CURRENT_MC_VERSION);
    let mut events = Vec::new();
    let mut chats = 0;
    for packet in tap.drain() {
        let first = var_int(&packet, 0);
        let second = first.and_then(|(_, next)| var_int(&packet, next));
        let ids: Vec<i32> = [first, second].iter().flatten().map(|(v, _)| *v).collect();
        if ids.contains(&event) {
            let mut hex = String::new();
            for byte in &packet[..] {
                let _ = write!(hex, "{byte:02x}");
            }
            events.push(hex);
        } else if ids.contains(&chat) {
            chats += 1;
        }
    }
    format!("events {events:?} chats {chats}")
}

fn drops(world: &World, pos: BlockPos) -> Vec<String> {
    let mut found: Vec<String> = world
        .entities
        .load()
        .iter()
        .filter_map(|entity| {
            let at = entity.get_entity().pos.load();
            // Drops land within a quarter block of the block's centre.
            let near = (at.x - f64::from(pos.0.x) - 0.5).abs() < 0.5
                && (at.z - f64::from(pos.0.z) - 0.5).abs() < 0.5;
            let item = entity.get_item_entity()?;
            if !near {
                return None;
            }
            let stack = item
                .get_item_stack()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            Some(format!("{}x{}", stack.item.registry_key, stack.item_count))
        })
        .collect();
    found.sort();
    found
}

struct Players {
    breaker: Arc<Player>,
    breaker_tap: PacketTap,
    watcher: Arc<Player>,
    watcher_tap: PacketTap,
}

fn watch(player: &Player, pos: BlockPos) {
    player.get_entity().set_pos(Vector3::new(
        f64::from(pos.0.x) + 2.5,
        f64::from(pos.0.y),
        f64::from(pos.0.z) + 2.5,
    ));
    player.watched_section.store(Cylindrical::new(
        pos.chunk_position(),
        NonZero::<u8>::MIN.saturating_add(1),
    ));
}

fn run(out: &mut String, world: &Arc<World>, players: &mut Players, index: usize, scene: &Scene) {
    let (spawn, _, _) = world.get_spawn_location();
    let radius = 16;
    let pos = match scene.place {
        Place::Far => BlockPos::new(index as i32 * 16 + 200, 64, 8),
        Place::Spawn => BlockPos::new(spawn.0.x + index as i32 % 5, 64, spawn.0.z),
        Place::PastProtection => BlockPos::new(spawn.0.x + radius + 1, 64, spawn.0.z),
    };
    lay(world, pos, scene.kind);
    let breaker = &players.breaker;
    watch(breaker, pos);
    watch(&players.watcher, pos);
    let mode = match scene.cause {
        Cause::Creative => GameMode::Creative,
        _ => GameMode::Survival,
    };
    breaker.gamemode.store(mode);
    let level = if scene.cause == Cause::Operator {
        PermissionLvl::Four
    } else {
        PermissionLvl::Zero
    };
    breaker.permission_lvl.store(level);
    drop(packets(&mut players.breaker_tap));
    drop(packets(&mut players.watcher_tap));
    let flags = if scene.skip_drops {
        BlockFlags::NOTIFY_ALL | BlockFlags::SKIP_DROPS
    } else {
        BlockFlags::NOTIFY_ALL
    };
    let cause = (scene.cause != Cause::Nobody).then_some(breaker);
    let returned = world.break_block(&pos, cause, flags);
    let returned = returned.map(|state| Block::from_state_id(state).name);
    let left = world.get_block(&pos).name;
    let _ = writeln!(
        out,
        "{:?} by {} skip drops {} at {}: returned {returned:?} left {left} drops {:?}; breaker {}; watcher {}",
        scene.kind,
        match scene.cause {
            Cause::Nobody => "nobody",
            Cause::Survival => "survival",
            Cause::Creative => "creative",
            Cause::Operator => "operator",
        },
        scene.skip_drops,
        match scene.place {
            Place::Far => "far",
            Place::Spawn => "spawn",
            Place::PastProtection => "past protection",
        },
        drops(world, pos),
        packets(&mut players.breaker_tap),
        packets(&mut players.watcher_tap),
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn break_block_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let (spawn, _, _) = world.get_spawn_location();
    let mut chunks = vec![spawn.chunk_position()];
    chunks.push(BlockPos::new(spawn.0.x + 17, 64, spawn.0.z).chunk_position());
    for index in 0..SCENES.len() as i32 {
        chunks.push(BlockPos::new(index * 16 + 200, 64, 8).chunk_position());
    }
    for chunk in &chunks {
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
    let (breaker, breaker_tap) =
        test_support::java_player_with_tap(&world, GameMode::Survival).ok_or("no packet tap")?;
    let (watcher, watcher_tap) =
        test_support::java_player_with_tap(&world, GameMode::Survival).ok_or("no packet tap")?;
    world.add_player(&breaker)?;
    world.add_player(&watcher)?;
    let mut players = Players {
        breaker,
        breaker_tap,
        watcher,
        watcher_tap,
    };
    let mut out = String::new();
    for (index, scene) in SCENES.iter().enumerate() {
        run(&mut out, &world, &mut players, index, scene);
    }
    world.remove_player(&players.breaker, false).await;
    world.remove_player(&players.watcher, false).await;
    server.stop();
    script_golden::bless_or_compare("break_block.txt", &out)
}
