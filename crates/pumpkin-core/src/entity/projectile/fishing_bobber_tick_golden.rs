//! A record of what `FishingBobberEntity::process_tick` does, made before it is split
//! (`tests/golden/fishing_bobber_tick.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene is built in its own loaded area of a real server's world (stone floor at y 63).
//! A bobber is built from its public fields with the scene's start, velocity, water flag,
//! countdowns, hooked entity and owner, spawned in the world and ticked by calling `tick`
//! directly. Every tick prints the bobber's position, last position, velocity, ground and hook
//! state, the countdowns, the mobs' position and removal and the packets the watcher got (id,
//! length and a hash of the bytes: entity ids are fixed). A wait countdown at or above 100 is
//! printed as `rand` (it is rolled at random); one scene restarts the wait many times and
//! prints its smallest and largest value.

use std::fmt::Write;
use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use pumpkin_data::Block;
use pumpkin_data::entity::EntityType;
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::version::JavaMinecraftVersion;
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use super::fishing_bobber::FishingBobberEntity;
use super::owner::ProjectileOwner;
use super::snowball::SnowballEntity;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::area_effect_cloud::AreaEffectCloudEntity;
use crate::entity::living::LivingEntity;
use crate::entity::{Entity, EntityBase};
use crate::net::java::PacketTap;
use crate::server::Server;
use crate::world::World;

const FLOOR_Y: i32 = 63;
const BOBBER_ID: i32 = 92_000;
const MOB_ID: i32 = 91_000;
/// How many waits the sampling scene rolls.
const SAMPLES: usize = 12_000;

type Vec3 = (f64, f64, f64);

#[derive(Clone, Copy)]
enum Solid {
    Stone,
    Stair,
    Fence,
    Slab,
}

#[derive(Clone, Copy)]
enum Kind {
    Zombie,
    /// Marked removed, but still in the world's lists.
    Removed,
    /// Flagged removed, but still found by its id.
    Doomed,
    Cloud,
    /// A projectile, standing still.
    Snowball,
}

#[derive(Clone, Copy)]
struct Mob {
    kind: Kind,
    at: Vec3,
}

#[derive(Clone, Copy)]
enum Owner {
    None,
    /// A zombie standing here, in the world.
    At(Vec3),
    /// An entity that was never added to the world.
    Ghost,
}

#[derive(Clone, Copy)]
enum Hook {
    None,
    /// Already hooked on the scene's mob with this index.
    Mob(usize),
    /// Already hooked on an id nothing has.
    Missing,
}

/// Where the scene's area is: the swept box takes `min` and `max` of coordinates of different
/// sizes, so the same flights are also made where x, y and z are in other orders, and below 0.
#[derive(Clone, Copy)]
enum Region {
    Far,
    Corner,
    Negative,
}

struct Scene {
    name: &'static str,
    owner: Owner,
    start: Vec3,
    velocity: Vec3,
    ticks: usize,
    water: bool,
    wait: i32,
    bite: i32,
    in_ground: bool,
    hook: Hook,
    /// The velocity is replaced before this tick.
    turn: Option<(usize, Vec3)>,
    blocks: &'static [(Solid, i32, i32, i32)],
    mobs: &'static [Mob],
    region: Region,
    /// The entity id of the scene's owner (slot 0); mob `n` has this id plus `n`.
    mob_base: i32,
    /// Restart the wait this many times instead of ticking `ticks` times.
    samples: usize,
}

