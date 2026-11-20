//! A record of what `ArrowEntity::tick` and `ArrowEntity::on_hit` do, made before the two
//! functions were split (`tests/golden/arrow_tick.txt`; rewrite it on purpose with
//! `KVASIR_BLESS=1`).
//!
//! Each scene is built in its own loaded area of a real server's world (stone floor at y 63,
//! blocks placed without neighbour updates), its mobs and arrows are spawned with fixed entity
//! ids and UUIDs and the arrows are ticked 60 times by calling `tick` directly, in spawn order
//! (the mobs stay where they are). A survival player stands in a corner of every area as a
//! watcher: it is the owner of some arrows and its packet queue is read after every tick.
//!
//! After every tick each arrow is recorded: position, velocity, rotation, in-ground state,
//! shake, life and in-ground time, the hit flag, the pierced list, critical and flame flags,
//! fire ticks, removed; each mob: health, effects (sorted), position, velocity, fire ticks,
//! removed; the states of the scene's watched blocks, the number of entities in the world and
//! what the watcher was sent (Add Entity and sound packets by length only: their bytes are
//! random like in vanilla).
//!
//! Random in the engine and so not recorded exactly: the critical bonus (a scene with many
//! arrows records the range of health lost and whether any arrow got a bonus), the sound pitch,
//! and the speed an arrow keeps when its block is broken (0 here: it stopped on hitting it).
//!
//! `KVASIR_DUMP=<file>` appends every recorded tick as text, to find what changed.

use std::fmt::Write;
use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

use pumpkin_data::Block;
use pumpkin_data::data_component::DataComponent;
use pumpkin_data::data_component_impl::{
    DataComponentImpl, PotionContentsImpl, PotionDurationScaleImpl,
};
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::sound::Sound;
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::version::JavaMinecraftVersion;
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use super::arrow::{ArrowEntity, ArrowPickup};
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::item::ItemEntity;
use crate::entity::living::LivingEntity;
use crate::entity::player::Player;
use crate::entity::{Entity, EntityBase};
use crate::net::java::PacketTap;
use crate::server::Server;
use crate::world::World;

const TICKS: usize = 60;
const FLOOR_Y: i32 = 63;
/// Packet ids whose bytes are random like in vanilla: Add Entity (UUID, velocity) and the two
/// sound packets (seed).
const RANDOM_PACKETS: [u8; 2] = [1, 118];

/// FNV-1a: a hash that does not change with the Rust version.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

type Pos = (i32, i32, i32);
type Vec3 = (f64, f64, f64);

/// A box of one block kind, corners inclusive, relative to the scene's corner.
#[derive(Clone, Copy)]
struct Fill(&'static Block, Pos, Pos);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Arrow,
    Spectral,
    /// A tipped arrow whose potion duration is scaled by one half.
    Tipped,
    /// A tipped arrow with the default duration scale.
    TippedFull,
}

#[derive(Clone, Copy)]
enum Owner {
    None,
    /// The mob with this index in the scene.
    Mob(usize),
    Watcher,
}

#[derive(Clone, Copy)]
enum Start {
    Flying,
    /// Burning with this many fire ticks.
    Burning(i32),
    /// Stuck in the block at `at` with this life and shake.
    Stuck {
        at: Pos,
        life: u32,
        shake: u8,
    },
    /// Already marked as having hit something.
    Landed,
}

#[derive(Clone, Copy)]
struct Shot {
    kind: Kind,
    at: Vec3,
    velocity: Vec3,
    crit: bool,
    flame: bool,
    pierce: u8,
    punch: u8,
    owner: Owner,
    start: Start,
    damage: Option<f64>,
    /// Touching water, as the entity's own tick would have found out.
    wet: bool,
}

const fn shot(at: Vec3, velocity: Vec3) -> Shot {
    Shot {
        kind: Kind::Arrow,
        at,
        velocity,
        crit: false,
        flame: false,
        pierce: 0,
        punch: 0,
        owner: Owner::None,
        start: Start::Flying,
        damage: None,
        wet: false,
    }
}

const fn of_kind(shot: Shot, kind: Kind) -> Shot {
    Shot { kind, ..shot }
}

const fn critical(shot: Shot) -> Shot {
    Shot { crit: true, ..shot }
}

const fn flaming(shot: Shot) -> Shot {
    Shot {
        flame: true,
        ..shot
    }
}

const fn piercing(shot: Shot, pierce: u8) -> Shot {
    Shot { pierce, ..shot }
}

const fn punching(shot: Shot, punch: u8) -> Shot {
    Shot { punch, ..shot }
}

const fn owned_by(shot: Shot, owner: Owner) -> Shot {
    Shot { owner, ..shot }
}

const fn starting(shot: Shot, start: Start) -> Shot {
    Shot { start, ..shot }
}

const fn wetted(shot: Shot) -> Shot {
    Shot { wet: true, ..shot }
}

const fn hurting(shot: Shot, damage: f64) -> Shot {
    Shot {
        damage: Some(damage),
        ..shot
    }
}

#[derive(Clone, Copy)]
struct Mob {
    kind: &'static EntityType,
    at: Vec3,
    health: f32,
    invulnerable: bool,
}

const fn mob(kind: &'static EntityType, at: Vec3) -> Mob {
    Mob {
        kind,
        at,
        health: 20.0,
        invulnerable: false,
    }
}

const fn zombie(at: Vec3) -> Mob {
    mob(&EntityType::ZOMBIE, at)
}

const fn with_health(mob: Mob, health: f32) -> Mob {
    Mob { health, ..mob }
}

const fn immune(mob: Mob) -> Mob {
    Mob {
        invulnerable: true,
        ..mob
    }
}

#[derive(Clone, Copy)]
struct Scene {
    name: &'static str,
    fills: &'static [Fill],
    shots: &'static [Shot],
    mobs: &'static [Mob],
    /// Blocks whose state is recorded after every tick.
    watch: &'static [Pos],
    /// Critical hits: record the health lost as a range instead of every mob's health.
    random: bool,
    /// Mobs die and drop loot: random items and sounds, so no packets and no item count.
    loot: bool,
    /// Arrows leave their stuck block with a random velocity: record bounds, not values.
    rolls: bool,
    ticks: usize,
}

const fn scene(
    name: &'static str,
    fills: &'static [Fill],
    shots: &'static [Shot],
    mobs: &'static [Mob],
) -> Scene {
    Scene {
        name,
        fills,
        shots,
        mobs,
        watch: &[],
        random: false,
        loot: false,
        rolls: false,
        ticks: TICKS,
    }
}

const fn watching(scene: Scene, watch: &'static [Pos]) -> Scene {
    Scene { watch, ..scene }
}

