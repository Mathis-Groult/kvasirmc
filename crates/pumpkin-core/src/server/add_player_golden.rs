//! A record of what `Server::add_player` returns and changes, made before the function was
//! split (`tests/golden/add_player.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene starts its own real server (overworld, nether and end), may write a saved player
//! file (`world/players/data/<uuid>.dat`: dimension, position, rotation, game mode, the
//! `HasPlayedBefore` flag; or garbage), an advancements file, an operator entry or a different
//! configuration, then calls `add_player` once (the duplicate scene twice) with a fixed profile
//! over a Java client whose packets a task drains. One line per call: whether a player came
//! back, its world, position, rotation, game mode, permission level, view distance, played-before
//! flag, health and abilities, who stands in the world and the status list, the user cache
//! entry, the management notifications, whether the client closed and the packets it was sent
//! (the kick reason is in them), and the advancement state.
//! Not covered: plugins that cancel the login event (none is loaded); the skin download
//! (needs a skin server).

use std::fmt::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arc_swap::ArcSwap;
use base64::Engine as _;
use base64::prelude::BASE64_STANDARD;
use bytes::Bytes;
use pumpkin_config::op::Op;
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_nbt::tag::NbtTag;
use pumpkin_protocol::Property;
use pumpkin_util::{GameMode, PermissionLvl};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

use crate::entity::ai::test_support::{self, Identity};
use crate::net::java::PacketTap;
use crate::net::{GameProfile, PlayerConfig};
use crate::server::Server;
use crate::world::World;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const JOINER: Identity = Identity {
    name: "Joiner",
    uuid: Uuid::from_u128(0x0000_00f1),
    entity_id: 9_301,
};

const SECOND: Identity = Identity {
    name: "Joiner",
    uuid: Uuid::from_u128(0x0000_00f1),
    entity_id: 9_302,
};

/// What the player's save file holds before the call.
#[derive(Clone, Copy)]
enum Saved {
    /// No file.
    Nothing,
    /// A file that is not an NBT file.
    Garbage,
    /// A file with the given entries.
    File(Data),
}

#[derive(Clone, Copy)]
struct Data {
    dimension: Option<&'static str>,
    pos: Option<[f64; 3]>,
    rotation: Option<[f32; 2]>,
    game_mode: Option<i32>,
    played_before: bool,
}

const FULL: Data = Data {
    dimension: Some("minecraft:overworld"),
    pos: Some([10.5, 70.0, -3.5]),
    rotation: Some([45.0, 15.0]),
    game_mode: Some(1),
    played_before: true,
};

/// The advancements file before the call.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Advancements {
    Nothing,
    Garbage,
    /// One known advancement with a done criterion and one unknown name.
    Progress,
}

/// The skin the profile points at (the skin task runs after `add_player` returned).
#[derive(Clone, Copy, PartialEq, Eq)]
enum SkinHost {
    /// No `textures` property.
    Nothing,
    /// A local server that sends a 64x64 png.
    Served { slim: bool },
    /// A url nobody listens on.
    Unreachable,
}

#[derive(Clone, Copy)]
struct Scene {
    name: &'static str,
    saved: Saved,
    save_enabled: bool,
    /// `None`: no player config; else `(server_listing, view_distance)`.
    config: Option<(bool, u8)>,
    default_mode: GameMode,
    /// An operator entry for the player with this level.
    op: Option<PermissionLvl>,
    default_op_level: PermissionLvl,
    advancements: Advancements,
    closed: bool,
    no_worlds: bool,
    /// A second call with the same profile over another client.
    twice: bool,
    skin: SkinHost,
}

const BASE: Scene = Scene {
    name: "",
    saved: Saved::Nothing,
    save_enabled: true,
    config: None,
    default_mode: GameMode::Survival,
    op: None,
    default_op_level: PermissionLvl::Zero,
    advancements: Advancements::Nothing,
    closed: false,
    no_worlds: false,
    twice: false,
    skin: SkinHost::Nothing,
};

const fn named(name: &'static str, scene: Scene) -> Scene {
    Scene { name, ..scene }
}

const fn saved(name: &'static str, data: Data) -> Scene {
    Scene {
        name,
        saved: Saved::File(data),
        ..BASE
    }
}

