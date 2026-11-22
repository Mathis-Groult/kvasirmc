//! A record of what `ThrownItemEntity::process_tick` does, made before it is split
//! (`tests/golden/thrown_item_tick.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene is built in its own loaded area of a real server's world (stone floor at y 63).
//! A projectile is made from `ThrownItemEntity` with the scene's start and velocity, spawned
//! in the world and ticked by calling `tick` directly until it removes itself. Most scenes use
//! a probe projectile whose `on_hit` records the hit it was given (kind, block or entity, face,
//! hit position, normal); one scene uses the real `SnowballEntity` against a blaze. Every tick
//! prints the projectile's position, last position, velocity, rotation, removal and the hit
//! flag, the hits recorded, the target blocks' state, the mobs' health and the packets the
//! watcher got (id, length and a hash of the bytes: entity ids are fixed).

use std::fmt::Write;
use std::num::NonZero;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use pumpkin_data::Block;
use pumpkin_data::entity::EntityType;
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use super::owner::ProjectileOwner;
use super::snowball::SnowballEntity;
use super::{ProjectileHit, ThrownItemEntity};
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::area_effect_cloud::AreaEffectCloudEntity;
use crate::entity::living::LivingEntity;
use crate::entity::{Entity, EntityBase};
use crate::net::java::PacketTap;
use crate::server::Server;
use crate::world::World;

const FLOOR_Y: i32 = 63;

type Vec3 = (f64, f64, f64);

#[derive(Clone, Copy)]
enum Solid {
    Stone,
    Target,
    Stair,
    Fence,
    Slab,
}

#[derive(Clone, Copy)]
enum Kind {
    Zombie,
    /// Marked removed, but still in the world's lists.
    Removed,
    Blaze,
    Cloud,
    /// A second projectile, standing still.
    Snowball,
}

#[derive(Clone, Copy)]
struct Mob {
    kind: Kind,
    at: Vec3,
}

struct Scene {
    name: &'static str,
    /// The owner (a zombie, standing here); none = the projectile has no owner.
    owner: Option<Vec3>,
    start: Vec3,
    velocity: Vec3,
    ticks: usize,
    water: bool,
    /// The velocity is replaced before this tick.
    turn: Option<(usize, Vec3)>,
    blocks: &'static [(Solid, i32, i32, i32)],
    mobs: &'static [Mob],
    /// `has_hit` already set before the first tick.
    pre_hit: bool,
    collides_with_projectiles: bool,
    /// The real snowball instead of the probe.
    real: bool,
}

const fn scene(name: &'static str, start: Vec3, velocity: Vec3, ticks: usize) -> Scene {
    Scene {
        name,
        owner: None,
        start,
        velocity,
        ticks,
        water: false,
        turn: None,
        blocks: &[],
        mobs: &[],
        pre_hit: false,
        collides_with_projectiles: false,
        real: false,
    }
}

const fn stone(x: i32, y: i32, z: i32) -> (Solid, i32, i32, i32) {
    (Solid::Stone, x, y, z)
}

const fn zombie(at: Vec3) -> Mob {
    Mob {
        kind: Kind::Zombie,
        at,
    }
}

const fn mob(kind: Kind, at: Vec3) -> Mob {
    Mob { kind, at }
}

/// Free flight: gravity, drag, water, negative velocity, chunk borders.
const FLIGHT: &[Scene] = &[
    scene("fall", (4.5, 70.0, 4.5), (0.5, 0.2, 0.1), 8),
    Scene {
        water: true,
        ..scene("water", (4.5, 70.0, 4.5), (0.5, 0.2, 0.1), 8)
    },
    scene("reverse", (10.5, 70.0, 10.5), (-0.5, -0.2, -0.3), 8),
    scene("chunk_cross", (14.5, 70.0, 14.5), (0.9, 0.0, 0.9), 6),
    scene("chunk_cross_neg", (17.5, 70.0, 17.5), (-0.9, 0.0, -0.9), 6),
    scene("straight_up", (4.5, 66.0, 4.5), (0.0, 1.5, 0.0), 6),
    scene("diagonal", (4.5, 66.0, 4.5), (-0.4, 0.7, 0.9), 6),
];

