//! A record of what `RangedCrossbowAttackGoal::tick` does, made before the function was split
//! (`tests/golden/ranged_crossbow_tick.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene puts a fresh mob (a pillager, or a skeleton that is no crossbow mob) on a real
//! server's world with a target, optionally behind a stone wall, gives the goal a chosen state
//! (crossbow state, see time, attack delay, path delay, charge ticks), the navigator a running
//! path of speed 9 and the mob a crossbow, an empty hand or a locked equipment, then ticks the
//! goal. One line per tick: the goal's fields, the navigator (idle or its speed and target
//! block after its own tick), the head yaw and pitch after the look control's tick, the active
//! item, the crossbow flag, the arrows in the world and the packets a watcher got.
//! The two random draws (`20 + 0..20`: path delay and attack delay) are replaced by a fixed
//! value after each tick and shown as `D`; the end of the file reports the lowest and highest
//! draw over many goals instead.

use std::fmt::Write;
use std::num::NonZero;
use std::sync::Arc;

use pumpkin_data::Block;
use pumpkin_data::data_component_impl::EquipmentSlot;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use super::{CrossbowState, RangedCrossbowAttackGoal};
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::goal::Goal;
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::living::LivingEntity;
use crate::entity::mob::Mob;
use crate::entity::mob::pillager::PillagerEntity;
use crate::entity::mob::skeleton::skeleton::SkeletonEntity;
use crate::entity::player::Player;
use crate::entity::{Entity, EntityBase};
use crate::net::java::PacketTap;
use crate::world::World;

const FLOOR_Y: i32 = 63;
/// The mob stands here (scene-relative).
const MOB_AT: Vec3 = (12.5, 64.0, 12.5);
const SPEED: f64 = 1.25;
const RANGE: f32 = 7.0;
/// The navigator's speed before each scene, so a goal that sets a path shows another speed.
const BEFORE_SPEED: f64 = 9.0;
/// Add Entity: its bytes hold a UUID. Hashed as a length only.
const ADD_ENTITY_PACKET: u8 = 1;
/// The world sound packet: its seed is random, so the last 8 bytes are cut before hashing.
const SOUND_PACKET: u8 = 119;
const DRAWS: usize = 400;

type Vec3 = (f64, f64, f64);

/// FNV-1a: a hash that does not change with the Rust version.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Pillager,
    /// Has no crossbow flag to set.
    Skeleton,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Weapon {
    Crossbow,
    Empty,
    /// A crossbow, but the equipment is locked while the goal ticks.
    Locked,
}

#[derive(Clone, Copy)]
struct Scene {
    name: &'static str,
    kind: Kind,
    /// Where the target stands (`None`: the mob has no target).
    target: Option<Vec3>,
    /// A stone wall between the mob and a target east of it.
    wall: bool,
    weapon: Weapon,
    state: CrossbowState,
    see_time: i32,
    attack_delay: i32,
    path_delay: i32,
    charge_ticks: i32,
    ticks: usize,
    /// Also tick `DRAWS` fresh goals once and report the lowest and highest random draw.
    draws: bool,
    /// What a drawn path delay becomes after the tick.
    path_redraw: i32,
    /// What a drawn attack delay becomes after the tick.
    attack_redraw: i32,
}

const NEAR: Vec3 = (16.5, 64.0, 12.5);
const FAR: Vec3 = (23.0, 64.0, 12.5);

const SCENE: Scene = Scene {
    name: "",
    kind: Kind::Pillager,
    target: Some(NEAR),
    wall: false,
    weapon: Weapon::Crossbow,
    state: CrossbowState::Uncharged,
    see_time: 0,
    attack_delay: 0,
    path_delay: 0,
    charge_ticks: 0,
    ticks: 6,
    draws: false,
    path_redraw: 4,
    attack_redraw: 3,
};

const fn scene(name: &'static str) -> Scene {
    Scene { name, ..SCENE }
}

const fn at(scene: Scene, target: Vec3) -> Scene {
    Scene {
        target: Some(target),
        ..scene
    }
}

const fn walled(scene: Scene) -> Scene {
    Scene {
        wall: true,
        ..scene
    }
}

const fn in_state(scene: Scene, state: CrossbowState) -> Scene {
    Scene { state, ..scene }
}

const fn seen_for(scene: Scene, see_time: i32) -> Scene {
    Scene { see_time, ..scene }
}

const fn delayed(scene: Scene, attack_delay: i32) -> Scene {
    Scene {
        attack_delay,
        ..scene
    }
}

