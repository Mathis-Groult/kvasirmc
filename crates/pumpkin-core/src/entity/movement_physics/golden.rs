//! A record of what `Entity::adjust_movement_for_collisions` does, made before the function was
//! split (`tests/golden/movement_collisions.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene lays a few blocks in its own loaded area of a real server's world (floor, wall,
//! corner, ceiling, slab, fence, a gap between two floor blocks, stairs), puts a zombie
//! (`LivingEntity`, not in the world) at a fixed spot with its collision flags preset to `true`,
//! and asks for every movement of a fixed list. One line per movement: the adjusted movement,
//! the ground, vertical and horizontal collision flags and the supporting block.

use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use pumpkin_data::entity::EntityType;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::world::BlockFlags;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;
use crate::entity::living::LivingEntity;
use crate::entity::{Entity, EntityBase};
use crate::world::World;

/// Blocks of a scene: offset from the scene's centre block and the block.
type Cells = &'static [((i32, i32, i32), &'static Block)];

struct Scene {
    name: &'static str,
    cells: Cells,
    /// The zombie's feet, as an offset from the centre block's corner.
    at: (f64, f64, f64),
}

const FLOOR: Cells = &[
    ((-1, -1, -1), &Block::STONE),
    ((0, -1, -1), &Block::STONE),
    ((1, -1, -1), &Block::STONE),
    ((-1, -1, 0), &Block::STONE),
    ((0, -1, 0), &Block::STONE),
    ((1, -1, 0), &Block::STONE),
    ((-1, -1, 1), &Block::STONE),
    ((0, -1, 1), &Block::STONE),
    ((1, -1, 1), &Block::STONE),
];
const WALL_EAST: Cells = &[((1, 0, 0), &Block::STONE), ((1, 1, 0), &Block::STONE)];
const WALL_NORTH: Cells = &[((0, 0, -1), &Block::STONE), ((0, 1, -1), &Block::STONE)];
const CORNER: Cells = &[
    ((1, 0, 0), &Block::STONE),
    ((0, 0, 1), &Block::STONE),
    ((1, 0, 1), &Block::STONE),
    ((0, -1, 0), &Block::STONE),
];
const CEILING: Cells = &[((0, 2, 0), &Block::STONE), ((0, -1, 0), &Block::STONE)];
const SLAB: Cells = &[((0, -1, 0), &Block::OAK_SLAB)];
const FENCE: Cells = &[((1, 0, 0), &Block::OAK_FENCE), ((0, -1, 0), &Block::STONE)];
const GAP: Cells = &[((-1, -1, 0), &Block::STONE), ((1, -1, 0), &Block::STONE)];
const STAIRS: Cells = &[((1, 0, 0), &Block::OAK_STAIRS), ((0, -1, 0), &Block::STONE)];
const TWO_FLOORS: Cells = &[((0, -1, 0), &Block::STONE), ((0, -3, 0), &Block::STONE)];

const SCENES: &[Scene] = &[
    Scene {
        name: "open air",
        cells: &[],
        at: (0.5, 0.0, 0.5),
    },
    Scene {
        name: "on a floor",
        cells: FLOOR,
        at: (0.5, 0.0, 0.5),
    },
    Scene {
        name: "above a floor",
        cells: FLOOR,
        at: (0.5, 0.3, 0.5),
    },
    Scene {
        name: "on the floor's edge",
        cells: FLOOR,
        at: (1.5, 0.0, 0.5),
    },
    Scene {
        name: "next to an east wall",
        cells: WALL_EAST,
        at: (0.6, 0.0, 0.5),
    },
    Scene {
        name: "touching an east wall",
        cells: WALL_EAST,
        at: (0.7, 0.0, 0.5),
    },
    Scene {
        name: "next to a north wall",
        cells: WALL_NORTH,
        at: (0.5, 0.0, 0.4),
    },
    Scene {
        name: "in a corner",
        cells: CORNER,
        at: (0.5, 0.0, 0.5),
    },
    Scene {
        name: "under a ceiling",
        cells: CEILING,
        at: (0.5, 0.0, 0.5),
    },
    Scene {
        name: "above a slab",
        cells: SLAB,
        at: (0.5, -0.2, 0.5),
    },
    Scene {
        name: "next to a fence",
        cells: FENCE,
        at: (0.5, 0.0, 0.5),
    },
    Scene {
        name: "over a gap",
        cells: GAP,
        at: (0.5, 0.0, 0.5),
    },
    Scene {
        name: "astride a gap",
        cells: GAP,
        at: (0.0, 0.0, 0.5),
    },
    Scene {
        name: "next to stairs",
        cells: STAIRS,
        at: (0.5, 0.0, 0.5),
    },
    Scene {
        name: "above two floors",
        cells: TWO_FLOORS,
        at: (0.5, 0.5, 0.5),
    },
];

