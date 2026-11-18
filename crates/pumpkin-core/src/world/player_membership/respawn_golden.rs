//! A record of what `World::respawn_player` sends and sets, made before the function was split
//! (`tests/golden/respawn_player.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene starts its own real server (overworld, nether and end, empty chunks loaded by
//! hand: no chunk system runs in a test server), puts a Java watcher and a Bedrock watcher in
//! the overworld and a Java watcher in the nether, then respawns one player who died (or, with
//! `alive`, left the End) in the scene's dimension with the scene's respawn point. Every player
//! has a fixed name, UUID and entity id (`test_support::Identity`), so packets are the same in
//! every run. One line per scene: each player's packets (length and FNV-1a of the bytes), then
//! the respawner's world, position, rotation, experience, inventory, food, respawn point, which
//! worlds list and track it, and the respawn anchor's state.
//! Not covered: plugins that move the spawn or cancel the world change (none is loaded).

use std::fmt::Write;
use std::num::NonZero;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use pumpkin_data::block_properties::{
    BedPart, HorizontalFacing, RespawnAnchorLikeProperties, WhiteBedLikeProperties,
};
use pumpkin_data::dimension::Dimension;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use crate::entity::EntityBase;
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::player::{Player, RespawnPoint};
use crate::net::java::PacketTap;
use crate::server::Server;
use crate::world::World;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const RESPAWNER: Identity = Identity {
    name: "Respawner",
    uuid: Uuid::from_u128(0x0000_00d1),
    entity_id: 9_101,
};
const JAVA_WATCHER: Identity = Identity {
    name: "JavaWatcher",
    uuid: Uuid::from_u128(0x0000_00d2),
    entity_id: 9_102,
};
const BEDROCK_WATCHER: Identity = Identity {
    name: "BedrockWatcher",
    uuid: Uuid::from_u128(0x0000_00d3),
    entity_id: 9_103,
};
const NETHER_WATCHER: Identity = Identity {
    name: "NetherWatcher",
    uuid: Uuid::from_u128(0x0000_00d4),
    entity_id: 9_104,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dim {
    Overworld,
    Nether,
    End,
}

/// The respawner's stored respawn point.
#[derive(Clone, Copy)]
enum Point {
    None,
    /// A red bed in the overworld at [`BED`] (head).
    Bed,
    /// A non-forced point in the overworld where no bed stands.
    NoBed,
    /// A forced point (`/spawnpoint`) in air.
    ForcedInAir,
    /// A forced point inside the stone column.
    ForcedInStone,
    /// A respawn anchor in the nether at [`ANCHOR`] with this many charges.
    Anchor(u8),
    /// A forced point in air in the End.
    ForcedInEnd,
}

#[derive(Clone, Copy)]
struct Scene {
    name: &'static str,
    /// The level spawn stands over an empty chunk (no top block).
    void_spawn: bool,
    dies_in: Dim,
    point: Point,
    alive: bool,
    keep_inventory: bool,
    gamemode: GameMode,
    bedrock: bool,
    /// Appended after the first mutation run: the watchers see the respawner before it
    /// respawns (watched section and sent chunks set by hand), and the respawner watches its
    /// chunks and has its client's player set (so the center chunk is really sent).
    seen: bool,
    /// Appended: the level spawn at this x, z over its own lower column.
    spawn_xz: Option<(i32, i32)>,
    /// Appended: the level spawn chunk is not loaded; a stand-in chunk system answers it.
    unloaded_spawn: bool,
    /// Appended scenes end their line with the respawner's chunk-send epoch and tickets.
    extra: bool,
    /// Appended: before respawning from the nether, the respawner still counts as a viewer of
    /// the overworld Java watcher (a pairing left over from an earlier world).
    stale_pairing: bool,
}

const DEAD: Scene = Scene {
    name: "",
    void_spawn: false,
    dies_in: Dim::Overworld,
    point: Point::None,
    alive: false,
    keep_inventory: false,
    gamemode: GameMode::Survival,
    bedrock: false,
    seen: false,
    spawn_xz: None,
    unloaded_spawn: false,
    extra: false,
    stale_pairing: false,
};

const WIDE: Scene = Scene {
    extra: true,
    ..DEAD
};

const BED: BlockPos = BlockPos::new(8, 65, 7);
const ANCHOR: BlockPos = BlockPos::new(8, 65, 8);

fn scenes() -> Vec<Scene> {
    vec![
        Scene {
            name: "dead, no respawn point",
            ..DEAD
        },
        Scene {
            name: "dead, keep inventory",
            keep_inventory: true,
            ..DEAD
        },
        Scene {
            name: "dead, level spawn over an empty chunk",
            void_spawn: true,
            ..DEAD
        },
        Scene {
            name: "dead in creative",
            gamemode: GameMode::Creative,
            ..DEAD
        },
        Scene {
            name: "dead, bed",
            point: Point::Bed,
            ..DEAD
        },
        Scene {
            name: "dead, respawn point without a bed",
            point: Point::NoBed,
            ..DEAD
        },
        Scene {
            name: "dead, forced point in air",
            point: Point::ForcedInAir,
            ..DEAD
        },
        Scene {
            name: "dead, forced point in stone",
            point: Point::ForcedInStone,
            ..DEAD
        },
        Scene {
            name: "dead in the nether, no respawn point",
            dies_in: Dim::Nether,
            ..DEAD
        },
        Scene {
            name: "dead in the nether, keep inventory",
            dies_in: Dim::Nether,
            keep_inventory: true,
            ..DEAD
        },
        Scene {
            name: "alive, leaving the end",
            dies_in: Dim::End,
            alive: true,
            ..DEAD
        },
        Scene {
            name: "dead in the overworld, charged anchor in the nether",
            point: Point::Anchor(2),
            ..DEAD
        },
        Scene {
            name: "dead in the nether, empty anchor",
            dies_in: Dim::Nether,
            point: Point::Anchor(0),
            ..DEAD
        },
        Scene {
            name: "Bedrock player dead, no respawn point",
            bedrock: true,
            ..DEAD
        },
    ]
}

fn widened_scenes() -> Vec<Scene> {
    vec![
        Scene {
            name: "seen, dead, no respawn point",
            seen: true,
            ..WIDE
        },
        Scene {
            name: "seen, dead in the nether",
            dies_in: Dim::Nether,
            seen: true,
            ..WIDE
        },
        Scene {
            name: "seen, dead in the overworld, charged anchor in the nether",
            point: Point::Anchor(2),
            seen: true,
            ..WIDE
        },
        Scene {
            name: "seen Bedrock player, charged anchor in the nether",
            point: Point::Anchor(2),
            bedrock: true,
            seen: true,
            ..WIDE
        },
        Scene {
            name: "Bedrock player, forced point in the End",
            point: Point::ForcedInEnd,
            bedrock: true,
            ..WIDE
        },
        Scene {
            name: "level spawn at x 3, z -6",
            spawn_xz: Some((3, -6)),
            ..WIDE
        },
        Scene {
            name: "level spawn chunk not loaded yet",
            unloaded_spawn: true,
            ..WIDE
        },
        Scene {
            name: "dead in the nether, still paired with an overworld watcher",
            dies_in: Dim::Nether,
            stale_pairing: true,
            ..WIDE
        },
    ]
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn packet_text(packet: &[u8]) -> String {
    if let Some(dump) = std::env::var_os("KVASIR_DUMP") {
        use std::io::Write as _;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(dump)
        {
            let hex = packet.iter().fold(String::new(), |mut hex, byte| {
                let _ = write!(hex, "{byte:02x}");
                hex
            });
            let _ = writeln!(file, "{hex}");
        }
    }
    format!("{}:{:08x}", packet.len(), fnv(packet) as u32)
}

fn java_packets(tap: &mut PacketTap) -> String {
    tap.drain()
        .iter()
        .map(|p| packet_text(p))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A packet tap drained by a task while the respawn runs: Bedrock `send_game_packet` and Java
/// `send_packet_now` wait until the writer took each packet, and in a test nothing else reads
/// the queue.
struct Drained {
    packets: Arc<Mutex<Vec<Bytes>>>,
    /// Drain passes completed, so [`Drained::take`] can wait for a pass that began after it.
    passes: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}

impl Drained {
    fn start(mut drain: impl FnMut() -> Vec<Bytes> + Send + 'static) -> Self {
        let packets = Arc::new(Mutex::new(Vec::new()));
        let passes = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let task = tokio::spawn({
            let packets = Arc::clone(&packets);
            let passes = Arc::clone(&passes);
            let stop = Arc::clone(&stop);
            async move {
                loop {
                    let done = stop.load(Ordering::Acquire);
                    let drained = drain();
                    packets
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .extend(drained);
                    passes.fetch_add(1, Ordering::AcqRel);
                    if done {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                }
            }
        });
        Self {
            packets,
            passes,
            stop,
            task,
        }
    }

    /// Everything received since the last call.
    async fn take(&self) -> String {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        // Scenes run side by side: on a busy host 20 ms may pass without a drain pass, so also
        // wait for two passes (the second began after this call).
        let start = self.passes.load(Ordering::Acquire);
        while self.passes.load(Ordering::Acquire) < start + 2 {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        std::mem::take(
            &mut *self
                .packets
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
        .iter()
        .map(|p| packet_text(p))
        .collect::<Vec<_>>()
        .join(" ")
    }

    async fn finish(self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.task.await;
    }
}

struct Worlds {
    overworld: Arc<World>,
    nether: Arc<World>,
    end: Arc<World>,
}

impl Worlds {
    fn of(server: &Server) -> Self {
        Self {
            overworld: server.get_world_from_dimension(&Dimension::OVERWORLD),
            nether: server.get_world_from_dimension(&Dimension::THE_NETHER),
            end: server.get_world_from_dimension(&Dimension::THE_END),
        }
    }

    const fn get(&self, dim: Dim) -> &Arc<World> {
        match dim {
            Dim::Overworld => &self.overworld,
            Dim::Nether => &self.nether,
            Dim::End => &self.end,
        }
    }
}

fn load_chunks(world: &World, center: Vector2<i32>) {
    for dx in -1..=1 {
        for dz in -1..=1 {
            let at = Vector2::new(center.x + dx, center.y + dz);
            world
                .level
                .loaded_chunks
                .insert(at, pumpkin_world::chunk::ChunkData::empty_sync(at.x, at.y));
        }
    }
}

fn put(world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
    world.set_block_state(&pos, state, BlockFlags::NOTIFY_ALL);
}

fn bed_state(part: BedPart) -> BlockStateId {
    let mut bed = WhiteBedLikeProperties::default(&Block::RED_BED);
    bed.facing = HorizontalFacing::North;
    bed.part = part;
    bed.to_state_id(&Block::RED_BED)
}

fn anchor_state(charges: u8) -> BlockStateId {
    RespawnAnchorLikeProperties { charges }.to_state_id(&Block::RESPAWN_ANCHOR)
}

/// Stone floors at y = 64 around the origin of every dimension, a stone column at the
/// overworld origin up to y = 70, the bed or the anchor, and the level spawn and game rule.
fn prepare(server: &Server, worlds: &Worlds, scene: &Scene) {
    for world in [&worlds.overworld, &worlds.nether, &worlds.end] {
        load_chunks(world, Vector2::new(0, 0));
        for x in -4..=12 {
            for z in -4..=12 {
                put(
                    world,
                    BlockPos::new(x, 64, z),
                    Block::STONE.default_state.id,
                );
            }
        }
    }
    load_chunks(&worlds.overworld, Vector2::new(4, 4));
    for y in 65..=70 {
        put(
            &worlds.overworld,
            BlockPos::new(0, y, 0),
            Block::STONE.default_state.id,
        );
    }
    match scene.point {
        Point::Bed => {
            put(&worlds.overworld, BED, bed_state(BedPart::Head));
            put(&worlds.overworld, BED.south(), bed_state(BedPart::Foot));
        }
        Point::Anchor(charges) => put(&worlds.nether, ANCHOR, anchor_state(charges)),
        _ => {}
    }
    let mut level = (**server.level_info.load()).clone();
    level.spawn_x = if scene.void_spawn { 64 } else { 0 };
    level.spawn_z = level.spawn_x;
    if let Some((x, z)) = scene.spawn_xz {
        for y in 65..=67 {
            put(
                &worlds.overworld,
                BlockPos::new(x, y, z),
                Block::STONE.default_state.id,
            );
        }
        level.spawn_x = x;
        level.spawn_z = z;
    }
    if scene.unloaded_spawn {
        level.spawn_x = 200;
        level.spawn_z = 200;
    }
    level.spawn_y = 72;
    level.spawn_yaw = 45.0;
    level.spawn_pitch = 5.0;
    level.game_rules.keep_inventory = scene.keep_inventory;
    server.level_info.store(Arc::new(level));
}

fn respawn_point(point: Point) -> Option<RespawnPoint> {
    let (dimension, position, force) = match point {
        Point::None => return None,
        Point::Bed => (Dimension::OVERWORLD, BED, false),
        Point::NoBed => (Dimension::OVERWORLD, BlockPos::new(6, 65, 6), false),
        Point::ForcedInAir => (Dimension::OVERWORLD, BlockPos::new(5, 65, 9), true),
        Point::ForcedInStone => (Dimension::OVERWORLD, BlockPos::new(0, 66, 0), true),
        Point::Anchor(_) => (Dimension::THE_NETHER, ANCHOR, false),
        Point::ForcedInEnd => (Dimension::THE_END, BlockPos::new(5, 65, 9), true),
    };
    Some(RespawnPoint {
        dimension,
        position,
        yaw: 120.0,
        force,
    })
}

/// The state a death leaves behind: experience, an apple, low food, a portal cooldown.
fn give_state(player: &Player, scene: &Scene) {
    let entity = player.get_entity();
    entity.set_pos(Vector3::new(3.5, 65.0, -2.5));
    entity.last_pos.store(Vector3::new(3.25, 65.5, -2.75));
    entity.set_rotation(30.0, 10.0);
    entity.portal_cooldown.store(7, Ordering::Relaxed);
    player.experience_level.store(5, Ordering::Relaxed);
    player.experience_points.store(40, Ordering::Relaxed);
    player.experience_progress.store(0.5);
    player.hunger_manager.level.store(3);
    player
        .inventory
        .main_inventory
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)[0] = ItemStack::new(3, &Item::APPLE);
    *player
        .respawn_point
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = respawn_point(scene.point);
}

fn lists(world: &World, player: &Player) -> bool {
    world
        .players
        .load()
        .iter()
        .any(|p| p.gameprofile.id == player.gameprofile.id)
}

fn player_text(player: &Player, worlds: &Worlds) -> String {
    let entity = player.get_entity();
    let position = entity.pos.load();
    let items = player
        .inventory
        .main_inventory
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .filter(|stack| !stack.is_empty())
        .count();
    let point = player
        .respawn_point
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .map(|p| {
            format!(
                "{} {:?} {} {}",
                p.dimension.minecraft_name, p.position, p.yaw, p.force
            )
        });
    let mut text = format!(
        "in {} pos {} {} {} rot {} {} last {:?} xp {} {} {} items {items} food {} point {point:?}",
        player.world().dimension.minecraft_name,
        position.x,
        position.y,
        position.z,
        entity.yaw.load(),
        entity.pitch.load(),
        entity.last_pos.load() == position,
        player.experience_level.load(Ordering::Relaxed),
        player.experience_points.load(Ordering::Relaxed),
        player.experience_progress.load(),
        player.hunger_manager.level.load(),
    );
    for (name, world) in [("overworld", &worlds.overworld), ("nether", &worlds.nether)] {
        let tracked = world
            .entity_tracker
            .get_tracked_entity(player.entity_id())
            .is_some();
        let _ = write!(
            text,
            " {name} lists {} tracks {tracked}",
            lists(world, player)
        );
    }
    let anchor = worlds.nether.get_block_state_id(&ANCHOR);
    let _ = write!(text, " anchor {}", anchor.as_u16());
    text
}

/// The respawner's chunk-send epoch and whether it holds chunk tickets (appended scenes).
fn chunk_text(player: &Player) -> String {
    let tickets = player
        .held_chunk_tickets
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_some();
    format!(
        " epoch {} tickets {tickets}",
        player.chunk_send_epoch.load(Ordering::Relaxed)
    )
}

/// Lets `watcher` see everything in the chunks around the origin, as if the chunk system had
/// sent them, and pairs it with the respawner's tracked entity in `world`.
fn see(watcher: &Arc<Player>, world: &Arc<World>, respawner: &Player) {
    watch_origin(watcher);
    if let Some(tracked) = world
        .entity_tracker
        .get_tracked_entity(respawner.entity_id())
    {
        tracked.update_player(watcher, world);
    }
}

/// `viewer` watches the chunks around the origin, all of them sent.
fn watch_origin(viewer: &Player) {
    viewer.watched_section.store(Cylindrical::new(
        Vector2::new(0, 0),
        NonZero::new(4).unwrap_or(NonZero::<u8>::MIN),
    ));
    let mut sender = viewer
        .chunk_sender
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for x in -2..=2 {
        for z in -2..=2 {
            sender.mark_sent_out_of_band(Vector2::new(x, z));
        }
    }
}

/// Stands in for the chunk system, which does not run in a test server: answers every request
/// for chunk (12, 12) with an empty chunk holding a stone column at (200, 64..=70, 200).
fn answer_spawn_chunk_requests(
    world: &Arc<World>,
) -> (Arc<AtomicBool>, tokio::task::JoinHandle<()>) {
    let chunk = pumpkin_world::chunk::ChunkData::empty_sync(12, 12);
    for y in 64..=70 {
        chunk.set_block_absolute_y(8, y, 8, Block::STONE.default_state.id);
    }
    let stop = Arc::new(AtomicBool::new(false));
    let task = tokio::spawn({
        let listener = Arc::clone(&world.level.chunk_listener);
        let stop = Arc::clone(&stop);
        async move {
            while !stop.load(Ordering::Acquire) {
                listener.process_new_chunk(Vector2::new(12, 12), &chunk);
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
        }
    });
    (stop, task)
}

async fn run_scene(scene: Scene) -> Result<String, Box<dyn std::error::Error>> {
    let scene = &scene;
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let worlds = Worlds::of(&server);
    prepare(&server, &worlds, scene);

    let (java, mut java_tap) =
        test_support::java_player_as(&worlds.overworld, GameMode::Survival, &JAVA_WATCHER)
            .ok_or("no Java packet tap")?;
    java.get_entity().set_pos(Vector3::new(2.5, 65.0, 2.5));
    worlds.overworld.add_player(&java)?;
    let (bedrock, bedrock_tap) =
        test_support::bedrock_player_as(&worlds.overworld, GameMode::Survival, &BEDROCK_WATCHER)
            .await?;
    bedrock.get_entity().set_pos(Vector3::new(-2.5, 65.0, -2.5));
    worlds.overworld.add_player(&bedrock)?;
    let mut bedrock_tap = bedrock_tap;
    let bedrock_watcher = Drained::start(move || bedrock_tap.drain());
    let (nether, mut nether_tap) =
        test_support::java_player_as(&worlds.nether, GameMode::Survival, &NETHER_WATCHER)
            .ok_or("no Java packet tap")?;
    nether.get_entity().set_pos(Vector3::new(1.5, 65.0, 1.5));
    worlds.nether.add_player(&nether)?;

    let home = worlds.get(scene.dies_in);
    let (player, received) = if scene.bedrock {
        let (player, mut tap) =
            test_support::bedrock_player_as(home, scene.gamemode, &RESPAWNER).await?;
        (player, Drained::start(move || tap.drain()))
    } else {
        let (player, mut tap) = test_support::java_player_as(home, scene.gamemode, &RESPAWNER)
            .ok_or("no Java packet tap")?;
        (player, Drained::start(move || tap.drain()))
    };
    give_state(&player, scene);
    home.add_player(&player)?;
    if scene.seen {
        if let crate::net::ClientPlatform::Java(client) = player.client.as_ref() {
            client.set_player(Arc::clone(&player));
        }
        crate::world::chunker::update_position(&player);
        for (watcher, world) in [
            (&java, &worlds.overworld),
            (&bedrock, &worlds.overworld),
            (&nether, &worlds.nether),
        ] {
            see(watcher, world, &player);
        }
    }
    if scene.stale_pairing {
        watch_origin(&player);
        if let Some(tracked) = worlds
            .overworld
            .entity_tracker
            .get_tracked_entity(JAVA_WATCHER.entity_id)
        {
            tracked.update_player(&player, &worlds.overworld);
        }
    }
    let _ = received.take().await;
    let _ = java_tap.drain();
    let _ = nether_tap.drain();
    let _ = bedrock_watcher.take().await;

    let chunk_system = scene
        .unloaded_spawn
        .then(|| answer_spawn_chunk_requests(&worlds.overworld));
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        home.respawn_player(&player, scene.alive),
    )
    .await
    .map_err(|_| "respawn_player did not finish in 20 s")?;
    if let Some((stop, task)) = chunk_system {
        stop.store(true, Ordering::Release);
        let _ = task.await;
    }

    let line = format!(
        "{}: respawner [{}] | java watcher [{}] | bedrock watcher [{}] | nether watcher [{}] | {}{}\n",
        scene.name,
        received.take().await,
        java_packets(&mut java_tap),
        bedrock_watcher.take().await,
        java_packets(&mut nether_tap),
        player_text(&player, &worlds),
        if scene.extra {
            chunk_text(&player)
        } else {
            String::new()
        },
    );
    received.finish().await;
    bedrock_watcher.finish().await;
    server.stop();
    Ok(line)
}

/// [`run_scene`] with a `Send` error, so it can run on its own task.
async fn run_scene_sendable(scene: Scene) -> Result<String, String> {
    run_scene(scene).await.map_err(|error| error.to_string())
}

/// Runs `scenes` side by side (one server each; one by one took 12.5 s for 14 scenes) and
/// joins their lines in scene order.
async fn run_all(scenes: Vec<Scene>) -> Result<String, Box<dyn std::error::Error>> {
    let runs: Vec<_> = scenes
        .into_iter()
        .map(|scene| tokio::spawn(run_scene_sendable(scene)))
        .collect();
    let mut report = String::new();
    for run in runs {
        report.push_str(&run.await??);
    }
    Ok(report)
}

#[tokio::test(flavor = "multi_thread")]
async fn respawn_player_is_the_same() -> TestResult {
    let report = run_all(scenes()).await?;
    crate::command::commands::script_golden::bless_or_compare("respawn_player.txt", &report)?;
    Ok(())
}

/// The scenes appended after the first mutation run, in their own test: each test server costs
/// about 2 CPU-seconds to start, so all 22 scenes in one test stayed over the 10 s budget.
#[tokio::test(flavor = "multi_thread")]
async fn respawn_player_seen_is_the_same() -> TestResult {
    let report = run_all(widened_scenes()).await?;
    crate::command::commands::script_golden::bless_or_compare("respawn_player_seen.txt", &report)?;
    Ok(())
}
