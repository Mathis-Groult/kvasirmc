//! A record of what `MobSpawnerBlockEntity::tick` does, made before the function was split
//! (`tests/golden/mob_spawner_tick.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene builds a spawner block entity with preset settings over an empty, loaded area of
//! a real server's world, places a listener player (in range or not), lays stone around the
//! spawner or mobs near it, and ticks the spawner a few times. A line per tick: the delay, the
//! mobs spawned so far (type, block offset from the spawner, yaw within 0..360) and the world
//! and block event packets the listener got (the queued block event is flushed with a spawner
//! block put at the position only for the flush, so it never blocks a spawn).
//!
//! The spawn position and yaw are random (unseeded `rand`): single spawns are recorded only as
//! "inside the vanilla box" checks, and one scene spawns 600 mobs to record the mean and spread
//! of the offsets in coarse classes far from their edges. A delay reset is recorded as inside
//! `min..max` (or the exact value when `max <= min`).

use std::fmt::Write;
use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use crossbeam::atomic::AtomicCell;
use pumpkin_data::entity::EntityType;
use pumpkin_data::packet::CURRENT_MC_VERSION;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_protocol::java::client::play::{CBlockEvent, CWorldEvent};
use pumpkin_protocol::packet::MultiVersionJavaPacket;
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use pumpkin_world::world::BlockFlags;

use super::MobSpawnerBlockEntity;
use crate::block::entities::BlockEntity;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;
use crate::entity::living::LivingEntity;
use crate::entity::player::Player;
use crate::entity::{Entity, EntityBase};
use crate::net::java::PacketTap;
use crate::world::World;

const SPACING: i32 = 48;

struct Scene {
    name: &'static str,
    entity: Option<&'static EntityType>,
    delay: i32,
    min_delay: i32,
    max_delay: i32,
    spawn_count: i32,
    spawn_range: i32,
    max_nearby: i32,
    player_range: i32,
    /// Listener offset from the spawner block's centre.
    player: (f64, f64, f64),
    /// Mobs already there: type and offset of their feet from the spawner block's centre.
    crowd: &'static [(&'static EntityType, f64, f64, f64)],
    stone: bool,
    ticks: usize,
    stats: bool,
    /// Adds the mean of the yaw, the pitch and the follow-range spawn bonus to the stats line.
    angles: bool,
    /// Resets the delay this many times (delay -1, one tick each) and records their spread.
    resets: usize,
}

const fn scene(name: &'static str, delay: i32) -> Scene {
    Scene {
        name,
        entity: Some(&EntityType::ZOMBIE),
        delay,
        min_delay: 200,
        max_delay: 800,
        spawn_count: 4,
        spawn_range: 4,
        max_nearby: 6,
        player_range: 16,
        player: (0.0, 0.0, 3.0),
        crowd: &[],
        stone: false,
        ticks: 3,
        stats: false,
        angles: false,
        resets: 0,
    }
}

const fn player_at(mut scene: Scene, x: f64, y: f64, z: f64) -> Scene {
    scene.player = (x, y, z);
    scene
}

const fn delays(mut scene: Scene, min: i32, max: i32) -> Scene {
    scene.min_delay = min;
    scene.max_delay = max;
    scene
}

const fn spawning(mut scene: Scene, count: i32, range: i32, max_nearby: i32) -> Scene {
    scene.spawn_count = count;
    scene.spawn_range = range;
    scene.max_nearby = max_nearby;
    scene
}

const fn crowd(mut scene: Scene, mobs: &'static [(&'static EntityType, f64, f64, f64)]) -> Scene {
    scene.crowd = mobs;
    scene
}

const fn walled(mut scene: Scene) -> Scene {
    scene.stone = true;
    scene
}

const fn without_entity(mut scene: Scene) -> Scene {
    scene.entity = None;
    scene
}

const fn with_angles(mut scene: Scene) -> Scene {
    scene.angles = true;
    scene
}

const fn many_resets(mut scene: Scene, resets: usize) -> Scene {
    scene.resets = resets;
    scene.ticks = 0;
    scene
}

