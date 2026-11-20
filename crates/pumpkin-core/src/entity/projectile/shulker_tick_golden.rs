//! A record of what `ShulkerBulletEntity::tick` does, made before it is split
//! (`tests/golden/shulker_tick.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene is built in its own loaded area of a real server's world (stone floor at y 63).
//! An owner zombie sits in the area, a bullet is made from it with `ShulkerBulletEntity::new`
//! (so it starts at the owner's box centre) and ticked by calling `tick` directly. After every
//! tick the bullet's position, velocity and removal, the target's health and effects, the
//! entity count and the packets the watcher got (by id and length: entity ids are not fixed) are
//! hashed; the hash of every 10 ticks is reported. The bullet picks its axis steps with
//! random numbers, so scenes marked `random` only report where it ended.

use std::fmt::Write;
use std::num::NonZero;
use std::sync::Arc;

use pumpkin_data::Block;
use pumpkin_data::entity::EntityType;
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use super::shulker_bullet::ShulkerBulletEntity;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::living::LivingEntity;
use crate::entity::mob::shulker::Axis;
use crate::entity::{Entity, EntityBase};
use crate::net::java::PacketTap;
use crate::server::Server;
use crate::world::World;

const FLOOR_Y: i32 = 63;

type Vec3 = (f64, f64, f64);

#[derive(Clone, Copy)]
struct Target {
    at: Vec3,
    health: f32,
}

#[derive(Clone, Copy)]
enum Aim {
    /// No target: the bullet only falls.
    Nobody,
    /// A target the bullet knows by id.
    Mob(Target),
    /// An id that no entity has.
    Missing,
}

struct Scene {
    name: &'static str,
    owner: Vec3,
    aim: Aim,
    /// Where the bullet is told the target is.
    told: Vec3,
    axis: Axis,
    /// Stone blocks, relative to the scene's corner.
    stones: &'static [(i32, i32, i32)],
    ticks: usize,
    random: bool,
}

const fn scene(name: &'static str, owner: Vec3, aim: Aim, told: Vec3, ticks: usize) -> Scene {
    Scene {
        name,
        owner,
        aim,
        told,
        axis: Axis::Y,
        stones: &[],
        ticks,
        random: false,
    }
}

const fn alive(at: Vec3) -> Aim {
    Aim::Mob(Target { at, health: 20.0 })
}

