//! A record of what `World::spawn_bedrock_player` sends and sets, made before the function was
//! split (`tests/golden/bedrock_player_join.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene starts its own real server with a Java player and a Bedrock player already in the
//! world, then lets a Bedrock player join (first join over a built column in survival, creative
//! and spectator, a first join over an empty chunk, a returning player at its stored position,
//! and a second Bedrock player joining while the first stays). Every player has a fixed name,
//! UUID and entity id (`test_support::Identity`), so the packets are the same in every run. Per
//! scene: every packet the joiner and both watchers receive (id, length, FNV-1a of the bytes),
//! the joiner's position, rotation, abilities and `has_played_before`.
//! Not covered: a plugin that cancels the join event (no plugin is loaded).
//! Re-blessed after the heightmap fix of `ChunkData::update_heightmap` (arguments were x, z, y
//! order, the stone column read as top block 0): first joins over the column spawn at y = 71.
//! `KVASIR_DUMP=<file>` appends each scene's packet list.

use std::fmt::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use pumpkin_data::Block;
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use crate::entity::EntityBase;
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::player::Player;
use crate::net::bedrock::BedrockPacketTap;
use crate::server::Server;
use crate::world::World;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const JAVA_WATCHER: Identity = Identity {
    name: "JavaWatcher",
    uuid: Uuid::from_u128(0x0000_00a1),
    entity_id: 9_001,
};
const BEDROCK_WATCHER: Identity = Identity {
    name: "BedrockWatcher",
    uuid: Uuid::from_u128(0x0000_00b1),
    entity_id: 9_002,
};
const JOINER: Identity = Identity {
    name: "Joiner",
    uuid: Uuid::from_u128(0x0000_00c1),
    entity_id: 9_003,
};
const SECOND: Identity = Identity {
    name: "Second",
    uuid: Uuid::from_u128(0x0000_00c2),
    entity_id: 9_004,
};

/// Where the joiner spawns and in which game mode.
#[derive(Clone, Copy)]
enum Spawn {
    /// First join at the level spawn, over a stone column (top block at y = 70).
    Column,
    /// First join at a level spawn over an empty chunk (no top block: `spawn_y`).
    Void,
    /// A returning player at its stored position.
    Returning,
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn var_uint(bytes: &[u8]) -> Option<(u32, usize)> {
    let mut value = 0u32;
    for (i, byte) in bytes.iter().enumerate().take(5) {
        value |= u32::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }
    }
    None
}

/// `id:length:hash` of a queued Bedrock packet (`VarUInt` length, `VarUInt` header, payload).
fn bedrock_text(packet: &[u8]) -> String {
    let id = var_uint(packet)
        .and_then(|(_, used)| var_uint(&packet[used..]))
        .map_or_else(
            || "?".to_string(),
            |(header, _)| (header & 0x3ff).to_string(),
        );
    format!("{id}:{}:{:08x}", packet.len(), fnv(packet) as u32)
}

fn java_text(packet: &[u8]) -> String {
    format!("{}:{:08x}", packet.len(), fnv(packet) as u32)
}

/// A Bedrock tap drained by a task while the join runs: `send_game_packet` waits until the
/// writer took each packet, and in a test nothing else reads the queue.
struct Drained {
    packets: Arc<Mutex<Vec<Bytes>>>,
    stop: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}

