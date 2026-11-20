//! A record of what `AreaEffectCloudEntity::tick` does, made before it is split
//! (`tests/golden/area_effect_cloud_tick.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene is built in its own loaded area of a real server's world (stone floor at y 63).
//! A cloud with the scene's numbers is spawned with a fixed entity id, zombies stand around it
//! (and a survival watcher in a corner), and the cloud is ticked by calling `tick` directly.
//! After every tick the cloud's age, radius, duration, reapplication map and removal, each
//! zombie's health and effects, the entity count and what the watcher was sent (sounds by
//! length only: their seed is random) are hashed; the hash of every 10 ticks is reported.
//! `KVASIR_DUMP=<file>` appends every recorded tick as text.

use std::fmt::Write;
use std::num::NonZero;
use std::sync::Arc;

use pumpkin_data::Block;
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::version::JavaMinecraftVersion;
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::area_effect_cloud::AreaEffectCloudEntity;
use crate::entity::living::LivingEntity;
use crate::entity::player::Player;
use crate::entity::{Entity, EntityBase};
use crate::net::java::PacketTap;
use crate::server::Server;
use crate::world::World;

const FLOOR_Y: i32 = 63;
const SOUND_PACKET: u8 = 119;

type Vec3 = (f64, f64, f64);

#[derive(Clone, Copy)]
struct Numbers {
    duration: i32,
    radius: f32,
    delay: i32,
    wait: i32,
    radius_on_use: f32,
    duration_on_use: i32,
}

struct Scene {
    name: &'static str,
    numbers: Numbers,
    effects: &'static [&'static StatusEffect],
    /// Where the zombies stand, relative to the scene's corner.
    zombies: &'static [Vec3],
    /// Another cloud next to the first, to be skipped by it.
    twin: bool,
    /// Stands the watcher inside the cloud.
    watcher_inside: bool,
    ticks: usize,
}

const BASE: Numbers = Numbers {
    duration: 40,
    radius: 3.0,
    delay: 10,
    wait: 5,
    radius_on_use: 0.0,
    duration_on_use: 0,
};

const CLOUD_AT: Vec3 = (12.0, 64.0, 12.0);

/// Zombies 0.5, 1.5, 2.5 and 3.5 blocks from the cloud on the x axis, and one above it.
const RING: &[Vec3] = &[
    (12.5, 64.0, 12.0),
    (13.5, 64.0, 12.0),
    (14.5, 64.0, 12.0),
    (15.5, 64.0, 12.0),
    (12.0, 66.0, 12.0),
];

const fn scene(
    name: &'static str,
    numbers: Numbers,
    effects: &'static [&'static StatusEffect],
    ticks: usize,
) -> Scene {
    Scene {
        name,
        numbers,
        effects,
        zombies: RING,
        twin: false,
        watcher_inside: false,
        ticks,
    }
}

const SCENES: &[Scene] = &[
    scene("slowness", BASE, &[&StatusEffect::SLOWNESS], 60),
    scene("instant_damage", BASE, &[&StatusEffect::INSTANT_DAMAGE], 60),
    scene("instant_health", BASE, &[&StatusEffect::INSTANT_HEALTH], 60),
    scene(
        "two_effects",
        BASE,
        &[&StatusEffect::SLOWNESS, &StatusEffect::POISON],
        60,
    ),
    scene("no_effects", BASE, &[], 60),
    scene(
        "shrinks_on_use",
        Numbers {
            radius_on_use: -0.5,
            radius: 2.0,
            ..BASE
        },
        &[&StatusEffect::SLOWNESS],
        60,
    ),
    scene(
        "shrunk_away",
        Numbers {
            radius_on_use: -2.7,
            ..BASE
        },
        &[&StatusEffect::SLOWNESS],
        30,
    ),
    scene(
        "cut_short",
        Numbers {
            duration_on_use: -40,
            ..BASE
        },
        &[&StatusEffect::SLOWNESS],
        30,
    ),
    scene(
        "grows_on_use",
        Numbers {
            radius_on_use: 0.4,
            radius: 1.0,
            ..BASE
        },
        &[&StatusEffect::SLOWNESS],
        60,
    ),
    scene(
        "shortens_on_use",
        Numbers {
            duration_on_use: -12,
            ..BASE
        },
        &[&StatusEffect::INSTANT_DAMAGE],
        60,
    ),
    scene(
        "lengthens_on_use",
        Numbers {
            duration_on_use: 5,
            delay: 3,
            ..BASE
        },
        &[&StatusEffect::INSTANT_DAMAGE],
        80,
    ),
    scene(
        "waits_past_end",
        Numbers {
            wait: 30,
            duration: 20,
            ..BASE
        },
        &[&StatusEffect::SLOWNESS],
        40,
    ),
    scene(
        "no_wait",
        Numbers { wait: 0, ..BASE },
        &[&StatusEffect::SLOWNESS],
        20,
    ),
    scene(
        "wait_ends_on_age",
        Numbers { wait: 1, ..BASE },
        &[&StatusEffect::SLOWNESS],
        6,
    ),
    scene(
        "short_life",
        Numbers {
            duration: 12,
            wait: 2,
            radius: 1.0,
            ..BASE
        },
        &[&StatusEffect::SLOWNESS],
        30,
    ),
    Scene {
        twin: true,
        ..scene("twin", BASE, &[&StatusEffect::SLOWNESS], 30)
    },
    Scene {
        watcher_inside: true,
        zombies: &[],
        ..scene("player", BASE, &[&StatusEffect::SLOWNESS], 40)
    },
    Scene {
        watcher_inside: true,
        zombies: &[],
        ..scene("player_instant", BASE, &[&StatusEffect::INSTANT_DAMAGE], 40)
    },
];

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
}

