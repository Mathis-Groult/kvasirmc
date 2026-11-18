//! A record of what `VillagerEntity::villager_mob_tick` does, made before the function was split
//! (`tests/golden/villager_tick.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene puts a fresh villager (fixed id) on a real server's world, 64 blocks from the
//! previous scene, with the scene's counters, trade state, gossips, profession, home, beds, pose,
//! time of day and game time, plus neighbouring villagers, a nearby iron golem or a villager
//! that already claimed a bed, then runs one tick. One line per scene: the counters, gossips,
//! level, offers, regeneration, decay and work times, home, pose, the beds' occupied flags,
//! the job site and the villagers and golems around.
//! Not covered: working at the job site (it draws a random number: every scene keeps the time
//! of day outside 2000..9000 or the last work recent), and the packets (none of them reach a
//! player without tracker pairing; a spawned golem's id and UUID are random).

use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use pumpkin_data::Block;
use pumpkin_data::block_properties::{BedPart, HorizontalFacing, WhiteBedLikeProperties};
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::entity::{EntityPose, EntityType};
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::version::JavaMinecraftVersion;
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use std::num::NonZero;
use uuid::Uuid;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::passive::villager::{
    GossipType, VillagerData, VillagerEntity, VillagerProfession, VillagerType,
};
use crate::entity::player::Player;
use crate::entity::{Entity, EntityBase};
use crate::world::World;

