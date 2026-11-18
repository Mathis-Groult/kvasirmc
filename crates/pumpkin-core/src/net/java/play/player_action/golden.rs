//! A record of what `JavaClient::handle_player_action` does, made before the function was split
//! (`tests/golden/player_action.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene lays one block in a loaded area of a real server's world, puts a fresh breaker
//! (fixed identity, two blocks away, the scene's game mode and items) and a fresh watcher next to
//! it, then hands the breaker's client the scene's player-action packets in order: start, cancel
//! or finish digging (in reach or not), drop one item or the stack, release the item in use,
//! swap hands, spear jab, change destroy direction, an unknown status. One line per scene: the
//! block left, the items lying around, the breaker's mining state, packet sequence, held and
//! offhand items, exhaustion, `Used`/`Mined` statistics and item in use, then the packets of the
//! breaker and the watcher (length and FNV-1a of the bytes).
//! Not covered: plugins cancelling the interact, damage or abort events (none is loaded).

use std::fmt::Write;
use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use pumpkin_data::block_properties::TntLikeProperties;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::{Block, BlockStateId, Enchantment};
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::java::server::play::SPlayerAction;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::{GameMode, Hand};
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::EntityBase;
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::player::Player;
use crate::entity::player::statistics::StatisticCategory;
use crate::net::ClientPlatform;
use crate::net::java::PacketTap;
use crate::server::Server;
use crate::world::World;

const BREAKER: Identity = Identity {
    name: "Breaker",
    uuid: Uuid::from_u128(0x0000_00e1),
    entity_id: 9_201,
};
const WATCHER: Identity = Identity {
    name: "Watcher",
    uuid: Uuid::from_u128(0x0000_00e2),
    entity_id: 9_202,
};

const STARTED: i32 = 0;
const CANCELLED: i32 = 1;
const FINISHED: i32 = 2;
const DROP_STACK: i32 = 3;
const DROP_ONE: i32 = 4;
const RELEASE: i32 = 5;
const SWAP: i32 = 6;
const SPEAR_JAB: i32 = 7;
const CHANGE_DIRECTION: i32 = 8;
const UNKNOWN: i32 = 42;

#[derive(Clone, Copy)]
enum Kind {
    Stone,
    Air,
    Torch,
    Dirt,
    RedstoneOre,
    UnstableTnt,
    Ice,
}