const fn scene(name: &'static str, start: Vec3, velocity: Vec3, ticks: usize) -> Scene {
    Scene {
        name,
        owner: Owner::None,
        start,
        velocity,
        ticks,
        water: false,
        wait: 300,
        bite: 0,
        in_ground: false,
        hook: Hook::None,
        turn: None,
        blocks: &[],
        mobs: &[],
        region: Region::Far,
        mob_base: MOB_ID,
        samples: 0,
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

const ZOMBIE_AT_8: &[Mob] = &[zombie((8.5, 66.0, 8.5))];
const STONE_AT_8: &[(Solid, i32, i32, i32)] = &[stone(8, 66, 8)];

/// Free flight in air: gravity, drag, negative velocity, chunk borders.
const FLIGHT: &[Scene] = &[
    scene("fall", (4.5, 70.0, 4.5), (0.5, 0.2, 0.1), 8),
    scene("reverse", (10.5, 70.0, 10.5), (-0.5, -0.2, -0.3), 8),
    scene("chunk_cross", (14.5, 70.0, 14.5), (0.9, 0.0, 0.9), 6),
    scene("chunk_cross_neg", (17.5, 70.0, 17.5), (-0.9, 0.0, -0.9), 6),
    scene("straight_up", (4.5, 66.0, 4.5), (0.0, 1.5, 0.0), 6),
    scene("diagonal", (4.5, 66.0, 4.5), (-0.4, 0.7, 0.9), 6),
];

/// In water: buoyancy, drag, the wait and bite countdowns, bubbles and the splash.
const WATER: &[Scene] = &[
    Scene {
        water: true,
        wait: 50,
        ..scene("water_flight", (4.5, 70.0, 4.5), (0.5, 0.2, 0.1), 6)
    },
    Scene {
        water: true,
        wait: 3,
        ..scene("wait_runs_out", (4.5, 70.0, 4.5), (0.0, 0.0, 0.0), 12)
    },
    Scene {
        water: true,
        wait: 1,
        ..scene("wait_one", (4.5, 70.0, 4.5), (0.0, 0.0, 0.0), 4)
    },
    Scene {
        water: true,
        wait: 0,
        ..scene("wait_zero", (4.5, 70.0, 4.5), (0.0, 0.0, 0.0), 3)
    },
    Scene {
        water: true,
        wait: 50,
        bite: 12,
        ..scene("bite_runs_out", (4.5, 70.0, 4.5), (0.0, 0.0, 0.0), 15)
    },
    Scene {
        water: true,
        wait: 50,
        bite: 1,
        ..scene("bite_one", (4.5, 70.0, 4.5), (0.0, 0.0, 0.0), 4)
    },
    Scene {
        water: true,
        wait: 50,
        bite: 6,
        ..scene("bite_six", (4.5, 70.0, 4.5), (0.0, 0.0, 0.0), 8)
    },
    Scene {
        water: true,
        wait: 2,
        ..scene("whole_cycle", (4.5, 70.0, 4.5), (0.0, 0.0, 0.0), 50)
    },
    Scene {
        water: true,
        samples: SAMPLES,
        ..scene("wait_range", (4.5, 70.0, 4.5), (0.0, 0.0, 0.0), 0)
    },
];

/// One stone block at (8, 66, 8): each face, a graze, a slow approach, a start inside it.
const FACES: &[Scene] = &[
    Scene {
        blocks: STONE_AT_8,
        ..scene("face_west", (5.0, 66.5, 8.5), (2.0, 0.0, 0.0), 3)
    },
    Scene {
        blocks: STONE_AT_8,
        ..scene("face_east", (12.0, 66.5, 8.5), (-2.0, 0.0, 0.0), 3)
    },
    Scene {
        blocks: &[stone(8, 68, 8)],
        ..scene("face_down", (8.5, 65.0, 8.5), (0.0, 2.0, 0.0), 3)
    },
    Scene {
        blocks: STONE_AT_8,
        ..scene("face_up", (8.5, 70.0, 8.5), (0.0, -2.0, 0.0), 3)
    },
    Scene {
        blocks: STONE_AT_8,
        ..scene("face_north", (8.5, 66.5, 5.0), (0.0, 0.0, 2.0), 3)
    },
    Scene {
        blocks: STONE_AT_8,
        ..scene("face_south", (8.5, 66.5, 12.0), (0.0, 0.0, -2.0), 3)
    },
    Scene {
        blocks: STONE_AT_8,
        ..scene("approach", (4.5, 66.5, 8.5), (0.3, 0.0, 0.0), 12)
    },
    Scene {
        blocks: STONE_AT_8,
        ..scene("inside", (8.5, 66.5, 8.5), (0.1, 0.0, 0.0), 3)
    },
    Scene {
        blocks: STONE_AT_8,
        ..scene("graze", (4.5, 66.5, 9.6), (2.0, 0.0, 0.0), 3)
    },
    Scene {
        blocks: STONE_AT_8,
        ..scene("miss", (4.5, 66.5, 9.8), (2.0, 0.0, 0.0), 3)
    },
    Scene {
        blocks: STONE_AT_8,
        water: true,
        ..scene("water_hit", (5.0, 66.5, 8.5), (2.0, 0.0, 0.0), 3)
    },
    Scene {
        in_ground: true,
        ..scene("already_stuck", (4.5, 70.0, 4.5), (0.5, 0.2, 0.1), 3)
    },
    Scene {
        in_ground: true,
        water: true,
        wait: 0,
        ..scene("stuck_in_water", (4.5, 70.0, 4.5), (0.0, 0.0, 0.0), 3)
    },
    scene("land_on_floor", (4.5, 65.6, 4.5), (0.0, -0.3, 0.0), 6),
    Scene {
        blocks: &[
            (Solid::Stair, 8, 66, 8),
            (Solid::Fence, 10, 66, 8),
            (Solid::Slab, 12, 66, 8),
        ],
        ..scene("shapes_east", (5.0, 66.4, 8.5), (1.0, 0.0, 0.0), 14)
    },
    Scene {
        blocks: &[stone(16, 66, 8)],
        ..scene("border_east", (13.0, 66.5, 8.5), (1.0, 0.0, 0.0), 4)
    },
    Scene {
        blocks: &[stone(15, 66, 8)],
        ..scene("border_west", (19.0, 66.5, 8.5), (-1.0, 0.0, 0.0), 4)
    },
    Scene {
        blocks: &[stone(8, 66, 16)],
        ..scene("border_south", (8.5, 66.5, 13.0), (0.0, 0.0, 1.0), 4)
    },
    Scene {
        blocks: &[stone(8, 66, 15)],
        ..scene("border_north", (8.5, 66.5, 19.0), (0.0, 0.0, -1.0), 4)
    },
];

/// The faces again where the coordinates are in other orders and below 0.
const REGIONS: &[Scene] = &[
    region_face(
        "corner_west",
        Region::Corner,
        (5.0, 66.5, 8.5),
        (2.0, 0.0, 0.0),
    ),
    region_face(
        "corner_east",
        Region::Corner,
        (12.0, 66.5, 8.5),
        (-2.0, 0.0, 0.0),
    ),
    region_face(
        "corner_up",
        Region::Corner,
        (8.5, 70.0, 8.5),
        (0.0, -2.0, 0.0),
    ),
    region_face(
        "corner_north",
        Region::Corner,
        (8.5, 66.5, 5.0),
        (0.0, 0.0, 2.0),
    ),
    region_face(
        "corner_south",
        Region::Corner,
        (8.5, 66.5, 12.0),
        (0.0, 0.0, -2.0),
    ),
    region_face(
        "negative_west",
        Region::Negative,
        (5.0, 66.5, 8.5),
        (2.0, 0.0, 0.0),
    ),
    region_face(
        "negative_east",
        Region::Negative,
        (12.0, 66.5, 8.5),
        (-2.0, 0.0, 0.0),
    ),
    region_face(
        "negative_up",
        Region::Negative,
        (8.5, 70.0, 8.5),
        (0.0, -2.0, 0.0),
    ),
    region_face(
        "negative_north",
        Region::Negative,
        (8.5, 66.5, 5.0),
        (0.0, 0.0, 2.0),
    ),
    region_face(
        "negative_south",
        Region::Negative,
        (8.5, 66.5, 12.0),
        (0.0, 0.0, -2.0),
    ),
];

const fn region_face(name: &'static str, region: Region, start: Vec3, velocity: Vec3) -> Scene {
    Scene {
        blocks: STONE_AT_8,
        region,
        ..scene(name, start, velocity, 3)
    }
}

/// One zombie at (8.5, 66, 8.5) (box half width 0.3, grown by 0.3 for the hook search, and by
/// the swept box): hooked from each side, a graze and a miss.
const MOBS: &[Scene] = &[
    Scene {
        mobs: ZOMBIE_AT_8,
        ..scene("mob_west", (6.5, 66.8, 8.5), (1.0, 0.0, 0.0), 4)
    },
    Scene {
        mobs: ZOMBIE_AT_8,
        ..scene("mob_east", (10.5, 66.8, 8.5), (-1.0, 0.0, 0.0), 4)
    },
    Scene {
        mobs: ZOMBIE_AT_8,
        ..scene("mob_top", (8.5, 69.0, 8.5), (0.0, -0.5, 0.0), 8)
    },
    Scene {
        mobs: ZOMBIE_AT_8,
        ..scene("mob_below", (8.5, 64.4, 8.5), (0.0, 1.0, 0.0), 4)
    },
    Scene {
        mobs: ZOMBIE_AT_8,
        ..scene("mob_north", (8.5, 66.8, 6.5), (0.0, 0.0, 1.0), 4)
    },
    Scene {
        mobs: ZOMBIE_AT_8,
        ..scene("mob_south", (8.5, 66.8, 10.5), (0.0, 0.0, -1.0), 4)
    },
    Scene {
        mobs: ZOMBIE_AT_8,
        ..scene("mob_graze", (6.5, 66.8, 9.05), (1.0, 0.0, 0.0), 4)
    },
    Scene {
        mobs: ZOMBIE_AT_8,
        ..scene("mob_miss", (6.5, 66.8, 9.45), (1.0, 0.0, 0.0), 4)
    },
    Scene {
        mobs: ZOMBIE_AT_8,
        ..scene("mob_graze_x", (9.05, 66.8, 6.5), (0.0, 0.0, 1.0), 4)
    },
    Scene {
        mobs: ZOMBIE_AT_8,
        ..scene("mob_miss_x", (9.45, 66.8, 6.5), (0.0, 0.0, 1.0), 4)
    },
    Scene {
        mobs: ZOMBIE_AT_8,
        water: true,
        wait: 0,
        ..scene("mob_in_water", (6.5, 66.8, 8.5), (1.0, 0.0, 0.0), 4)
    },
    Scene {
        mobs: &[
            mob(Kind::Removed, (8.5, 66.0, 8.5)),
            zombie((11.5, 66.0, 8.5)),
        ],
        ..scene("mob_removed", (6.5, 66.8, 8.5), (3.0, 0.0, 0.0), 3)
    },
    Scene {
        mobs: &[mob(Kind::Cloud, (8.5, 66.0, 8.5))],
        ..scene("cloud_hooked", (6.5, 66.8, 8.5), (1.0, 0.0, 0.0), 4)
    },
    Scene {
        mobs: &[
            mob(Kind::Snowball, (8.5, 66.8, 8.5)),
            zombie((11.5, 66.0, 8.5)),
        ],
        ..scene("projectile_skipped", (6.5, 66.8, 8.5), (3.0, 0.0, 0.0), 3)
    },
    Scene {
        mobs: &[zombie((8.5, 66.0, 8.5)), zombie((11.5, 66.0, 8.5))],
        ..scene("two_mobs", (6.5, 66.8, 8.5), (4.0, 0.0, 0.0), 3)
    },
    Scene {
        mobs: &[zombie((11.5, 66.0, 8.5)), zombie((8.5, 66.0, 8.5))],
        ..scene("two_mobs_swapped", (6.5, 66.8, 8.5), (4.0, 0.0, 0.0), 3)
    },
    Scene {
        blocks: &[stone(9, 66, 8)],
        mobs: ZOMBIE_AT_8,
        ..scene("block_beside_mob", (6.5, 66.8, 8.5), (2.0, 0.0, 0.0), 3)
    },
    Scene {
        blocks: &[stone(8, 66, 8)],
        mobs: &[zombie((11.5, 66.0, 8.5))],
        ..scene("block_before_mob", (5.0, 66.8, 8.5), (3.0, 0.0, 0.0), 3)
    },
];

/// Already hooked: it follows the mob; a removed or missing target is let go.
const HOOKS: &[Scene] = &[
    Scene {
        mobs: ZOMBIE_AT_8,
        hook: Hook::Mob(0),
        ..scene("hooked_follows", (4.5, 70.0, 4.5), (0.5, 0.2, 0.1), 3)
    },
    Scene {
        mobs: ZOMBIE_AT_8,
        hook: Hook::Mob(0),
        water: true,
        wait: 0,
        ..scene("hooked_in_water", (4.5, 70.0, 4.5), (0.5, 0.2, 0.1), 3)
    },
    Scene {
        mobs: &[mob(Kind::Removed, (8.5, 66.0, 8.5))],
        hook: Hook::Mob(0),
        ..scene("hooked_removed", (4.5, 70.0, 4.5), (0.5, 0.2, 0.1), 4)
    },
    Scene {
        hook: Hook::Missing,
        ..scene("hooked_missing", (4.5, 70.0, 4.5), (0.5, 0.2, 0.1), 4)
    },
    Scene {
        mobs: ZOMBIE_AT_8,
        hook: Hook::Mob(0),
        in_ground: true,
        ..scene("hooked_but_stuck", (4.5, 70.0, 4.5), (0.5, 0.2, 0.1), 3)
    },
];

/// The owner: not hooked while the bobber is still on it, hooked once it has left, a vanished
/// owner counts as left.
const OWNERS: &[Scene] = &[
    Scene {
        owner: Owner::At((4.5, 66.0, 4.5)),
        ..scene("owner_inside", (4.5, 67.0, 4.5), (0.01, 0.0, 0.0), 5)
    },
    Scene {
        owner: Owner::At((4.5, 66.0, 4.5)),
        ..scene("owner_leaves", (4.5, 67.0, 4.5), (0.3, 0.0, 0.0), 12)
    },
    Scene {
        owner: Owner::At((4.5, 66.0, 4.5)),
        turn: Some((8, (-0.6, 0.0, 0.0))),
        ..scene("owner_back", (4.5, 67.0, 4.5), (0.3, 0.0, 0.0), 14)
    },
    Scene {
        owner: Owner::Ghost,
        mobs: ZOMBIE_AT_8,
        ..scene("owner_ghost", (6.5, 66.8, 8.5), (1.0, 0.0, 0.0), 4)
    },
    Scene {
        owner: Owner::At((1.5, 66.0, 1.5)),
        mobs: ZOMBIE_AT_8,
        ..scene("owner_far", (6.5, 66.8, 8.5), (1.0, 0.0, 0.0), 4)
    },
];

/// Round 2 (added after a mutation run): owners hooked once left (in water, so the bobber does
/// not fall past them), a target flagged removed but still found, entity id 1, tracked data.
const WIDEN: &[Scene] = &[
    Scene {
        owner: Owner::At((4.5, 66.0, 4.5)),
        water: true,
        turn: Some((8, (-0.6, 0.0, 0.0))),
        ..scene("owner_back_water", (4.5, 67.0, 4.5), (0.3, 0.0, 0.0), 22)
    },
    Scene {
        owner: Owner::At((4.5, 66.0, 4.5)),
        water: true,
        ..scene("owner_inside_water", (4.5, 67.0, 4.5), (0.01, 0.0, 0.0), 6)
    },
    Scene {
        mobs: &[mob(Kind::Doomed, (8.5, 66.0, 8.5))],
        hook: Hook::Mob(0),
        ..scene("hooked_doomed", (4.5, 70.0, 4.5), (0.5, 0.2, 0.1), 4)
    },
    Scene {
        mobs: ZOMBIE_AT_8,
        hook: Hook::Mob(0),
        mob_base: 0,
        ..scene("hooked_id_one", (4.5, 70.0, 4.5), (0.5, 0.2, 0.1), 3)
    },
    Scene {
        mobs: ZOMBIE_AT_8,
        mob_base: 0,
        ..scene("hook_id_one", (6.5, 66.8, 8.5), (1.0, 0.0, 0.0), 4)
    },
];

fn scenes() -> impl Iterator<Item = &'static Scene> {
    [FLIGHT, WATER, FACES, REGIONS, MOBS, HOOKS, OWNERS, WIDEN]
        .into_iter()
        .flatten()
}

struct Area<'a> {
    world: &'a Arc<World>,
    x: i32,
    z: i32,
    dy: i32,
}