const SCENES: &[Scene] = &[
    scene("fall", (4.5, 70.0, 4.5), Aim::Nobody, (4.5, 70.0, 4.5), 40),
    scene(
        "missing",
        (4.5, 70.0, 4.5),
        Aim::Missing,
        (12.5, 70.0, 4.5),
        40,
    ),
    scene(
        "dead_target",
        (4.5, 70.0, 4.5),
        Aim::Mob(Target {
            at: (12.5, 70.0, 4.5),
            health: 0.0,
        }),
        (12.5, 70.0, 4.5),
        40,
    ),
    scene(
        "near",
        (4.5, 66.0, 4.5),
        alive((6.0, 66.0, 4.5)),
        (6.0, 66.0, 4.5),
        40,
    ),
    scene(
        "near_above",
        (4.5, 66.0, 4.5),
        alive((4.5, 67.5, 4.5)),
        (4.5, 67.5, 4.5),
        40,
    ),
    scene(
        "far_east",
        (4.5, 66.0, 4.5),
        alive((14.5, 66.0, 4.5)),
        (14.5, 66.0, 4.5),
        9,
    ),
    Scene {
        axis: Axis::X,
        ..scene(
            "far_up",
            (4.5, 66.0, 4.5),
            alive((4.5, 74.0, 4.5)),
            (4.5, 74.0, 4.5),
            9,
        )
    },
    Scene {
        axis: Axis::X,
        ..scene(
            "far_down",
            (4.5, 72.0, 4.5),
            alive((4.5, 64.0, 4.5)),
            (4.5, 64.0, 4.5),
            9,
        )
    },
    scene(
        "far_south",
        (4.5, 66.0, 4.5),
        alive((4.5, 66.0, 14.5)),
        (4.5, 66.0, 14.5),
        9,
    ),
    scene(
        "far_west",
        (14.5, 66.0, 14.5),
        alive((4.5, 66.0, 14.5)),
        (4.5, 66.0, 14.5),
        9,
    ),
    scene(
        "far_north",
        (4.5, 66.0, 14.5),
        alive((4.5, 66.0, 4.5)),
        (4.5, 66.0, 4.5),
        9,
    ),
    Scene {
        axis: Axis::X,
        ..scene(
            "avoid_x",
            (4.5, 66.0, 4.5),
            alive((14.5, 70.0, 4.5)),
            (14.5, 70.0, 4.5),
            9,
        )
    },
    Scene {
        axis: Axis::Z,
        ..scene(
            "avoid_z",
            (4.5, 66.0, 4.5),
            alive((14.5, 66.0, 14.5)),
            (14.5, 66.0, 14.5),
            9,
        )
    },
    Scene {
        random: true,
        stones: &[(5, 66, 4), (5, 67, 4), (5, 65, 4), (5, 66, 3), (5, 66, 5)],
        ..scene(
            "wall",
            (4.5, 66.0, 4.5),
            alive((9.5, 66.0, 4.5)),
            (9.5, 66.0, 4.5),
            9,
        )
    },
    Scene {
        stones: &[(6, 66, 4), (6, 67, 4), (6, 65, 4), (6, 66, 3), (6, 66, 5)],
        ..scene(
            "hit_wall",
            (4.5, 66.0, 4.5),
            Aim::Nobody,
            (9.5, 66.0, 4.5),
            40,
        )
    },
    Scene {
        random: true,
        ..scene(
            "wander",
            (4.5, 66.0, 4.5),
            alive((14.5, 70.0, 14.5)),
            (14.5, 70.0, 14.5),
            60,
        )
    },
    Scene {
        random: true,
        ..scene(
            "age",
            (4.5, 150.0, 4.5),
            alive((4.5, 300.0, 4.5)),
            (4.5, 300.0, 4.5),
            160,
        )
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

fn spawn_zombie(area: &Area<'_>, slot: usize, at: Vec3, health: f32) -> Arc<LivingEntity> {
    let entity = Entity::from_uuid_with_id(
        80_000 + slot as i32,
        Uuid::from_u128(0x5348_554c_0000 + slot as u128),
        Arc::clone(area.world),
        area.point(at),
        &EntityType::ZOMBIE,
    );
    let living = Arc::new(LivingEntity::new(entity));
    living.health.store(health);
    let base: Arc<dyn EntityBase> = living.clone();
    area.world.spawn_entity_non_save(base);
    living
}

fn effects_text(living: &LivingEntity) -> String {
    let effects = living
        .active_effects
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut effects: Vec<_> = effects
        .values()
        .map(|e| format!("{}:{}:{}", e.effect_type.id, e.duration, e.amplifier))
        .collect();
    effects.sort();
    effects.join(",")
}

fn state_text(
    bullet: &ShulkerBulletEntity,
    target: Option<&Arc<LivingEntity>>,
    world: &World,
    random: bool,
) -> String {
    let entity = bullet.get_entity();
    let mut text = String::new();
    if !random {
        let pos = entity.pos.load();
        let vel = entity.velocity.load();
        let _ = write!(
            text,
            "pos {:.4},{:.4},{:.4} vel {:.4},{:.4},{:.4} ",
            pos.x, pos.y, pos.z, vel.x, vel.y, vel.z
        );
    }
    let _ = write!(text, "removed {}", entity.is_removed());
    if let Some(target) = target {
        let _ = write!(
            text,
            " health {:.2} effects [{}]",
            target.health.load(),
            effects_text(target)
        );
    }
    let _ = write!(text, " entities {}", world.entities.load().len());
    text
}

fn fly(
    server: &Arc<Server>,
    world: &Arc<World>,
    scene: &Scene,
    bullet: &Arc<ShulkerBulletEntity>,
    target: Option<&Arc<LivingEntity>>,
    tap: &mut PacketTap,
    report: &mut String,
) -> TestResult {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut gone = None;
    for tick in 1..=scene.ticks {
        if !bullet.get_entity().is_removed() {
            EntityBase::tick(bullet.as_ref(), bullet.as_ref(), server);
        } else if gone.is_none() {
            gone = Some(tick);
        }
        let mut text = state_text(bullet, target, world, scene.random);
        if !scene.random {
            let sent: Vec<String> = tap
                .drain()
                .iter()
                .map(|p| format!("{}:{}", p.first().copied().unwrap_or(0), p.len()))
                .collect();
            let _ = write!(text, " sent [{}]", sent.join(" "));
        }
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
        if !scene.random && (tick % 10 == 0 || tick == scene.ticks) {
            let _ = writeln!(report, "{} ticks to {tick}: {hash:016x}", scene.name);
        }
    }
    let _ = writeln!(
        report,
        "{} end: {}{}",
        scene.name,
        state_text(bullet, target, world, true),
        if scene.random {
            String::new()
        } else {
            format!(", removed after tick {gone:?}")
        }
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
    for (x, y, z) in scene.stones {
        area.put(&Block::STONE, *x, *y, *z);
    }

    let who = Identity {
        name: "ShulkerWatch",
        uuid: Uuid::from_u128(0x5368_756c_6b00_0000 + index as u128),
        entity_id: 60_000 + index as i32,
    };
    let (watcher, mut tap) = test_support::java_player_as(world, GameMode::Survival, &who)
        .ok_or("the watcher's client no longer owned its outgoing queue")?;
    watcher.get_entity().set_pos(area.point((0.5, 64.0, 23.5)));
    watcher.watched_section.store(Cylindrical::new(
        Vector2::new((area.x + 12) >> 4, (area.z + 12) >> 4),
        NonZero::<u8>::MIN.saturating_add(3),
    ));
    world.add_player(&watcher)?;

    let owner = spawn_zombie(&area, 0, scene.owner, 20.0);
    let target = match scene.aim {
        Aim::Mob(target) => {
            let zombie = spawn_zombie(&area, 1, target.at, target.health);
            if target.health <= 0.0 {
                zombie.get_entity().remove();
            }
            Some(zombie)
        }
        Aim::Nobody | Aim::Missing => None,
    };
    let target_id = match (&target, scene.aim) {
        (Some(target), _) => target.get_entity().entity_id,
        (None, Aim::Missing) => 99_999,
        (None, _) => -1,
    };
    let bullet = Arc::new(ShulkerBulletEntity::new(
        owner.get_entity(),
        target_id,
        area.point(scene.told),
        scene.axis,
    ));
    let base: Arc<dyn EntityBase> = bullet.clone();
    world.spawn_entity_non_save(base);
    drop(tap.drain());

    fly(
        server,
        world,
        scene,
        &bullet,
        target.as_ref(),
        &mut tap,
        report,
    )?;

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
async fn shulker_tick() -> TestResult {
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
    script_golden::bless_or_compare("shulker_tick.txt", &report)
}
