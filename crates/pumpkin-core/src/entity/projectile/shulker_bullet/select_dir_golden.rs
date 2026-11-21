//! A record of what `ShulkerBulletEntity::select_next_dir` picks, made before it is split
//! (`tests/golden/shulker_select_dir.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene puts a bullet at a point, calls `select_next_dir` with an avoided axis and a target
//! point (or none) and reads the direction, the steering delta and the flight steps it stored.
//! The choice among the open axes and the step count use the thread's random generator, so every
//! scene is drawn 2,000 times and reports which directions came up (with the delta of each, and
//! their share: `even` when all shares are within 0.05 of 1/n, else rounded to a tenth) and the
//! step counts seen. Blocks next to the bullet are stone placed in a loaded area of the test
//! world; the `negative` and `far` scenes sit where no chunk is loaded (an unloaded neighbour counts as empty).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::sync::Arc;

use pumpkin_data::Block;
use pumpkin_data::entity::EntityType;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use super::ShulkerBulletEntity;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::Entity;
use crate::entity::ai::test_support;
use crate::world::World;

const DRAWS: usize = 2_000;
/// The area's corner in the world; `RAW` scenes give world coordinates (no chunk loaded there).
const AREA: (f64, f64) = (16.0, 16.0);
const RAW: (f64, f64) = (0.0, 0.0);
const NO_AVOID: u8 = 3;

type Vec3 = (f64, f64, f64);

struct Scene {
    name: String,
    base: (f64, f64),
    at: Vec3,
    target: Option<Vec3>,
    avoid: u8,
    /// Stone blocks, relative to the area's corner.
    stones: Vec<(i32, i32, i32)>,
    /// The choice is uniform over what comes up (else shares are rounded to a tenth).
    even: bool,
    /// The bullet carries a steering delta before every call (a call that finds itself on its
    /// target must set it back to zero).
    primed: bool,
}

/// The bullet's block is (4, 66, 4) and it sits at that block's centre.
const HOME: Vec3 = (4.5, 66.5, 4.5);

fn scene(name: &str, base: (f64, f64), at: Vec3, target: Option<Vec3>, avoid: u8) -> Scene {
    Scene {
        name: name.to_string(),
        base,
        at,
        target,
        avoid,
        stones: Vec::new(),
        even: true,
        primed: false,
    }
}

/// A target whose block is the bullet's block moved by (dx, dy, dz).
fn toward(name: &str, (dx, dy, dz): (i32, i32, i32), avoid: u8) -> Scene {
    let target = (
        HOME.0 + f64::from(dx),
        66.0 + f64::from(dy),
        HOME.2 + f64::from(dz),
    );
    scene(name, AREA, HOME, Some(target), avoid)
}

const DIRS: [(&str, (i32, i32, i32)); 6] = [
    ("east", (3, 0, 0)),
    ("west", (-3, 0, 0)),
    ("up", (0, 3, 0)),
    ("down", (0, -3, 0)),
    ("south", (0, 0, 3)),
    ("north", (0, 0, -3)),
];

/// Every single-axis target against every avoided axis (3 avoids none).
fn axis_scenes() -> Vec<Scene> {
    let mut scenes = Vec::new();
    for (name, offset) in DIRS {
        for avoid in [NO_AVOID, 0, 1, 2] {
            scenes.push(toward(&format!("{name}_avoid{avoid}"), offset, avoid));
        }
    }
    scenes
}

/// Targets on all three axes (every sign), and unit offsets that make an axis open.
fn diagonal_scenes() -> Vec<Scene> {
    let mut scenes = Vec::new();
    for avoid in [NO_AVOID, 0, 1, 2] {
        scenes.push(toward(&format!("ene_avoid{avoid}"), (3, 3, 3), avoid));
    }
    for (name, offset) in [
        ("wdn", (-3, -3, -3)),
        ("eds", (3, -3, 3)),
        ("wun", (-3, 3, -3)),
        ("one_east_far_up", (1, 3, 0)),
        ("one_south_far_west", (-3, 0, 1)),
        ("one_down_far_north", (0, -1, -3)),
    ] {
        scenes.push(toward(name, offset, NO_AVOID));
    }
    scenes
}