const STONE: &Block = &Block::STONE;
const WATER: &Block = &Block::WATER;
const LAVA: &Block = &Block::LAVA;

/// Three zombies in a row at x 8, 12 and 16 on each of three lanes.
const ROWS: &[Mob] = &[
    zombie((8.5, 64.0, 4.5)),
    zombie((12.5, 64.0, 4.5)),
    zombie((16.5, 64.0, 4.5)),
    zombie((8.5, 64.0, 12.5)),
    zombie((12.5, 64.0, 12.5)),
    zombie((16.5, 64.0, 12.5)),
    zombie((8.5, 64.0, 20.5)),
    zombie((12.5, 64.0, 20.5)),
    zombie((16.5, 64.0, 20.5)),
];

/// 42 critical arrows, one per lane in two columns, each at its own zombie: the bonus gives 3, 4
/// or 5 health lost (a third each), and all three show in all but one run in ten million.
const CRIT_MOBS: &[Mob] = &[
    zombie((10.5, 64.0, 1.5)),
    zombie((10.5, 64.0, 2.5)),
    zombie((10.5, 64.0, 3.5)),
    zombie((10.5, 64.0, 4.5)),
    zombie((10.5, 64.0, 5.5)),
    zombie((10.5, 64.0, 6.5)),
    zombie((10.5, 64.0, 7.5)),
    zombie((10.5, 64.0, 8.5)),
    zombie((10.5, 64.0, 9.5)),
    zombie((10.5, 64.0, 10.5)),
    zombie((10.5, 64.0, 11.5)),
    zombie((10.5, 64.0, 12.5)),
    zombie((10.5, 64.0, 13.5)),
    zombie((10.5, 64.0, 14.5)),
    zombie((10.5, 64.0, 15.5)),
    zombie((10.5, 64.0, 16.5)),
    zombie((10.5, 64.0, 17.5)),
    zombie((10.5, 64.0, 18.5)),
    zombie((10.5, 64.0, 19.5)),
    zombie((10.5, 64.0, 20.5)),
    zombie((10.5, 64.0, 21.5)),
    zombie((16.5, 64.0, 1.5)),
    zombie((16.5, 64.0, 2.5)),
    zombie((16.5, 64.0, 3.5)),
    zombie((16.5, 64.0, 4.5)),
    zombie((16.5, 64.0, 5.5)),
    zombie((16.5, 64.0, 6.5)),
    zombie((16.5, 64.0, 7.5)),
    zombie((16.5, 64.0, 8.5)),
    zombie((16.5, 64.0, 9.5)),
    zombie((16.5, 64.0, 10.5)),
    zombie((16.5, 64.0, 11.5)),
    zombie((16.5, 64.0, 12.5)),
    zombie((16.5, 64.0, 13.5)),
    zombie((16.5, 64.0, 14.5)),
    zombie((16.5, 64.0, 15.5)),
    zombie((16.5, 64.0, 16.5)),
    zombie((16.5, 64.0, 17.5)),
    zombie((16.5, 64.0, 18.5)),
    zombie((16.5, 64.0, 19.5)),
    zombie((16.5, 64.0, 20.5)),
    zombie((16.5, 64.0, 21.5)),
];

const CRIT_SHOTS: &[Shot] = &[
    critical(shot((6.5, 65.3, 1.5), (1.5, 0.0, 0.0))),
    critical(shot((6.5, 65.3, 2.5), (1.5, 0.0, 0.0))),
    critical(shot((6.5, 65.3, 3.5), (1.5, 0.0, 0.0))),
    critical(shot((6.5, 65.3, 4.5), (1.5, 0.0, 0.0))),
    critical(shot((6.5, 65.3, 5.5), (1.5, 0.0, 0.0))),
    critical(shot((6.5, 65.3, 6.5), (1.5, 0.0, 0.0))),
    critical(shot((6.5, 65.3, 7.5), (1.5, 0.0, 0.0))),
    critical(shot((6.5, 65.3, 8.5), (1.5, 0.0, 0.0))),
    critical(shot((6.5, 65.3, 9.5), (1.5, 0.0, 0.0))),
    critical(shot((6.5, 65.3, 10.5), (1.5, 0.0, 0.0))),
    critical(shot((6.5, 65.3, 11.5), (1.5, 0.0, 0.0))),
    critical(shot((6.5, 65.3, 12.5), (1.5, 0.0, 0.0))),
    critical(shot((6.5, 65.3, 13.5), (1.5, 0.0, 0.0))),
    critical(shot((6.5, 65.3, 14.5), (1.5, 0.0, 0.0))),
    critical(shot((6.5, 65.3, 15.5), (1.5, 0.0, 0.0))),
    critical(shot((6.5, 65.3, 16.5), (1.5, 0.0, 0.0))),
    critical(shot((6.5, 65.3, 17.5), (1.5, 0.0, 0.0))),
    critical(shot((6.5, 65.3, 18.5), (1.5, 0.0, 0.0))),
    critical(shot((6.5, 65.3, 19.5), (1.5, 0.0, 0.0))),
    critical(shot((6.5, 65.3, 20.5), (1.5, 0.0, 0.0))),
    critical(shot((6.5, 65.3, 21.5), (1.5, 0.0, 0.0))),
    critical(shot((12.5, 65.3, 1.5), (1.5, 0.0, 0.0))),
    critical(shot((12.5, 65.3, 2.5), (1.5, 0.0, 0.0))),
    critical(shot((12.5, 65.3, 3.5), (1.5, 0.0, 0.0))),
    critical(shot((12.5, 65.3, 4.5), (1.5, 0.0, 0.0))),
    critical(shot((12.5, 65.3, 5.5), (1.5, 0.0, 0.0))),
    critical(shot((12.5, 65.3, 6.5), (1.5, 0.0, 0.0))),
    critical(shot((12.5, 65.3, 7.5), (1.5, 0.0, 0.0))),
    critical(shot((12.5, 65.3, 8.5), (1.5, 0.0, 0.0))),
    critical(shot((12.5, 65.3, 9.5), (1.5, 0.0, 0.0))),
    critical(shot((12.5, 65.3, 10.5), (1.5, 0.0, 0.0))),
    critical(shot((12.5, 65.3, 11.5), (1.5, 0.0, 0.0))),
    critical(shot((12.5, 65.3, 12.5), (1.5, 0.0, 0.0))),
    critical(shot((12.5, 65.3, 13.5), (1.5, 0.0, 0.0))),
    critical(shot((12.5, 65.3, 14.5), (1.5, 0.0, 0.0))),
    critical(shot((12.5, 65.3, 15.5), (1.5, 0.0, 0.0))),
    critical(shot((12.5, 65.3, 16.5), (1.5, 0.0, 0.0))),
    critical(shot((12.5, 65.3, 17.5), (1.5, 0.0, 0.0))),
    critical(shot((12.5, 65.3, 18.5), (1.5, 0.0, 0.0))),
    critical(shot((12.5, 65.3, 19.5), (1.5, 0.0, 0.0))),
    critical(shot((12.5, 65.3, 20.5), (1.5, 0.0, 0.0))),
    critical(shot((12.5, 65.3, 21.5), (1.5, 0.0, 0.0))),
];

