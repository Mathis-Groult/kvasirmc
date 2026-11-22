//! A record of what `DyingPhase::tick` does, made before it is split
//! (`tests/golden/dying_tick.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene is a dragon that is not in the world, ticked by calling the phase directly until
//! it removes itself. Its fight origin is a fresh column in the End (or the overworld, which has
//! no dragon fight), with a watcher beside the dragon. After every tick the dragon's death
//! time, health, yaw, velocity, target, removal and the packets the watcher got are hashed; the
//! hash of every 10 ticks and every tick with a sound, an explosion particle or new orbs are
//! reported. The particle position is random, so only its ranges are reported. The amount of
//! each orb is read by letting the watcher pick it up.

use std::fmt::Write;
use std::num::NonZero;
use std::sync::Arc;

use pumpkin_data::Block;
use pumpkin_data::dimension::Dimension;
use pumpkin_data::entity::EntityType;
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use super::{DyingPhase, Phase};
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::boss::ender_dragon::EnderDragonEntity;
use crate::entity::{Entity, EntityBase};
use crate::net::java::PacketTap;
use crate::world::World;
use crate::world::dragon_fight::DragonFight;

type Vec3 = (f64, f64, f64);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Realm {
    End,
    Overworld,
}

struct Scene {
    name: &'static str,
    realm: Realm,
    /// Top of a stone column at the fight origin (blocks from y 60 up to it).
    column: Option<i32>,
    /// Where the dragon is, from the fight origin.
    dragon: Vec3,
    yaw: f32,
    /// A target the dragon already has, from the fight origin.
    preset: Option<Vec3>,
    killed_before: bool,
    /// The fight tracks another dragon.
    foreign: bool,
    /// The death time the phase starts from (before the first tick).
    start: i32,
}

const fn scene(name: &'static str, dragon: Vec3) -> Scene {
    Scene {
        name,
        realm: Realm::End,
        column: None,
        dragon,
        yaw: 0.0,
        preset: None,
        killed_before: false,
        foreign: false,
        start: 0,
    }
}

fn scenes() -> Vec<Scene> {
    let mut all = fight_scenes();
    all.extend(distance_scenes());
    all.extend(other_scenes());
    all
}

/// The whole death with and without a previously killed dragon, over a column of stone.
fn fight_scenes() -> Vec<Scene> {
    vec![
        Scene {
            column: Some(70),
            yaw: 40.0,
            ..scene("first_kill", (12.5, 75.0, -9.5))
        },
        Scene {
            column: Some(80),
            yaw: -120.0,
            killed_before: true,
            ..scene("second_kill", (-30.5, 90.0, 20.5))
        },
        Scene {
            realm: Realm::Overworld,
            column: Some(70),
            yaw: 15.0,
            ..scene("no_fight", (12.5, 75.0, -9.5))
        },
    ]
}

/// The steering band is 10 to 150 blocks from the podium, ends included.
fn distance_scenes() -> Vec<Scene> {
    vec![
        scene("empty_origin_near", (3.5, 65.0, 0.5)),
        scene("just_under_10", (10.0, 65.0, 0.5)),
        Scene {
            yaw: 30.0,
            ..scene("exactly_10", (10.5, 65.0, 0.5))
        },
        Scene {
            yaw: -75.0,
            ..scene("exactly_150", (150.5, 65.0, 0.5))
        },
        scene("just_over_150", (150.6, 65.0, 0.5)),
        Scene {
            column: Some(66),
            yaw: 200.0,
            ..scene("vertical", (0.5, 120.0, 0.5))
        },
        Scene {
            column: Some(66),
            yaw: 100.0,
            ..scene("steep_climb", (1.5, 120.0, 0.5))
        },
    ]
}

fn other_scenes() -> Vec<Scene> {
    vec![
        Scene {
            column: Some(70),
            preset: Some((0.5, 100.0, 0.5)),
            yaw: 10.0,
            ..scene("preset_target", (50.5, 65.0, 0.5))
        },
        Scene {
            foreign: true,
            column: Some(72),
            ..scene("foreign_fight", (12.5, 75.0, 9.5))
        },
        Scene {
            start: 195,
            killed_before: true,
            column: Some(72),
            ..scene("late_start", (12.5, 75.0, 9.5))
        },
        Scene {
            start: 200,
            column: Some(72),
            ..scene("past_the_end", (12.5, 75.0, 9.5))
        },
    ]
}

const SOUND: u8 = 119;
const PARTICLE: u8 = 48;