/// One stone block at (8, 66, 8): each face, a graze, a slow approach, a start inside it.
const FACES: &[Scene] = &[
    Scene {
        blocks: &[stone(8, 66, 8)],
        ..scene("face_west", (5.0, 66.5, 8.5), (4.0, 0.0, 0.0), 3)
    },
    Scene {
        blocks: &[stone(8, 66, 8)],
        ..scene("face_east", (12.0, 66.5, 8.5), (-4.0, 0.0, 0.0), 3)
    },
    Scene {
        blocks: &[stone(8, 68, 8)],
        ..scene("face_down", (8.5, 65.0, 8.5), (0.0, 4.0, 0.0), 3)
    },
    Scene {
        blocks: &[stone(8, 66, 8)],
        ..scene("face_up", (8.5, 70.0, 8.5), (0.0, -4.0, 0.0), 3)
    },
    Scene {
        blocks: &[stone(8, 66, 8)],
        ..scene("face_north", (8.5, 66.5, 5.0), (0.0, 0.0, 4.0), 3)
    },
    Scene {
        blocks: &[stone(8, 66, 8)],
        ..scene("face_south", (8.5, 66.5, 12.0), (0.0, 0.0, -4.0), 3)
    },
    Scene {
        blocks: &[stone(8, 66, 8)],
        ..scene("approach", (4.5, 66.5, 8.5), (1.5, 0.0, 0.0), 5)
    },
    Scene {
        blocks: &[stone(8, 66, 8)],
        ..scene("inside", (8.5, 66.5, 8.5), (0.5, 0.0, 0.0), 3)
    },
    Scene {
        blocks: &[stone(8, 66, 8)],
        ..scene("graze", (4.5, 66.5, 9.2), (4.0, 0.0, 0.0), 3)
    },
    Scene {
        blocks: &[stone(8, 66, 8)],
        pre_hit: true,
        ..scene("pre_hit", (5.0, 66.5, 8.5), (4.0, 0.0, 0.0), 3)
    },
    Scene {
        blocks: &[stone(8, 66, 8)],
        water: true,
        ..scene("water_hit", (5.0, 66.5, 8.5), (4.0, 0.0, 0.0), 3)
    },
];

/// Two blocks in a row, from both sides: the nearer one wins whatever the search order.
const ROWS: &[Scene] = &[
    Scene {
        blocks: &[stone(8, 66, 8), stone(10, 66, 8)],
        ..scene("row_east", (5.0, 66.5, 8.5), (6.0, 0.0, 0.0), 2)
    },
    Scene {
        blocks: &[stone(8, 66, 8), stone(10, 66, 8)],
        ..scene("row_west", (13.0, 66.5, 8.5), (-6.0, 0.0, 0.0), 2)
    },
    Scene {
        blocks: &[stone(8, 66, 8), stone(8, 68, 8)],
        ..scene("row_up", (8.5, 73.0, 8.5), (0.0, -7.0, 0.0), 2)
    },
    Scene {
        blocks: &[stone(8, 66, 8), stone(8, 68, 8)],
        ..scene("row_down", (8.5, 64.5, 8.5), (0.0, 6.0, 0.0), 2)
    },
    Scene {
        blocks: &[stone(8, 66, 8), stone(8, 66, 10)],
        ..scene("row_south", (8.5, 66.5, 5.0), (0.0, 0.0, 6.0), 2)
    },
    Scene {
        blocks: &[stone(8, 66, 8), stone(8, 66, 10)],
        ..scene("row_north", (8.5, 66.5, 13.0), (0.0, 0.0, -6.0), 2)
    },
];