const fn path_delayed(scene: Scene, path_delay: i32) -> Scene {
    Scene {
        path_delay,
        ..scene
    }
}

const fn charged_for(scene: Scene, charge_ticks: i32) -> Scene {
    Scene {
        charge_ticks,
        ..scene
    }
}

const fn armed_with(scene: Scene, weapon: Weapon) -> Scene {
    Scene { weapon, ..scene }
}

const fn running(scene: Scene, ticks: usize) -> Scene {
    Scene { ticks, ..scene }
}

const fn drawing(scene: Scene) -> Scene {
    Scene {
        draws: true,
        ..scene
    }
}

const fn skeleton(scene: Scene) -> Scene {
    Scene {
        kind: Kind::Skeleton,
        ..scene
    }
}

const fn targetless(scene: Scene) -> Scene {
    Scene {
        target: None,
        ..scene
    }
}

const CHARGING: CrossbowState = CrossbowState::Charging;
const CHARGED: CrossbowState = CrossbowState::Charged;
const READY: CrossbowState = CrossbowState::ReadyToAttack;

/// A target off the mob's heading and above it, so the look limits show.
const ABOVE: Vec3 = (15.5, 67.0, 15.5);

const SCENES: &[Scene] = &[
    targetless(scene("no_target")),
    targetless(in_state(scene("no_target_charging"), CHARGING)),
    // A target in view and in range, from a fresh goal: it walks to it for four ticks (see
    // time under five), then charges, waits, shoots and starts over.
    running(scene("near_seen_fresh"), 90),
    running(seen_for(scene("near_seen_charge"), 10), 90),
    running(skeleton(seen_for(scene("skeleton_charge"), 10)), 60),
    // The see-time threshold: after the tick it is 4 (move) or 5 (stay).
    seen_for(scene("see_time_3"), 3),
    seen_for(scene("see_time_4"), 4),
    // The range edge: squared distances 49 (stay), 50.41 (move) and 48.4 (stay).
    seen_for(at(scene("range_exact"), (19.5, 64.0, 12.5)), 10),
    seen_for(at(scene("range_just_out"), (19.6, 64.0, 12.5)), 10),
    seen_for(at(scene("range_just_in"), (19.4, 64.0, 12.5)), 10),
    // Far away: walks, at full speed unless the crossbow is charged or being loaded.
    seen_for(at(scene("far_uncharged"), FAR), 10),
    in_state(seen_for(at(scene("far_charging"), FAR), 10), CHARGING),
    in_state(seen_for(at(scene("far_charged"), FAR), 10), CHARGED),
    in_state(seen_for(at(scene("far_ready"), FAR), 10), READY),
    // Far away with an attack delay running: the mob stands still (the delay must be exactly 0
    // to move).
    delayed(
        in_state(seen_for(at(scene("far_delay_1"), FAR), 10), CHARGED),
        1,
    ),
    delayed(
        in_state(seen_for(at(scene("far_delay_minus1"), FAR), 10), CHARGED),
        -1,
    ),
    delayed(seen_for(at(scene("far_delay_uncharged"), FAR), 10), 2),
    // Behind a wall: the see time falls to 0, the mob walks to its target.
    walled(seen_for(at(scene("walled_near"), NEAR), 10)),
    walled(in_state(seen_for(scene("walled_ready"), 10), READY)),
    walled(in_state(
        seen_for(at(scene("walled_charged"), NEAR), 10),
        CHARGED,
    )),
    // The path delay counts down, a path is set when it reaches 0, and the mob stops when it
    // need not move.
    path_delayed(seen_for(at(scene("path_delay_5"), FAR), 10), 5),
    path_delayed(seen_for(at(scene("path_delay_1"), FAR), 10), 1),
    path_delayed(seen_for(at(scene("path_delay_2"), FAR), 10), 2),
    path_delayed(seen_for(scene("path_delay_stops"), 10), 7),
    running(path_delayed(at(scene("path_delay_negative"), FAR), -3), 8),
    // The charge: the middle sound at 10, the end at 25, past it at once.
    charged_for(in_state(seen_for(scene("charge_8"), 10), CHARGING), 8),
    charged_for(in_state(seen_for(scene("charge_10"), 10), CHARGING), 10),
    charged_for(in_state(seen_for(scene("charge_23"), 10), CHARGING), 23),
    charged_for(in_state(seen_for(scene("charge_24"), 10), CHARGING), 24),
    charged_for(in_state(seen_for(scene("charge_30"), 10), CHARGING), 30),
    charged_for(
        in_state(
            seen_for(skeleton(scene("charge_24_skeleton")), 10),
            CHARGING,
        ),
        24,
    ),
    // Charging while the target is out of reach still charges (the move only slows).
    charged_for(
        in_state(seen_for(at(scene("charge_far"), FAR), 10), CHARGING),
        5,
    ),
    // The attack delay: ready at 0 or below, not at 1 or 2.
    delayed(in_state(seen_for(scene("charged_delay_0"), 10), CHARGED), 0),
    delayed(in_state(seen_for(scene("charged_delay_1"), 10), CHARGED), 1),
    delayed(in_state(seen_for(scene("charged_delay_2"), 10), CHARGED), 2),
    delayed(
        in_state(seen_for(scene("charged_delay_minus"), 10), CHARGED),
        -4,
    ),
    // Ready: shoots when it sees the target and starts over, else waits.
    running(in_state(seen_for(scene("ready_seen"), 10), READY), 4),
    in_state(seen_for(scene("ready_far"), 10), READY),
    // Weapons: an empty hand, a locked equipment.
    armed_with(seen_for(scene("empty_hand"), 10), Weapon::Empty),
    armed_with(seen_for(scene("locked_equipment"), 10), Weapon::Locked),
    // The look: a target off to the side and above, from a fresh goal and a charging one.
    running(seen_for(at(scene("look_above"), ABOVE), 10), 5),
    running(at(scene("look_far_walking"), (2.5, 64.0, 2.5)), 6),
    // The random draws: the attack delay when the charge ends, the path delay when a path is set.
    drawing(running(
        charged_for(
            in_state(seen_for(scene("draw_attack_delay"), 10), CHARGING),
            24,
        ),
        1,
    )),
    drawing(running(seen_for(at(scene("draw_path_delay"), FAR), 10), 1)),
];