const fn statistics(mut scene: Scene) -> Scene {
    scene.stats = true;
    scene.ticks = 1;
    scene
}

const Z: &EntityType = &EntityType::ZOMBIE;
const S: &EntityType = &EntityType::SKELETON;
const SIX_NEAR: &[(&EntityType, f64, f64, f64)] = &[
    (Z, 1.0, 0.0, 1.0),
    (Z, -1.0, 0.0, 1.0),
    (Z, 1.0, 0.0, -1.0),
    (Z, -1.0, 0.0, -1.0),
    (Z, 2.0, 0.0, 0.0),
    (Z, -2.0, 0.0, 0.0),
];
const FIVE_NEAR: &[(&EntityType, f64, f64, f64)] = &[
    (Z, 1.0, 0.0, 1.0),
    (Z, -1.0, 0.0, 1.0),
    (Z, 1.0, 0.0, -1.0),
    (Z, -1.0, 0.0, -1.0),
    (Z, 2.0, 0.0, 0.0),
];
const OTHER_TYPE: &[(&EntityType, f64, f64, f64)] = &[(S, 1.0, 0.0, 1.0), (S, -1.0, 0.0, 1.0)];
// Vanilla counts mobs whose box meets the spawner block grown by the spawn range (4 here) on all
// three axes. Offsets are from the block centre; a zombie is 0.6 wide and 1.95 tall, so its box
// meets the grown one up to 4.8 sideways, 4.5 above and 6.45 below (feet offset).
const EDGES_IN: &[(&EntityType, f64, f64, f64)] = &[
    (Z, 4.79, 0.0, 0.0),
    (Z, 0.0, 0.0, -4.79),
    (Z, 0.0, 4.49, 0.0),
    (Z, 0.0, -6.44, 0.0),
];
const EDGES_OUT: &[(&EntityType, f64, f64, f64)] = &[
    (Z, 4.81, 0.0, 0.0),
    (Z, -4.81, 0.0, 0.0),
    (Z, 0.0, 0.0, 4.81),
    (Z, 0.0, 0.0, -4.81),
    (Z, 0.0, 4.51, 0.0),
    (Z, 0.0, -6.46, 0.0),
];

const SCENES: &[Scene] = &[
    without_entity(scene("no entity type", 0)),
    player_at(scene("player out of range", 0), 0.0, 0.0, 16.01),
    player_at(scene("player at the range", 0), 0.0, 0.0, 16.0),
    player_at(scene("player above, in range", 0), 0.0, 15.9, 0.0),
    player_at(scene("player diagonal, out of range", 0), 11.4, 0.0, 11.4),
    scene("delay counting down", 2),
    scene("delay of one", 1),
    scene("delay below zero", -1),
    scene("delay far below zero", -7),
    delays(scene("delay below zero, max equal min", -1), 300, 300),
    delays(scene("delay below zero, max below min", -1), 300, 100),
    delays(scene("delay below zero, range of one", -1), 40, 41),
    scene("spawning in open air", 0),
    spawning(scene("spawning one", 0), 1, 4, 6),
    spawning(scene("spawning none", 0), 0, 4, 6),
    spawning(scene("spawning in range one", 0), 4, 1, 6),
    spawning(scene("spawning in range zero", 0), 3, 0, 6),
    walled(scene("walled in by stone", 0)),
    crowd(scene("six zombies near", 0), SIX_NEAR),
    crowd(scene("five zombies near", 0), FIVE_NEAR),
    crowd(
        spawning(scene("five near, cap five", 0), 4, 4, 5),
        FIVE_NEAR,
    ),
    crowd(
        spawning(scene("other mobs near, cap two", 0), 4, 4, 2),
        OTHER_TYPE,
    ),
    crowd(
        spawning(scene("zombies on the count edges, cap four", 0), 4, 4, 4),
        EDGES_IN,
    ),
    crowd(
        spawning(scene("zombies past the count edges, cap one", 0), 4, 4, 1),
        EDGES_OUT,
    ),
    crowd(
        spawning(scene("zombies at range two edges, cap one", 0), 4, 2, 1),
        &[(Z, 2.79, 0.0, 0.0)],
    ),
    crowd(
        spawning(scene("zombie past range two edges, cap one", 0), 4, 2, 1),
        &[(Z, 2.81, 0.0, 0.0)],
    ),
    statistics(spawning(scene("600 spawns", 0), 600, 4, 1000)),
    statistics(spawning(scene("600 spawns in range two", 0), 600, 2, 1000)),
    // Appended after the first mutation run: the spread of reset delays, and the yaw, pitch
    // and spawn finalisation (follow-range bonus) of spawned mobs.
    many_resets(scene("fifty resets", -1), 50),
    with_angles(statistics(spawning(
        scene("600 spawns, angles", 0),
        600,
        4,
        1000,
    ))),
];