/// Blocks with several collision shapes (the shape index maps back to the block).
const SHAPES: &[Scene] = &[
    Scene {
        blocks: &[
            (Solid::Stair, 8, 66, 8),
            (Solid::Fence, 10, 66, 8),
            (Solid::Slab, 12, 66, 8),
        ],
        ..scene("shapes_east", (5.0, 66.4, 8.5), (8.0, 0.0, 0.0), 2)
    },
    Scene {
        blocks: &[
            (Solid::Stair, 8, 66, 8),
            (Solid::Fence, 10, 66, 8),
            (Solid::Slab, 12, 66, 8),
        ],
        ..scene("shapes_west", (15.0, 66.4, 8.5), (-8.0, 0.0, 0.0), 2)
    },
    Scene {
        blocks: &[(Solid::Stair, 8, 66, 8), (Solid::Slab, 8, 66, 10)],
        ..scene("stair_top", (8.9, 70.0, 8.5), (0.0, -4.0, 0.0), 2)
    },
    Scene {
        blocks: &[(Solid::Fence, 8, 66, 8), (Solid::Stone, 10, 66, 8)],
        ..scene("fence_high", (5.0, 67.2, 8.5), (8.0, 0.0, 0.0), 2)
    },
];

/// A target block: its power follows the hit position (`on_projectile_hit`).
const TARGETS: &[Scene] = &[
    Scene {
        blocks: &[(Solid::Target, 8, 66, 8)],
        ..scene("target_centre", (5.0, 66.5, 8.5), (4.0, 0.0, 0.0), 3)
    },
    Scene {
        blocks: &[(Solid::Target, 8, 66, 8)],
        ..scene("target_edge", (5.0, 66.95, 8.05), (4.0, 0.0, 0.0), 3)
    },
    Scene {
        blocks: &[(Solid::Target, 8, 66, 8)],
        ..scene("target_top", (8.2, 70.0, 8.8), (0.0, -4.0, 0.0), 3)
    },
];

/// Chunk borders at x and z 16 (relative to the scene's corner).
const BORDERS: &[Scene] = &[
    Scene {
        blocks: &[stone(16, 66, 8)],
        ..scene("border_east", (12.0, 66.5, 8.5), (5.0, 0.0, 0.0), 3)
    },
    Scene {
        blocks: &[stone(15, 66, 8)],
        ..scene("border_west", (20.0, 66.5, 8.5), (-5.0, 0.0, 0.0), 3)
    },
    Scene {
        blocks: &[stone(8, 66, 16)],
        ..scene("border_south", (8.5, 66.5, 12.0), (0.0, 0.0, 5.0), 3)
    },
    Scene {
        blocks: &[stone(8, 66, 15)],
        ..scene("border_north", (8.5, 66.5, 20.0), (0.0, 0.0, -5.0), 3)
    },
    Scene {
        blocks: &[stone(15, 66, 15), stone(16, 66, 16)],
        ..scene("border_corner", (13.0, 66.5, 13.0), (3.0, 0.0, 3.0), 3)
    },
    Scene {
        mobs: &[zombie((16.0, 66.0, 8.5))],
        ..scene("border_mob", (12.0, 66.8, 8.5), (5.0, 0.0, 0.0), 3)
    },
];

/// One zombie at (8.5, 66, 8.5): hit from each side, a graze and a miss around its box
/// (half width 0.3, grown by 0.3 for the projectile).
const MOBS: &[Scene] = &[
    Scene {
        mobs: &[zombie((8.5, 66.0, 8.5))],
        ..scene("mob_west", (4.5, 66.8, 8.5), (4.0, 0.0, 0.0), 3)
    },
    Scene {
        mobs: &[zombie((8.5, 66.0, 8.5))],
        ..scene("mob_east", (12.5, 66.8, 8.5), (-4.0, 0.0, 0.0), 3)
    },
    Scene {
        mobs: &[zombie((8.5, 66.0, 8.5))],
        ..scene("mob_top", (8.5, 72.0, 8.5), (0.0, -4.0, 0.0), 3)
    },
    Scene {
        mobs: &[zombie((8.5, 66.0, 8.5))],
        ..scene("mob_below", (8.5, 64.2, 8.5), (0.0, 4.0, 0.0), 3)
    },
    Scene {
        mobs: &[zombie((8.5, 66.0, 8.5))],
        ..scene("mob_north", (8.5, 66.8, 5.0), (0.0, 0.0, 4.0), 3)
    },
    Scene {
        mobs: &[zombie((8.5, 66.0, 8.5))],
        ..scene("mob_south", (8.5, 66.8, 12.0), (0.0, 0.0, -4.0), 3)
    },
    Scene {
        mobs: &[zombie((8.5, 66.0, 8.5))],
        ..scene("mob_graze", (4.5, 66.8, 9.05), (4.0, 0.0, 0.0), 3)
    },
    Scene {
        mobs: &[zombie((8.5, 66.0, 8.5))],
        ..scene("mob_miss", (4.5, 66.8, 9.25), (4.0, 0.0, 0.0), 3)
    },
    Scene {
        mobs: &[zombie((8.5, 66.0, 8.5))],
        owner: Some((1.5, 66.0, 1.5)),
        ..scene("mob_with_owner", (4.5, 66.8, 8.5), (4.0, 0.0, 0.0), 3)
    },
    Scene {
        mobs: &[mob(Kind::Blaze, (8.5, 66.0, 8.5))],
        real: true,
        ..scene("blaze_real", (4.5, 66.8, 8.5), (4.0, 0.0, 0.0), 3)
    },
    Scene {
        mobs: &[
            mob(Kind::Removed, (8.5, 66.0, 8.5)),
            zombie((11.5, 66.0, 8.5)),
        ],
        ..scene("mob_removed", (4.5, 66.8, 8.5), (8.0, 0.0, 0.0), 3)
    },
    Scene {
        mobs: &[
            mob(Kind::Cloud, (8.5, 66.0, 8.5)),
            zombie((11.5, 66.0, 8.5)),
        ],
        ..scene("cloud_skipped", (4.5, 66.8, 8.5), (8.0, 0.0, 0.0), 3)
    },
];