/// Distance 2 is near (not over 4), the first far point is just past it; also the bullet on its
/// target, no target, a fractional position, and a target height that rounds up or down.
fn range_scenes() -> Vec<Scene> {
    let mut scenes = vec![
        toward("near_edge_east", (2, 0, 0), NO_AVOID),
        toward("far_edge_diag", (2, 1, 0), NO_AVOID),
        toward("near_diag", (1, 1, 1), NO_AVOID),
        toward("on_target", (0, 0, 0), NO_AVOID),
        scene("none_centre", AREA, HOME, None, NO_AVOID),
        scene("none_fraction", AREA, (4.9, 66.2, 4.1), None, 1),
        scene(
            "fraction_far",
            AREA,
            (4.9, 66.2, 4.1),
            Some((9.5, 70.0, 1.5)),
            NO_AVOID,
        ),
        scene(
            "fraction_near",
            AREA,
            (4.1, 66.9, 4.8),
            Some((5.5, 66.0, 5.5)),
            NO_AVOID,
        ),
        scene("y_rounds_up", AREA, HOME, Some((4.5, 66.6, 4.5)), NO_AVOID),
        scene(
            "y_rounds_down",
            AREA,
            HOME,
            Some((4.5, 66.4, 4.5)),
            NO_AVOID,
        ),
    ];
    // Negative coordinates: the target block is the floor, not the truncation.
    scenes.push(scene(
        "negative_far",
        RAW,
        (-4.5, 66.5, -4.5),
        Some((-7.3, 67.4, -2.3)),
        NO_AVOID,
    ));
    scenes.push(scene(
        "negative_near",
        RAW,
        (-4.5, 66.5, -4.5),
        Some((-5.3, 66.4, -3.3)),
        NO_AVOID,
    ));
    scenes
}

/// Stone beside the bullet: a blocked axis drops out, the others stay; nothing open falls back
/// to random directions (five tries for a free one).
fn blocked_scenes() -> Vec<Scene> {
    let beside = [
        ("east", (5, 66, 4)),
        ("west", (3, 66, 4)),
        ("up", (4, 67, 4)),
        ("down", (4, 65, 4)),
        ("south", (4, 66, 5)),
        ("north", (4, 66, 3)),
    ];
    let mut scenes = Vec::new();
    for (name, stone) in beside {
        let mut diagonal = toward(&format!("diag_{name}_blocked"), (3, 3, 3), NO_AVOID);
        diagonal.stones = vec![stone];
        scenes.push(diagonal);
    }
    for (name, offset) in DIRS {
        // The stone is on the side the target is on.
        let stone = beside
            .iter()
            .find(|(side, _)| *side == name)
            .map_or((0, 0, 0), |(_, at)| *at);
        let mut one = toward(&format!("{name}_blocked"), offset, NO_AVOID);
        one.stones = vec![stone];
        one.even = false;
        scenes.push(one);
        // A stone on the other side does not matter.
        let other = beside
            .iter()
            .find(|(side, _)| *side != name && opposite(side) == name)
            .map_or((0, 0, 0), |(_, at)| *at);
        let mut behind = toward(&format!("{name}_behind_blocked"), offset, NO_AVOID);
        behind.stones = vec![other];
        scenes.push(behind);
    }
    scenes.extend(boxed_scenes(&beside));
    scenes
}

fn opposite(side: &str) -> &'static str {
    match side {
        "east" => "west",
        "west" => "east",
        "up" => "down",
        "down" => "up",
        "south" => "north",
        _ => "south",
    }
}

fn boxed_scenes(beside: &[(&str, (i32, i32, i32)); 6]) -> Vec<Scene> {
    let all: Vec<_> = beside.iter().map(|(_, at)| *at).collect();
    let mut boxed = toward("boxed_east", (3, 0, 0), NO_AVOID);
    boxed.stones = all.clone();
    let mut two = toward("east_up_blocked", (3, 3, 3), NO_AVOID);
    two.stones = vec![beside[0].1, beside[2].1];
    let mut one_open = toward("boxed_one_open_up", (3, 0, 0), NO_AVOID);
    one_open.stones = all.into_iter().filter(|at| *at != beside[2].1).collect();
    one_open.even = false;
    let mut unloaded = scene(
        "far_diag",
        RAW,
        (500.5, 66.5, 500.5),
        Some((503.5, 69.0, 503.5)),
        NO_AVOID,
    );
    unloaded.even = true;
    vec![boxed, two, one_open, unloaded]
}

fn all_scenes() -> Vec<Scene> {
    let mut scenes = axis_scenes();
    scenes.extend(diagonal_scenes());
    scenes.extend(range_scenes());
    scenes.extend(blocked_scenes());
    scenes.push(primed_scene());
    scenes
}