impl<'a> Area<'a> {
    fn new(world: &'a Arc<World>, index: usize, region: Region) -> Self {
        let (x, z, dy) = match region {
            Region::Far => (index as i32 * 64 + 16, 16, 0),
            Region::Corner => (16, 208, 0),
            Region::Negative => (-112, -48, -80),
        };
        Self { world, x, z, dy }
    }

    /// The four-by-four chunks around the area.
    fn chunks(&self) -> impl Iterator<Item = Vector2<i32>> {
        let (x, z) = ((self.x >> 4) - 1, (self.z >> 4) - 1);
        (x..x + 4).flat_map(move |cx| (z..z + 4).map(move |cz| Vector2::new(cx, cz)))
    }

    fn put(&self, block: &Block, x: i32, y: i32, z: i32) {
        self.world.set_block_state(
            &self.cell((x, y, z)),
            block.default_state.id,
            BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
        );
    }

    fn point(&self, (x, y, z): Vec3) -> Vector3<f64> {
        Vector3::new(
            f64::from(self.x) + x,
            y + f64::from(self.dy),
            f64::from(self.z) + z,
        )
    }

    fn relative(&self, v: Vector3<f64>) -> Vec3 {
        (
            v.x - f64::from(self.x),
            v.y - f64::from(self.dy),
            v.z - f64::from(self.z),
        )
    }