fn fnv(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

const FNV_START: u64 = 0xcbf2_9ce4_8422_2325;

/// The float that ends a sound packet before its 8-byte seed, `back` bytes from the end.
fn sound_float(packet: &[u8], back: usize) -> f32 {
    let at = packet.len().saturating_sub(back);
    packet
        .get(at..at + 4)
        .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
        .map_or(f32::NAN, f32::from_be_bytes)
}

/// The position of a particle packet (after the id, the particle id and two flags).
fn particle_pos(packet: &[u8]) -> Option<Vec3> {
    let at = |from: usize| {
        packet
            .get(from..from + 8)
            .and_then(|bytes| <[u8; 8]>::try_from(bytes).ok())
            .map(f64::from_be_bytes)
    };
    Some((at(4)?, at(12)?, at(20)?))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// A packet by id and length; a sound by its volume and pitch, a particle by everything after
/// its random position. Other packets name entities, whose ids are not fixed: by id only.
fn packet_text(packet: &[u8]) -> String {
    match packet.first().copied().unwrap_or(0) {
        SOUND => format!(
            "sound:{} volume {} pitch one {}",
            packet.len(),
            sound_float(packet, 16),
            sound_float(packet, 12) == 1.0
        ),
        PARTICLE => format!(
            "particle:{} tail {}",
            packet.len(),
            hex(packet.get(27..).unwrap_or_default())
        ),
        id => format!("packet {id}"),
    }
}

/// The ranges of the explosion particles around the dragon, over a whole scene.
#[derive(Default)]
struct Spread {
    samples: usize,
    min: Option<Vec3>,
    max: Option<Vec3>,
}

impl Spread {
    fn add(&mut self, (x, y, z): Vec3) {
        self.samples += 1;
        let (lo, hi) = (self.min.unwrap_or((x, y, z)), self.max.unwrap_or((x, y, z)));
        self.min = Some((lo.0.min(x), lo.1.min(y), lo.2.min(z)));
        self.max = Some((hi.0.max(x), hi.1.max(y), hi.2.max(z)));
    }

    /// Whether the samples lie in `[from, to]` and cover over `wide` of it, per axis.
    fn text(&self) -> String {
        let (Some(lo), Some(hi)) = (self.min, self.max) else {
            return "no particles".to_string();
        };
        // Few samples may leave a range narrow by chance: judge the spread from 20 on.
        let judged = self.samples >= 20;
        let axis = |lo: f64, hi: f64, from: f64, to: f64, wide: f64| {
            let spread = if judged {
                (hi - lo > wide).to_string()
            } else {
                "unjudged".to_string()
            };
            format!("{} {spread}", lo >= from && hi <= to)
        };
        format!(
            "{} particles; x in [-4, 4] and wider than 5: {}; y in [0, 4] and wider than 2.5: {}; z in [-4, 4] and wider than 5: {}",
            self.samples,
            axis(lo.0, hi.0, -4.0, 4.0, 5.0),
            axis(lo.1, hi.1, 0.0, 4.0, 2.5),
            axis(lo.2, hi.2, -4.0, 4.0, 5.0)
        )
    }
}

fn effects_text(dragon: &EnderDragonEntity) -> String {
    let entity = &dragon.mob_entity.living_entity.entity;
    let pos = entity.pos.load();
    let vel = entity.velocity.load();
    let death = *dragon
        .dragon_death_time
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let target = *dragon
        .target_location
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let accel = *dragon
        .yaw_rot_accel
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let parts = dragon
        .parts
        .iter()
        .filter(|part| part.entity.is_removed())
        .count();
    format!(
        "death {death} health {:.2} yaw {:.4} accel {accel:.4} pos {:.4},{:.4},{:.4} vel {:.4},{:.4},{:.4} target {target:?} removed {} parts removed {parts}",
        dragon.mob_entity.living_entity.health.load(),
        entity.yaw.load(),
        pos.x,
        pos.y,
        pos.z,
        vel.x,
        vel.y,
        vel.z,
        entity.is_removed()
    )
}

fn orb_count(world: &World) -> usize {
    world
        .entities
        .load()
        .iter()
        .filter(|entity| entity.get_entity().entity_type == &EntityType::EXPERIENCE_ORB)
        .count()
}

struct Ctx<'a> {
    world: &'a Arc<World>,
    scene: &'a Scene,
    origin: BlockPos,
}

/// Ticks the dragon until it removes itself (the phase ends it by tick 200 at the latest).
fn fly(
    ctx: &Ctx<'_>,
    dragon: &Arc<EnderDragonEntity>,
    tap: &mut PacketTap,
    report: &mut String,
) -> Spread {
    let name = ctx.scene.name;
    let mut hash = FNV_START;
    let mut spread = Spread::default();
    let mut orbs = 0;
    let pos = dragon.mob_entity.living_entity.entity.pos.load();
    for tick in 1..=260 {
        if dragon.mob_entity.living_entity.entity.is_removed() {
            let _ = writeln!(report, "{name} removed after tick {}", tick - 1);
            break;
        }
        DyingPhase.tick(dragon);
        let mut text = effects_text(dragon);
        let mut events = Vec::new();
        for packet in tap.drain() {
            if packet.first() == Some(&PARTICLE)
                && let Some((x, y, z)) = particle_pos(&packet)
            {
                spread.add((x - pos.x, y - pos.y, z - pos.z));
            }
            events.push(packet_text(&packet));
        }
        let now = orb_count(ctx.world);
        let _ = write!(text, " orbs {now} sent [{}]", events.join(" "));
        hash = fnv(hash, text.as_bytes());
        let listed: Vec<&String> = events.iter().filter(|e| !e.starts_with("packet")).collect();
        if now != orbs || !listed.is_empty() {
            let _ = writeln!(report, "{name} tick {tick}: orbs {now} {listed:?}");
        }
        orbs = now;
        if tick % 10 == 0 {
            let _ = writeln!(report, "{name} ticks to {tick}: {hash:016x}");
        }
    }
    let _ = writeln!(report, "{name} state: {}", effects_text(dragon));
    let _ = writeln!(report, "{name} hash: {hash:016x}");
    spread
}

/// Each orb's amount, by letting the watcher pick them up one by one.
fn orb_amounts(world: &World, watcher: &Arc<crate::entity::player::Player>) -> Vec<i32> {
    let orbs: Vec<_> = world
        .entities
        .load()
        .iter()
        .filter(|entity| entity.get_entity().entity_type == &EntityType::EXPERIENCE_ORB)
        .cloned()
        .collect();
    let mut amounts = Vec::new();
    for orb in orbs {
        *watcher
            .experience_pick_up_delay
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = 0;
        let before = watcher
            .xp_state
            .total
            .load(std::sync::atomic::Ordering::Relaxed);
        orb.on_player_collision(watcher);
        amounts.push(
            watcher
                .xp_state
                .total
                .load(std::sync::atomic::Ordering::Relaxed)
                - before,
        );
    }
    amounts.sort_unstable();
    amounts
}

/// The blocks of a box, by name, hashed.
fn box_hash(world: &World, from: BlockPos, to: BlockPos) -> String {
    let mut hash = FNV_START;
    for x in from.0.x..=to.0.x {
        for y in from.0.y..=to.0.y {
            for z in from.0.z..=to.0.z {
                let block = world.get_block(&BlockPos::new(x, y, z));
                hash = fnv(hash, block.name.as_bytes());
            }
        }
    }
    format!("{hash:016x}")
}

/// What the death left in the fight and the world around the origin.
fn aftermath(ctx: &Ctx<'_>, report: &mut String) {
    let name = ctx.scene.name;
    let Some(fight) = &ctx.world.dragon_fight else {
        let _ = writeln!(report, "{name} fight: none");
        return;
    };
    let fight = fight
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let portal = fight.exit_portal_location;
    let _ = writeln!(
        report,
        "{name} fight: killed {} previously {} uuid {:?} portal {:?} gateways {:?}",
        fight.dragon_killed,
        fight.previously_killed,
        fight.dragon_uuid,
        portal.map(|p| p.0.y),
        fight.gateways
    );
    let origin = ctx.origin;
    let y = portal.map_or(65, |p| p.0.y);
    let _ = writeln!(
        report,
        "{name} podium {} egg {} gateway {}",
        box_hash(
            ctx.world,
            BlockPos::new(origin.0.x - 4, y - 2, origin.0.z - 4),
            BlockPos::new(origin.0.x + 4, y + 6, origin.0.z + 4)
        ),
        ctx.world
            .get_block(&BlockPos::new(origin.0.x, y + 4, origin.0.z))
            .name,
        box_hash(
            ctx.world,
            BlockPos::new(origin.0.x - 2, 73, origin.0.z + 95),
            BlockPos::new(origin.0.x, 77, origin.0.z + 97)
        )
    );
}

fn load_chunks(world: &World, around: BlockPos) {
    let (cx, cz) = (around.0.x >> 4, around.0.z >> 4);
    for x in cx - 1..=cx + 1 {
        for z in cz - 1..=cz + 1 {
            world.level.loaded_chunks.insert(
                Vector2::new(x, z),
                pumpkin_world::chunk::ChunkData::empty_sync(x, z),
            );
        }
    }
}

fn unload_chunks(world: &World, around: BlockPos) {
    let (cx, cz) = (around.0.x >> 4, around.0.z >> 4);
    for x in cx - 1..=cx + 1 {
        for z in cz - 1..=cz + 1 {
            world.level.loaded_chunks.remove(&Vector2::new(x, z));
        }
    }
}

/// The world's fight starts again at the scene's origin, with one known gateway to place.
fn reset_fight(world: &World, scene: &Scene, origin: BlockPos) {
    let Some(fight) = &world.dragon_fight else {
        return;
    };
    let mut fight = fight
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *fight = DragonFight::new_with_seed(7, origin);
    fight.gateways = vec![5];
    fight.previously_killed = scene.killed_before;
    if scene.foreign {
        fight.dragon_uuid = Some(Uuid::from_u128(0xf0e1_0000));
    }
}

fn make_dragon(ctx: &Ctx<'_>, index: usize) -> Arc<EnderDragonEntity> {
    let origin = ctx.origin;
    let at = Vector3::new(
        f64::from(origin.0.x) + ctx.scene.dragon.0,
        ctx.scene.dragon.1,
        f64::from(origin.0.z) + ctx.scene.dragon.2,
    );
    let entity = Entity::from_uuid_with_id(
        70_000 + index as i32 * 10,
        Uuid::from_u128(0x4452_4147_4f4e_0000 + index as u128),
        Arc::clone(ctx.world),
        at,
        &EntityType::ENDER_DRAGON,
    );
    entity.yaw.store(ctx.scene.yaw);
    let dragon = EnderDragonEntity::new(entity);
    *dragon
        .fight_origin
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = origin;
    DyingPhase.begin(&dragon);
    *dragon
        .dragon_death_time
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = ctx.scene.start;
    if let Some((x, y, z)) = ctx.scene.preset {
        *dragon
            .target_location
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Vector3::new(
            f64::from(origin.0.x) + x,
            y,
            f64::from(origin.0.z) + z,
        ));
    }
    dragon
}