/// Movements asked in every scene.
const MOVES: &[(f64, f64, f64)] = &[
    (0.0, 0.0, 0.0),
    (0.0, -0.08, 0.0),
    (0.0, -0.5, 0.0),
    (0.0, -3.0, 0.0),
    (0.0, 0.42, 0.0),
    (0.0, 1.5, 0.0),
    (0.3, 0.0, 0.0),
    (-0.3, 0.0, 0.0),
    (0.0, 0.0, 0.3),
    (0.0, 0.0, -0.3),
    (0.3, 0.0, 0.3),
    (0.3, -0.5, 0.3),
    (-0.3, -0.5, -0.3),
    (1.2, -0.2, 0.0),
    (0.2, 0.6, -0.2),
    (0.05, -0.08, 1.0),
    // Appended after the first mutation run: exactly one block up and down.
    (0.0, 1.0, 0.0),
    (0.0, -1.0, 0.0),
];

fn put(world: &Arc<World>, pos: &BlockPos, state: BlockStateId) {
    world.set_block_state(
        pos,
        state,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

fn lay(world: &Arc<World>, centre: BlockPos, cells: Cells) {
    for dy in -4..=3 {
        for dz in -2..=2 {
            for dx in -2..=2 {
                let pos = BlockPos::new(centre.0.x + dx, centre.0.y + dy, centre.0.z + dz);
                put(world, &pos, Block::AIR.default_state.id);
            }
        }
    }
    for ((dx, dy, dz), block) in cells {
        let pos = BlockPos::new(centre.0.x + dx, centre.0.y + dy, centre.0.z + dz);
        put(world, &pos, block.default_state.id);
    }
}

fn line(entity: &Entity, centre: BlockPos, moved: Vector3<f64>) -> String {
    let support = entity.supporting_block_pos.load().map_or_else(
        || "none".to_string(),
        |pos| {
            format!(
                "({}, {}, {})",
                pos.0.x - centre.0.x,
                pos.0.y - centre.0.y,
                pos.0.z - centre.0.z
            )
        },
    );
    format!(
        "({:.9}, {:.9}, {:.9}) ground {} vertical {} horizontal {} support {support}",
        moved.x,
        moved.y,
        moved.z,
        entity.on_ground.load(Ordering::SeqCst),
        entity.vertical_collision.load(Ordering::SeqCst),
        entity.horizontal_collision.load(Ordering::SeqCst),
    )
}

fn run(out: &mut String, world: &Arc<World>, index: usize, scene: &Scene) {
    let centre = BlockPos::new(index as i32 * 16 + 8, 64, 8);
    lay(world, centre, scene.cells);
    let (x, y, z) = scene.at;
    let feet = Vector3::new(
        f64::from(centre.0.x) + x,
        f64::from(centre.0.y) + y,
        f64::from(centre.0.z) + z,
    );
    let zombie = LivingEntity::new(Entity::new(world.clone(), feet, &EntityType::ZOMBIE));
    let entity = &zombie.entity;
    for (mx, my, mz) in MOVES {
        entity.on_ground.store(true, Ordering::SeqCst);
        entity.vertical_collision.store(true, Ordering::SeqCst);
        entity.horizontal_collision.store(true, Ordering::SeqCst);
        entity
            .supporting_block_pos
            .store(Some(BlockPos::new(0, -64, 0)));
        let movement = Vector3::new(*mx, *my, *mz);
        let moved = entity.adjust_movement_for_collisions(movement, &zombie as &dyn EntityBase);
        let _ = writeln!(
            out,
            "{}: ({mx}, {my}, {mz}) -> {}",
            scene.name,
            line(entity, centre, moved)
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn movement_collisions_are_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    for cx in -1..=SCENES.len() as i32 {
        for cz in -1..=1 {
            world.level.loaded_chunks.insert(
                Vector2::new(cx, cz),
                pumpkin_world::chunk::ChunkData::empty_sync(cx, cz),
            );
        }
    }
    let mut out = String::new();
    for (index, scene) in SCENES.iter().enumerate() {
        run(&mut out, &world, index, scene);
    }
    server.stop();
    script_golden::bless_or_compare("movement_collisions.txt", &out)
}