    fn cell(&self, (x, y, z): (i32, i32, i32)) -> BlockPos {
        BlockPos(Vector3::new(self.x + x, y + self.dy, self.z + z))
    }
}

const fn block_of(solid: Solid) -> &'static Block {
    match solid {
        Solid::Stone => &Block::STONE,
        Solid::Stair => &Block::OAK_STAIRS,
        Solid::Fence => &Block::OAK_FENCE,
        Solid::Slab => &Block::STONE_SLAB,
    }
}

fn triple((x, y, z): Vec3) -> String {
    format!("{x:.4},{y:.4},{z:.4}")
}

fn spawn_mob(area: &Area<'_>, base: i32, slot: usize, mob: Mob) -> Option<Arc<LivingEntity>> {
    let id = base + slot as i32;
    let uuid = Uuid::from_u128(0x4669_7368_0000_0000 + slot as u128);
    let r#type = match mob.kind {
        Kind::Zombie | Kind::Removed | Kind::Doomed => &EntityType::ZOMBIE,
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
        Kind::Zombie | Kind::Removed | Kind::Doomed => {
            let living = Arc::new(LivingEntity::new(entity));
            let base: Arc<dyn EntityBase> = living.clone();
            area.world.spawn_entity_non_save(base);
            match mob.kind {
                Kind::Removed => living.get_entity().remove(),
                Kind::Doomed => living.get_entity().removed.store(true, Ordering::Relaxed),
                _ => {}
            }
            Some(living)
        }
    }
}