const SCENES: &[Scene] = &[
    // Flight and the ground.
    scene(
        "free_flight",
        &[],
        &[shot((2.5, 70.5, 12.5), (0.5, 0.3, 0.1))],
        &[],
    ),
    scene(
        "into_the_floor",
        &[],
        &[
            shot((2.5, 66.5, 12.5), (0.6, -0.1, 0.0)),
            shot((6.5, 70.5, 6.5), (0.0, -1.0, 0.0)),
        ],
        &[],
    ),
    scene(
        "wall_from_west",
        &[Fill(STONE, (10, 64, 8), (10, 68, 16))],
        &[shot((2.5, 65.5, 12.5), (1.5, 0.0, 0.0))],
        &[],
    ),
    scene(
        "wall_every_face",
        &[
            Fill(STONE, (10, 66, 4), (10, 66, 4)),
            Fill(STONE, (10, 66, 9), (10, 66, 9)),
            Fill(STONE, (16, 66, 14), (16, 66, 14)),
            Fill(STONE, (18, 66, 8), (18, 66, 8)),
            Fill(STONE, (20, 66, 20), (20, 66, 20)),
            Fill(STONE, (14, 72, 20), (14, 72, 20)),
        ],
        &[
            shot((6.5, 66.8, 4.5), (1.5, 0.0, 0.0)),
            shot((14.5, 66.8, 9.5), (-1.5, 0.0, 0.0)),
            shot((16.5, 66.8, 18.5), (0.0, 0.0, -1.5)),
            shot((18.5, 66.8, 4.5), (0.0, 0.0, 1.5)),
            shot((20.5, 70.5, 20.5), (0.0, -1.5, 0.0)),
            shot((14.5, 69.0, 20.5), (0.0, 1.5, 0.0)),
        ],
        &[],
    ),
    scene(
        "inside_a_block",
        &[Fill(STONE, (10, 65, 12), (10, 65, 12))],
        &[shot((10.5, 65.5, 12.5), (0.1, 0.0, 0.0))],
        &[],
    ),
    scene(
        "already_hit",
        &[Fill(STONE, (10, 64, 8), (10, 68, 16))],
        &[starting(
            shot((6.5, 65.5, 12.5), (1.0, 0.0, 0.0)),
            Start::Landed,
        )],
        &[],
    ),
    // Blocks that react to an arrow.
    watching(
        scene(
            "target_block",
            &[
                Fill(&Block::TARGET, (10, 65, 12), (10, 65, 12)),
                Fill(&Block::TARGET, (10, 65, 6), (10, 65, 6)),
            ],
            &[
                owned_by(shot((7.5, 65.8, 12.5), (1.5, 0.0, 0.0)), Owner::Watcher),
                shot((7.5, 65.8, 6.5), (1.5, 0.0, 0.0)),
            ],
            &[],
        ),
        &[(10, 65, 12), (10, 65, 6)],
    ),
    watching(
        scene(
            "reacting_blocks",
            &[
                Fill(&Block::BELL, (10, 65, 3), (10, 65, 3)),
                Fill(&Block::TNT, (10, 65, 7), (10, 65, 7)),
                Fill(&Block::TNT, (10, 65, 11), (10, 65, 11)),
                Fill(&Block::BIG_DRIPLEAF, (10, 65, 15), (10, 65, 15)),
                Fill(&Block::CHORUS_FLOWER, (10, 65, 19), (10, 65, 19)),
            ],
            &[
                shot((7.5, 65.8, 3.5), (1.5, 0.0, 0.0)),
                flaming(shot((7.5, 65.8, 7.5), (1.5, 0.0, 0.0))),
                shot((7.5, 65.8, 11.5), (1.5, 0.0, 0.0)),
                shot((7.5, 65.8, 15.5), (1.5, 0.0, 0.0)),
                shot((7.5, 65.8, 19.5), (1.5, 0.0, 0.0)),
            ],
            &[],
        ),
        &[
            (10, 65, 3),
            (10, 65, 7),
            (10, 65, 11),
            (10, 65, 15),
            (10, 65, 19),
        ],
    ),
    scene(
        "collision_shapes",
        &[
            Fill(&Block::STONE_SLAB, (10, 64, 2), (10, 64, 2)),
            Fill(&Block::STONE_SLAB, (10, 64, 4), (10, 64, 4)),
            Fill(&Block::OAK_FENCE, (10, 64, 8), (10, 64, 8)),
            Fill(&Block::OAK_FENCE, (10, 64, 10), (10, 64, 10)),
            Fill(&Block::TORCH, (10, 64, 14), (10, 64, 14)),
            Fill(&Block::SHORT_GRASS, (10, 64, 16), (10, 64, 16)),
            Fill(&Block::GLASS, (10, 64, 20), (10, 64, 20)),
        ],
        &[
            shot((7.5, 64.75, 2.5), (1.5, 0.0, 0.0)),
            shot((7.5, 64.3, 4.5), (1.5, 0.0, 0.0)),
            shot((7.5, 65.4, 8.5), (1.5, 0.0, 0.0)),
            shot((7.5, 65.8, 10.5), (1.5, 0.0, 0.0)),
            shot((7.5, 64.6, 14.5), (1.5, 0.0, 0.0)),
            shot((7.5, 64.6, 16.5), (1.5, 0.0, 0.0)),
            shot((7.5, 64.6, 20.5), (1.5, 0.0, 0.0)),
        ],
        &[],
    ),
    // Water, lava and fire.
    scene(
        "water",
        &[Fill(WATER, (4, 64, 4), (19, 69, 20))],
        &[
            shot((2.5, 66.5, 12.5), (1.0, 0.1, 0.0)),
            shot((8.5, 66.5, 8.5), (0.5, 0.5, 0.0)),
            critical(shot((8.5, 66.5, 16.5), (0.5, 0.0, 0.5))),
        ],
        &[],
    ),
    scene(
        "lava_and_fire",
        &[
            Fill(LAVA, (6, 64, 3), (8, 66, 5)),
            Fill(&Block::FIRE, (7, 64, 10), (7, 64, 10)),
            Fill(&Block::SOUL_FIRE, (7, 64, 16), (7, 64, 16)),
        ],
        &[
            shot((7.5, 65.5, 4.5), (0.0, 0.0, 0.0)),
            shot((7.5, 64.5, 10.5), (0.0, 0.0, 0.0)),
            shot((7.5, 64.5, 16.5), (0.0, 0.0, 0.0)),
        ],
        &[],
    ),
    scene(
        "burning_down",
        &[],
        &[
            flaming(starting(
                shot((4.5, 70.5, 4.5), (0.0, 0.0, 0.0)),
                Start::Burning(5),
            )),
            starting(shot((4.5, 70.5, 8.5), (0.0, 0.0, 0.0)), Start::Burning(5)),
            flaming(starting(
                shot((4.5, 70.5, 12.5), (0.0, 0.0, 0.0)),
                Start::Burning(0),
            )),
            flaming(shot((4.5, 70.5, 16.5), (0.0, 0.0, 0.0))),
        ],
        &[],
    ),
    scene(
        "burning_in_water",
        &[Fill(WATER, (4, 64, 4), (19, 69, 8))],
        &[
            flaming(starting(
                shot((8.5, 66.5, 6.5), (0.1, 0.0, 0.0)),
                Start::Burning(60),
            )),
            starting(shot((8.5, 66.5, 5.5), (0.1, 0.0, 0.0)), Start::Burning(60)),
        ],
        &[],
    ),
    // Stuck arrows.
    scene(
        "stuck_stays",
        &[Fill(STONE, (4, 65, 4), (4, 65, 8))],
        &[
            starting(
                shot((4.5, 66.0, 4.5), (0.0, 0.0, 0.0)),
                Start::Stuck {
                    at: (4, 65, 4),
                    life: 0,
                    shake: 7,
                },
            ),
            starting(
                shot((4.5, 66.0, 6.5), (0.0, 0.0, 0.0)),
                Start::Stuck {
                    at: (4, 65, 6),
                    life: 100,
                    shake: 3,
                },
            ),
            starting(
                shot((4.5, 66.0, 8.5), (0.0, 0.0, 0.0)),
                Start::Stuck {
                    at: (4, 65, 8),
                    life: 599,
                    shake: 0,
                },
            ),
        ],
        &[],
    ),
    scene(
        "stuck_released",
        &[],
        &[starting(
            shot((4.5, 65.5, 12.5), (0.0, 0.0, 0.0)),
            Start::Stuck {
                at: (4, 65, 12),
                life: 5,
                shake: 2,
            },
        )],
        &[],
    ),
    scene(
        "stuck_despawns",
        &[Fill(STONE, (4, 65, 4), (4, 65, 12))],
        &[
            starting(
                shot((4.5, 66.0, 4.5), (0.0, 0.0, 0.0)),
                Start::Stuck {
                    at: (4, 65, 4),
                    life: 1190,
                    shake: 0,
                },
            ),
            starting(
                shot((4.5, 66.0, 8.5), (0.0, 0.0, 0.0)),
                Start::Stuck {
                    at: (4, 65, 8),
                    life: 1199,
                    shake: 0,
                },
            ),
            starting(
                shot((4.5, 66.0, 12.5), (0.0, 0.0, 0.0)),
                Start::Stuck {
                    at: (4, 65, 12),
                    life: 1200,
                    shake: 0,
                },
            ),
        ],
        &[],
    ),
    // Hitting entities.
    scene(
        "entity_basic",
        &[],
        &[shot((6.5, 65.3, 12.5), (1.0, 0.0, 0.0))],
        &[zombie((10.5, 64.0, 12.5))],
    ),
    scene(
        "entity_speeds",
        &[],
        &[
            shot((6.5, 65.3, 4.5), (0.5, 0.05, 0.0)),
            shot((6.5, 65.3, 12.5), (2.0, 0.0, 0.0)),
            shot((6.5, 65.3, 20.5), (3.0, 0.0, 0.0)),
        ],
        &[
            zombie((10.5, 64.0, 4.5)),
            zombie((10.5, 64.0, 12.5)),
            zombie((10.5, 64.0, 20.5)),
        ],
    ),
    scene(
        "entity_base_damage",
        &[],
        &[
            hurting(shot((6.5, 65.3, 4.5), (1.0, 0.0, 0.0)), 5.0),
            hurting(shot((6.5, 65.3, 12.5), (1.0, 0.0, 0.0)), 3.5),
            hurting(shot((6.5, 65.3, 20.5), (1.0, 0.0, 0.0)), 0.0),
        ],
        &[
            zombie((10.5, 64.0, 4.5)),
            zombie((10.5, 64.0, 12.5)),
            zombie((10.5, 64.0, 20.5)),
        ],
    ),
    Scene {
        loot: true,
        ..scene(
            "entity_kills",
            &[],
            &[
                shot((6.5, 65.3, 4.5), (1.5, 0.0, 0.0)),
                shot((6.5, 65.3, 12.5), (1.5, 0.0, 0.0)),
            ],
            &[
                with_health(zombie((10.5, 64.0, 4.5)), 1.0),
                with_health(zombie((10.5, 64.0, 12.5)), 3.0),
            ],
        )
    },
    scene(
        "pierce_levels",
        &[],
        &[
            shot((6.0, 65.9, 4.5), (1.5, 0.0, 0.0)),
            piercing(shot((6.0, 65.9, 12.5), (1.5, 0.0, 0.0)), 1),
            piercing(shot((6.0, 65.9, 20.5), (1.5, 0.0, 0.0)), 2),
        ],
        ROWS,
    ),
    scene(
        "pierce_into_wall",
        &[Fill(STONE, (16, 64, 3), (16, 68, 6))],
        &[piercing(shot((5.0, 65.9, 4.5), (1.5, 0.0, 0.0)), 5)],
        &[
            zombie((8.5, 64.0, 4.5)),
            zombie((10.5, 64.0, 4.5)),
            zombie((12.5, 64.0, 4.5)),
        ],
    ),
    scene(
        "punch",
        &[],
        &[
            punching(shot((6.5, 65.3, 4.5), (1.0, 0.0, 0.0)), 1),
            punching(shot((6.5, 65.3, 12.5), (1.0, 0.0, 0.0)), 2),
            punching(shot((6.5, 65.3, 20.5), (0.0, 0.0, 0.0)), 3),
            punching(shot((10.5, 65.3, 20.5), (0.5, 0.0, 0.2)), 2),
        ],
        &[
            zombie((10.5, 64.0, 4.5)),
            zombie((10.5, 64.0, 12.5)),
            zombie((10.5, 64.0, 20.5)),
        ],
    ),
    scene(
        "flame_arrows",
        &[],
        &[
            flaming(shot((6.5, 65.3, 4.5), (1.0, 0.0, 0.0))),
            flaming(shot((6.5, 65.3, 12.5), (1.0, 0.0, 0.0))),
            shot((6.5, 65.3, 20.5), (1.0, 0.0, 0.0)),
        ],
        &[
            zombie((10.5, 64.0, 4.5)),
            mob(&EntityType::ENDERMAN, (10.5, 64.0, 12.5)),
            zombie((10.5, 64.0, 20.5)),
        ],
    ),
    Scene {
        random: true,
        ..scene("critical_hits", &[], CRIT_SHOTS, CRIT_MOBS)
    },
    scene(
        "arrow_kinds",
        &[],
        &[
            shot((6.5, 65.3, 2.5), (1.0, 0.0, 0.0)),
            of_kind(shot((6.5, 65.3, 6.5), (1.0, 0.0, 0.0)), Kind::Spectral),
            of_kind(shot((6.5, 65.3, 10.5), (1.0, 0.0, 0.0)), Kind::Tipped),
            of_kind(shot((6.5, 65.3, 14.5), (1.0, 0.0, 0.0)), Kind::TippedFull),
            of_kind(
                flaming(shot((6.5, 65.3, 18.5), (1.0, 0.0, 0.0))),
                Kind::Spectral,
            ),
        ],
        &[
            zombie((10.5, 64.0, 2.5)),
            zombie((10.5, 64.0, 6.5)),
            zombie((10.5, 64.0, 10.5)),
            zombie((10.5, 64.0, 14.5)),
            zombie((10.5, 64.0, 18.5)),
        ],
    ),
    scene(
        "damage_refused",
        &[],
        &[
            of_kind(shot((6.5, 65.3, 4.5), (1.0, 0.0, 0.0)), Kind::Tipped),
            of_kind(
                punching(shot((6.5, 65.3, 12.5), (1.0, 0.0, 0.0)), 2),
                Kind::Spectral,
            ),
        ],
        &[
            immune(zombie((10.5, 64.0, 4.5))),
            immune(zombie((10.5, 64.0, 12.5))),
        ],
    ),
    // Whose arrow it is.
    scene(
        "owner_in_the_way",
        &[],
        &[
            owned_by(shot((10.7, 65.0, 4.5), (0.0, 1.0, 0.0)), Owner::Mob(0)),
            owned_by(shot((6.5, 65.3, 12.5), (1.0, 0.0, 0.0)), Owner::Mob(1)),
            owned_by(shot((8.5, 65.3, 20.5), (1.0, 0.0, 0.0)), Owner::Mob(2)),
            shot((8.5, 65.3, 22.5), (1.0, 0.0, 0.0)),
        ],
        &[
            zombie((10.5, 64.0, 4.5)),
            zombie((10.5, 64.0, 12.5)),
            zombie((8.5, 64.0, 20.5)),
            zombie((8.5, 64.0, 22.5)),
        ],
    ),
    scene(
        "passes_through_things",
        &[],
        &[
            shot((6.5, 65.8, 2.5), (1.5, 0.0, 0.0)),
            shot((6.5, 65.8, 6.5), (1.5, 0.0, 0.0)),
            shot((6.5, 65.8, 10.5), (1.5, 0.0, 0.0)),
            shot((6.5, 65.8, 14.5), (1.5, 0.0, 0.0)),
            shot((6.5, 65.8, 18.5), (1.5, 0.0, 0.0)),
            shot((6.5, 65.8, 22.5), (1.5, 0.0, 0.0)),
        ],
        &[
            mob(&EntityType::ARROW, (9.5, 64.0, 2.5)),
            mob(&EntityType::SPECTRAL_ARROW, (9.5, 64.0, 6.5)),
            mob(&EntityType::ITEM, (9.5, 64.0, 10.5)),
            mob(&EntityType::FALLING_BLOCK, (9.5, 64.0, 14.5)),
            mob(&EntityType::AREA_EFFECT_CLOUD, (8.5, 64.0, 18.5)),
            with_health(zombie((8.5, 64.0, 22.5)), 0.0),
            zombie((13.5, 64.0, 2.5)),
            zombie((13.5, 64.0, 6.5)),
            zombie((13.5, 64.0, 10.5)),
            zombie((13.5, 64.0, 14.5)),
            zombie((13.5, 64.0, 18.5)),
            zombie((13.5, 64.0, 22.5)),
        ],
    ),
    scene(
        "wall_and_entity",
        &[
            Fill(STONE, (10, 64, 2), (10, 68, 6)),
            Fill(STONE, (14, 64, 10), (14, 68, 14)),
        ],
        &[
            shot((6.5, 65.3, 4.5), (1.5, 0.0, 0.0)),
            shot((6.5, 65.3, 12.5), (1.5, 0.0, 0.0)),
        ],
        &[zombie((13.0, 64.0, 4.5)), zombie((11.5, 64.0, 12.5))],
    ),
    scene(
        "near_misses",
        &[],
        &[
            shot((6.5, 65.3, 3.0), (1.0, 0.0, 0.0)),
            shot((6.5, 65.3, 5.65), (1.0, 0.0, 0.0)),
            shot((6.5, 65.3, 8.95), (1.0, 0.0, 0.0)),
            shot((6.5, 66.25, 12.5), (1.0, 0.0, 0.0)),
            shot((6.5, 67.0, 16.5), (1.0, 0.0, 0.0)),
            shot((6.5, 64.6, 20.5), (1.0, 0.05, 0.0)),
        ],
        &[
            zombie((10.5, 64.0, 3.5)),
            zombie((10.5, 64.0, 6.5)),
            zombie((10.5, 64.0, 9.5)),
            zombie((10.5, 64.0, 12.5)),
            zombie((10.5, 64.0, 16.5)),
            zombie((10.5, 65.0, 20.5)),
        ],
    ),
    scene(
        "arrow_from_a_height",
        &[],
        &[
            shot((10.7, 80.5, 4.5), (0.0, -1.0, 0.0)),
            shot((7.5, 68.5, 12.5), (0.6, -0.5, 0.0)),
            shot((10.7, 62.5, 20.5), (0.0, 1.5, 0.0)),
        ],
        &[
            zombie((10.5, 64.0, 4.5)),
            zombie((10.5, 64.0, 12.5)),
            zombie((10.5, 62.0, 20.5)),
        ],
    ),
    // Water puts a burning arrow out.
    scene(
        "wet_flight",
        &[Fill(WATER, (4, 64, 4), (19, 69, 20))],
        &[
            wetted(shot((6.5, 66.5, 6.5), (1.0, 0.2, 0.0))),
            flaming(shot((6.5, 66.5, 22.5), (1.0, 0.0, 0.0))),
            wetted(flaming(shot((21.5, 66.5, 14.5), (0.0, 0.0, 0.0)))),
            wetted(flaming(shot((6.5, 66.5, 10.5), (1.0, 0.0, 0.0)))),
            wetted(starting(
                shot((6.5, 66.5, 14.5), (1.0, 0.0, 0.0)),
                Start::Burning(60),
            )),
            wetted(critical(shot((6.5, 66.5, 18.5), (1.0, 0.1, 0.0)))),
        ],
        &[],
    ),
    // A stuck arrow whose block went away leaves with a random part of its speed.
    Scene {
        rolls: true,
        ticks: 1,
        ..scene(
            "release_speed",
            &[],
            &[
                starting(
                    shot((4.5, 65.5, 6.5), (5.0, 5.0, 5.0)),
                    Start::Stuck {
                        at: (4, 65, 6),
                        life: 3,
                        shake: 0,
                    },
                ),
                starting(
                    shot((4.5, 65.5, 12.5), (-5.0, 0.0, 3.0)),
                    Start::Stuck {
                        at: (4, 65, 12),
                        life: 3,
                        shake: 0,
                    },
                ),
            ],
            &[],
        )
    },
    // Targets just inside the reach the search box gives an arrow's path.
    scene(
        "search_box_reach",
        &[],
        &[
            shot((6.5, 65.3, 4.0), (1.0, 0.0, 0.0)),
            shot((6.5, 66.15, 12.5), (1.0, 0.0, 0.0)),
            shot((6.5, 65.3, 21.0), (1.0, 0.0, 0.0)),
        ],
        &[
            zombie((10.5, 64.0, 4.5)),
            zombie((10.5, 64.0, 12.5)),
            zombie((10.5, 64.0, 20.5)),
        ],
    ),
    // A burning arrow sets its target alight, except an enderman.
    scene(
        "burning_arrow_hits",
        &[],
        &[
            starting(shot((6.5, 65.3, 4.5), (1.0, 0.0, 0.0)), Start::Burning(60)),
            starting(shot((6.5, 65.3, 12.5), (1.0, 0.0, 0.0)), Start::Burning(60)),
            flaming(shot((6.5, 65.3, 20.5), (1.0, 0.0, 0.0))),
        ],
        &[
            zombie((10.5, 64.0, 4.5)),
            mob(&EntityType::ENDERMAN, (10.5, 64.0, 12.5)),
            zombie((10.5, 64.0, 20.5)),
        ],
    ),
];