fn saved_scenes() -> Vec<Scene> {
    vec![
        named("new player", BASE),
        saved("returning player", FULL),
        saved(
            "returning in the nether",
            Data {
                dimension: Some("minecraft:the_nether"),
                game_mode: Some(2),
                ..FULL
            },
        ),
        saved(
            "returning in the end",
            Data {
                dimension: Some("minecraft:the_end"),
                game_mode: Some(3),
                ..FULL
            },
        ),
        saved(
            "unknown dimension",
            Data {
                dimension: Some("minecraft:nowhere"),
                ..FULL
            },
        ),
        saved(
            "no dimension key",
            Data {
                dimension: None,
                ..FULL
            },
        ),
        saved(
            "saved but not played before",
            Data {
                played_before: false,
                ..FULL
            },
        ),
        saved(
            "saved without position or game mode",
            Data {
                pos: None,
                rotation: None,
                game_mode: None,
                ..FULL
            },
        ),
        named(
            "garbage file",
            Scene {
                saved: Saved::Garbage,
                ..BASE
            },
        ),
        named(
            "file while saving is off",
            Scene {
                saved: Saved::File(FULL),
                save_enabled: false,
                ..BASE
            },
        ),
    ]
}

fn config_scenes() -> Vec<Scene> {
    vec![
        named(
            "config without listing",
            Scene {
                config: Some((false, 5)),
                ..BASE
            },
        ),
        named(
            "config with listing",
            Scene {
                config: Some((true, 5)),
                ..BASE
            },
        ),
        named(
            "creative default mode",
            Scene {
                default_mode: GameMode::Creative,
                ..BASE
            },
        ),
        named(
            "saved mode beats default mode",
            Scene {
                default_mode: GameMode::Creative,
                saved: Saved::File(Data {
                    game_mode: Some(2),
                    ..FULL
                }),
                ..BASE
            },
        ),
        named(
            "operator level 3",
            Scene {
                op: Some(PermissionLvl::Three),
                ..BASE
            },
        ),
        named(
            "default op level 2",
            Scene {
                default_op_level: PermissionLvl::Two,
                ..BASE
            },
        ),
    ]
}

fn refusal_scenes() -> Vec<Scene> {
    vec![
        named(
            "admission closed",
            Scene {
                closed: true,
                config: Some((true, 5)),
                ..BASE
            },
        ),
        named(
            "no world",
            Scene {
                no_worlds: true,
                ..BASE
            },
        ),
        named(
            "duplicate uuid",
            Scene {
                twice: true,
                config: Some((true, 5)),
                ..BASE
            },
        ),
        named(
            "garbage advancements",
            Scene {
                advancements: Advancements::Garbage,
                ..BASE
            },
        ),
        named(
            "saved advancements",
            Scene {
                advancements: Advancements::Progress,
                ..BASE
            },
        ),
    ]
}

fn skin_scenes() -> Vec<Scene> {
    vec![
        named(
            "skin served wide",
            Scene {
                skin: SkinHost::Served { slim: false },
                ..BASE
            },
        ),
        named(
            "skin served slim",
            Scene {
                skin: SkinHost::Served { slim: true },
                ..BASE
            },
        ),
        named(
            "skin unreachable",
            Scene {
                skin: SkinHost::Unreachable,
                ..BASE
            },
        ),
    ]
}