/// Several things on the path: the nearest wins, whatever the order they are found in.
const ORDER: &[Scene] = &[
    Scene {
        mobs: &[zombie((8.5, 66.0, 8.5)), zombie((11.5, 66.0, 8.5))],
        ..scene("mobs_east", (5.0, 66.8, 8.5), (8.0, 0.0, 0.0), 2)
    },
    Scene {
        mobs: &[zombie((8.5, 66.0, 8.5)), zombie((11.5, 66.0, 8.5))],
        ..scene("mobs_west", (15.0, 66.8, 8.5), (-8.0, 0.0, 0.0), 2)
    },
    Scene {
        blocks: &[stone(11, 66, 8)],
        mobs: &[zombie((8.5, 66.0, 8.5))],
        ..scene("mob_before_block", (5.0, 66.8, 8.5), (8.0, 0.0, 0.0), 2)
    },
    Scene {
        blocks: &[stone(8, 66, 8)],
        mobs: &[zombie((11.5, 66.0, 8.5))],
        ..scene("block_before_mob", (5.0, 66.8, 8.5), (8.0, 0.0, 0.0), 2)
    },
    Scene {
        blocks: &[stone(11, 66, 8)],
        mobs: &[zombie((8.5, 66.0, 8.5))],
        ..scene(
            "block_after_mob_west",
            (14.0, 66.8, 8.5),
            (-8.0, 0.0, 0.0),
            2,
        )
    },
    Scene {
        blocks: &[stone(8, 66, 8)],
        mobs: &[zombie((11.5, 66.0, 8.5))],
        ..scene(
            "mob_after_block_west",
            (14.0, 66.8, 8.5),
            (-8.0, 0.0, 0.0),
            2,
        )
    },
];

/// Other projectiles, owners and turning back.
const OWNERS: &[Scene] = &[
    Scene {
        mobs: &[mob(Kind::Snowball, (8.5, 66.8, 8.5))],
        ..scene("proj_skipped", (4.5, 66.8, 8.5), (4.0, 0.0, 0.0), 3)
    },
    Scene {
        mobs: &[mob(Kind::Snowball, (8.5, 66.8, 8.5))],
        collides_with_projectiles: true,
        ..scene("proj_hit", (4.5, 66.8, 8.5), (4.0, 0.0, 0.0), 3)
    },
    Scene {
        owner: Some((4.5, 66.0, 4.5)),
        ..scene("owner_inside", (4.5, 67.0, 4.5), (0.1, 0.0, 0.0), 5)
    },
    Scene {
        owner: Some((4.5, 66.0, 4.5)),
        ..scene("owner_pass", (4.5, 67.0, 4.5), (1.0, 0.0, 0.0), 5)
    },
    Scene {
        owner: Some((4.5, 66.0, 4.5)),
        turn: Some((4, (-3.0, 0.0, 0.0))),
        ..scene("owner_back", (4.5, 67.0, 4.5), (3.0, 0.0, 0.0), 8)
    },
    Scene {
        owner: Some((4.5, 66.0, 4.5)),
        turn: Some((2, (-3.0, 0.0, 0.0))),
        ..scene("owner_too_soon", (4.5, 67.0, 4.5), (0.3, 0.0, 0.0), 6)
    },
];