/// One scene's area: its corner in the world.
struct Area<'a> {
    world: &'a Arc<World>,
    x: i32,
    z: i32,
}

impl Area<'_> {
    fn put(&self, block: &Block, (x, y, z): Pos) {
        self.world.set_block_state(
            &BlockPos(Vector3::new(self.x + x, y, self.z + z)),
            block.default_state.id,
            BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
        );
    }

    fn fill(&self, Fill(block, from, to): Fill) {
        for x in from.0..=to.0 {
            for y in from.1..=to.1 {
                for z in from.2..=to.2 {
                    self.put(block, (x, y, z));
                }
            }
        }
    }

    fn block_pos(&self, (x, y, z): Pos) -> BlockPos {
        BlockPos(Vector3::new(self.x + x, y, self.z + z))
    }

    fn point(&self, (x, y, z): Vec3) -> Vector3<f64> {
        Vector3::new(f64::from(self.x) + x, y, f64::from(self.z) + z)
    }
}

/// An entity id and UUID that depend only on the scene.
fn ids(index: usize, slot: usize) -> (i32, Uuid) {
    let n = index * 100 + slot;
    (
        70_000 + n as i32,
        Uuid::from_u128(0x4152_5257_0000_0000_0000 + n as u128),
    )
}

fn stack_of(kind: Kind) -> ItemStack {
    match kind {
        Kind::Arrow => ItemStack::new(1, &Item::ARROW),
        Kind::Spectral => ItemStack::new(1, &Item::SPECTRAL_ARROW),
        Kind::Tipped | Kind::TippedFull => {
            let mut tipped = ItemStack::new(1, &Item::TIPPED_ARROW);
            tipped.patch.push((
                DataComponent::PotionContents,
                Some(
                    PotionContentsImpl {
                        potion_id: Some(5),
                        custom_color: None,
                        custom_effects: Vec::new(),
                        custom_name: None,
                    }
                    .to_dyn(),
                ),
            ));
            if kind == Kind::Tipped {
                tipped.patch.push((
                    DataComponent::PotionDurationScale,
                    Some(PotionDurationScaleImpl { scale: 0.5 }.to_dyn()),
                ));
            }
            tipped
        }
    }
}