/// The bullet is on its target (zero distance) with a delta left from an earlier aim.
fn primed_scene() -> Scene {
    let mut on_target = toward("on_target_primed", (0, 0, 0), NO_AVOID);
    on_target.primed = true;
    on_target
}

fn dir_name(dir: u8) -> &'static str {
    match dir {
        0 => "down",
        1 => "up",
        2 => "north",
        3 => "south",
        4 => "west",
        5 => "east",
        _ => "none",
    }
}

#[derive(Default)]
struct Seen {
    count: usize,
    deltas: BTreeSet<String>,
}

fn draw(bullet: &ShulkerBulletEntity, scene: &Scene, target: Option<Vector3<f64>>) -> String {
    let mut dirs: BTreeMap<u8, Seen> = BTreeMap::new();
    let mut steps = BTreeSet::new();
    for _ in 0..DRAWS {
        if scene.primed {
            bullet.target_delta_x.store(0.5);
            bullet.target_delta_y.store(-0.25);
            bullet.target_delta_z.store(0.125);
        }
        bullet.select_next_dir(scene.avoid, target);
        let seen = dirs
            .entry(
                bullet
                    .current_dir
                    .load(std::sync::atomic::Ordering::Relaxed),
            )
            .or_default();
        seen.count += 1;
        seen.deltas.insert(format!(
            "{:.6},{:.6},{:.6}",
            bullet.target_delta_x.load(),
            bullet.target_delta_y.load(),
            bullet.target_delta_z.load()
        ));
        steps.insert(
            bullet
                .flight_steps
                .load(std::sync::atomic::Ordering::Relaxed),
        );
    }
    describe(scene, &dirs, &steps)
}

fn describe(scene: &Scene, dirs: &BTreeMap<u8, Seen>, steps: &BTreeSet<i32>) -> String {
    let mut text = format!("{}:\n", scene.name);
    // A blocked direction can only come up as the fifth, unchecked fallback draw: about 1 in
    // 7,800, so a handful of times in 2,000 draws at most. It is left out.
    let dirs: Vec<_> = dirs
        .iter()
        .filter(|(_, seen)| seen.count >= DRAWS / 50)
        .collect();
    let even_share = 1.0 / dirs.len() as f64;
    for (dir, seen) in dirs {
        let share = seen.count as f64 / DRAWS as f64;
        let share = if scene.even && (share - even_share).abs() < 0.05 {
            "even".to_string()
        } else {
            format!("{share:.1}")
        };
        let deltas: Vec<_> = seen.deltas.iter().cloned().collect();
        let _ = writeln!(
            text,
            "  {} share {share} delta {}",
            dir_name(*dir),
            deltas.join(" | ")
        );
    }
    let _ = writeln!(text, "  steps {steps:?}");
    text
}

fn put(world: &Arc<World>, (x, y, z): (i32, i32, i32), block: &Block) {
    world.set_block_state(
        &BlockPos(Vector3::new(16 + x, y, 16 + z)),
        block.default_state.id,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

fn run_scene(world: &Arc<World>, scene: &Scene) -> String {
    for stone in &scene.stones {
        put(world, *stone, &Block::STONE);
    }
    let point = |(x, y, z): Vec3| Vector3::new(scene.base.0 + x, y, scene.base.1 + z);
    let entity = Entity::from_uuid_with_id(
        70_000,
        Uuid::from_u128(0x5348_5344_0000),
        Arc::clone(world),
        point(scene.at),
        &EntityType::SHULKER_BULLET,
    );
    let bullet = ShulkerBulletEntity::orphan(entity);
    let text = draw(&bullet, scene, scene.target.map(point));
    for stone in &scene.stones {
        put(world, *stone, &Block::AIR);
    }
    text
}

#[tokio::test(flavor = "multi_thread")]
async fn shulker_select_dir() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server
        .worlds
        .load()
        .first()
        .cloned()
        .ok_or("the server has no world")?;
    for cx in 0..4 {
        for cz in 0..4 {
            world.level.loaded_chunks.insert(
                Vector2::new(cx, cz),
                pumpkin_world::chunk::ChunkData::empty_sync(cx, cz),
            );
        }
    }
    let mut report = String::new();
    for scene in all_scenes() {
        report.push_str(&run_scene(&world, &scene));
    }
    server.stop();
    script_golden::bless_or_compare("shulker_select_dir.txt", &report)
}