/// The owner the scene asks for (a zombie in the world, or one that is not).
fn make_owner(area: &Area<'_>, scene: &Scene) -> (ProjectileOwner, Option<Arc<LivingEntity>>) {
    match scene.owner {
        Owner::None => (ProjectileOwner::new(None), None),
        Owner::At(at) => {
            let living = spawn_mob(area, scene.mob_base, 0, zombie(at));
            let owner = ProjectileOwner::new(living.as_ref().map(|owner| owner.get_entity()));
            (owner, living)
        }
        Owner::Ghost => {
            let entity = Entity::from_uuid_with_id(
                MOB_ID + 500,
                Uuid::from_u128(0x4669_7368_0000_0500),
                Arc::clone(area.world),
                area.point((8.5, 66.0, 8.5)),
                &EntityType::ZOMBIE,
            );
            (ProjectileOwner::new(Some(&entity)), None)
        }
    }
}

fn make_bobber(area: &Area<'_>, scene: &Scene, owner: ProjectileOwner) -> Arc<FishingBobberEntity> {
    let entity = Entity::from_uuid_with_id(
        BOBBER_ID,
        Uuid::from_u128(0x4669_7368_0000_9999),
        Arc::clone(area.world),
        area.point(scene.start),
        &EntityType::FISHING_BOBBER,
    );
    entity.velocity.store(Vector3::new(
        scene.velocity.0,
        scene.velocity.1,
        scene.velocity.2,
    ));
    entity.touching_water.store(scene.water, Ordering::Relaxed);
    let hooked = match scene.hook {
        Hook::None => 0,
        Hook::Mob(slot) => scene.mob_base + 1 + slot as i32,
        Hook::Missing => MOB_ID + 700,
    };
    Arc::new(FishingBobberEntity {
        entity,
        owner,
        hooked_entity_id: AtomicI32::new(hooked),
        in_ground: AtomicBool::new(scene.in_ground),
        has_hit: AtomicBool::new(false),
        wait_countdown: AtomicI32::new(scene.wait),
        bite_countdown: AtomicI32::new(scene.bite),
    })
}

