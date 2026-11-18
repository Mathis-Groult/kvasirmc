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
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use uuid::Uuid;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;
use crate::entity::passive::villager::{
    GossipType, VillagerData, VillagerEntity, VillagerProfession, VillagerType,
};
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
    scenes
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
        put_bed(
            world,
            base.offset(Vector3::new(bed.at.0, bed.at.1, bed.at.2)),
            bed.occupied,
        );
    }
    let feet = Vector3::new(f64::from(base.0.x) + 0.5, 64.0, f64::from(base.0.z) + 0.5);
    let id = 70_000 + index as i32 * 8;
    let first_bed = scene
        .beds
        .first()
        .map(|bed| base.offset(Vector3::new(bed.at.0, bed.at.1, bed.at.2)));
    if scene.claimed {
        let other = villager(world, id + 1, feet.add(&Vector3::new(-3.0, 0.0, 0.0)))?;
        *other
            .home_pos
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = first_bed;
    }
    for n in 0..scene.neighbours {
        let x = f64::from(n) * 2.0 - 3.0;
        villager(
            world,
            id + 2 + i32::from(n),
            feet.add(&Vector3::new(x, 0.0, 4.0)),
        )?;
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
    villager.villager_mob_tick();
    Ok(format!(
        "{}: {}\n",
        scene.name,
        villager_text(world, &villager, base, scene)
    ))
}

#[tokio::test(flavor = "multi_thread")]
async fn villager_tick_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let scenes = scenes();
    for index in 0..scenes.len() {
        let chunk = origin(index).chunk_position();
        for dx in -2..=2 {
            for dz in -2..=2 {
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
    }
    server.stop();
    script_golden::bless_or_compare("villager_tick.txt", &out)
}