fn spawn_mob(
    area: &Area<'_>,
    index: usize,
    slot: usize,
    spec: &Mob,
) -> Result<Arc<dyn EntityBase>, String> {
    let (id, uuid) = ids(index, 50 + slot);
    let entity = Entity::from_uuid_with_id(
        id,
        uuid,
        Arc::clone(area.world),
        area.point(spec.at),
        spec.kind,
    );
    let mob: Arc<dyn EntityBase> = if spec.kind == &EntityType::ITEM {
        Arc::new(ItemEntity::new(entity, ItemStack::new(1, &Item::DIAMOND)))
    } else {
        let living = LivingEntity::new(entity);
        living.health.store(spec.health);
        living.entity.invulnerable.store(spec.invulnerable, Relaxed);
        Arc::new(living)
    };
    if !area.world.spawn_entity(Arc::clone(&mob)) {
        return Err(format!("{} was not spawned", spec.kind.resource_name));
    }
    Ok(mob)
}

fn spawn_arrow(
    area: &Area<'_>,
    index: usize,
    slot: usize,
    spec: &Shot,
    mobs: &[Arc<dyn EntityBase>],
    watcher: &Player,
) -> Result<Arc<ArrowEntity>, String> {
    let (id, uuid) = ids(index, slot);
    let kind = if spec.kind == Kind::Spectral {
        &EntityType::SPECTRAL_ARROW
    } else {
        &EntityType::ARROW
    };
    let entity =
        Entity::from_uuid_with_id(id, uuid, Arc::clone(area.world), area.point(spec.at), kind);
    let owner = match spec.owner {
        Owner::None => None,
        Owner::Mob(mob) => Some(mobs[mob].get_entity()),
        Owner::Watcher => Some(watcher.get_entity()),
    };
    let arrow = Arc::new(ArrowEntity::new_with_item(
        entity,
        owner,
        &stack_of(spec.kind),
        ArrowPickup::Allowed,
    ));
    let entity = arrow.get_entity();
    entity.velocity.store(Vector3::new(
        spec.velocity.0,
        spec.velocity.1,
        spec.velocity.2,
    ));
    entity.touching_water.store(spec.wet, Relaxed);
    arrow.is_critical.store(spec.crit, Relaxed);
    arrow.is_flame.store(spec.flame, Relaxed);
    arrow.pierce_level.store(spec.pierce, Relaxed);
    arrow.punch_level.store(spec.punch, Relaxed);
    if let Some(damage) = spec.damage {
        arrow.set_base_damage(damage);
    }
    match spec.start {
        Start::Flying => {}
        Start::Burning(ticks) => entity.fire_ticks.store(ticks, Relaxed),
        Start::Stuck { at, life, shake } => {
            arrow.in_ground.store(true, Relaxed);
            arrow.life.store(life, Relaxed);
            arrow.shake_time.store(shake, Relaxed);
            *arrow
                .last_block_pos
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(area.block_pos(at));
        }
        Start::Landed => arrow.has_hit.store(true, Relaxed),
    }
    let as_dyn: Arc<dyn EntityBase> = Arc::clone(&arrow) as Arc<dyn EntityBase>;
    if !area.world.spawn_entity(as_dyn) {
        return Err(format!("arrow {slot} was not spawned"));
    }
    Ok(arrow)
}