fn spawn_zombie(area: &Area<'_>, slot: usize, at: Vec3) -> Arc<LivingEntity> {
    let entity = Entity::from_uuid_with_id(
        81_000 + slot as i32,
        Uuid::from_u128(0x4146_4643_0000 + slot as u128),
        Arc::clone(area.world),
        area.point(at),
        &EntityType::ZOMBIE,
    );
    let living = Arc::new(LivingEntity::new(entity));
    let base: Arc<dyn EntityBase> = living.clone();
    area.world.spawn_entity_non_save(base);
    living
}

fn spawn_cloud(area: &Area<'_>, slot: usize, scene: &Scene, at: Vec3) -> Arc<dyn EntityBase> {
    let entity = Entity::from_uuid_with_id(
        82_000 + slot as i32,
        Uuid::from_u128(0x4146_4643_1000 + slot as u128),
        Arc::clone(area.world),
        area.point(at),
        &EntityType::AREA_EFFECT_CLOUD,
    );
    let n = scene.numbers;
    let effects = scene
        .effects
        .iter()
        .map(|effect| (*effect, 200, 0, false, true, true))
        .collect();
    let cloud = AreaEffectCloudEntity::create(
        entity,
        ItemStack::new(1, &Item::GLASS_BOTTLE),
        effects,
        n.duration,
        n.radius,
        n.delay,
        n.wait,
        n.radius_on_use,
        n.duration_on_use,
    );
    area.world.spawn_entity_non_save(Arc::clone(&cloud));
    cloud
}

fn effects_text(living: &LivingEntity) -> String {
    let effects = living
        .active_effects
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut effects: Vec<_> = effects
        .values()
        .map(|e| {
            format!(
                "{}:{}:{}:{}{}{}{}",
                e.effect_type.id,
                e.duration,
                e.amplifier,
                u8::from(e.ambient),
                u8::from(e.show_particles),
                u8::from(e.show_icon),
                u8::from(e.blend)
            )
        })
        .collect();
    effects.sort();
    effects.join(",")
}

fn cloud_text(cloud: &dyn EntityBase) -> String {
    let Some(cloud) = cloud.cast_any().downcast_ref::<AreaEffectCloudEntity>() else {
        return "not a cloud".to_string();
    };
    let get = |value: &std::sync::Mutex<i32>| {
        *value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    };
    let radius = *cloud
        .radius
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut map: Vec<_> = cloud
        .reapplication_map
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .map(|(id, left)| (*id, *left))
        .collect();
    map.sort_unstable();
    format!(
        "age {} radius {radius:.4} duration {} map {map:?} removed {}",
        get(&cloud.age),
        get(&cloud.duration),
        cloud.entity.is_removed()
    )
}