/// A bed: its head's offset from the villager's block, which way it faces, whether it is
/// occupied, and whether the villager's home points at its foot instead of its head.
#[derive(Clone, Copy)]
struct Bed {
    at: (i32, i32, i32),
    occupied: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Home {
    None,
    /// The first bed's head.
    Bed,
    /// The first bed's foot.
    Foot,
    /// A block of stone three blocks away.
    Stone,
}

#[derive(Clone, Copy)]
struct Scene {
    name: &'static str,
    /// The villager's tick count (golem and bed logic run on multiples of 20 and 100).
    age: i32,
    /// Ticks into the day (`time_of_day`).
    time: i64,
    /// Game time (`world_age`).
    game_time: i64,
    unhappy: i32,
    sound: i32,
    /// A player traded since the last tick; the trading gossip already held.
    traded: Option<i32>,
    merchant_timer: i32,
    level_up: bool,
    trading: bool,
    /// The last gossip decay; the villager holds minor-negative 30 and trading 3 gossips.
    last_decay: Option<i64>,
    profession: VillagerProfession,
    level: i32,
    home: Home,
    beds: &'static [Bed],
    sleeping: bool,
    /// Another villager whose home is the first bed's head.
    claimed: bool,
    /// Villagers standing around (not counting this one).
    neighbours: u8,
    golem_near: bool,
    last_worked: i64,
    /// Appended: the claiming villager's offset (default three blocks west).
    claimant_at: Option<(f64, f64, f64)>,
    /// Appended: the neighbours stand around this offset instead of four blocks south.
    neighbours_at: Option<(f64, f64, f64)>,
    /// Appended: 63 more villagers in the same state tick too; the line says whether any worked.
    workers: bool,
    /// Appended: a composter (farmer job site) two blocks away.
    composter: bool,
    /// Appended scenes write beds without neighbour updates (the old writer let a bed's halves
    /// reshape each other, so an occupied bed came out free), pair a watcher with the villager,
    /// and end their line with the regeneration flags, the villager's dirty synced data, the
    /// watcher's packets and the golems' positions.
    extra: bool,
}

const SCENE: Scene = Scene {
    name: "",
    age: 1,
    time: 10_000,
    game_time: 50_000,
    unhappy: 0,
    sound: 0,
    traded: None,
    merchant_timer: 0,
    level_up: false,
    trading: false,
    last_decay: None,
    profession: VillagerProfession::Farmer,
    level: 1,
    home: Home::None,
    beds: &[],
    sleeping: false,
    claimed: false,
    neighbours: 0,
    golem_near: false,
    last_worked: 0,
    claimant_at: None,
    neighbours_at: None,
    workers: false,
    composter: false,
    extra: false,
};

const fn scene(name: &'static str) -> Scene {
    Scene { name, ..SCENE }
}

const fn bed(x: i32, z: i32) -> Bed {
    bed_at(x, 0, z)
}

const fn bed_at(x: i32, y: i32, z: i32) -> Bed {
    Bed {
        at: (x, y, z),
        occupied: false,
    }
}

const NEAR_BED: &[Bed] = &[bed(1, 0)];
const BEDS: &[Bed] = &[bed(5, 3), bed(-2, -9), bed(12, 12)];
const SIDE_EDGE_BEDS: &[Bed] = &[bed(17, 0), bed(0, -17), bed(16, 16)];
const HEIGHT_EDGE_BEDS: &[Bed] = &[bed_at(2, 5, 2), bed_at(3, -4, 3), bed(15, 0)];
const FAR_BED: &[Bed] = &[bed(2, 2)];
const CLOSE_BED: &[Bed] = &[bed(2, 1)];

fn scenes() -> Vec<Scene> {
    let mut scenes = counter_scenes();
    scenes.extend(home_scenes());
    scenes.extend(sleep_scenes());
    scenes.extend(golem_scenes());
    scenes.extend(widened_scenes());
    scenes.extend(widened_home_scenes());
    scenes.extend(widened_golem_scenes());
    scenes
}

const fn wide(name: &'static str) -> Scene {
    Scene {
        name,
        extra: true,
        ..SCENE
    }
}

const fn home_edge(name: &'static str, beds: &'static [Bed]) -> Scene {
    Scene {
        age: 40,
        beds,
        ..wide(name)
    }
}

const fn claim(name: &'static str, at: (f64, f64, f64)) -> Scene {
    Scene {
        age: 40,
        beds: BEDS,
        claimed: true,
        claimant_at: Some(at),
        ..wide(name)
    }
}

const fn golem_edge(name: &'static str, at: (f64, f64, f64)) -> Scene {
    Scene {
        neighbours_at: Some(at),
        extra: true,
        ..golem_scene(name)
    }
}

const OCCUPIED_NEAR_BED: &[Bed] = &[Bed {
    at: (1, 0, 0),
    occupied: true,
}];
const TIED_BEDS: &[Bed] = &[bed(4, 0), bed(-4, 0)];
const CENTRE_TWO_BED: &[Bed] = &[bed(2, 0)];
const FAR_HOME_BED: &[Bed] = &[bed(5, 3)];

/// Appended: trade cap, watched counters and merchant update, work, job site, sleep.
fn widened_scenes() -> Vec<Scene> {
    vec![
        Scene {
            traded: Some(24),
            ..wide("traded at the trading cap, watched")
        },
        Scene {
            unhappy: 3,
            ..wide("unhappy counter, watched")
        },
        Scene {
            merchant_timer: 1,
            level_up: true,
            ..wide("merchant timer ends with a level up, watched")
        },
        Scene {
            time: 3_000,
            workers: true,
            ..wide("work hours, 64 villagers")
        },
        Scene {
            age: 40,
            composter: true,
            ..wide("composter beside")
        },
        Scene {
            age: 40,
            beds: FAR_HOME_BED,
            home: Home::Stone,
            sleeping: true,
            time: 13_000,
            ..wide("asleep at night, home gone, another bed five blocks away")
        },
        Scene {
            age: 40,
            beds: NEAR_BED,
            home: Home::Bed,
            time: 13_000,
            ..wide("night beside the bed: sleeps, watched")
        },
        Scene {
            age: 40,
            beds: OCCUPIED_NEAR_BED,
            home: Home::Bed,
            time: 13_000,
            ..wide("night beside an occupied bed")
        },
        Scene {
            age: 40,
            beds: CENTRE_TWO_BED,
            home: Home::Bed,
            time: 13_000,
            ..wide("night, bed centre exactly two blocks away")
        },
        Scene {
            age: 40,
            beds: OCCUPIED_NEAR_BED,
            home: Home::Bed,
            sleeping: true,
            ..wide("morning: wakes up from an occupied bed")
        },
    ]
}

/// Appended: one bed at each edge of the bed search, ties, and claims at the claim box edges.
fn widened_home_scenes() -> Vec<Scene> {
    vec![
        home_edge(
            "bed at x+16",
            &[Bed {
                at: (16, 0, 0),
                occupied: false,
            }],
        ),
        home_edge(
            "bed at x+17",
            &[Bed {
                at: (17, 0, 0),
                occupied: false,
            }],
        ),
        home_edge(
            "bed at x-16",
            &[Bed {
                at: (-16, 0, 0),
                occupied: false,
            }],
        ),
        home_edge(
            "bed at x-17",
            &[Bed {
                at: (-17, 0, 0),
                occupied: false,
            }],
        ),
        home_edge(
            "bed at z+16",
            &[Bed {
                at: (0, 0, 16),
                occupied: false,
            }],
        ),
        home_edge(
            "bed at z+17",
            &[Bed {
                at: (0, 0, 17),
                occupied: false,
            }],
        ),
        home_edge(
            "bed head at z-17, foot at z-16",
            &[Bed {
                at: (0, 0, -17),
                occupied: false,
            }],
        ),
        home_edge(
            "bed head at z-18",
            &[Bed {
                at: (0, 0, -18),
                occupied: false,
            }],
        ),
        home_edge(
            "bed at y+4",
            &[Bed {
                at: (3, 4, 3),
                occupied: false,
            }],
        ),
        home_edge(
            "bed at y+5",
            &[Bed {
                at: (3, 5, 3),
                occupied: false,
            }],
        ),
        home_edge(
            "bed at y-4",
            &[Bed {
                at: (3, -4, 3),
                occupied: false,
            }],
        ),
        home_edge(
            "bed at y-5",
            &[Bed {
                at: (3, -5, 3),
                occupied: false,
            }],
        ),
        home_edge("two beds at the same distance", TIED_BEDS),
        claim("claimed from three blocks east", (3.0, 0.0, 0.0)),
        claim("claimed from two blocks below", (0.0, -2.0, 0.0)),
        claim("claimed from three blocks north", (0.0, 0.0, -3.0)),
        claim("claim from 40 blocks east", (40.0, 0.0, 0.0)),
        claim("claim from 40 blocks west", (-40.0, 0.0, 0.0)),
        claim("claim from 20 blocks below", (0.0, -20.0, 0.0)),
        claim("claim from 20 blocks above", (0.0, 20.0, 0.0)),
    ]
}

/// Appended: golem spawning at age 0, with one neighbour, and neighbours at the box edges.
fn widened_golem_scenes() -> Vec<Scene> {
    vec![
        Scene {
            age: 0,
            extra: true,
            ..golem_scene("golem: age 0")
        },
        Scene {
            neighbours: 1,
            extra: true,
            ..golem_scene("golem: one neighbour")
        },
        Scene {
            extra: true,
            ..golem_scene("golem: three neighbours, position")
        },
        golem_edge("golem: neighbours 12 west", (-12.0, 0.0, 0.0)),
        golem_edge("golem: neighbours 12 east", (12.0, 0.0, 0.0)),
        golem_edge("golem: neighbours 12 north", (0.0, 0.0, -12.0)),
        golem_edge("golem: neighbours 12 south", (0.0, 0.0, 12.0)),
        golem_edge("golem: neighbours 4 below", (0.0, -4.0, 0.0)),
        golem_edge("golem: neighbours 12 above", (0.0, 12.0, 0.0)),
    ]
}

fn counter_scenes() -> Vec<Scene> {
    vec![
        scene("nothing to do"),
        Scene {
            unhappy: 3,
            sound: 5,
            ..scene("unhappy and trade sound counters")
        },
        Scene {
            unhappy: 1,
            sound: 1,
            ..scene("counters reach zero")
        },
        Scene {
            traded: Some(0),
            ..scene("traded with a player")
        },
        Scene {
            traded: Some(19),
            ..scene("traded with a player near the trading cap")
        },
        Scene {
            merchant_timer: 3,
            ..scene("merchant timer counting down")
        },
        Scene {
            merchant_timer: 1,
            ..scene("merchant timer ends")
        },
        Scene {
            merchant_timer: 1,
            level_up: true,
            ..scene("merchant timer ends with a level up")
        },
        Scene {
            merchant_timer: 1,
            level_up: true,
            trading: true,
            ..scene("merchant timer while trading")
        },
        Scene {
            last_decay: Some(0),
            ..scene("first gossip decay time")
        },
        Scene {
            last_decay: Some(30_000),
            ..scene("gossips decay not yet due")
        },
        Scene {
            last_decay: Some(26_000),
            ..scene("gossips decay")
        },
        Scene {
            time: 3_000,
            last_worked: 49_900,
            ..scene("work hours, worked recently")
        },
    ]
}

fn home_scenes() -> Vec<Scene> {
    vec![
        Scene {
            age: 40,
            ..scene("no bed around")
        },
        Scene {
            age: 41,
            beds: BEDS,
            ..scene("beds around, off the 20-tick beat")
        },
        Scene {
            age: 40,
            beds: BEDS,
            ..scene("finds the nearest bed")
        },
        Scene {
            age: 40,
            beds: BEDS,
            claimed: true,
            ..scene("nearest bed claimed by another villager")
        },
        Scene {
            age: 40,
            beds: SIDE_EDGE_BEDS,
            ..scene("beds at the side edges of the search")
        },
        Scene {
            age: 40,
            beds: HEIGHT_EDGE_BEDS,
            ..scene("beds at the height edges of the search")
        },
        Scene {
            age: 40,
            beds: BEDS,
            home: Home::Foot,
            ..scene("home at a bed's foot")
        },
        Scene {
            age: 40,
            home: Home::Stone,
            ..scene("home is not a bed")
        },
        Scene {
            age: 40,
            home: Home::Stone,
            sleeping: true,
            time: 13_000,
            ..scene("sleeping on a bed that is gone")
        },
    ]
}

fn sleep_scenes() -> Vec<Scene> {
    vec![
        Scene {
            age: 40,
            beds: NEAR_BED,
            home: Home::Bed,
            ..scene("day at home")
        },
        Scene {
            age: 40,
            beds: NEAR_BED,
            home: Home::Bed,
            time: 13_000,
            ..scene("night beside the bed: sleeps")
        },
        Scene {
            age: 40,
            beds: FAR_BED,
            home: Home::Bed,
            time: 13_000,
            ..scene("night, bed a bit too far")
        },
        Scene {
            age: 40,
            beds: CLOSE_BED,
            home: Home::Bed,
            time: 13_000,
            ..scene("night, bed one and a half blocks away")
        },
        Scene {
            age: 40,
            beds: &[Bed {
                at: (1, 0, 0),
                occupied: true,
            }],
            home: Home::Bed,
            time: 13_000,
            ..scene("night, bed already occupied")
        },
        Scene {
            age: 40,
            beds: NEAR_BED,
            home: Home::Bed,
            time: 12_000,
            ..scene("dusk at 12000")
        },
        Scene {
            age: 40,
            beds: NEAR_BED,
            home: Home::Bed,
            time: 11_999,
            ..scene("just before dusk")
        },
        Scene {
            age: 40,
            beds: NEAR_BED,
            home: Home::Bed,
            time: 23_000,
            ..scene("dawn at 23000")
        },
        Scene {
            age: 40,
            beds: NEAR_BED,
            home: Home::Bed,
            time: 23_001,
            ..scene("just after dawn")
        },
        Scene {
            age: 40,
            beds: NEAR_BED,
            home: Home::Bed,
            time: 37_000,
            ..scene("night of the second day")
        },
        Scene {
            age: 40,
            beds: &[Bed {
                at: (1, 0, 0),
                occupied: true,
            }],
            home: Home::Bed,
            sleeping: true,
            ..scene("morning: wakes up")
        },
        Scene {
            age: 40,
            sleeping: true,
            ..scene("sleeping without a home")
        },
        Scene {
            age: 40,
            beds: NEAR_BED,
            home: Home::Bed,
            sleeping: true,
            time: 13_000,
            ..scene("sleeping at night")
        },
    ]
}

const fn golem_scene(name: &'static str) -> Scene {
    Scene {
        name,
        age: 200,
        beds: NEAR_BED,
        home: Home::Bed,
        neighbours: 3,
        last_worked: 40_000,
        ..SCENE
    }
}

fn golem_scenes() -> Vec<Scene> {
    vec![
        golem_scene("golem: three neighbours, worked today"),
        Scene {
            neighbours: 2,
            ..golem_scene("golem: two neighbours")
        },
        Scene {
            golem_near: true,
            ..golem_scene("golem: one already near")
        },
        Scene {
            last_worked: 26_000,
            ..golem_scene("golem: last work a day ago")
        },
        Scene {
            last_worked: 26_001,
            ..golem_scene("golem: last work just under a day ago")
        },
        Scene {
            profession: VillagerProfession::Nitwit,
            ..golem_scene("golem: nitwit")
        },
        Scene {
            age: 220,
            ..golem_scene("golem: off the 100-tick beat")
        },
        Scene {
            home: Home::None,
            beds: &[],
            ..golem_scene("golem: no home")
        },
    ]
}

const fn origin(index: usize) -> BlockPos {
    BlockPos::new(index as i32 * 64 + 300, 64, -200)
}

fn villager(world: &Arc<World>, id: i32, at: Vector3<f64>) -> Result<Arc<VillagerEntity>, String> {
    let entity = Entity::from_uuid_with_id(
        id,
        Uuid::from_u128(0x7669_6c00_0000 + id as u128),
        world.clone(),
        at,
        &EntityType::VILLAGER,
    );
    let villager = VillagerEntity::new(entity);
    if world.spawn_entity(villager.clone()) {
        Ok(villager)
    } else {
        Err(format!("villager {id} not spawned"))
    }
}

fn put_bed(world: &Arc<World>, head: BlockPos, occupied: bool) {
    let mut props = WhiteBedLikeProperties::default(&Block::RED_BED);
    props.facing = HorizontalFacing::North;
    props.occupied = occupied;
    props.part = BedPart::Head;
    world.set_block_state(
        &head,
        props.to_state_id(&Block::RED_BED),
        pumpkin_world::world::BlockFlags::FORCE_STATE,
    );
    props.part = BedPart::Foot;
    world.set_block_state(
        &head.offset(HorizontalFacing::South.to_offset()),
        props.to_state_id(&Block::RED_BED),
        pumpkin_world::world::BlockFlags::FORCE_STATE,
    );
}

fn bed_text(world: &World, base: BlockPos, beds: &[Bed]) -> String {
    beds.iter()
        .map(|bed| {
            let head = base.offset(Vector3::new(bed.at.0, bed.at.1, bed.at.2));
            let state = world.get_block_state_id(&head);
            let block = Block::from_state_id(state);
            if block.id == Block::RED_BED.id {
                WhiteBedLikeProperties::from_state_id(state)
                    .occupied
                    .to_string()
            } else {
                block.name.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Gives the villager the scene's counters, trade state, gossips and profession.
fn prepare(villager: &VillagerEntity, scene: &Scene) {
    let entity = villager.get_entity();
    entity.tick_count.store(scene.age, Ordering::Relaxed);
    villager
        .unhappy_counter
        .store(scene.unhappy, Ordering::Relaxed);
    villager
        .trade_sound_cooldown
        .store(scene.sound, Ordering::Relaxed);
    villager
        .merchant_update_timer
        .store(scene.merchant_timer, Ordering::Relaxed);
    villager
        .increase_profession_level_on_update
        .store(scene.level_up, Ordering::Relaxed);
    villager.is_trading.store(scene.trading, Ordering::Relaxed);
    villager
        .last_worked_at_poi
        .store(scene.last_worked, Ordering::Relaxed);
    *villager
        .villager_data
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) =
        VillagerData::new(VillagerType::Plains, scene.profession, scene.level);
    let player = Uuid::from_u128(0x0000_0bb1);
    let mut gossips = villager
        .gossips
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(value) = scene.traded {
        *villager
            .last_traded_player
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(player);
        if value > 0 {
            gossips
                .entry(player)
                .or_default()
                .insert(GossipType::Trading, value);
        }
    }
    if let Some(last) = scene.last_decay {
        villager
            .last_gossip_decay_time
            .store(last, Ordering::Relaxed);
        let other = gossips.entry(Uuid::from_u128(0x0000_0bb2)).or_default();
        other.insert(GossipType::MinorNegative, 30);
        other.insert(GossipType::Trading, 3);
    }
    drop(gossips);
    if scene.sleeping {
        entity.set_pose(EntityPose::Sleeping);
    }
}

fn villager_text(
    world: &World,
    villager: &VillagerEntity,
    base: BlockPos,
    scene: &Scene,
) -> String {
    let data = *villager
        .villager_data
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut gossips: Vec<String> = villager
        .gossips
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .flat_map(|(who, kinds)| {
            kinds
                .iter()
                .map(move |(kind, value)| format!("{:x}/{}={value}", who.as_u128(), kind.name()))
        })
        .collect();
    gossips.sort();
    let regeneration = villager
        .mob_entity
        .living_entity
        .get_effect(&StatusEffect::REGENERATION)
        .map(|effect| (effect.duration, effect.amplifier));
    let home = *villager
        .home_pos
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let job = *villager
        .job_site
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut text = format!(
        "unhappy {} sound {} gossips {gossips:?} timer {} level up {} level {} profession {} offers {} regeneration {regeneration:?} decay {} worked {} home {:?} sleeping {} beds [{}] job {:?}",
        villager.unhappy_counter.load(Ordering::Relaxed),
        villager.trade_sound_cooldown.load(Ordering::Relaxed),
        villager.merchant_update_timer.load(Ordering::Relaxed),
        villager
            .increase_profession_level_on_update
            .load(Ordering::Relaxed),
        data.level.0,
        data.profession.0,
        villager
            .offers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len(),
        villager.last_gossip_decay_time.load(Ordering::Relaxed),
        villager.last_worked_at_poi.load(Ordering::Relaxed),
        home.map(|pos| pos.0 - base.0),
        villager.get_entity().pose.load() == EntityPose::Sleeping,
        bed_text(world, base, scene.beds),
        job.map(|pos| pos.0 - base.0),
    );
    let around = |kind: &EntityType| {
        world
            .entities
            .load()
            .iter()
            .filter(|other| {
                let at = other.get_entity().pos.load();
                other.get_entity().entity_type == kind
                    && (at.x - f64::from(base.0.x)).abs() < 24.0
                    && (at.z - f64::from(base.0.z)).abs() < 24.0
            })
            .count()
    };
    let _ = write!(
        text,
        " villagers {} golems {}",
        around(&EntityType::VILLAGER),
        around(&EntityType::IRON_GOLEM)
    );
    text
}

fn run(world: &Arc<World>, index: usize, scene: &Scene) -> Result<String, String> {
    let base = origin(index);
    let mut time = world
        .level_time
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    time.time_of_day = scene.time;
    time.world_age = scene.game_time;
    drop(time);
    for bed in scene.beds {
        let head = base.offset(Vector3::new(bed.at.0, bed.at.1, bed.at.2));
        if scene.extra {
            put_bed_exact(world, head, bed.occupied);
        } else {
            put_bed(world, head, bed.occupied);
        }
    }
    let feet = Vector3::new(f64::from(base.0.x) + 0.5, 64.0, f64::from(base.0.z) + 0.5);
    let id = 70_000 + index as i32 * 8;
    let first_bed = scene
        .beds
        .first()
        .map(|bed| base.offset(Vector3::new(bed.at.0, bed.at.1, bed.at.2)));
    if scene.claimed {
        let (x, y, z) = scene.claimant_at.unwrap_or((-3.0, 0.0, 0.0));
        let other = villager(world, id + 1, feet.add(&Vector3::new(x, y, z)))?;
        *other
            .home_pos
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = first_bed;
    }
    for n in 0..scene.neighbours {
        let at = scene.neighbours_at.map_or_else(
            || Vector3::new(f64::from(n) * 2.0 - 3.0, 0.0, 4.0),
            |(x, y, z)| Vector3::new(x + f64::from(n) * 0.6, y, z + f64::from(n) * 0.6),
        );
        villager(world, id + 2 + i32::from(n), feet.add(&at))?;
    }
    if scene.golem_near {
        let golem = Entity::from_uuid_with_id(
            id + 7,
            Uuid::from_u128(0x676f_6c00_0000 + index as u128),
            world.clone(),
            feet.add(&Vector3::new(6.0, 0.0, -6.0)),
            &EntityType::IRON_GOLEM,
        );
        let golem = crate::entity::passive::iron_golem::IronGolemEntity::new(golem);
        if !world.spawn_entity(golem) {
            return Err("golem not spawned".to_string());
        }
    }
    let villager = villager(world, id, feet)?;
    let home = match scene.home {
        Home::None => None,
        Home::Bed => first_bed,
        Home::Foot => first_bed.map(|head| head.offset(HorizontalFacing::South.to_offset())),
        Home::Stone => {
            let stone = base.offset(Vector3::new(3, 0, 0));
            world.set_block_state(
                &stone,
                Block::STONE.default_state.id,
                pumpkin_world::world::BlockFlags::FORCE_STATE,
            );
            Some(stone)
        }
    };
    *villager
        .home_pos
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = home;
    prepare(&villager, scene);
    if scene.extra {
        return run_extra(world, index, scene, &villager);
    }
    villager.villager_mob_tick();
    Ok(format!(
        "{}: {}\n",
        scene.name,
        villager_text(world, &villager, base, scene)
    ))
}

const WATCHER: Identity = Identity {
    name: "Watcher",
    uuid: Uuid::from_u128(0x0000_0bb3),
    entity_id: 69_999,
};

/// Appended scenes: the composter, the watcher, the extra workers, one tick, and the line with
/// its suffix.
fn run_extra(
    world: &Arc<World>,
    index: usize,
    scene: &Scene,
    villager: &Arc<VillagerEntity>,
) -> Result<String, String> {
    let base = origin(index);
    if scene.composter {
        world.set_block_state(
            &base.offset(Vector3::new(2, 0, -2)),
            Block::COMPOSTER.default_state.id,
            pumpkin_world::world::BlockFlags::FORCE_STATE,
        );
    }
    let (watcher, mut tap) = test_support::java_player_as(world, GameMode::Survival, &WATCHER)
        .ok_or("no Java packet tap")?;
    world.add_player(&watcher)?;
    watch(&watcher, world, base, villager.get_entity().entity_id);
    let feet = villager.get_entity().pos.load();
    // With extra workers only they tick: whether one villager works is a coin flip, so the
    // line shows the scene's villager untouched and whether any of the 63 worked.
    let mut workers = Vec::new();
    if !scene.workers {
        workers.push(Arc::clone(villager));
    } else {
        for n in 1..64 {
            let worker = self::villager(world, 900_000 + index as i32 * 64 + n, feet)?;
            prepare(&worker, scene);
            workers.push(worker);
        }
    }
    villager.get_entity().synched_data.clear_dirty();
    drop(tap.drain());
    for worker in &workers {
        worker.villager_mob_tick();
    }
    let dirty = villager
        .get_entity()
        .synched_data
        .pack_dirty_for_version(&JavaMinecraftVersion::V_26_3)
        .map(|bytes| format!("{}:{:08x}", bytes.len(), fnv(&bytes) as u32));
    let regeneration = villager
        .mob_entity
        .living_entity
        .get_effect(&StatusEffect::REGENERATION)
        .map(|e| (e.ambient, e.show_particles, e.show_icon, e.blend));
    let worked = workers
        .iter()
        .any(|w| w.last_worked_at_poi.load(Ordering::Relaxed) == scene.game_time);
    let packets: Vec<String> = tap
        .drain()
        .iter()
        .map(|p| match p.first() {
            // Add Entity of a spawned golem: its UUID is random.
            Some(1) => format!("{}:id1", p.len()),
            _ => format!("{}:{:08x}", p.len(), fnv(p) as u32),
        })
        .collect();
    let line = format!(
        "{}: {} | regeneration flags {regeneration:?} dirty {dirty:?} watcher {packets:?} worked {worked} golems at {:?}\n",
        scene.name,
        villager_text(world, villager, base, scene),
        golems_at(world, base),
    );
    Ok(line)
}

/// The watcher stands near the villager with every chunk around it sent, paired with it.
fn watch(watcher: &Arc<Player>, world: &Arc<World>, base: BlockPos, villager_id: i32) {
    watcher.get_entity().set_pos(Vector3::new(
        f64::from(base.0.x),
        64.0,
        f64::from(base.0.z) - 6.0,
    ));
    let chunk = base.chunk_position();
    watcher.watched_section.store(Cylindrical::new(
        chunk,
        NonZero::new(3).unwrap_or(NonZero::<u8>::MIN),
    ));
    let mut sender = watcher
        .chunk_sender
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for dx in -3..=3 {
        for dz in -3..=3 {
            sender.mark_sent_out_of_band(Vector2::new(chunk.x + dx, chunk.y + dz));
        }
    }
    drop(sender);
    if let Some(tracked) = world.entity_tracker.get_tracked_entity(villager_id) {
        tracked.update_player(watcher, world);
    }
}

/// Iron golems within 24 blocks: position relative to the scene's origin, two decimals, sorted.
fn golems_at(world: &World, base: BlockPos) -> Vec<String> {
    let mut found: Vec<String> = world
        .entities
        .load()
        .iter()
        .filter(|e| e.get_entity().entity_type == &EntityType::IRON_GOLEM)
        .map(|e| e.get_entity().pos.load())
        .filter(|at| {
            (at.x - f64::from(base.0.x)).abs() < 24.0 && (at.z - f64::from(base.0.z)).abs() < 24.0
        })
        .map(|at| {
            format!(
                "{:.2},{:.2},{:.2}",
                at.x - f64::from(base.0.x),
                at.y - f64::from(base.0.y),
                at.z - f64::from(base.0.z)
            )
        })
        .collect();
    found.sort();
    found
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Appended scenes' bed writer: foot, then head, notifying listeners only, so that neither half
/// reshapes the other.
fn put_bed_exact(world: &Arc<World>, head: BlockPos, occupied: bool) {
    let mut props = WhiteBedLikeProperties::default(&Block::RED_BED);
    props.facing = HorizontalFacing::North;
    props.occupied = occupied;
    props.part = BedPart::Foot;
    world.set_block_state(
        &head.offset(HorizontalFacing::South.to_offset()),
        props.to_state_id(&Block::RED_BED),
        pumpkin_world::world::BlockFlags::NOTIFY_LISTENERS,
    );
    props.part = BedPart::Head;
    world.set_block_state(
        &head,
        props.to_state_id(&Block::RED_BED),
        pumpkin_world::world::BlockFlags::NOTIFY_LISTENERS,
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn villager_tick_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let scenes = scenes();
    for index in 0..scenes.len() {
        let chunk = origin(index).chunk_position();
        for dx in -3..=3 {
            for dz in -3..=3 {
                let at = Vector2::new(chunk.x + dx, chunk.y + dz);
                world
                    .level
                    .loaded_chunks
                    .insert(at, pumpkin_world::chunk::ChunkData::empty_sync(at.x, at.y));
            }
        }
    }
    let mut out = String::new();
    for (index, scene) in scenes.iter().enumerate() {
        out.push_str(&run(&world, index, scene)?);
        for player in world.players.load().iter() {
            world.remove_player(player, false).await;
        }
    }
    server.stop();
    script_golden::bless_or_compare("villager_tick.txt", &out)
}