fn put(world: &Arc<World>, pos: &BlockPos, state: BlockStateId) {
    world.set_block_state(
        pos,
        state,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

fn spawner(position: BlockPos, scene: &Scene) -> MobSpawnerBlockEntity {
    MobSpawnerBlockEntity {
        position,
        delay: AtomicI32::new(scene.delay),
        max_delay: scene.max_delay,
        min_delay: scene.min_delay,
        spawn_count: scene.spawn_count,
        spawn_range: scene.spawn_range,
        max_nearby_entities: scene.max_nearby,
        required_player_range: scene.player_range,
        entity_type: AtomicCell::new(scene.entity),
    }
}

fn lay(world: &Arc<World>, pos: BlockPos, scene: &Scene) {
    for dy in -1..=3 {
        for dz in -6..=6 {
            for dx in -6..=6 {
                let at = BlockPos::new(pos.0.x + dx, pos.0.y + dy, pos.0.z + dz);
                let state = if scene.stone && (dx, dy, dz) != (0, 0, 0) {
                    Block::STONE.default_state.id
                } else {
                    Block::AIR.default_state.id
                };
                put(world, &at, state);
            }
        }
    }
    let centre = pos.to_centered_f64();
    for (entity_type, x, y, z) in scene.crowd {
        let at = Vector3::new(centre.x + x, centre.y + y, centre.z + z);
        let mob = LivingEntity::new(Entity::new(world.clone(), at, entity_type));
        world.spawn_entity(Arc::new(mob));
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

/// The world and block event packets queued for the listener, as hex.
fn events(tap: &mut PacketTap) -> String {
    let ids = [
        CWorldEvent::to_id(CURRENT_MC_VERSION),
        CBlockEvent::to_id(CURRENT_MC_VERSION),
    ];
    let mut runs: Vec<(String, usize)> = Vec::new();
    for packet in tap.drain() {
        let first = var_int(&packet, 0);
        let second = first.and_then(|(_, next)| var_int(&packet, next));
        if [first, second]
            .iter()
            .flatten()
            .any(|(value, _)| ids.contains(value))
        {
            let mut hex = String::new();
            for byte in &packet[..] {
                let _ = write!(hex, "{byte:02x}");
            }
            match runs.last_mut() {
                Some((last, count)) if *last == hex => *count += 1,
                _ => runs.push((hex, 1)),
            }
        }
    }
    let mut text = String::new();
    for (hex, count) in runs {
        let _ = write!(text, " packet {hex} x{count}");
    }
    text
}

/// The mobs spawned so far: those not in the scene's crowd within its area.
fn spawned(world: &World, pos: BlockPos, crowd: usize) -> Vec<Arc<dyn EntityBase>> {
    let x0 = f64::from(pos.0.x - SPACING / 2);
    let mut all: Vec<Arc<dyn EntityBase>> = world
        .entities
        .load()
        .iter()
        .filter(|entity| {
            let x = entity.get_entity().pos.load().x;
            x >= x0 && x < x0 + f64::from(SPACING)
        })
        .cloned()
        .collect();
    all.sort_by_key(|entity| entity.get_entity().entity_id);
    all.split_off(crowd.min(all.len()))
}

/// One spawned mob as text: inside the box `[-range + 0.5, range + 0.5)` around the block's
/// corner horizontally and one of the three block layers vertically, or its raw offsets.
fn mob_text(mob: &dyn EntityBase, pos: BlockPos, range: i32) -> String {
    let entity = mob.get_entity();
    let at = entity.pos.load();
    let (dx, dy, dz) = (
        at.x - f64::from(pos.0.x),
        at.y - f64::from(pos.0.y),
        at.z - f64::from(pos.0.z),
    );
    let reach = f64::from(range);
    let inside = |d: f64| d >= 0.5 - reach && d <= 0.5 + reach;
    let layer = [-1.0, 0.0, 1.0].contains(&dy);
    let yaw = entity.yaw.load();
    let name = entity.entity_type.resource_name;
    if inside(dx) && inside(dz) && layer && (0.0..360.0).contains(&yaw) {
        name.to_string()
    } else {
        format!("{name} OUTSIDE ({dx:.3}, {dy:.3}, {dz:.3}) yaw {yaw:.1}")
    }
}

/// Mean and spread of many spawn offsets, in classes far from their edges.
fn stats_text(mobs: &[Arc<dyn EntityBase>], pos: BlockPos, range: i32) -> String {
    let n = mobs.len() as f64;
    let offsets: Vec<(f64, f64, f64)> = mobs
        .iter()
        .map(|mob| {
            let at = mob.get_entity().pos.load();
            (
                at.x - f64::from(pos.0.x),
                at.y - f64::from(pos.0.y),
                at.z - f64::from(pos.0.z),
            )
        })
        .collect();
    let class = |values: Vec<f64>| {
        let mean = values.iter().sum::<f64>() / n;
        let sd = (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt();
        let expected_sd = f64::from(range) / 6f64.sqrt();
        format!(
            "mean 0.5{:+} sd {}",
            (mean - 0.5).round() as i64,
            if (sd - expected_sd).abs() < 0.25 * expected_sd.max(0.1) {
                "as expected".to_string()
            } else {
                format!("{sd:.2}")
            }
        )
    };
    let layers: Vec<String> = [-1.0, 0.0, 1.0]
        .iter()
        .map(|layer| {
            let count = offsets.iter().filter(|o| o.1 == *layer).count();
            if count as f64 > n / 6.0 {
                "many"
            } else {
                "few"
            }
            .to_string()
        })
        .collect();
    format!(
        "{} mobs, x {}, z {}, layers {layers:?}",
        mobs.len(),
        class(offsets.iter().map(|o| o.0).collect()),
        class(offsets.iter().map(|o| o.2).collect()),
    )
}

/// The delay after a tick: exact when it stayed or counted down from a known value, a random
/// reset recorded as inside `min..max` (exact when `max <= min`, the only deterministic reset),
/// and later values as the count since that reset (`reset-N`).
fn delay_text(delay: i32, scene: &Scene, before: i32, reset: &mut Option<i32>) -> String {
    let counted = delay == before || delay == before - 1;
    if counted {
        return reset.map_or_else(
            || delay.to_string(),
            |base| format!("reset-{}", base - delay),
        );
    }
    if scene.max_delay <= scene.min_delay {
        *reset = None;
        return delay.to_string();
    }
    *reset = Some(delay);
    if (scene.min_delay..scene.max_delay).contains(&delay) {
        "reset in min..max".to_string()
    } else {
        format!("reset OUTSIDE {delay}")
    }
}

fn run(
    out: &mut String,
    world: &Arc<World>,
    listener: &mut (Arc<Player>, PacketTap),
    index: usize,
    scene: &Scene,
) {
    let pos = BlockPos::new(index as i32 * SPACING + SPACING / 2, 64, 8);
    lay(world, pos, scene);
    let centre = pos.to_centered_f64();
    let (px, py, pz) = scene.player;
    let (player, tap) = listener;
    player
        .get_entity()
        .set_pos(Vector3::new(centre.x + px, centre.y + py, centre.z + pz));
    player.watched_section.store(Cylindrical::new(
        pos.chunk_position(),
        NonZero::<u8>::MIN.saturating_add(1),
    ));
    drop(events(tap));
    let entity = Arc::new(spawner(pos, scene));
    let mut reset = None;
    for tick in 1..=scene.ticks {
        let before = entity.delay.load(Ordering::Relaxed);
        entity.tick(world);
        let delay = entity.delay.load(Ordering::Relaxed);
        put(world, &pos, Block::SPAWNER.default_state.id);
        world.flush_synced_block_events();
        put(world, &pos, Block::AIR.default_state.id);
        let mobs = spawned(world, pos, scene.crowd.len());
        let mobs_text = if scene.stats {
            let mut text = stats_text(&mobs, pos, scene.spawn_range);
            if scene.angles {
                text.push_str(&angles_text(&mobs));
            }
            text
        } else {
            let list: Vec<String> = mobs
                .iter()
                .map(|mob| mob_text(mob.as_ref(), pos, scene.spawn_range))
                .collect();
            format!("{list:?}")
        };
        let _ = writeln!(
            out,
            "{}: tick {tick} delay {} spawned {mobs_text}{}",
            scene.name,
            delay_text(delay, scene, before, &mut reset),
            events(tap)
        );
    }
    if scene.resets > 0 {
        let _ = writeln!(
            out,
            "{}: {}",
            scene.name,
            resets_text(&entity, world, scene)
        );
    }
}

/// The spread of many reset delays: all inside `min..max`, and wider than half of it.
fn resets_text(entity: &MobSpawnerBlockEntity, world: &Arc<World>, scene: &Scene) -> String {
    let mut delays = Vec::new();
    for _ in 0..scene.resets {
        entity.delay.store(-1, Ordering::Relaxed);
        entity.tick(world);
        delays.push(entity.delay.load(Ordering::Relaxed));
    }
    let inside = delays
        .iter()
        .all(|delay| (scene.min_delay..scene.max_delay).contains(delay));
    let low = delays.iter().min().copied().unwrap_or_default();
    let high = delays.iter().max().copied().unwrap_or_default();
    let wide = high - low > (scene.max_delay - scene.min_delay) / 2;
    format!("{} resets, inside {inside}, wide {wide}", delays.len())
}

/// Yaw mean (in quarter turns) and spread, pitches, and the follow-range spawn bonus.
fn angles_text(mobs: &[Arc<dyn EntityBase>]) -> String {
    let n = mobs.len() as f64;
    let yaws: Vec<f64> = mobs
        .iter()
        .map(|mob| f64::from(mob.get_entity().yaw.load()))
        .collect();
    let mean = yaws.iter().sum::<f64>() / n;
    let sd = (yaws.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt();
    let level = mobs.iter().all(|mob| mob.get_entity().pitch.load() == 0.0);
    let bonus = mobs.iter().all(|mob| {
        mob.get_living_entity().is_some_and(|living| {
            living
                .attributes
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .values()
                .any(|attribute| {
                    attribute
                        .modifiers
                        .iter()
                        .any(|modifier| modifier.id == "minecraft:random_spawn_bonus")
                })
        })
    });
    format!(
        ", yaw mean {} quarter turns, yaw spread uniform {}, pitch zero {level}, spawn bonus {bonus}",
        (mean / 90.0).round() as i64,
        // A uniform yaw over 0..360 has a standard deviation of 360 / sqrt(12) = 103.9.
        (sd - 103.9).abs() < 15.0,
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn mob_spawner_tick_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let chunks = (SCENES.len() as i32 * SPACING) / 16 + 1;
    for cx in -1..=chunks {
        for cz in -1..=1 {
            world.level.loaded_chunks.insert(
                Vector2::new(cx, cz),
                pumpkin_world::chunk::ChunkData::empty_sync(cx, cz),
            );
        }
    }
    let (player, tap) =
        test_support::java_player_with_tap(&world, GameMode::Survival).ok_or("no packet tap")?;
    world.add_player(&player)?;
    let mut listener = (player, tap);
    let mut out = String::new();
    for (index, scene) in SCENES.iter().enumerate() {
        run(&mut out, &world, &mut listener, index, scene);
    }
    world.remove_player(&listener.0, false).await;
    server.stop();
    script_golden::bless_or_compare("mob_spawner_tick.txt", &out)
}