struct Area<'a> {
    world: &'a Arc<World>,
    x: i32,
    z: i32,
}

impl Area<'_> {
    fn put(&self, block: &Block, (x, y, z): (i32, i32, i32)) {
        self.world.set_block_state(
            &BlockPos(Vector3::new(self.x + x, y, self.z + z)),
            block.default_state.id,
            BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
        );
    }

    fn point(&self, (x, y, z): Vec3) -> Vector3<f64> {
        Vector3::new(f64::from(self.x) + x, y, f64::from(self.z) + z)
    }
}

/// An entity id and UUID that depend only on the scene.
fn ids(index: usize, slot: usize) -> (i32, Uuid) {
    let n = index * 10 + slot;
    (
        80_000 + n as i32,
        Uuid::from_u128(0x4372_6f73_7362_0000_0000 + n as u128),
    )
}

fn spawn_mob(area: &Area<'_>, index: usize, scene: &Scene) -> Result<Arc<dyn Mob>, String> {
    let (id, uuid) = ids(index, 0);
    let (kind, entity_type) = match scene.kind {
        Kind::Pillager => ("pillager", &EntityType::PILLAGER),
        Kind::Skeleton => ("skeleton", &EntityType::SKELETON),
    };
    let entity = Entity::from_uuid_with_id(
        id,
        uuid,
        Arc::clone(area.world),
        area.point(MOB_AT),
        entity_type,
    );
    let (mob, spawned): (Arc<dyn Mob>, bool) = match scene.kind {
        Kind::Pillager => {
            let pillager = PillagerEntity::new(entity);
            let spawned = area.world.spawn_entity(pillager.clone());
            (pillager, spawned)
        }
        Kind::Skeleton => {
            let skeleton = SkeletonEntity::new(entity);
            let spawned = area.world.spawn_entity(skeleton.clone());
            (skeleton, spawned)
        }
    };
    if !spawned {
        return Err(format!("{kind} was not spawned"));
    }
    Ok(mob)
}

fn spawn_target(area: &Area<'_>, index: usize, at: Vec3) -> Result<Arc<dyn EntityBase>, String> {
    let (id, uuid) = ids(index, 1);
    let entity = Entity::from_uuid_with_id(
        id,
        uuid,
        Arc::clone(area.world),
        area.point(at),
        &EntityType::ZOMBIE,
    );
    let target: Arc<dyn EntityBase> = Arc::new(LivingEntity::new(entity));
    if !area.world.spawn_entity(Arc::clone(&target)) {
        return Err("the target was not spawned".to_string());
    }
    Ok(target)
}

