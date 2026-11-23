//! A record of what `PathFinder::find_path` returns, made before it is split
//! (`tests/golden/find_path.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Scenes are cuboids of stone laid in a real world, a bat (flying evaluator, mid-air at
//! 8,64,8) or a zombie (walking evaluator, on a stone floor at y 64) and a list of targets.
//! Each search prints the path (reached flag, distance to the target, node positions) and the
//! state the search left in the evaluator (nodes seen, nodes closed, a hash of every node's
//! position, closed flag, parent, costs). Some scenes search twice with the same finder.

use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use pumpkin_data::Block;
use pumpkin_data::entity::EntityType;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::Entity;
use crate::entity::ai::pathfinder::fly_node_evaluator::FlyNodeEvaluator;
use crate::entity::ai::pathfinder::node::Node;
use crate::entity::ai::pathfinder::node_evaluator::MobData;
use crate::entity::ai::pathfinder::path::Path;
use crate::entity::ai::pathfinder::pathfinding_context::PathfindingContext;
use crate::entity::ai::pathfinder::walk_node_evaluator::WalkNodeEvaluator;
use crate::entity::ai::pathfinder::{EvaluatorKind, PathFinder};
use crate::entity::ai::test_support::world;
use crate::entity::living::LivingEntity;
use crate::world::World;

type Pos = (i32, i32, i32);
type Cuboid = (i32, i32, i32, i32, i32, i32);

#[derive(Clone, Copy)]
enum Kind {
    Bat,
    Zombie,
    /// A flying evaluator that was never prepared: it has no start node.
    Unprepared,
}

#[derive(Clone, Copy)]
struct Scene {
    name: &'static str,
    kind: Kind,
    walls: &'static [Cuboid],
    targets: &'static [Pos],
    max_path_length: f32,
    reach: i32,
    max_nodes: usize,
    multiplier: f32,
    /// A second search with the same finder: (targets, max nodes).
    then: Option<(&'static [Pos], usize)>,
}

const fn scene(name: &'static str, kind: Kind, targets: &'static [Pos]) -> Scene {
    Scene {
        name,
        kind,
        walls: &[],
        targets,
        max_path_length: 20.0,
        reach: 0,
        max_nodes: 400,
        multiplier: 1.0,
        then: None,
    }
}

impl Scene {
    const fn walls(mut self, walls: &'static [Cuboid]) -> Self {
        self.walls = walls;
        self
    }

    const fn length(mut self, length: f32) -> Self {
        self.max_path_length = length;
        self
    }

    const fn reach(mut self, reach: i32) -> Self {
        self.reach = reach;
        self
    }

    const fn nodes(mut self, max_nodes: usize, multiplier: f32) -> Self {
        self.max_nodes = max_nodes;
        self.multiplier = multiplier;
        self
    }