fn scenes() -> impl Iterator<Item = &'static Scene> {
    [
        FLIGHT, FACES, ROWS, SHAPES, TARGETS, BORDERS, MOBS, ORDER, OWNERS,
    ]
    .into_iter()
    .flatten()
}

struct Area<'a> {
    world: &'a Arc<World>,
    x: i32,
    z: i32,
}

impl Area<'_> {
    fn put(&self, block: &Block, x: i32, y: i32, z: i32) {
        self.world.set_block_state(
            &BlockPos(Vector3::new(self.x + x, y, self.z + z)),
            block.default_state.id,
            BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
        );
    }

    fn point(&self, (x, y, z): Vec3) -> Vector3<f64> {
        Vector3::new(f64::from(self.x) + x, y, f64::from(self.z) + z)
    }

    fn relative(&self, v: Vector3<f64>) -> Vec3 {
        (v.x - f64::from(self.x), v.y, v.z - f64::from(self.z))
    }

    fn cell(&self, (x, y, z): (i32, i32, i32)) -> BlockPos {
        BlockPos(Vector3::new(self.x + x, y, self.z + z))
    }
}

const fn block_of(solid: Solid) -> &'static Block {
    match solid {
        Solid::Stone => &Block::STONE,
        Solid::Target => &Block::TARGET,
        Solid::Stair => &Block::OAK_STAIRS,
        Solid::Fence => &Block::OAK_FENCE,
        Solid::Slab => &Block::STONE_SLAB,
    }
}

/// A projectile that records the hits it is given.
struct Probe {
    thrown: ThrownItemEntity,
    origin: (f64, f64),
    hits: Mutex<Vec<String>>,
}

fn triple((x, y, z): Vec3) -> String {
    format!("{x:.4},{y:.4},{z:.4}")
}

impl Probe {
    fn at(&self, v: Vector3<f64>) -> String {
        triple((v.x - self.origin.0, v.y, v.z - self.origin.1))
    }

    fn describe(&self, hit: &ProjectileHit) -> String {
        let normal = hit.normal();
        let normal = triple((normal.x, normal.y, normal.z));
        match hit {
            ProjectileHit::Block { pos, face, .. } => format!(
                "block {},{},{} face {face:?} at {} normal {normal}",
                f64::from(pos.0.x) - self.origin.0,
                pos.0.y,
                f64::from(pos.0.z) - self.origin.1,
                self.at(hit.hit_pos()),
            ),
            ProjectileHit::Entity { entity, .. } => format!(
                "entity {} at {} normal {normal}",
                entity.get_entity().entity_id,
                self.at(hit.hit_pos()),
            ),
        }
    }
}

impl EntityBase for Probe {
    fn get_projectile_owner(&self) -> Option<&ProjectileOwner> {
        Some(&self.thrown.owner)
    }

    fn tick(&self, caller: &dyn EntityBase, _server: &Server) {
        self.thrown.process_tick(caller);
    }

    fn get_entity(&self) -> &Entity {
        &self.thrown.entity
    }

    fn get_living_entity(&self) -> Option<&LivingEntity> {
        None
    }

    fn cast_any(&self) -> &dyn std::any::Any {
        self
    }

    fn on_hit(&self, hit: ProjectileHit) {
        let text = self.describe(&hit);
        self.hits
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(text);
    }
}

enum Shot {
    Probe(Arc<Probe>),
    Snow(Arc<SnowballEntity>),
}

impl Shot {
    fn base(&self) -> Arc<dyn EntityBase> {
        match self {
            Self::Probe(probe) => probe.clone(),
            Self::Snow(snow) => snow.clone(),
        }
    }

    fn thrown(&self) -> &ThrownItemEntity {
        match self {
            Self::Probe(probe) => &probe.thrown,
            Self::Snow(snow) => &snow.thrown,
        }
    }

