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

use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::{Block, BlockStateId};
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
    let away = if scene.far { 8.0 } else { 2.0 };
    place(
        &breaker,
        Vector3::new(
            f64::from(pos.0.x) + 0.5,
            64.0,
            f64::from(pos.0.z) + 0.5 + away,
        ),
        pos,
    );
    place(
        &watcher,
        Vector3::new(f64::from(pos.0.x) + 3.5, 64.0, f64::from(pos.0.z) + 0.5),
        pos,
    );
    breaker
        .client_loaded
        .store(!scene.unloaded, Ordering::Relaxed);
    let inventory = breaker.inventory();
    if let Some((item, count)) = scene.held {
        inventory.set_stack_in_hand(Hand::Right, ItemStack::new(count, item));
    }
    if let Some((item, count)) = scene.offhand {
        inventory.set_stack_in_hand(Hand::Left, ItemStack::new(count, item));
    }
    if let Some(item) = scene.using {
        breaker
            .living_entity
            .set_active_hand(Hand::Right, ItemStack::new(1, item), 72_000);
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
    let line = format!(
        "{}: left {} items {:?} {} | breaker [{}] | watcher [{}]\n",
        scene.name,
        world.get_block(&pos).name,
        items_near(world, pos),
        breaker_text(&breaker, block),
        packets(&mut breaker_tap),
        packets(&mut watcher_tap),
    );
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