    const fn then(mut self, targets: &'static [Pos], max_nodes: usize) -> Self {
        self.then = Some((targets, max_nodes));
        self
    }
}

/// Six stone faces around the block (8,64,8).
const CAGE: &[Cuboid] = &[
    (7, 63, 7, 9, 63, 9),
    (7, 65, 7, 9, 65, 9),
    (7, 64, 7, 7, 64, 9),
    (9, 64, 7, 9, 64, 9),
    (7, 64, 7, 9, 64, 7),
    (7, 64, 9, 9, 64, 9),
];
/// Six stone faces around the block (14,64,8).
const TARGET_CAGE: &[Cuboid] = &[
    (13, 63, 7, 15, 63, 9),
    (13, 65, 7, 15, 65, 9),
    (13, 64, 7, 13, 64, 9),
    (15, 64, 7, 15, 64, 9),
    (13, 64, 7, 15, 64, 7),
    (13, 64, 9, 15, 64, 9),
];
const WALL: &[Cuboid] = &[(10, 56, 2, 10, 72, 14)];
const WALL_WITH_GAP: &[Cuboid] = &[
    (10, 56, 2, 10, 72, 8),
    (10, 56, 10, 10, 72, 14),
    (10, 56, 9, 10, 63, 9),
    (10, 65, 9, 10, 72, 9),
];
const LOW_WALL: &[Cuboid] = &[(10, 65, 4, 10, 66, 12)];
const FLOOR: &[Cuboid] = &[(0, 64, 0, 31, 64, 31)];
const HIGH_WALL_ON_FLOOR: &[Cuboid] = &[(0, 64, 0, 31, 64, 31), (10, 65, 0, 10, 68, 31)];
const WALL_ON_FLOOR: &[Cuboid] = &[(0, 64, 0, 31, 64, 31), (10, 65, 4, 10, 66, 12)];
const FLOOR_CAGE: &[Cuboid] = &[
    (0, 64, 0, 31, 64, 31),
    (13, 65, 7, 13, 66, 9),
    (15, 65, 7, 15, 66, 9),
    (13, 65, 7, 15, 66, 7),
    (13, 65, 9, 15, 66, 9),
    (13, 67, 7, 15, 67, 9),
];

const fn bat(name: &'static str, targets: &'static [Pos]) -> Scene {
    scene(name, Kind::Bat, targets)
}

const fn zombie(name: &'static str, targets: &'static [Pos]) -> Scene {
    scene(name, Kind::Zombie, targets)
}

fn open_air_scenes() -> Vec<Scene> {
    vec![
        bat("open straight", &[(12, 64, 8)]),
        bat("open diagonal", &[(11, 66, 11)]),
        bat("open far corner", &[(14, 60, 3)]),
        bat("reach 1", &[(13, 64, 8)]).reach(1),
        bat("reach 2", &[(13, 64, 8)]).reach(2),
        bat("reach 3 offset", &[(12, 66, 10)]).reach(3),
        bat("start is the target", &[(8, 64, 8)]),
        bat("start within reach", &[(9, 64, 8)]).reach(1),
        bat("start outside reach", &[(10, 64, 8)]).reach(1),
        bat("target left", &[(3, 64, 8)]),
        bat("target above", &[(8, 70, 8)]),
        bat("target below", &[(8, 58, 9)]),
    ]
}

fn obstacle_scenes() -> Vec<Scene> {
    vec![
        bat("wall in the way", &[(13, 64, 8)])
            .walls(WALL)
            .length(30.0),
        bat("wall with a gap", &[(13, 64, 8)])
            .walls(WALL_WITH_GAP)
            .length(30.0),
        bat("wall too far around", &[(13, 64, 8)])
            .walls(WALL)
            .length(5.0),
        bat("low wall", &[(13, 65, 8)]).walls(LOW_WALL),
        bat("caged start", &[(12, 64, 8)]).walls(CAGE),
        bat("caged start with reach", &[(10, 64, 8)])
            .walls(CAGE)
            .reach(2),
        bat("caged target", &[(14, 64, 8)]).walls(TARGET_CAGE),
        bat("caged target with reach", &[(14, 64, 8)])
            .walls(TARGET_CAGE)
            .reach(1),
        bat("target inside stone", &[(11, 64, 8)]).walls(&[(11, 64, 8, 11, 64, 8)]),
    ]
}

fn limit_scenes() -> Vec<Scene> {
    vec![
        bat("ten nodes", &[(16, 64, 8)]).nodes(10, 1.0),
        bat("one node", &[(16, 64, 8)]).nodes(1, 1.0),
        bat("two nodes", &[(16, 64, 8)]).nodes(2, 1.0),
        bat("multiplier up", &[(16, 64, 8)]).nodes(4, 2.5),
        bat("multiplier down", &[(16, 64, 8)]).nodes(40, 0.25),
        bat("zero nodes", &[(16, 64, 8)]).nodes(0, 1.0),
        bat("length 1", &[(14, 64, 8)]).length(1.0),
        bat("length 2", &[(14, 64, 8)]).length(2.0),
        bat("length 3.5", &[(14, 64, 8)]).length(3.5),
        bat("length 4", &[(14, 64, 6)]).length(4.0),
        bat("length 0", &[(14, 64, 8)]).length(0.0),
        bat("length 6 diagonal", &[(14, 70, 14)]).length(6.0),
        bat("length 8 target near edge", &[(14, 64, 8)])
            .length(6.0)
            .nodes(120, 1.0),
        bat("walls and length", &[(13, 64, 8)])
            .walls(WALL_WITH_GAP)
            .length(7.0),
    ]
}

fn multi_target_scenes() -> Vec<Scene> {
    vec![
        bat("three targets", &[(12, 64, 8), (8, 64, 12), (4, 64, 8)]),
        bat(
            "three targets, reach 2",
            &[(12, 64, 8), (8, 64, 12), (4, 64, 8)],
        )
        .reach(2),
        bat("two targets reached together", &[(11, 64, 8), (11, 64, 10)]).reach(2),
        bat("far then near", &[(14, 64, 8), (10, 64, 8)]),
        bat("near then far", &[(10, 64, 8), (14, 64, 8)]),
        bat("same target twice", &[(12, 64, 8), (12, 64, 8)]),
        bat("two targets cut by length", &[(14, 64, 8), (2, 64, 8)]).length(3.0),
        bat(
            "two targets cut by length, swapped",
            &[(2, 64, 8), (14, 64, 8)],
        )
        .length(3.0),
        bat("two targets, one caged", &[(14, 64, 8), (8, 64, 13)]).walls(TARGET_CAGE),
        bat(
            "two targets, one caged, swapped",
            &[(8, 64, 13), (14, 64, 8)],
        )
        .walls(TARGET_CAGE),
        bat("uneven partial", &[(14, 64, 8), (2, 70, 8)]).length(4.0),
        bat("uneven partial, swapped", &[(2, 70, 8), (14, 64, 8)]).length(4.0),
        bat("caged start, two targets", &[(12, 64, 8), (4, 64, 8)]).walls(CAGE),
        bat("reached and unreachable", &[(14, 64, 8), (8, 64, 11)]).walls(TARGET_CAGE),
        bat("start and far target", &[(8, 64, 8), (13, 64, 13)]),
        bat("two near targets, reach 1", &[(9, 64, 8), (8, 64, 9)]).reach(1),
    ]
}

fn reuse_scenes() -> Vec<Scene> {
    vec![
        bat("reuse after a cut search", &[(16, 64, 8)])
            .nodes(8, 1.0)
            .then(&[(8, 64, 13)], 400),
        bat("reuse after a full search", &[(12, 64, 8)]).then(&[(4, 64, 8)], 400),
        bat("reuse into a cut search", &[(12, 64, 8)]).then(&[(8, 64, 13)], 3),
        bat("reuse for the same target", &[(12, 64, 8)]).then(&[(12, 64, 8)], 400),
        bat("empty targets", &[]).then(&[(8, 64, 12)], 400),
        scene("no start node", Kind::Unprepared, &[(12, 64, 8)]),
    ]
}

fn walking_scenes() -> Vec<Scene> {
    vec![
        zombie("walk open ground", &[(14, 65, 8)]).walls(FLOOR),
        zombie("walk diagonal", &[(12, 65, 12)]).walls(FLOOR),
        zombie("walk reach 2", &[(14, 65, 8)]).walls(FLOOR).reach(2),
        zombie("walk start is the target", &[(8, 65, 8)]).walls(FLOOR),
        zombie("walk around a wall", &[(13, 65, 8)])
            .walls(WALL_ON_FLOOR)
            .length(30.0),
        zombie("walk wall too high", &[(13, 65, 8)])
            .walls(HIGH_WALL_ON_FLOOR)
            .length(40.0)
            .nodes(300, 1.0),
        zombie("walk caged target", &[(14, 65, 8)])
            .walls(FLOOR_CAGE)
            .length(30.0),
        zombie("walk follow range", &[(14, 65, 8)])
            .walls(FLOOR)
            .length(3.0),
        zombie("walk few nodes", &[(14, 65, 8)])
            .walls(FLOOR)
            .nodes(9, 1.0),
        zombie("walk two targets", &[(14, 65, 8), (8, 65, 3)]).walls(FLOOR),
        zombie("walk reuse", &[(14, 65, 8)])
            .walls(FLOOR)
            .nodes(6, 1.0)
            .then(&[(8, 65, 3)], 400),
    ]
}

fn all_scenes() -> Vec<Scene> {
    let mut scenes = open_air_scenes();
    scenes.extend(obstacle_scenes());
    scenes.extend(limit_scenes());
    scenes.extend(multi_target_scenes());
    scenes.extend(reuse_scenes());
    scenes.extend(walking_scenes());
    scenes
}

/// Four loaded chunks (x and z 0..32) holding the scene's stone cuboids.
fn lay_walls(world: &Arc<World>, walls: &[Cuboid]) {
    let chunks: Vec<_> = (0..4)
        .map(|index| pumpkin_world::chunk::ChunkData::empty_sync(index % 2, index / 2))
        .collect();
    for &(x0, y0, z0, x1, y1, z1) in walls {
        for x in x0..=x1 {
            for y in y0..=y1 {
                for z in z0..=z1 {
                    chunks[((x / 16) + 2 * (z / 16)) as usize].set_block_absolute_y(
                        (x % 16) as usize,
                        y,
                        (z % 16) as usize,
                        Block::STONE.default_state.id,
                    );
                }
            }
        }
    }
    for (index, chunk) in chunks.into_iter().enumerate() {
        let position = Vector2::new(index as i32 % 2, index as i32 / 2);
        world.level.loaded_chunks.insert(position, chunk);
    }
}

fn mob(world: &Arc<World>, kind: Kind) -> LivingEntity {
    let (entity_type, y) = match kind {
        Kind::Zombie => (&EntityType::ZOMBIE, 65.0),
        Kind::Bat | Kind::Unprepared => (&EntityType::BAT, 64.0),
    };
    let living = LivingEntity::new(Entity::new(
        world.clone(),
        Vector3::new(8.5, y, 8.5),
        entity_type,
    ));
    living
        .entity
        .on_ground
        .store(matches!(kind, Kind::Zombie), Ordering::Relaxed);
    living
}

fn evaluator(world: &Arc<World>, living: &LivingEntity, kind: Kind) -> EvaluatorKind {
    let mut evaluator = match kind {
        Kind::Zombie => EvaluatorKind::Walk(WalkNodeEvaluator::new()),
        Kind::Bat | Kind::Unprepared => EvaluatorKind::Fly(FlyNodeEvaluator::new()),
    };
    if !matches!(kind, Kind::Unprepared) {
        let data = MobData::from_entity(living, world);
        evaluator.prepare(
            PathfindingContext::new(BlockPos::floored_v(data.position).0, world.clone()),
            data,
        );
    }
    evaluator
}

fn nodes_of(evaluator: &EvaluatorKind) -> Vec<Node> {
    let map = match evaluator {
        EvaluatorKind::Walk(e) => &e.base.nodes,
        EvaluatorKind::Fly(e) => &e.walk.base.nodes,
        EvaluatorKind::Swim(e) => &e.base.nodes,
        EvaluatorKind::Amphibious(e) => &e.walk.base.nodes,
    };
    let mut nodes: Vec<Node> = map.values().copied().collect();
    nodes.sort_by_key(|node| (node.pos.0.x, node.pos.0.y, node.pos.0.z));
    nodes
}

/// FNV-1a over every field a search writes into a node.
fn fingerprint(nodes: &[Node]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut feed = |value: u64| {
        for byte in value.to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    };
    for node in nodes {
        feed(node.pos.0.x as u64);
        feed(node.pos.0.y as u64);
        feed(node.pos.0.z as u64);
        feed(u64::from(node.closed));
        feed(node.came_from.map_or(7, |from| {
            (from.x as u64) ^ ((from.y as u64) << 16) ^ ((from.z as u64) << 32)
        }));
        for float in [node.g, node.h, node.f, node.walked_dist, node.cost_malus] {
            feed(u64::from(float.to_bits()));
        }
    }
    hash
}

fn path_text(path: Option<&Path>) -> String {
    let Some(path) = path else {
        return "none".to_string();
    };
    let nodes = path
        .get_nodes()
        .iter()
        .map(|node| format!("{},{},{}", node.pos.0.x, node.pos.0.y, node.pos.0.z))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "reached {} target {:?} dist {} [{nodes}]",
        path.can_reach(),
        path.get_target().0,
        path.get_dist_to_target()
    )
}