    fn take_hits(&self) -> Vec<String> {
        match self {
            Self::Probe(probe) => {
                std::mem::take(&mut *probe.hits.lock().unwrap_or_else(PoisonError::into_inner))
            }
            Self::Snow(_) => Vec::new(),
        }
    }
}

fn spawn_mob(area: &Area<'_>, slot: usize, mob: Mob) -> Option<Arc<LivingEntity>> {
    let id = 91_000 + slot as i32;
    let uuid = Uuid::from_u128(0x5468_726f_776e_0000 + slot as u128);
    let r#type = match mob.kind {
        Kind::Zombie | Kind::Removed => &EntityType::ZOMBIE,
        Kind::Blaze => &EntityType::BLAZE,
        Kind::Cloud => &EntityType::AREA_EFFECT_CLOUD,
        Kind::Snowball => &EntityType::SNOWBALL,
    };
    let entity =
        Entity::from_uuid_with_id(id, uuid, Arc::clone(area.world), area.point(mob.at), r#type);
    match mob.kind {
        Kind::Cloud => {
            area.world
                .spawn_entity_non_save(AreaEffectCloudEntity::new(entity));
            None
        }
        Kind::Snowball => {
            let base: Arc<dyn EntityBase> = Arc::new(SnowballEntity::new(entity));
            area.world.spawn_entity_non_save(base);
            None
        }
        Kind::Zombie | Kind::Removed | Kind::Blaze => {
            let living = Arc::new(LivingEntity::new(entity));
            let base: Arc<dyn EntityBase> = living.clone();
            area.world.spawn_entity_non_save(base);
            if matches!(mob.kind, Kind::Removed) {
                living.get_entity().remove();
            }
            Some(living)
        }
    }
}

fn make_shot(area: &Area<'_>, scene: &Scene, owner: Option<&Arc<LivingEntity>>) -> Shot {
    let entity = Entity::from_uuid_with_id(
        92_000,
        Uuid::from_u128(0x5468_726f_776e_9999),
        Arc::clone(area.world),
        area.point(scene.start),
        &EntityType::SNOWBALL,
    );
    entity.velocity.store(Vector3::new(
        scene.velocity.0,
        scene.velocity.1,
        scene.velocity.2,
    ));
    entity.touching_water.store(scene.water, Ordering::Relaxed);
    let thrown = ThrownItemEntity {
        entity,
        owner: ProjectileOwner::new(owner.map(|owner| owner.get_entity())),
        collides_with_projectiles: scene.collides_with_projectiles,
        has_hit: AtomicBool::new(scene.pre_hit),
        gravity: 0.03,
    };
    if scene.real {
        Shot::Snow(Arc::new(SnowballEntity { thrown }))
    } else {
        Shot::Probe(Arc::new(Probe {
            thrown,
            origin: (f64::from(area.x), f64::from(area.z)),
            hits: Mutex::new(Vec::new()),
        }))
    }
}

const SOUND: u8 = 119;

/// A packet by id, length and a hash of its bytes (a sound by id and length).
fn packet_text(packet: &[u8]) -> String {
    // Sounds carry a random seed (and the hurt sound a random pitch): id and length only.
    if packet.first() == Some(&SOUND) {
        return format!("{SOUND}:{}", packet.len());
    }
    let hash = packet.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!(
        "{}:{}:{:08x}",
        packet.first().copied().unwrap_or(0),
        packet.len(),
        hash as u32
    )
}

struct Watch<'a> {
    area: &'a Area<'a>,
    scene: &'a Scene,
    shot: &'a Shot,
    mobs: &'a [Arc<LivingEntity>],
}