const SOUND: u8 = 119;

/// A packet by id, length and a hash of its bytes (a sound by id and length).
fn packet_text(packet: &[u8]) -> String {
    // Sounds carry a random seed: id and length only.
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

/// The wait countdown is rolled at random once it passes 100.
fn wait_text(wait: i32) -> String {
    if wait >= 100 {
        "rand".to_string()
    } else {
        wait.to_string()
    }
}

/// The tracked data changed since the last look, by a hash of its bytes.
fn meta_text(entity: &Entity) -> Option<String> {
    let data = &entity.synched_data;
    let bytes = data.pack_dirty_for_version(&JavaMinecraftVersion::V_26_3)?;
    data.clear_dirty();
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    });
    Some(format!("{:08x}", hash as u32))
}

struct Watch<'a> {
    area: &'a Area<'a>,
    scene: &'a Scene,
    bobber: &'a FishingBobberEntity,
    base: &'a Arc<dyn EntityBase>,
    mobs: &'a [Arc<LivingEntity>],
}

impl Watch<'_> {
    fn state(&self, sent: &[bytes::Bytes]) -> String {
        let entity = &self.bobber.entity;
        let (pos, last, vel) = (
            entity.pos.load(),
            entity.last_pos.load(),
            entity.velocity.load(),
        );
        let mut text = format!(
            "pos {} last {} vel {} removed {} in_ground {} has_hit {} hooked {} wait {} bite {}",
            triple(self.area.relative(pos)),
            triple(self.area.relative(last)),
            triple((vel.x, vel.y, vel.z)),
            entity.is_removed(),
            self.bobber.in_ground.load(Ordering::Relaxed),
            self.bobber.has_hit.load(Ordering::Relaxed),
            self.bobber.hooked_entity_id.load(Ordering::Relaxed),
            wait_text(self.bobber.wait_countdown.load(Ordering::Relaxed)),
            self.bobber.bite_countdown.load(Ordering::Relaxed),
        );
        for mob in self.mobs {
            let _ = write!(
                text,
                " mob {} at {} removed {}",
                mob.get_entity().entity_id,
                triple(self.area.relative(mob.get_entity().pos.load())),
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
    let base = watch.base;
    for tick in 1..=watch.scene.ticks {
        if let Some((at, velocity)) = watch.scene.turn
            && at == tick
        {
            base.get_entity()
                .velocity
                .store(Vector3::new(velocity.0, velocity.1, velocity.2));
        }
        EntityBase::tick(base.as_ref(), base.as_ref(), server);
        let text = watch.state(&tap.drain());
        let _ = writeln!(report, "{} t{tick}: {text}", watch.scene.name);
        if let Some(meta) = meta_text(&watch.bobber.entity) {
            let _ = writeln!(report, "{} t{tick} meta {meta}", watch.scene.name);
        }
    }
    let _ = writeln!(report, "{} done", watch.scene.name);
}

/// Restarts the wait over and over in water and reports the extremes of what was rolled.
fn sample(server: &Arc<Server>, watch: &Watch<'_>, tap: &mut PacketTap, report: &mut String) {
    let base = watch.base;
    let bobber = watch.bobber;
    let (mut low, mut high, mut bites) = (i32::MAX, i32::MIN, 0);
    for _ in 0..watch.scene.samples {
        bobber.entity.set_pos(watch.area.point(watch.scene.start));
        bobber.entity.velocity.store(Vector3::new(0.0, 0.0, 0.0));
        bobber.wait_countdown.store(0, Ordering::Relaxed);
        bobber.bite_countdown.store(0, Ordering::Relaxed);
        EntityBase::tick(base.as_ref(), base.as_ref(), server);
        let wait = bobber.wait_countdown.load(Ordering::Relaxed);
        low = low.min(wait);
        high = high.max(wait);
        if bobber.bite_countdown.load(Ordering::Relaxed) == 40 {
            bites += 1;
        }
        drop(tap.drain());
    }
    let _ = writeln!(
        report,
        "{} wait min {low} max {high} bites {bites} of {}",
        watch.scene.name, watch.scene.samples
    );
}

fn run_scene(
    server: &Arc<Server>,
    world: &Arc<World>,
    index: usize,
    scene: &Scene,
    report: &mut String,
) -> TestResult {
    let area = Area::new(world, index, scene.region);
    for chunk in area.chunks() {
        world.level.loaded_chunks.insert(
            chunk,
            pumpkin_world::chunk::ChunkData::empty_sync(chunk.x, chunk.y),
        );
    }
    for x in 0..24 {
        for z in 0..24 {
            area.put(&Block::STONE, x, FLOOR_Y, z);
        }
    }
    for (solid, x, y, z) in scene.blocks {
        area.put(block_of(*solid), *x, *y, *z);
    }

    let who = Identity {
        name: "BobberWatch",
        uuid: Uuid::from_u128(0x4669_7368_7700_0000 + index as u128),
        entity_id: 62_000 + index as i32,
    };
    let (watcher, mut tap) = test_support::java_player_as(world, GameMode::Survival, &who)
        .ok_or("the watcher's client no longer owned its outgoing queue")?;
    watcher.get_entity().set_pos(area.point((0.5, 64.0, 23.5)));
    watcher.watched_section.store(Cylindrical::new(
        Vector2::new((area.x + 12) >> 4, (area.z + 12) >> 4),
        NonZero::<u8>::MIN.saturating_add(3),
    ));
    world.add_player(&watcher)?;

    let (owner, owner_mob) = make_owner(&area, scene);
    let mobs: Vec<Arc<LivingEntity>> = scene
        .mobs
        .iter()
        .enumerate()
        .filter_map(|(slot, mob)| spawn_mob(&area, scene.mob_base, slot + 1, *mob))
        .collect();
    let bobber = make_bobber(&area, scene, owner);
    let base: Arc<dyn EntityBase> = bobber.clone();
    world.spawn_entity_non_save(Arc::clone(&base));
    drop(tap.drain());

    let mut shown = mobs;
    shown.extend(owner_mob);
    let watch = Watch {
        area: &area,
        scene,
        bobber: &bobber,
        base: &base,
        mobs: &shown,
    };
    if scene.samples > 0 {
        sample(server, &watch, &mut tap, report);
    } else {
        fly(server, &watch, &mut tap, report);
    }

    for entity in world.entities.load().iter() {
        world.remove_entity(entity.as_ref());
    }
    for chunk in area.chunks() {
        world.level.loaded_chunks.remove(&chunk);
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn fishing_bobber_tick() -> TestResult {
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
    script_golden::bless_or_compare("fishing_bobber_tick.txt", &report)
}