fn search_text(
    finder: &mut PathFinder,
    evaluator: &mut EvaluatorKind,
    scene: &Scene,
    targets: &[Pos],
) -> String {
    let targets: Vec<BlockPos> = targets
        .iter()
        .map(|&(x, y, z)| BlockPos::new(x, y, z))
        .collect();
    let path = finder.find_path(
        evaluator,
        &targets,
        scene.max_path_length,
        scene.reach,
        scene.multiplier,
    );
    let nodes = nodes_of(evaluator);
    format!(
        "{} | seen {} closed {} hash {:016x}",
        path_text(path.as_ref()),
        nodes.len(),
        nodes.iter().filter(|node| node.closed).count(),
        fingerprint(&nodes)
    )
}

fn scene_text(world: &Arc<World>, scene: &Scene) -> String {
    lay_walls(world, scene.walls);
    let living = mob(world, scene.kind);
    let mut evaluator = evaluator(world, &living, scene.kind);
    let mut finder = PathFinder::new(scene.max_nodes);
    let mut text = search_text(&mut finder, &mut evaluator, scene, scene.targets);
    if let Some((targets, max_nodes)) = scene.then {
        evaluator.done();
        evaluator = self::evaluator(world, &living, scene.kind);
        finder.set_max_visited_nodes(max_nodes);
        let second = search_text(&mut finder, &mut evaluator, scene, targets);
        let _ = write!(text, "\n    then {second}");
    }
    evaluator.done();
    text
}

/// A world per scene: a chunk read once is cached, so a later scene's stone would not show.
#[tokio::test(flavor = "multi_thread")]
async fn find_path() -> TestResult {
    let mut report = String::new();
    for scene in all_scenes() {
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        let _ = writeln!(report, "{}: {}", scene.name, scene_text(&world, &scene));
        world.level.shutdown().await;
    }
    script_golden::bless_or_compare("find_path.txt", &report)
}