async fn run_scene(
    worlds: (&Arc<World>, &Arc<World>),
    index: usize,
    scene: &Scene,
    report: &mut String,
) -> TestResult {
    let world = if scene.realm == Realm::End {
        worlds.0
    } else {
        worlds.1
    };
    let origin = BlockPos::new(index as i32 * 160 + 32, 0, index as i32 * 8 + 24);
    let ctx = Ctx {
        world,
        scene,
        origin,
    };
    let gateway = BlockPos::new(origin.0.x - 1, 75, origin.0.z + 96);
    load_chunks(world, origin);
    load_chunks(world, gateway);
    if let Some(top) = scene.column {
        for y in 60..=top {
            world.set_block_state(
                &BlockPos::new(origin.0.x, y, origin.0.z),
                Block::STONE.default_state.id,
                BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
            );
        }
    }
    reset_fight(world, scene, origin);

    let who = Identity {
        name: "DragonWatch",
        uuid: Uuid::from_u128(0x4472_6167_6f6e_0000 + index as u128),
        entity_id: 60_000 + index as i32,
    };
    let (watcher, mut tap) = test_support::java_player_as(world, GameMode::Survival, &who)
        .ok_or("the watcher's client no longer owned its outgoing queue")?;
    let dragon = make_dragon(&ctx, index);
    let at = dragon.mob_entity.living_entity.entity.pos.load();
    watcher
        .get_entity()
        .set_pos(Vector3::new(at.x + 1.0, at.y, at.z + 1.0));
    watcher.watched_section.store(Cylindrical::new(
        Vector2::new((at.x as i32) >> 4, (at.z as i32) >> 4),
        NonZero::<u8>::MIN.saturating_add(3),
    ));
    world.add_player(&watcher)?;
    drop(tap.drain());

    let spread = fly(&ctx, &dragon, &mut tap, report);
    let _ = writeln!(report, "{} {}", scene.name, spread.text());
    let _ = writeln!(
        report,
        "{} orb amounts {:?}",
        scene.name,
        orb_amounts(world, &watcher)
    );
    aftermath(&ctx, report);

    for entity in world.entities.load().iter() {
        world.remove_entity(entity.as_ref());
    }
    world.remove_player(&watcher, false).await;
    unload_chunks(world, origin);
    unload_chunks(world, gateway);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn dying_tick() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let end = server.get_world_from_dimension(&Dimension::THE_END);
    let overworld = server.get_world_from_dimension(&Dimension::OVERWORLD);
    let mut report = String::new();
    for (index, scene) in scenes().iter().enumerate() {
        run_scene((&end, &overworld), index, scene, &mut report).await?;
    }
    server.stop();
    script_golden::bless_or_compare("dying_tick.txt", &report)
}