impl Watch<'_> {
    fn state(&self, sent: &[bytes::Bytes], hits: &[String]) -> String {
        let entity = &self.shot.thrown().entity;
        let (pos, last, vel) = (
            entity.pos.load(),
            entity.last_pos.load(),
            entity.velocity.load(),
        );
        let mut text = format!(
            "pos {} last {} vel {} rot {:.3},{:.3} removed {} has_hit {}",
            triple(self.area.relative(pos)),
            triple(self.area.relative(last)),
            triple((vel.x, vel.y, vel.z)),
            entity.yaw.load(),
            entity.pitch.load(),
            entity.is_removed(),
            self.shot.thrown().has_hit.load(Ordering::Relaxed),
        );
        let _ = write!(text, " hits [{}]", hits.join("; "));
        for (solid, x, y, z) in self.scene.blocks {
            if matches!(solid, Solid::Target) {
                let state = self
                    .area
                    .world
                    .get_block_state(&self.area.cell((*x, *y, *z)));
                let _ = write!(text, " target {}", state.id);
            }
        }
        for mob in self.mobs {
            let _ = write!(
                text,
                " mob {} health {:.1} removed {}",
                mob.get_entity().entity_id,
                mob.health.load(),
                mob.get_entity().is_removed()
            );
        }
        let sent: Vec<String> = sent.iter().map(|p| packet_text(p)).collect();
        let _ = write!(
            text,
            " entities {} sent [{}]",
            self.area.world.entities.load().len(),
            sent.join(" ")
        );
        text
    }
}

fn fly(server: &Arc<Server>, watch: &Watch<'_>, tap: &mut PacketTap, report: &mut String) {
    let base = watch.shot.base();
    for tick in 1..=watch.scene.ticks {
        if let Some((at, velocity)) = watch.scene.turn
            && at == tick
        {
            base.get_entity()
                .velocity
                .store(Vector3::new(velocity.0, velocity.1, velocity.2));
        }
        EntityBase::tick(base.as_ref(), base.as_ref(), server);
        let hits = watch.shot.take_hits();
        let text = watch.state(&tap.drain(), &hits);
        let _ = writeln!(report, "{} t{tick}: {text}", watch.scene.name);
        if base.get_entity().is_removed() {
            let _ = writeln!(report, "{} removed after tick {tick}", watch.scene.name);
            return;
        }
    }
    let _ = writeln!(report, "{} still flying", watch.scene.name);
}

fn run_scene(
    server: &Arc<Server>,
    world: &Arc<World>,
    index: usize,
    scene: &Scene,
    report: &mut String,
) -> TestResult {
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
    for x in 0..24 {
        for z in 0..24 {
            area.put(&Block::STONE, x, FLOOR_Y, z);
        }
    }
    for (solid, x, y, z) in scene.blocks {
        area.put(block_of(*solid), *x, *y, *z);
    }

    let who = Identity {
        name: "ThrownWatch",
        uuid: Uuid::from_u128(0x5468_726f_7700_0000 + index as u128),
        entity_id: 61_000 + index as i32,
    };
    let (watcher, mut tap) = test_support::java_player_as(world, GameMode::Survival, &who)
        .ok_or("the watcher's client no longer owned its outgoing queue")?;
    watcher.get_entity().set_pos(area.point((0.5, 64.0, 23.5)));
    watcher.watched_section.store(Cylindrical::new(
        Vector2::new((area.x + 12) >> 4, (area.z + 12) >> 4),
        NonZero::<u8>::MIN.saturating_add(3),
    ));
    world.add_player(&watcher)?;

    let owner = scene.owner.and_then(|at| {
        spawn_mob(
            &area,
            0,
            Mob {
                kind: Kind::Zombie,
                at,
            },
        )
    });
    let mobs: Vec<Arc<LivingEntity>> = scene
        .mobs
        .iter()
        .enumerate()
        .filter_map(|(slot, mob)| spawn_mob(&area, slot + 1, *mob))
        .collect();
    let shot = make_shot(&area, scene, owner.as_ref());
    world.spawn_entity_non_save(shot.base());
    drop(tap.drain());

    let watch = Watch {
        area: &area,
        scene,
        shot: &shot,
        mobs: &mobs,
    };
    fly(server, &watch, &mut tap, report);

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
async fn thrown_item_tick() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server
        .worlds
        .load()
        .first()
        .cloned()
        .ok_or("the server has no world")?;
    let mut report = String::new();
    for (index, scene) in scenes().enumerate() {
        run_scene(&server, &world, index, scene, &mut report)?;
        for player in world.players.load().iter() {
            world.remove_player(player, false).await;
        }
    }
    server.stop();
    script_golden::bless_or_compare("thrown_item_tick.txt", &report)
}