impl Drained {
    fn start(mut tap: BedrockPacketTap) -> Self {
        let packets = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let task = tokio::spawn({
            let packets = Arc::clone(&packets);
            let stop = Arc::clone(&stop);
            async move {
                loop {
                    let done = stop.load(Ordering::Acquire);
                    let drained = tap.drain();
                    packets
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .extend(drained);
                    if done {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                }
            }
        });
        Self {
            packets,
            stop,
            task,
        }
    }

    /// Everything received since the last call.
    async fn take(&self) -> Vec<String> {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        std::mem::take(
            &mut *self
                .packets
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
        .iter()
        .map(|packet| bedrock_text(packet))
        .collect()
    }

    async fn finish(self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.task.await;
    }
}

fn first_world(server: &Server) -> Result<Arc<World>, String> {
    server
        .worlds
        .load()
        .first()
        .cloned()
        .ok_or_else(|| "the server has no world".to_string())
}

/// Loads empty chunks at the level spawn (0, 0) and at (64, 64), with a stone column from
/// y = 64 to 70 at the first; the level spawn moves to (64, 64) for [`Spawn::Void`].
fn prepare_spawn(server: &Server, world: &Arc<World>, spawn: Spawn) {
    for (x, z) in [(0, 0), (4, 4)] {
        world.level.loaded_chunks.insert(
            Vector2::new(x, z),
            pumpkin_world::chunk::ChunkData::empty_sync(x, z),
        );
    }
    for y in 64..=70 {
        world.set_block_state(
            &BlockPos::new(0, y, 0),
            Block::STONE.default_state.id,
            BlockFlags::NOTIFY_ALL,
        );
    }
    let mut level = (**server.level_info.load()).clone();
    level.spawn_x = if matches!(spawn, Spawn::Void) { 64 } else { 0 };
    level.spawn_z = level.spawn_x;
    level.spawn_y = 72;
    level.spawn_yaw = 45.0;
    level.spawn_pitch = 5.0;
    server.level_info.store(Arc::new(level));
}

fn player_text(player: &Player) -> String {
    let entity = player.get_entity();
    let position = entity.pos.load();
    let abilities = player
        .abilities
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    format!(
        "pos {} {} {} rot {} {} last {:?} abilities fly {} {} creative {} invulnerable {} modify {} speeds {} {} played {}",
        position.x,
        position.y,
        position.z,
        entity.yaw.load(),
        entity.pitch.load(),
        entity.last_pos.load() == position,
        abilities.allow_flying,
        abilities.flying,
        abilities.creative,
        abilities.invulnerable,
        abilities.allow_modify_world,
        abilities.fly_speed,
        abilities.walk_speed,
        player.has_played_before.load(Ordering::Relaxed),
    )
}

async fn join(
    server: &Arc<Server>,
    world: &Arc<World>,
    player: &Arc<Player>,
) -> Result<(), String> {
    world.add_player(player)?;
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        world.spawn_bedrock_player(&server.basic_config, Arc::clone(player), server),
    )
    .await
    .map_err(|_| "spawn_bedrock_player did not finish in 20 s".to_string())
}

fn dump(scene: &str, text: &str) {
    if let Some(dump) = std::env::var_os("KVASIR_DUMP") {
        use std::io::Write as _;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(dump)
        {
            let _ = writeln!(file, "{scene} {text}");
        }
    }
}

async fn run_scene(
    name: &str,
    spawn: Spawn,
    gamemode: GameMode,
    second: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = first_world(&server)?;
    prepare_spawn(&server, &world, spawn);

    let (java, mut java_tap) =
        test_support::java_player_as(&world, GameMode::Survival, &JAVA_WATCHER)
            .ok_or("no Java packet tap")?;
    java.get_entity().set_pos(Vector3::new(2.5, 71.0, 2.5));
    world.add_player(&java)?;
    let (bedrock, bedrock_tap) =
        test_support::bedrock_player_as(&world, GameMode::Survival, &BEDROCK_WATCHER).await?;
    bedrock.get_entity().set_pos(Vector3::new(-2.5, 71.0, -2.5));
    world.add_player(&bedrock)?;
    let bedrock_watcher = Drained::start(bedrock_tap);
    let _ = java_tap.drain();
    let _ = bedrock_watcher.take().await;

    let (joiner, joiner_tap) = test_support::bedrock_player_as(&world, gamemode, &JOINER).await?;
    if matches!(spawn, Spawn::Returning) {
        joiner.get_entity().set_pos(Vector3::new(5.5, 80.0, 7.5));
        joiner.get_entity().set_rotation(90.0, 10.0);
        joiner.has_played_before.store(true, Ordering::Relaxed);
    }
    let joiner_packets = Drained::start(joiner_tap);
    join(&server, &world, &joiner).await?;

    let mut text = format!(
        "joiner [{}] | java watcher [{}] | bedrock watcher [{}] | {}",
        joiner_packets.take().await.join(" "),
        java_tap
            .drain()
            .iter()
            .map(|p| java_text(p))
            .collect::<Vec<_>>()
            .join(" "),
        bedrock_watcher.take().await.join(" "),
        player_text(&joiner),
    );
    if second {
        let (other, other_tap) =
            test_support::bedrock_player_as(&world, GameMode::Survival, &SECOND).await?;
        let other_packets = Drained::start(other_tap);
        join(&server, &world, &other).await?;
        let _ = write!(
            text,
            " || second [{}] | first [{}] | java watcher [{}] | bedrock watcher [{}] | {}",
            other_packets.take().await.join(" "),
            joiner_packets.take().await.join(" "),
            java_tap
                .drain()
                .iter()
                .map(|p| java_text(p))
                .collect::<Vec<_>>()
                .join(" "),
            bedrock_watcher.take().await.join(" "),
            player_text(&other),
        );
        other_packets.finish().await;
    }
    joiner_packets.finish().await;
    bedrock_watcher.finish().await;
    server.stop();
    dump(name, &text);
    Ok(format!(
        "{name}: {:016x} {}\n",
        fnv(text.as_bytes()),
        player_text(&joiner)
    ))
}

#[tokio::test(flavor = "multi_thread")]
async fn bedrock_player_join() -> TestResult {
    let scenes = [
        (
            "first join, survival",
            Spawn::Column,
            GameMode::Survival,
            false,
        ),
        (
            "first join, creative",
            Spawn::Column,
            GameMode::Creative,
            false,
        ),
        (
            "first join, spectator",
            Spawn::Column,
            GameMode::Spectator,
            false,
        ),
        (
            "first join over an empty chunk",
            Spawn::Void,
            GameMode::Survival,
            false,
        ),
        (
            "returning player",
            Spawn::Returning,
            GameMode::Survival,
            false,
        ),
        (
            "a second Bedrock player joins",
            Spawn::Column,
            GameMode::Survival,
            true,
        ),
    ];
    let mut report = String::new();
    for (name, spawn, gamemode, second) in scenes {
        report.push_str(&run_scene(name, spawn, gamemode, second).await?);
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("bedrock_player_join.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