fn scenes() -> Vec<Scene> {
    let mut all = saved_scenes();
    all.extend(config_scenes());
    all.extend(refusal_scenes());
    all.extend(skin_scenes());
    all
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// A packet tap drained by a task: the kick waits until the writer took its packet.
struct Drained {
    packets: Arc<Mutex<Vec<Bytes>>>,
    passes: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}

impl Drained {
    fn start(mut tap: PacketTap) -> Self {
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
                    let drained = tap.drain();
                    packets
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .extend(drained);
                    passes.fetch_add(1, Ordering::AcqRel);
                    if done {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(1)).await;
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

    /// Everything received since the last call, as `len:hash` per packet.
    async fn take(&self) -> String {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let start = self.passes.load(Ordering::Acquire);
        while self.passes.load(Ordering::Acquire) < start + 2 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        std::mem::take(
            &mut *self
                .packets
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
        .iter()
        .map(|packet| format!("{}:{:08x}", packet.len(), fnv(packet) as u32))
        .collect::<Vec<_>>()
        .join(" ")
    }

    async fn finish(self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.task.await;
    }
}

fn saved_compound(data: &Data) -> NbtCompound {
    let mut nbt = NbtCompound::new();
    if let Some(dimension) = data.dimension {
        nbt.put_string("Dimension", dimension.to_string());
    }
    if let Some([x, y, z]) = data.pos {
        nbt.put("Pos", NbtTag::List(vec![x.into(), y.into(), z.into()]));
    }
    if let Some([yaw, pitch]) = data.rotation {
        nbt.put("Rotation", NbtTag::List(vec![yaw.into(), pitch.into()]));
    }
    if let Some(mode) = data.game_mode {
        nbt.put_int("playerGameType", mode);
    }
    nbt.put_bool("HasPlayedBefore", data.played_before);
    nbt
}

fn write_files(folder: &tempfile::TempDir, scene: &Scene) -> TestResult {
    let players = folder.path().join("world").join("players");
    let data_dir = players.join("data");
    std::fs::create_dir_all(&data_dir)?;
    let file = data_dir.join(format!("{}.dat", JOINER.uuid));
    match scene.saved {
        Saved::Nothing => {}
        Saved::Garbage => std::fs::write(&file, b"not a player file")?,
        Saved::File(data) => {
            let out = std::fs::File::create(&file)?;
            pumpkin_nbt::nbt_compress::write_gzip_compound_tag(saved_compound(&data), out)?;
        }
    }
    let advancements = players.join("advancements");
    std::fs::create_dir_all(&advancements)?;
    let file = advancements.join(format!("{}.json", JOINER.uuid));
    match scene.advancements {
        Advancements::Nothing => {}
        Advancements::Garbage => std::fs::write(&file, b"{ not json")?,
        Advancements::Progress => std::fs::write(
            &file,
            br#"{"minecraft:story/root":{"crafting_table":{"secs_since_epoch":1,"nanos_since_epoch":0}},"minecraft:nowhere/none":{}}"#,
        )?,
    }
    Ok(())
}

async fn start(
    folder: &tempfile::TempDir,
    scene: &Scene,
) -> Result<Arc<Server>, Box<dyn std::error::Error>> {
    write_files(folder, scene)?;
    let mode = scene.default_mode;
    let level = scene.default_op_level;
    let save = scene.save_enabled;
    let server = test_support::server_with_configs(
        folder,
        |basic| basic.default_gamemode = mode,
        |advanced| {
            advanced.commands.default_op_level = level;
            advanced.player_data.save_player_data = save;
        },
    )
    .await?;
    if let Some(level) = scene.op {
        server
            .data
            .operator_config
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .ops
            .push(Op {
                uuid: JOINER.uuid,
                name: JOINER.name.to_string(),
                level,
                bypasses_player_limit: false,
            });
    }
    if scene.closed {
        server.close_admission();
    }
    Ok(server)
}

fn profile(skin_url: Option<&str>, slim: bool) -> GameProfile {
    let properties = skin_url
        .map(|url| {
            let model = if slim {
                r#","metadata":{"model":"slim"}"#
            } else {
                ""
            };
            let json = format!(r#"{{"textures":{{"SKIN":{{"url":"{url}"{model}}}}}}}"#);
            Property {
                name: "textures".into(),
                value: BASE64_STANDARD.encode(json).into(),
                signature: None,
            }
        })
        .into_iter()
        .collect();
    GameProfile {
        id: JOINER.uuid,
        name: JOINER.name.to_string(),
        properties: ArcSwap::from_pointee(properties),
        profile_actions: None,
    }
}

/// Serves a 64x64 png to every request until the runtime ends; returns the url.
async fn serve_skin() -> Result<String, Box<dyn std::error::Error>> {
    let mut png_bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_bytes, 64, 64);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&vec![128u8; 64 * 64 * 4])?;
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/skin.png", listener.local_addr()?);
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let png_bytes = png_bytes.clone();
            tokio::spawn(async move {
                let mut request = [0u8; 1024];
                let _ = stream.read(&mut request).await;
                let head = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: image/png\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    png_bytes.len()
                );
                let _ = stream.write_all(head.as_bytes()).await;
                let _ = stream.write_all(&png_bytes).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    Ok(url)
}

/// A url on a port that is closed again.
async fn dead_url() -> Result<String, Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/skin.png", listener.local_addr()?);
    Ok(url)
}

fn config_of(scene: &Scene) -> Option<PlayerConfig> {
    scene.config.map(|(listing, view)| PlayerConfig {
        server_listing: listing,
        view_distance: std::num::NonZero::new(view).unwrap_or(std::num::NonZero::<u8>::MIN),
        ..PlayerConfig::default()
    })
}

/// The state of the server around the call, as one text.
fn server_state(server: &Server) -> String {
    let mut text = String::new();
    for world in server.worlds.load().iter() {
        let names: Vec<String> = world
            .players
            .load()
            .iter()
            .map(|p| p.gameprofile.name.clone())
            .collect();
        let _ = write!(
            text,
            "{} [{}] ",
            world.dimension.minecraft_name,
            names.join(" ")
        );
    }
    let status = server
        .listing
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get_status_response(0);
    if let Some(players) = status.players {
        let sample: Vec<String> = players.sample.iter().map(|s| s.name.clone()).collect();
        let _ = write!(
            text,
            "| online {} sample [{}] ",
            players.online,
            sample.join(" ")
        );
    }
    let cached = server
        .data
        .user_cache
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get_by_uuid(JOINER.uuid)
        .map(|entry| entry.name);
    let _ = write!(text, "| user cache {cached:?}");
    text
}

fn player_state(
    player: &crate::entity::player::Player,
    world: &Arc<crate::world::World>,
    server: &Server,
) -> String {
    let entity = &player.living_entity.entity;
    let pos = entity.pos.load();
    let abilities = *player
        .abilities
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let advancements = player
        .advancements
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let same_world = server
        .worlds
        .load()
        .iter()
        .any(|w| Arc::ptr_eq(w, world) && Arc::ptr_eq(w, &player.world()));
    format!(
        "{} same {same_world} pos {} {} {} yaw {} pitch {} mode {} previous {:?} op {:?} view {} played_before {} health {} abilities creative {} flying {} allow {} invulnerable {} build {} | advancements changed {} owner {}",
        world.dimension.minecraft_name,
        pos.x,
        pos.y,
        pos.z,
        entity.yaw.load(),
        entity.pitch.load(),
        player.gamemode.load() as i32,
        player.previous_gamemode.load().map(|m| m as i32),
        player.permission_lvl.load(),
        player.config.load().view_distance,
        player.has_played_before.load(Ordering::Relaxed),
        player.living_entity.health.load(),
        abilities.creative,
        abilities.flying,
        abilities.allow_flying,
        abilities.invulnerable,
        abilities.allow_modify_world,
        advancements.progress_changed.len(),
        advancements
            .player
            .upgrade()
            .is_some_and(|owner| std::ptr::eq(&raw const *owner, &raw const *player)),
    )
}

async fn call(
    server: &Arc<Server>,
    home: &Arc<World>,
    who: &Identity,
    scene: &Scene,
    skin_url: Option<&str>,
) -> Result<String, String> {
    let (donor, tap) =
        test_support::java_player_as(home, GameMode::Survival, who).ok_or("no packet tap")?;
    let drained = Drained::start(tap);
    let mut hub = server.management_hub.subscribe();
    let client = Arc::clone(&donor.client);
    let returned = server.add_player(
        client.clone(),
        profile(skin_url, scene.skin == SkinHost::Served { slim: true }),
        config_of(scene),
    );
    let mut line = match &returned {
        Some((player, world)) => format!("player {}", player_state(player, world, server)),
        None => "None".to_string(),
    };
    for _ in 0..50 {
        if client.closed() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        if returned.is_some() {
            break;
        }
    }
    let _ = write!(line, " | closed {}", client.closed());
    if let (Some((player, _)), Some(url)) = (&returned, skin_url) {
        line.push_str(&skin_state(player, url).await);
    }
    let mut notices = Vec::new();
    while let Ok(notice) = hub.try_recv() {
        notices.push(notice);
    }
    let _ = write!(
        line,
        " | hub {notices:?} | packets [{}]",
        drained.take().await
    );
    drained.finish().await;
    Ok(line)
}

/// The Bedrock skin once the skin task is done (it never is for an unreachable url).
async fn skin_state(player: &crate::entity::player::Player, url: &str) -> String {
    for _ in 0..100 {
        if player.bedrock_skin.load().skin_id == url {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let skin = player.bedrock_skin.load();
    format!(
        " | skin from url {} full {} size {}x{} arm {} data {} bytes starting {:?}",
        skin.skin_id == url,
        skin.full_id == url,
        skin.image_width,
        skin.image_height,
        skin.arm_size,
        skin.skin_data.len(),
        skin.skin_data.first(),
    )
}

async fn run_scene(scene: &Scene) -> Result<String, Box<dyn std::error::Error>> {
    let folder = tempfile::tempdir()?;
    let server = start(&folder, scene).await?;
    let mut text = format!("{}: ", scene.name);
    let home = server
        .worlds
        .load()
        .first()
        .cloned()
        .ok_or("a new server has no world")?;
    if scene.no_worlds {
        server.worlds.store(Arc::new(Vec::new()));
    }
    let url = match scene.skin {
        SkinHost::Nothing => None,
        SkinHost::Served { .. } => Some(serve_skin().await?),
        SkinHost::Unreachable => Some(dead_url().await?),
    };
    let url = url.as_deref();
    text.push_str(&call(&server, &home, &JOINER, scene, url).await?);
    if scene.twice {
        text.push_str(" || second: ");
        text.push_str(&call(&server, &home, &SECOND, scene, url).await?);
    }
    let _ = write!(text, " || server {}", server_state(&server));
    server.stop();
    Ok(text)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn add_player_golden() -> TestResult {
    let mut report = String::new();
    for scene in scenes() {
        report.push_str(&run_scene(&scene).await?);
        report.push('\n');
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/add_player.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