/// The sound packet's id: its pitch and seed are random in vanilla, so only how many are
/// counted, and the rest of the bytes are hashed with the random ones cut out.
const SOUND_PACKET: u8 = 119;

/// The packet is the arrow-hit sound: its sound id (plus one, for the registry form) comes
/// right after the packet id as a varint.
fn is_arrow_hit(packet: &[u8]) -> bool {
    let mut id = Sound::EntityArrowHit as u32 + 1;
    let mut want = Vec::new();
    while id >= 0x80 {
        want.push((id & 0x7f) as u8 | 0x80);
        id >>= 7;
    }
    want.push(id as u8);
    packet.get(1..=want.len()) == Some(want.as_slice())
}

/// The pitch sits before the 8-byte seed that ends a sound packet.
fn pitch_of(packet: &[u8]) -> f32 {
    let at = packet.len().saturating_sub(12);
    packet
        .get(at..at + 4)
        .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
        .map_or(0.0, f32::from_be_bytes)
}

fn packets(tap: &mut PacketTap, tally: &mut Tally) -> String {
    tap.drain()
        .iter()
        .map(|p| match p.first() {
            Some(id) if RANDOM_PACKETS.contains(id) => format!("{}:id{id}", p.len()),
            Some(&SOUND_PACKET) => {
                tally.sounds += 1;
                if is_arrow_hit(p) {
                    tally.hit_pitches.push(pitch_of(p));
                }
                format!("{}:id{SOUND_PACKET}", p.len())
            }
            _ => format!("{}:{:08x}", p.len(), fnv(p) as u32),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn arrow_text(arrow: &ArrowEntity) -> String {
    let entity = arrow.get_entity();
    let pos = entity.pos.load();
    let velocity = entity.velocity.load();
    let pierced = arrow
        .pierced_entities
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let stuck_in = *arrow
        .last_block_pos
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    format!(
        "pos {:.4},{:.4},{:.4} vel {:.4},{:.4},{:.4} rot {:.3},{:.3} ground {} in {:?} shake {} life {} time {} hit {} pierced {:?} crit {} flame {} pierce {} punch {} fire {} burning {} removed {}",
        pos.x,
        pos.y,
        pos.z,
        velocity.x,
        velocity.y,
        velocity.z,
        entity.yaw.load(),
        entity.pitch.load(),
        arrow.in_ground.load(Relaxed),
        stuck_in.map(|p| (p.0.x, p.0.y, p.0.z)),
        arrow.shake_time.load(Relaxed),
        arrow.life.load(Relaxed),
        arrow.in_ground_time.load(Relaxed),
        arrow.has_hit.load(Relaxed),
        pierced,
        arrow.is_critical.load(Relaxed),
        arrow.is_flame.load(Relaxed),
        arrow.pierce_level.load(Relaxed),
        arrow.punch_level.load(Relaxed),
        entity.fire_ticks.load(Relaxed),
        entity.is_on_fire(),
        entity.is_removed(),
    )
}

/// What an arrow keeps when its position and speed are random.
fn arrow_state_text(arrow: &ArrowEntity) -> String {
    format!(
        "ground {} hit {} removed {}",
        arrow.in_ground.load(Relaxed),
        arrow.has_hit.load(Relaxed),
        arrow.get_entity().is_removed(),
    )
}

/// The tracked data an arrow has changed since the last tick, as the world would send it.
fn meta_text(arrow: &ArrowEntity) -> String {
    let data = &arrow.get_entity().synched_data;
    let text = data
        .pack_dirty_for_version(&JavaMinecraftVersion::V_26_3)
        .map_or_else(
            || "-".to_owned(),
            |bytes| format!("{:08x}", fnv(&bytes) as u32),
        );
    data.clear_dirty();
    text
}

fn mob_text(mob: &dyn EntityBase, random: bool) -> String {
    let entity = mob.get_entity();
    let mut text = String::new();
    if let Some(living) = mob.get_living_entity() {
        if random {
            let _ = write!(text, "dead {}", living.health.load() <= 0.0);
        } else {
            let pos = entity.pos.load();
            let velocity = entity.velocity.load();
            let _ = write!(
                text,
                "health {:.2} pos {:.4},{:.4},{:.4} vel {:.4},{:.4},{:.4} fire {} burning {}",
                living.health.load(),
                pos.x,
                pos.y,
                pos.z,
                velocity.x,
                velocity.y,
                velocity.z,
                entity.fire_ticks.load(Relaxed),
                entity.is_on_fire(),
            );
        }
        let effects = living
            .active_effects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut effects: Vec<_> = effects.values().collect();
        effects.sort_by_key(|effect| effect.effect_type.id);
        for effect in effects {
            let _ = write!(
                text,
                " {}:{}:{}",
                effect.effect_type.minecraft_name, effect.amplifier, effect.duration
            );
        }
    }
    let _ = write!(text, " removed {}", entity.is_removed());
    text
}

fn place_watcher(watcher: &Player, area: &Area<'_>) {
    watcher.get_entity().set_pos(area.point((0.5, 64.0, 23.5)));
    watcher.watched_section.store(Cylindrical::new(
        Vector2::new((area.x + 12) >> 4, (area.z + 12) >> 4),
        NonZero::<u8>::MIN.saturating_add(3),
    ));
}

/// Collected while a scene runs.
struct Tally {
    sounds: usize,
    /// Pitches of the arrow-hit sound, which vanilla draws from 1.2 / (0.9..1.1).
    hit_pitches: Vec<f32>,
}

fn report_end(
    scene: &Scene,
    arrows: &[Arc<ArrowEntity>],
    mobs: &[Arc<dyn EntityBase>],
    tally: &Tally,
    report: &mut String,
) {
    for (slot, arrow) in arrows
        .iter()
        .enumerate()
        .take(if scene.random { 3 } else { usize::MAX })
    {
        let text = if scene.rolls {
            arrow_state_text(arrow)
        } else {
            arrow_text(arrow)
        };
        let _ = writeln!(report, "{} arrow {slot}: {text}", scene.name);
    }
    if scene.rolls {
        report_release(scene, arrows, report);
    } else if scene.random {
        report_losses(scene, mobs, report);
    } else {
        for (slot, mob) in mobs.iter().enumerate() {
            let text = mob_text(mob.as_ref(), false);
            let _ = writeln!(report, "{} mob {slot}: {text}", scene.name);
        }
    }
    if !(scene.random || scene.loot || tally.sounds == 0) {
        let ranged = tally
            .hit_pitches
            .iter()
            .filter(|pitch| (1.09..=1.34).contains(*pitch))
            .count();
        let _ = writeln!(
            report,
            "{} sounds: {}, arrow hits: {}, pitched in range: {ranged}",
            scene.name,
            tally.sounds,
            tally.hit_pitches.len()
        );
    }
}

/// Damage of the critical arrows: which of the three values a bonus can give were seen.
fn report_losses(scene: &Scene, mobs: &[Arc<dyn EntityBase>], report: &mut String) {
    let mut seen = [0; 4];
    for mob in mobs {
        let lost = 20.0 - mob.get_living_entity().map_or(20.0, |l| l.health.load());
        let slot = match lost as i32 {
            3 => 0,
            4 => 1,
            5 => 2,
            _ => 3,
        };
        seen[slot] += 1;
    }
    let _ = writeln!(
        report,
        "{} lost 3 hp seen: {}, 4: {}, 5: {}, other: {}",
        scene.name,
        seen[0] > 0,
        seen[1] > 0,
        seen[2] > 0,
        seen[3]
    );
}

/// A released arrow keeps a random part of its speed: each axis must be in 0..0.2 of the old
/// one, and not zero.
fn report_release(scene: &Scene, arrows: &[Arc<ArrowEntity>], report: &mut String) {
    for (slot, (arrow, shot)) in arrows.iter().zip(scene.shots).enumerate() {
        let now = arrow.get_entity().velocity.load();
        let before: [f64; 3] = shot.velocity.into();
        let after = [now.x, now.y, now.z];
        let kept: Vec<_> = before
            .iter()
            .zip(after)
            .map(|(old, new)| {
                if *old == 0.0 {
                    "-".to_owned()
                } else {
                    let ratio = new / old;
                    format!("{}", (0.0..0.2).contains(&ratio) && ratio > 0.0)
                }
            })
            .collect();
        let _ = writeln!(
            report,
            "{} arrow {slot} kept: {}",
            scene.name,
            kept.join(" ")
        );
    }
}

/// One tick's text: the arrows, the mobs, the watched blocks, the entity count and what the
/// watcher was sent.
fn tick_text(
    scene: &Scene,
    area: &Area<'_>,
    arrows: &[Arc<ArrowEntity>],
    mobs: &[Arc<dyn EntityBase>],
    tap: &mut PacketTap,
    tally: &mut Tally,
) -> String {
    let mut text = String::new();
    for arrow in arrows {
        text.push_str(&if scene.rolls {
            arrow_state_text(arrow)
        } else {
            arrow_text(arrow)
        });
        let _ = write!(text, " {}|", meta_text(arrow));
    }
    for mob in mobs {
        text.push_str(&mob_text(mob.as_ref(), scene.random));
        text.push('|');
    }
    for at in scene.watch {
        let _ = write!(
            text,
            "{},",
            area.world.get_block_state_id(&area.block_pos(*at))
        );
    }
    let count = area
        .world
        .entities
        .load()
        .iter()
        .filter(|e| !scene.loot || e.get_entity().entity_type != &EntityType::ITEM)
        .count();
    let _ = write!(text, " entities {count}");
    if !(scene.random || scene.loot || scene.rolls) {
        let _ = write!(text, " sent [{}]", packets(tap, tally));
    }
    text
}

fn dump(scene: &Scene, tick: usize, text: &str) -> TestResult {
    if let Some(file) = std::env::var_os("KVASIR_DUMP") {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file)?;
        writeln!(file, "{} {tick} {text}", scene.name)?;
    }
    Ok(())
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
    area.fill(Fill(STONE, (0, FLOOR_Y, 0), (23, FLOOR_Y, 23)));
    for fill in scene.fills {
        area.fill(*fill);
    }

    let who = Identity {
        name: "ArrowWatch",
        uuid: Uuid::from_u128(0x4172_726f_7700_0000 + index as u128),
        entity_id: 60_000 + index as i32,
    };
    let (watcher, mut tap) = test_support::java_player_as(world, GameMode::Survival, &who)
        .ok_or("the watcher's client no longer owned its outgoing queue")?;
    place_watcher(&watcher, &area);
    world.add_player(&watcher)?;

    let mut mobs = Vec::new();
    for (slot, spec) in scene.mobs.iter().enumerate() {
        mobs.push(spawn_mob(&area, index, slot, spec)?);
    }
    let mut arrows = Vec::new();
    for (slot, spec) in scene.shots.iter().enumerate() {
        arrows.push(spawn_arrow(&area, index, slot, spec, &mobs, &watcher)?);
    }
    let mut tally = Tally {
        sounds: 0,
        hit_pitches: Vec::new(),
    };
    drop(packets(&mut tap, &mut tally));
    for arrow in &arrows {
        arrow.get_entity().synched_data.clear_dirty();
    }
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut from = 1;
    for tick in 1..=scene.ticks {
        for arrow in &arrows {
            if !arrow.get_entity().is_removed() {
                EntityBase::tick(arrow.as_ref(), arrow.as_ref(), server);
            }
        }
        let text = tick_text(scene, &area, &arrows, &mobs, &mut tap, &mut tally);
        dump(scene, tick, &text)?;
        hash = text.bytes().fold(hash, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
        if tick % 15 == 0 || tick == scene.ticks {
            let _ = writeln!(report, "{} ticks {from}-{tick}: {hash:016x}", scene.name);
            from = tick + 1;
        }
    }
    report_end(scene, &arrows, &mobs, &tally, report);

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
async fn arrow_tick() -> TestResult {
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
        run_scene(&server, &world, index, scene, &mut report)?;
        for player in world.players.load().iter() {
            world.remove_player(player, false).await;
        }
    }
    server.stop();
    script_golden::bless_or_compare("arrow_tick.txt", &report)
}