#[derive(Clone, Copy)]
struct Scene {
    name: &'static str,
    kind: Kind,
    gamemode: GameMode,
    held: Option<(&'static Item, u8)>,
    offhand: Option<(&'static Item, u8)>,
    /// The item in use (`ReleaseItemInUse`).
    using: Option<&'static Item>,
    /// The client has not sent `PlayerLoaded`.
    unloaded: bool,
    /// The breaker stands eight blocks away (out of reach).
    far: bool,
    actions: &'static [i32],
    /// Appended: the held item has Efficiency V.
    efficient: bool,
    /// Appended: the breaker stands just inside reach only with the 1.0 margin.
    margin: bool,
    /// Appended: the bow in use was drawn for 25 ticks and arrows are in the inventory.
    drawn: bool,
    /// Appended: the watcher stands two blocks in front of the breaker, whose last attack was
    /// long ago.
    jab_target: bool,
    /// Appended scenes end their line with the state id, whether the action counted as player
    /// activity, the entities nearby and the watcher's health.
    extra: bool,
}

const SCENE: Scene = Scene {
    name: "",
    kind: Kind::Stone,
    gamemode: GameMode::Survival,
    held: None,
    offhand: None,
    using: None,
    unloaded: false,
    far: false,
    actions: &[STARTED],
    efficient: false,
    margin: false,
    drawn: false,
    jab_target: false,
    extra: false,
};

const fn scene(name: &'static str, actions: &'static [i32]) -> Scene {
    Scene {
        name,
        actions,
        ..SCENE
    }
}

fn scenes() -> Vec<Scene> {
    let mut scenes = dig_scenes();
    scenes.extend(other_scenes());
    scenes.extend(widened_scenes());
    scenes
}

const PICKAXE: Option<(&Item, u8)> = Some((&Item::IRON_PICKAXE, 1));

fn dig_scenes() -> Vec<Scene> {
    let pickaxe = PICKAXE;
    vec![
        Scene {
            unloaded: true,
            ..scene("client not loaded, start", &[STARTED])
        },
        Scene {
            far: true,
            ..scene("start out of reach", &[STARTED])
        },
        Scene {
            gamemode: GameMode::Spectator,
            ..scene("start as spectator", &[STARTED])
        },
        Scene {
            gamemode: GameMode::Creative,
            ..scene("start in creative, empty hand", &[STARTED])
        },
        Scene {
            gamemode: GameMode::Creative,
            held: Some((&Item::DIAMOND_SWORD, 1)),
            ..scene("start in creative with a sword", &[STARTED])
        },
        Scene {
            gamemode: GameMode::Adventure,
            ..scene("start in adventure", &[STARTED])
        },
        Scene {
            kind: Kind::Air,
            ..scene("start on air", &[STARTED])
        },
        Scene {
            kind: Kind::Torch,
            ..scene("start on a torch (instant)", &[STARTED])
        },
        Scene {
            kind: Kind::Torch,
            held: pickaxe,
            ..scene("start on a torch holding a pickaxe", &[STARTED])
        },
        scene("start on stone by hand", &[STARTED]),
        Scene {
            held: pickaxe,
            ..scene("start on stone with a pickaxe", &[STARTED])
        },
        scene("start then cancel", &[STARTED, CANCELLED]),
        Scene {
            far: true,
            ..scene("cancel out of reach", &[CANCELLED])
        },
        Scene {
            held: pickaxe,
            ..scene("start then finish with a pickaxe", &[STARTED, FINISHED])
        },
        scene("start then finish by hand", &[STARTED, FINISHED]),
        Scene {
            gamemode: GameMode::Creative,
            ..scene("finish in creative", &[FINISHED])
        },
        Scene {
            far: true,
            held: pickaxe,
            ..scene("finish out of reach", &[FINISHED])
        },
        Scene {
            kind: Kind::Air,
            ..scene("finish on air", &[FINISHED])
        },
    ]
}

fn other_scenes() -> Vec<Scene> {
    let pickaxe = PICKAXE;
    vec![
        Scene {
            held: Some((&Item::APPLE, 5)),
            ..scene("drop one", &[DROP_ONE])
        },
        Scene {
            held: Some((&Item::APPLE, 5)),
            ..scene("drop the stack", &[DROP_STACK])
        },
        Scene {
            held: Some((&Item::BOW, 1)),
            using: Some(&Item::BOW),
            ..scene("release a bow", &[RELEASE])
        },
        scene("release with nothing in use", &[RELEASE]),
        Scene {
            held: Some((&Item::APPLE, 2)),
            offhand: Some((&Item::BREAD, 3)),
            ..scene("swap hands", &[SWAP])
        },
        Scene {
            gamemode: GameMode::Spectator,
            held: Some((&Item::COPPER_SPEAR, 1)),
            ..scene("spear jab as spectator", &[SPEAR_JAB])
        },
        Scene {
            held: Some((&Item::COPPER_SPEAR, 1)),
            ..scene("spear jab", &[SPEAR_JAB])
        },
        scene("change destroy direction", &[CHANGE_DIRECTION]),
        scene("unknown status", &[UNKNOWN]),
        Scene {
            held: pickaxe,
            ..scene(
                "start, cancel, start, finish",
                &[STARTED, CANCELLED, STARTED, FINISHED],
            )
        },
    ]
}

const WIDE: Scene = Scene {
    extra: true,
    ..SCENE
};

const fn wide(name: &'static str, actions: &'static [i32]) -> Scene {
    Scene {
        name,
        actions,
        ..WIDE
    }
}

fn widened_scenes() -> Vec<Scene> {
    vec![
        Scene {
            held: Some((&Item::IRON_SWORD, 1)),
            ..wide("start on stone with a sword in survival", &[STARTED])
        },
        Scene {
            gamemode: GameMode::Creative,
            held: PICKAXE,
            ..wide("start in creative with a pickaxe", &[STARTED])
        },
        Scene {
            gamemode: GameMode::Creative,
            held: Some((&Item::COPPER_SPEAR, 1)),
            ..wide("start in creative with a spear", &[STARTED])
        },
        Scene {
            held: Some((&Item::DEBUG_STICK, 1)),
            ..wide("start with a debug stick in survival", &[STARTED])
        },
        Scene {
            kind: Kind::RedstoneOre,
            held: PICKAXE,
            ..wide("start on redstone ore with a pickaxe", &[STARTED])
        },
        Scene {
            kind: Kind::Dirt,
            held: Some((&Item::IRON_SHOVEL, 1)),
            ..wide("start on dirt with a shovel", &[STARTED])
        },
        Scene {
            kind: Kind::Dirt,
            held: Some((&Item::IRON_SHOVEL, 1)),
            efficient: true,
            ..wide(
                "start on dirt with an efficiency V shovel (instant)",
                &[STARTED],
            )
        },
        Scene {
            kind: Kind::UnstableTnt,
            ..wide("start on unstable tnt (instant)", &[STARTED])
        },
        Scene {
            margin: true,
            ..wide("start just inside the reach margin", &[STARTED])
        },
        Scene {
            margin: true,
            ..wide(
                "start then cancel just inside the reach margin",
                &[STARTED, CANCELLED],
            )
        },
        Scene {
            margin: true,
            held: PICKAXE,
            ..wide("finish just inside the reach margin", &[FINISHED])
        },
        Scene {
            gamemode: GameMode::Creative,
            held: PICKAXE,
            ..wide("finish in creative with a pickaxe", &[FINISHED])
        },
        Scene {
            kind: Kind::Ice,
            held: PICKAXE,
            ..wide("finish on ice with a pickaxe", &[FINISHED])
        },
        Scene {
            held: Some((&Item::BOW, 1)),
            using: Some(&Item::BOW),
            drawn: true,
            ..wide("release a drawn bow with arrows", &[RELEASE])
        },
        Scene {
            held: Some((&Item::COPPER_SPEAR, 1)),
            jab_target: true,
            ..wide("spear jab at the watcher", &[SPEAR_JAB])
        },
        Scene {
            gamemode: GameMode::Spectator,
            held: Some((&Item::COPPER_SPEAR, 1)),
            jab_target: true,
            ..wide("spear jab at the watcher as spectator", &[SPEAR_JAB])
        },
        wide("change destroy direction, activity", &[CHANGE_DIRECTION]),
    ]
}

const fn position_of(index: usize) -> BlockPos {
    BlockPos::new(index as i32 * 16 + 200, 64, 8)
}

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
        Kind::Air => Block::AIR.default_state.id,
        Kind::Torch => Block::TORCH.default_state.id,
        Kind::Dirt => Block::DIRT.default_state.id,
        Kind::RedstoneOre => Block::REDSTONE_ORE.default_state.id,
        Kind::UnstableTnt => TntLikeProperties { unstable: true }.to_state_id(&Block::TNT),
        Kind::Ice => Block::ICE.default_state.id,
    };
    for x in -3..=9 {
        for z in -3..=3 {
            put(
                world,
                &BlockPos::new(pos.0.x + x, 63, pos.0.z + z),
                Block::STONE.default_state.id,
            );
        }
    }
    put(world, &pos, state);
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

/// Packet ids whose bytes are random like in vanilla: Add Entity (UUID, TNT and arrow
/// velocity) and the two sound packets (seed).
const RANDOM_PACKETS: [u8; 3] = [1, 118, 119];

/// Like [`packets`], for appended scenes: random packets show their id instead of a hash.
fn packets_masked(tap: &mut PacketTap) -> String {
    tap.drain()
        .iter()
        .map(|p| match p.first() {
            Some(id) if RANDOM_PACKETS.contains(id) => format!("{}:id{id}", p.len()),
            _ => format!("{}:{:08x}", p.len(), fnv(p) as u32),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn tap_text(tap: &mut PacketTap, masked: bool) -> String {
    if masked {
        packets_masked(tap)
    } else {
        packets(tap)
    }
}

/// Items (name x count, sorted) lying within six blocks of `pos`.
fn items_near(world: &World, pos: BlockPos) -> Vec<String> {
    let mut found: Vec<String> = world
        .entities
        .load()
        .iter()
        .filter_map(|entity| {
            let at = entity.get_entity().pos.load();
            let near =
                (at.x - f64::from(pos.0.x)).abs() < 6.0 && (at.z - f64::from(pos.0.z)).abs() < 6.0;
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

/// Non-player entities (type, sorted) within six blocks of `pos`.
fn entities_near(world: &World, pos: BlockPos) -> Vec<&'static str> {
    let mut found: Vec<&'static str> = world
        .entities
        .load()
        .iter()
        .filter(|entity| {
            let at = entity.get_entity().pos.load();
            (at.x - f64::from(pos.0.x)).abs() < 6.0 && (at.z - f64::from(pos.0.z)).abs() < 6.0
        })
        .map(|entity| entity.get_entity().entity_type.resource_name)
        .collect();
    found.sort_unstable();
    found
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

fn java(player: &Player) -> Option<&crate::net::java::JavaClient> {
    match player.client.as_ref() {
        ClientPlatform::Java(client) => Some(client),
        ClientPlatform::Bedrock(_) => None,
    }
}

fn breaker_text(player: &Player, block: &Block) -> String {
    let inventory = player.inventory();
    let item = inventory.held_item();
    let stats = player
        .stats
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let in_use = player
        .living_entity
        .item_in_use
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .map(stack_text);
    format!(
        "mining {} at {:?} stage {} speed {:08x} start {} sequence {} held {} offhand {} exhaustion {} used {} mined {} in use {in_use:?}",
        player.mining.load(Ordering::Relaxed),
        *player
            .mining_pos
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        player.current_block_destroy_stage.load(Ordering::Relaxed),
        player.current_block_breaking_speed.load(Ordering::Relaxed),
        player.start_mining_time.load(Ordering::Relaxed),
        java(player).map_or(-2, |client| client.packet_sequence.load(Ordering::Relaxed)),
        stack_text(&item),
        stack_text(&inventory.off_hand_item()),
        player.hunger_manager.exhaustion.load(),
        stats.get(StatisticCategory::Used, item.item.id as i32),
        stats.get(StatisticCategory::Mined, block.id.as_u16() as i32),
    )
}

fn place(player: &Player, at: Vector3<f64>, pos: BlockPos) {
    player.get_entity().set_pos(at);
    player.watched_section.store(Cylindrical::new(
        pos.chunk_position(),
        NonZero::<u8>::MIN.saturating_add(1),
    ));
}

fn run(
    server: &Arc<Server>,
    world: &Arc<World>,
    index: usize,
    scene: &Scene,
) -> Result<String, String> {
    let pos = position_of(index);
    lay(world, pos, scene.kind);
    let block = Block::from_state_id(world.get_block_state_id(&pos));
    let (breaker, mut breaker_tap) = test_support::java_player_as(world, scene.gamemode, &BREAKER)
        .ok_or("no Java packet tap")?;
    let (watcher, mut watcher_tap) =
        test_support::java_player_as(world, GameMode::Survival, &WATCHER)
            .ok_or("no Java packet tap")?;
    let away = if scene.far {
        8.0
    } else if scene.margin {
        5.5
    } else {
        2.0
    };
    place(
        &breaker,
        Vector3::new(
            f64::from(pos.0.x) + 0.5,
            64.0,
            f64::from(pos.0.z) + 0.5 + away,
        ),
        pos,
    );
    let watcher_at = if scene.jab_target {
        Vector3::new(f64::from(pos.0.x) + 0.5, 64.0, f64::from(pos.0.z) + 4.5)
    } else {
        Vector3::new(f64::from(pos.0.x) + 3.5, 64.0, f64::from(pos.0.z) + 0.5)
    };
    place(&watcher, watcher_at, pos);
    breaker
        .client_loaded
        .store(!scene.unloaded, Ordering::Relaxed);
    let inventory = breaker.inventory();
    if let Some((item, count)) = scene.held {
        let mut stack = ItemStack::new(count, item);
        if scene.efficient {
            stack.add_enchantment(&Enchantment::EFFICIENCY, 5);
        }
        inventory.set_stack_in_hand(Hand::Right, stack);
    }
    if let Some((item, count)) = scene.offhand {
        inventory.set_stack_in_hand(Hand::Left, ItemStack::new(count, item));
    }
    if let Some(item) = scene.using {
        breaker
            .living_entity
            .set_active_hand(Hand::Right, ItemStack::new(1, item), 72_000);
    }
    if scene.drawn {
        inventory.insert_stack_anywhere(&mut ItemStack::new(16, &Item::ARROW));
        breaker
            .living_entity
            .item_use_time
            .store(72_000 - 25, Ordering::Relaxed);
    }
    if scene.jab_target {
        breaker.last_attacked_ticks.store(100, Ordering::Relaxed);
    }
    if scene.extra {
        breaker.tick_counter.store(37, Ordering::Relaxed);
        if let Some(long_ago) = Instant::now().checked_sub(Duration::from_secs(600)) {
            breaker.last_action_time.store(long_ago);
        }
    }
    world.add_player(&breaker)?;
    world.add_player(&watcher)?;
    let client = java(&breaker).ok_or("the breaker is not a Java player")?;
    drop(packets(&mut breaker_tap));
    drop(packets(&mut watcher_tap));
    for (sequence, status) in scene.actions.iter().enumerate() {
        let packet = SPlayerAction {
            status: VarInt(*status),
            position: pos,
            face: 1,
            sequence: VarInt(sequence as i32 + 3),
        };
        client.handle_player_action(&breaker, &packet, server);
    }
    let mut line = format!(
        "{}: left {} items {:?} {} | breaker [{}] | watcher [{}]",
        scene.name,
        world.get_block(&pos).name,
        items_near(world, pos),
        breaker_text(&breaker, block),
        tap_text(&mut breaker_tap, scene.extra),
        tap_text(&mut watcher_tap, scene.extra),
    );
    if scene.extra {
        let _ = write!(
            line,
            " | state {} acted {} entities {:?} watcher health {}",
            world.get_block_state_id(&pos).as_u16(),
            breaker.last_action_time.load().elapsed() < Duration::from_secs(300),
            entities_near(world, pos),
            watcher.living_entity.health.load(),
        );
    }
    line.push('\n');
    Ok(line)
}

#[tokio::test(flavor = "multi_thread")]
async fn player_action_is_the_same() -> TestResult {
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
    script_golden::bless_or_compare("player_action.txt", &out)
}
