//! A record of what `Entity::remove_passenger_internal` does, made before the function was
//! split (`tests/golden/dismount.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Seventeen small terrains are built once, side by side (open floor, pillars on chosen sides, each corner left as the only free column, a step up on one side and a pit on the other,
//! all sides walled, water under the vehicle, low ceilings, a slab floor, no floor, closed in
//! with and without room on top of the vehicle, a ceiling that leaves room only to crouch). On each, a minecart or a horse carrying one or
//! two passengers (a zombie, a real player over an in-memory connection, a sneaking player)
//! faces 15 yaws around every direction boundary, and one passenger (or an id that does not
//! ride) is removed with and without repositioning. Each line hashes the outcome over the yaws:
//! the passenger's position, pose, sneaking flag and riding cooldown, who still rides, and the
//! teleport the player waits for. The poses seen are listed beside the hash.
//!
//! Not seen: the packets (nobody reads the in-memory connection) and a plugin cancelling the
//! dismount (the real server loads no plugin).

use std::collections::BTreeSet;
use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

use pumpkin_data::block_properties::{SlabType, WhiteWoolSlabLikeProperties};
use pumpkin_data::entity::EntityType;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::world::BlockFlags;

use crate::entity::ai::test_support;
use crate::entity::living::LivingEntity;
use crate::entity::player::Player;
use crate::entity::{Entity, EntityBase};
use crate::world::World;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// FNV-1a: a hash that does not change with the Rust version.
fn fnv(hash: u64, text: &str) -> u64 {
    let mut hash = hash;
    for byte in text.bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

const TERRAINS: [&str; 18] = [
    "open",
    "pillar_n",
    "pillars_ew",
    "pillars_nsew",
    "pillars_7",
    "walled",
    "water_below",
    "ceiling_1",
    "slab_floor_ceiling_2",
    "ceiling_2",
    "no_floor",
    "closed",
    "closed_room_on_top",
    "top_slab_ceiling",
    "all_but_ne",
    "all_but_nw",
    "all_but_se",
    "step_north_pit_south",
];

/// Neighbour columns in the order north, south, east, west, then the corners.
const COLUMNS: [(i32, i32); 8] = [
    (0, -1),
    (0, 1),
    (1, 0),
    (-1, 0),
    (1, -1),
    (-1, -1),
    (1, 1),
    (-1, 1),
];

const FLOOR_Y: i32 = 63;

/// The vehicle's block column of terrain `index`.
fn centre(index: usize) -> (i32, i32) {
    (index as i32 * 16 + 8, 8)
}

fn put(world: &Arc<World>, x: i32, y: i32, z: i32, state: BlockStateId) {
    world.set_block_state(
        &BlockPos(Vector3::new(x, y, z)),
        state,
        BlockFlags::NOTIFY_ALL,
    );
}

/// Fills the 5 x 5 square around the centre at height `y`.
fn layer(world: &Arc<World>, index: usize, y: i32, state: BlockStateId) {
    let (cx, cz) = centre(index);
    for dx in -2..=2 {
        for dz in -2..=2 {
            put(world, cx + dx, y, cz + dz, state);
        }
    }
}

fn pillars(world: &Arc<World>, index: usize, columns: &[usize]) {
    let (cx, cz) = centre(index);
    let stone = Block::STONE.default_state.id;
    for &column in columns {
        let (dx, dz) = COLUMNS[column];
        put(world, cx + dx, FLOOR_Y + 1, cz + dz, stone);
        put(world, cx + dx, FLOOR_Y + 2, cz + dz, stone);
    }
}

fn build(world: &Arc<World>, index: usize) {
    let stone = Block::STONE.default_state.id;
    let (cx, cz) = centre(index);
    if TERRAINS[index] != "no_floor" {
        let floor = if TERRAINS[index] == "slab_floor_ceiling_2" {
            Block::STONE_SLAB.default_state.id
        } else {
            stone
        };
        layer(world, index, FLOOR_Y, floor);
    }
    match TERRAINS[index] {
        "pillar_n" => pillars(world, index, &[0]),
        "pillars_ew" => pillars(world, index, &[2, 3]),
        "pillars_nsew" => pillars(world, index, &[0, 1, 2, 3]),
        "pillars_7" => pillars(world, index, &[0, 1, 2, 3, 4, 5, 6]),
        "all_but_ne" => pillars(world, index, &[0, 1, 2, 3, 5, 6, 7]),
        "all_but_nw" => pillars(world, index, &[0, 1, 2, 3, 4, 6, 7]),
        "all_but_se" => pillars(world, index, &[0, 1, 2, 3, 4, 5, 7]),
        "step_north_pit_south" => {
            // Nothing fits at the vehicle's level; one block up in the north column and one
            // block down in the south column both fit, so the search order of the heights shows.
            pillars(world, index, &[2, 3, 4, 5, 6, 7]);
            put(world, cx, FLOOR_Y + 1, cz - 1, stone);
            put(world, cx, FLOOR_Y, cz + 1, Block::AIR.default_state.id);
            put(world, cx, FLOOR_Y - 1, cz + 1, stone);
        }
        "walled" => pillars(world, index, &[0, 1, 2, 3, 4, 5, 6, 7]),
        "water_below" => put(world, cx, FLOOR_Y, cz, Block::WATER.default_state.id),
        "ceiling_1" => layer(world, index, FLOOR_Y + 2, stone),
        "slab_floor_ceiling_2" | "ceiling_2" => layer(world, index, FLOOR_Y + 3, stone),
        "top_slab_ceiling" => {
            let mut top = WhiteWoolSlabLikeProperties::default(&Block::STONE_SLAB);
            top.r#type = SlabType::Top;
            layer(
                world,
                index,
                FLOOR_Y + 2,
                top.to_state_id(&Block::STONE_SLAB),
            );
        }
        "closed" | "closed_room_on_top" => {
            for y in FLOOR_Y + 1..=FLOOR_Y + 3 {
                layer(world, index, y, stone);
            }
            put(world, cx, FLOOR_Y + 1, cz, Block::AIR.default_state.id);
            if TERRAINS[index] == "closed_room_on_top" {
                put(world, cx, FLOOR_Y + 2, cz, Block::AIR.default_state.id);
            }
        }
        _ => {}
    }
}

const YAWS: [f32; 15] = [
    0.0, 44.9, 45.0, 90.0, 134.9, 135.0, 180.0, 224.9, 225.0, 270.0, 314.9, 315.0, 359.9, -90.0,
    400.0,
];

/// Who rides (in order) and which of them leaves; `None` removes an id that does not ride.
struct Setup {
    name: &'static str,
    riders: &'static [Rider],
    leaves: Option<usize>,
}

#[derive(Clone, Copy)]
enum Rider {
    Zombie,
    Player,
    SneakingPlayer,
}

const SETUPS: [Setup; 5] = [
    Setup {
        name: "player",
        riders: &[Rider::Player],
        leaves: Some(0),
    },
    Setup {
        name: "zombie",
        riders: &[Rider::Zombie],
        leaves: Some(0),
    },
    Setup {
        name: "zombie+sneaking_player",
        riders: &[Rider::Zombie, Rider::SneakingPlayer],
        leaves: Some(1),
    },
    Setup {
        name: "player+zombie",
        riders: &[Rider::Player, Rider::Zombie],
        leaves: Some(1),
    },
    Setup {
        name: "zombie,missing",
        riders: &[Rider::Zombie],
        leaves: None,
    },
];

/// One ride: the vehicle, the riders and the players among them.
struct Ride {
    vehicle: Arc<dyn EntityBase>,
    riders: Vec<Arc<dyn EntityBase>>,
    players: Vec<Option<Arc<Player>>>,
}

fn mount(world: &Arc<World>, kind: &'static EntityType, at: Vector3<f64>, setup: &Setup) -> Ride {
    let vehicle: Arc<dyn EntityBase> = Arc::new(Entity::new(Arc::clone(world), at, kind));
    let mut riders: Vec<Arc<dyn EntityBase>> = Vec::new();
    let mut players = Vec::new();
    for rider in setup.riders {
        let seat = Vector3::new(at.x, at.y + 1.0, at.z);
        match rider {
            Rider::Zombie => {
                let zombie =
                    LivingEntity::new(Entity::new(Arc::clone(world), seat, &EntityType::ZOMBIE));
                riders.push(Arc::new(zombie));
                players.push(None);
            }
            Rider::Player | Rider::SneakingPlayer => {
                let player = test_support::java_player(world, GameMode::Survival);
                player.get_entity().set_pos(seat);
                if matches!(rider, Rider::SneakingPlayer) {
                    player.get_entity().sneaking.store(true, Relaxed);
                }
                riders.push(Arc::clone(&player) as Arc<dyn EntityBase>);
                players.push(Some(player));
            }
        }
    }
    for rider in &riders {
        vehicle
            .get_entity()
            .add_passenger_silently(Arc::clone(&vehicle), Arc::clone(rider));
    }
    Ride {
        vehicle,
        riders,
        players,
    }
}

/// The visible outcome of one dismount, without entity ids (they come from a global counter).
fn outcome(ride: &Ride, poses: &mut BTreeSet<String>) -> String {
    let vehicle = ride.vehicle.get_entity();
    let mut text = String::new();
    for (index, rider) in ride.riders.iter().enumerate() {
        let entity = rider.get_entity();
        let pos = entity.pos.load();
        let pose = format!("pose{}", entity.pose.load() as i32);
        let _ = write!(
            text,
            "[{index}: {:?} {:?} {:?} {pose} sneak {} cool {} rides {}",
            pos.x,
            pos.y,
            pos.z,
            entity.sneaking.load(Relaxed),
            entity.riding_cooldown.load(Relaxed),
            vehicle.has_passenger(entity.entity_id),
        );
        if let Some(player) = &ride.players[index] {
            let teleport = player
                .awaiting_teleport
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_ref()
                .map(|(id, at)| (id.0, *at));
            let _ = write!(
                text,
                " tp {teleport:?} count {}",
                player.teleport_id_count.load(Relaxed)
            );
        }
        text.push(']');
        poses.insert(pose);
    }
    text
}

fn run(world: &Arc<World>) -> String {
    let mut report = String::new();
    for (index, terrain) in TERRAINS.iter().enumerate() {
        let (cx, cz) = centre(index);
        for (kind_name, kind) in [
            ("minecart", &EntityType::MINECART),
            ("horse", &EntityType::HORSE),
        ] {
            for offset in [0.5, 0.05] {
                for setup in &SETUPS {
                    for reposition in [true, false] {
                        let mut hash = 0xcbf2_9ce4_8422_2325u64;
                        let mut poses = BTreeSet::new();
                        for yaw in YAWS {
                            let at = Vector3::new(
                                f64::from(cx) + offset,
                                f64::from(FLOOR_Y + 1),
                                f64::from(cz) + offset,
                            );
                            let ride = mount(world, kind, at, setup);
                            ride.vehicle.get_entity().yaw.store(yaw);
                            let leaving = setup
                                .leaves
                                .map_or(i32::MAX, |i| ride.riders[i].get_entity().entity_id);
                            ride.vehicle
                                .get_entity()
                                .remove_passenger_internal(leaving, reposition);
                            hash = fnv(hash, &outcome(&ride, &mut poses));
                        }
                        let poses: Vec<_> = poses.into_iter().collect();
                        let _ = writeln!(
                            report,
                            "{terrain} {kind_name} +{offset} {} reposition {reposition}: {hash:016x} poses {}",
                            setup.name,
                            poses.join(","),
                        );
                    }
                }
            }
        }
    }
    report
}

#[tokio::test(flavor = "multi_thread")]
async fn dismount() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server
        .worlds
        .load()
        .first()
        .cloned()
        .ok_or("the server has no world")?;
    for index in 0..TERRAINS.len() {
        world.level.loaded_chunks.insert(
            Vector2::new(index as i32, 0),
            pumpkin_world::chunk::ChunkData::empty_sync(index as i32, 0),
        );
    }
    for index in 0..TERRAINS.len() {
        build(&world, index);
    }
    let now = run(&world);
    server.stop();

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("dismount.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, now);
    Ok(())
}