fn packets(tap: &mut PacketTap) -> String {
    tap.drain()
        .iter()
        .map(|p| match p.first() {
            Some(&ADD_ENTITY_PACKET) => format!("{}:add", p.len()),
            Some(&SOUND_PACKET) => {
                format!(
                    "{}:{:08x}",
                    p.len(),
                    fnv(&p[..p.len().saturating_sub(8)]) as u32
                )
            }
            _ => format!("{}:{:08x}", p.len(), fnv(p) as u32),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn place_watcher(watcher: &Player, area: &Area<'_>) {
    watcher.get_entity().set_pos(area.point((0.5, 64.0, 20.5)));
    watcher.watched_section.store(Cylindrical::new(
        Vector2::new((area.x + 12) >> 4, (area.z + 12) >> 4),
        NonZero::<u8>::MIN.saturating_add(3),
    ));
}

fn arrows_in(world: &Arc<World>) -> usize {
    world
        .entities
        .load()
        .iter()
        .filter(|e| e.get_entity().entity_type == &EntityType::ARROW)
        .count()
}

/// The navigator after the goal's tick: idle, or its speed and the block it heads for after
/// its own tick.
fn nav_text(mob: &dyn Mob) -> String {
    let steered = mob.get_mob_entity().steered();
    let mut nav = steered.navigator();
    if nav.is_idle() {
        return "idle".to_string();
    }
    let speed = nav.get_speed_modifier();
    let _ = nav.tick(&mob.get_mob_entity().living_entity);
    let target = nav.get_target_pos().map_or_else(
        || "none".to_string(),
        |p| format!("{},{},{}", p.0.x, p.0.y, p.0.z),
    );
    format!("go@{speed} to {target}")
}

fn hand_text(mob: &dyn Mob) -> String {
    let living = &mob.get_mob_entity().living_entity;
    let hand = *living
        .active_hand
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let stack = living
        .item_in_use
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .map_or_else(
            || "none".to_string(),
            |s| format!("{}x{}", s.item.id, s.item_count),
        );
    format!(
        "{hand:?}/{stack}/{}",
        living
            .item_use_time
            .load(std::sync::atomic::Ordering::Relaxed)
    )
}

fn look_text(mob: &dyn Mob) -> String {
    mob.get_mob_entity()
        .look_control
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .tick(mob);
    let entity = mob.get_entity();
    format!("{:.2}/{:.2}", entity.head_yaw.load(), entity.pitch.load())
}

fn crossbow_text(mob: &dyn Mob) -> &'static str {
    match mob.as_crossbow_attack_mob() {
        Some(c) if c.is_charging_crossbow() => "yes",
        Some(_) => "no",
        None => "-",
    }
}

fn tick_text(
    goal: &mut RangedCrossbowAttackGoal,
    mob: &Arc<dyn Mob>,
    world: &Arc<World>,
    tap: &mut PacketTap,
    scene: &Scene,
) -> String {
    let (path_before, attack_before) = (goal.update_path_delay, goal.attack_delay);
    mob.get_mob_entity()
        .sensing
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .tick();
    let equipment = mob.get_mob_entity().living_entity.entity_equipment.clone();
    let locked = (scene.weapon == Weapon::Locked).then(|| {
        equipment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    });
    goal.tick(mob.as_ref());
    drop(locked);
    let path = redraw(&mut goal.update_path_delay, path_before, scene.path_redraw);
    let attack = redraw(&mut goal.attack_delay, attack_before, scene.attack_redraw);
    format!(
        "{:?} see={} attack={attack} path={path} charge={} nav={} look={} hand={} crossbow={} arrows={} sent [{}]",
        goal.state,
        goal.see_time,
        goal.charge_ticks,
        nav_text(mob.as_ref()),
        look_text(mob.as_ref()),
        hand_text(mob.as_ref()),
        crossbow_text(mob.as_ref()),
        arrows_in(world),
        packets(tap),
    )
}

/// A field that grew in the tick was drawn at random: show `D` and put the fixed value back.
fn redraw(field: &mut i32, before: i32, fixed: i32) -> String {
    if *field > before {
        *field = fixed;
        "D".to_string()
    } else {
        field.to_string()
    }
}

fn arm(mob: &dyn Mob, weapon: Weapon) {
    let stack = match weapon {
        Weapon::Crossbow | Weapon::Locked => ItemStack::new(1, &Item::CROSSBOW),
        Weapon::Empty => ItemStack::EMPTY.clone(),
    };
    mob.get_mob_entity()
        .living_entity
        .entity_equipment
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .put(&EquipmentSlot::MAIN_HAND, stack);
}

fn goal_of(scene: &Scene) -> RangedCrossbowAttackGoal {
    let mut goal = RangedCrossbowAttackGoal::new(SPEED, RANGE);
    goal.state = scene.state;
    goal.see_time = scene.see_time;
    goal.attack_delay = scene.attack_delay;
    goal.update_path_delay = scene.path_delay;
    goal.charge_ticks = scene.charge_ticks;
    goal
}

/// The lowest and highest value each random draw gave over `DRAWS` fresh goals.
fn report_draws(mob: &Arc<dyn Mob>, scene: &Scene, report: &mut String) {
    let mut path = (i32::MAX, i32::MIN);
    let mut attack = (i32::MAX, i32::MIN);
    for _ in 0..DRAWS {
        let mut goal = goal_of(scene);
        goal.tick(mob.as_ref());
        if goal.update_path_delay > scene.path_delay {
            path = (
                path.0.min(goal.update_path_delay),
                path.1.max(goal.update_path_delay),
            );
        }
        if goal.attack_delay > scene.attack_delay {
            attack = (
                attack.0.min(goal.attack_delay),
                attack.1.max(goal.attack_delay),
            );
        }
    }
    let _ = writeln!(
        report,
        "  draws over {DRAWS}: path {}..{} attack {}..{}",
        path.0, path.1, attack.0, attack.1
    );
}

fn run_scene(world: &Arc<World>, index: usize, scene: &Scene, report: &mut String) -> TestResult {
    let chunk_x = index as i32 * 4;
    for cx in chunk_x..chunk_x + 4 {
        for cz in 0..4 {
            world.level.loaded_chunks.insert(
                Vector2::new(cx, cz),
                pumpkin_world::chunk::ChunkData::empty_sync(cx, cz),
            );
        }
    }
    let area = Area {
        world,
        x: chunk_x * 16 + 16,
        z: 16,
    };
    for x in 0..=31 {
        for z in 0..=23 {
            area.put(&Block::STONE, (x, FLOOR_Y, z));
        }
    }
    if scene.wall {
        for y in 64..=66 {
            for z in 9..=16 {
                area.put(&Block::STONE, (14, y, z));
            }
        }
    }

    let who = Identity {
        name: "CrossbowWatch",
        uuid: Uuid::from_u128(0x4372_6f73_7377_0000 + index as u128),
        entity_id: 90_000 + index as i32,
    };
    let (watcher, mut tap) = test_support::java_player_as(world, GameMode::Survival, &who)
        .ok_or("the watcher's client no longer owned its outgoing queue")?;
    place_watcher(&watcher, &area);
    world.add_player(&watcher)?;

    let mob = spawn_mob(&area, index, scene)?;
    arm(mob.as_ref(), scene.weapon);
    let target = scene
        .target
        .map(|at| spawn_target(&area, index, at))
        .transpose()?;
    mob.get_mob_entity().set_target(target);
    let mob_pos = mob.get_entity().pos.load();
    mob.get_mob_entity().steered().navigator().set_progress(
        crate::entity::ai::pathfinder::NavigatorGoal {
            current_progress: mob_pos,
            destination: area.point(FAR),
            speed: BEFORE_SPEED,
        },
    );
    drop(packets(&mut tap));

    let mut goal = goal_of(scene);
    let _ = writeln!(report, "scene {}", scene.name);
    for tick in 1..=scene.ticks {
        let text = tick_text(&mut goal, &mob, world, &mut tap, scene);
        let _ = writeln!(report, "  {tick} {text}");
    }

    if scene.draws {
        report_draws(&mob, scene, report);
    }

    for entity in world.entities.load().iter() {
        world.remove_entity(entity.as_ref());
    }
    for cx in chunk_x..chunk_x + 4 {
        for cz in 0..4 {
            world.level.loaded_chunks.remove(&Vector2::new(cx, cz));
        }
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn ranged_crossbow_tick() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server
        .worlds
        .load()
        .first()
        .cloned()
        .ok_or("the server has no world")?;
    let mut report = String::new();
    for (index, scene) in SCENES.iter().enumerate() {
        run_scene(&world, index, scene, &mut report)?;
        for player in world.players.load().iter() {
            world.remove_player(player, false).await;
        }
    }
    server.stop();
    script_golden::bless_or_compare("ranged_crossbow_tick.txt", &report)
}