fn sent_text(tap: &mut PacketTap) -> String {
    tap.drain()
        .iter()
        .map(|p| {
            if p.first() == Some(&SOUND_PACKET) {
                return format!("{SOUND_PACKET}:{}", p.len());
            }
            let hash = p.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
            });
            format!(
                "{}:{}:{:08x}",
                p.first().copied().unwrap_or(0),
                p.len(),
                hash as u32
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

struct Cast {
    clouds: Vec<Arc<dyn EntityBase>>,
    zombies: Vec<Arc<LivingEntity>>,
    watcher: Arc<Player>,
}

/// The tracked data changed since the last look, by a hash of its bytes.
fn meta_text(entity: &Entity) -> String {
    let data = &entity.synched_data;
    let text = data
        .pack_dirty_for_version(&JavaMinecraftVersion::V_26_3)
        .map_or_else(
            || "-".to_owned(),
            |bytes| {
                let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
                    (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
                });
                format!("{:08x}", hash as u32)
            },
        );
    data.clear_dirty();
    text
}

fn tick_text(cast: &Cast, world: &World, tap: &mut PacketTap) -> String {
    let mut text = String::new();
    for cloud in &cast.clouds {
        let _ = write!(
            text,
            "{} meta {}|",
            cloud_text(cloud.as_ref()),
            meta_text(cloud.get_entity())
        );
    }
    for zombie in &cast.zombies {
        let _ = write!(
            text,
            "{:.2} [{}]|",
            zombie.health.load(),
            effects_text(zombie)
        );
    }
    let _ = write!(
        text,
        "watcher {:.2} [{}]",
        cast.watcher.living_entity.health.load(),
        effects_text(&cast.watcher.living_entity)
    );
    let _ = write!(text, " entities {}", world.entities.load().len());
    let _ = write!(text, " sent [{}]", sent_text(tap));
    text
}

fn fly(
    server: &Arc<Server>,
    world: &Arc<World>,
    scene: &Scene,
    cast: &Cast,
    tap: &mut PacketTap,
    report: &mut String,
) -> TestResult {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for tick in 1..=scene.ticks {
        for cloud in &cast.clouds {
            if !cloud.get_entity().is_removed() {
                EntityBase::tick(cloud.as_ref(), cloud.as_ref(), server);
            }
        }
        let text = tick_text(cast, world, tap);
        if let Some(file) = std::env::var_os("KVASIR_DUMP") {
            use std::io::Write as _;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(file)?;
            writeln!(file, "{} {tick} {text}", scene.name)?;
        }
        hash = text.bytes().fold(hash, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
        if tick % 10 == 0 || tick == scene.ticks {
            let _ = writeln!(report, "{} ticks to {tick}: {hash:016x}", scene.name);
        }
    }
    let _ = writeln!(
        report,
        "{} end: {}",
        scene.name,
        cloud_text(cast.clouds[0].as_ref())
    );
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
    for x in 0..24 {
        for z in 0..24 {
            area.put(&Block::STONE, x, FLOOR_Y, z);
        }
    }

    let who = Identity {
        name: "CloudWatch",
        uuid: Uuid::from_u128(0x436c_6f75_6400_0000 + index as u128),
        entity_id: 61_000 + index as i32,
    };
    let (watcher, mut tap) = test_support::java_player_as(world, GameMode::Survival, &who)
        .ok_or("the watcher's client no longer owned its outgoing queue")?;
    let at = if scene.watcher_inside {
        (13.0, 64.0, 12.0)
    } else {
        (0.5, 64.0, 23.5)
    };
    watcher.get_entity().set_pos(area.point(at));
    watcher.watched_section.store(Cylindrical::new(
        Vector2::new((area.x + 12) >> 4, (area.z + 12) >> 4),
        NonZero::<u8>::MIN.saturating_add(3),
    ));
    world.add_player(&watcher)?;

    let zombies = scene
        .zombies
        .iter()
        .enumerate()
        .map(|(slot, at)| spawn_zombie(&area, slot, *at))
        .collect();
    let mut clouds = vec![spawn_cloud(&area, 0, scene, CLOUD_AT)];
    if scene.twin {
        clouds.push(spawn_cloud(&area, 1, scene, (12.5, 64.0, 12.0)));
    }
    let cast = Cast {
        clouds,
        zombies,
        watcher,
    };
    drop(tap.drain());
    fly(server, world, scene, &cast, &mut tap, report)?;

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
async fn area_effect_cloud_tick() -> TestResult {
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
    script_golden::bless_or_compare("area_effect_cloud_tick.txt", &report)
}
