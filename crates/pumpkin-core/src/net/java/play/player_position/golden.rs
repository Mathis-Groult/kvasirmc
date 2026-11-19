//! A record of what `JavaClient::handle_position` and `JavaClient::handle_position_rotation` do,
//! made before they were split (`tests/golden/player_position.txt`; rewrite it on purpose with
//! `KVASIR_BLESS=1`).
//!
//! Each scene puts a fresh survival user (fixed identity, view distance 2, already watching its
//! chunk) on a scene position with a watcher three blocks away, sets the scene's state, then hands
//! the user's client the scene's movement packets in order. One line per scene: the user's
//! position, rotation, ground and elytra flags, fall distance, health, exhaustion, non-zero
//! statistics, pending teleport, watched chunk, whether the packets counted as activity and
//! whether the connection was closed (kicked), then the packets the user and the watcher were
//! sent (length and FNV-1a of the bytes; packets carrying a random seed by length and id only).
//! Not covered: riding (the handlers return at once for a passenger) and plugins cancelling the
//! move (none is loaded).

use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::java::server::play::{
    SConfirmTeleport, SPlayerPosition, SPlayerPositionRotation,
};
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use uuid::Uuid;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::EntityBase;
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::player::Player;
use crate::net::ClientPlatform;
use crate::net::java::PacketTap;
use crate::server::Server;
use crate::world::World;

const USER: Identity = Identity {
    name: "Mover",
    uuid: Uuid::from_u128(0x0000_00fc),
    entity_id: 9_801,
};

const WATCHER: Identity = Identity {
    name: "Onlooker",
    uuid: Uuid::from_u128(0x0000_00fd),
    entity_id: 9_802,
};

const GROUND: u8 = 1;
const AIR: u8 = 0;

/// One packet handed to the user's client.
#[derive(Clone, Copy)]
enum Step {
    /// A position packet moving by the offset from the user's position, with collision flags.
    Move(f64, f64, f64, u8),
    /// A position-rotation packet moving by the offset, with yaw, pitch and collision flags.
    Turn(f64, f64, f64, f32, f32, u8),
    /// A position packet naming this absolute position (on the ground).
    MoveTo(f64, f64, f64),
    /// A position-rotation packet naming this absolute position, yaw and pitch (on the ground).
    TurnTo(f64, f64, f64, f32, f32),
    /// A teleport confirmation of the last teleport id the server sent.
    Confirm,
}

#[derive(Clone, Copy)]
struct Scene {
    name: &'static str,
    steps: &'static [Step],
    gamemode: GameMode,
    /// The client has not reported that it loaded the world.
    unloaded: bool,
    /// A teleport is awaiting confirmation.
    awaiting: bool,
    /// The user's movement is locked.
    locked: bool,
    /// The user has the flying ability on.
    flying: bool,
    /// The user has no health left.
    dead: bool,
    /// The user is gliding with an elytra.
    gliding: bool,
    sprinting: bool,
    sneaking: bool,
    /// The user starts in the air (not on the ground).
    airborne: bool,
}

const SCENE: Scene = Scene {
    name: "",
    steps: &[],
    gamemode: GameMode::Survival,
    unloaded: false,
    awaiting: false,
    locked: false,
    flying: false,
    dead: false,
    gliding: false,
    sprinting: false,
    sneaking: false,
    airborne: false,
};

const fn scene(name: &'static str, steps: &'static [Step]) -> Scene {
    Scene {
        name,
        steps,
        ..SCENE
    }
}

fn scenes() -> Vec<Scene> {
    let mut scenes = guard_scenes();
    scenes.extend(move_scenes());
    scenes.extend(fall_scenes());
    scenes
}

fn guard_scenes() -> Vec<Scene> {
    vec![
        Scene {
            unloaded: true,
            ..scene("client not loaded", &[Step::Move(1.0, 0.0, 0.0, GROUND)])
        },
        Scene {
            unloaded: true,
            ..scene(
                "client not loaded, turn",
                &[Step::Turn(1.0, 0.0, 0.0, 90.0, 0.0, GROUND)],
            )
        },
        Scene {
            awaiting: true,
            ..scene("awaiting a teleport", &[Step::Move(1.0, 0.0, 0.0, GROUND)])
        },
        Scene {
            awaiting: true,
            ..scene(
                "awaiting a teleport, turn",
                &[Step::Turn(1.0, 0.0, 0.0, 90.0, 0.0, GROUND)],
            )
        },
        Scene {
            locked: true,
            ..scene("movement locked", &[Step::Move(1.0, 0.0, 0.0, GROUND)])
        },
        Scene {
            locked: true,
            ..scene(
                "movement locked, turn",
                &[Step::Turn(1.0, 0.0, 0.0, 90.0, 20.0, GROUND)],
            )
        },
        Scene {
            locked: true,
            ..scene(
                "movement locked, then confirmed and moved",
                &[
                    Step::Move(1.0, 0.0, 0.0, GROUND),
                    Step::Confirm,
                    Step::Move(1.0, 0.0, 0.0, GROUND),
                ],
            )
        },
        scene("NaN x", &[Step::MoveTo(f64::NAN, 64.0, 0.5)]),
        scene("NaN z", &[Step::MoveTo(0.5, 64.0, f64::NAN)]),
        scene("NaN y", &[Step::MoveTo(0.5, f64::NAN, 0.5)]),
        scene("infinite x", &[Step::MoveTo(f64::INFINITY, 64.0, 0.5)]),
        scene("x beyond the clamp", &[Step::MoveTo(5.0E7, 64.0, 0.5)]),
        scene("z below the clamp", &[Step::MoveTo(0.5, 64.0, -5.0E7)]),
        scene("y beyond the clamp", &[Step::MoveTo(0.5, 3.0E7, 0.5)]),
        scene("y below the clamp", &[Step::MoveTo(0.5, -3.0E7, 0.5)]),
        scene(
            "turn, NaN x",
            &[Step::TurnTo(f64::NAN, 64.0, 0.5, 0.0, 0.0)],
        ),
        scene(
            "turn, infinite y",
            &[Step::TurnTo(0.5, f64::INFINITY, 0.5, 0.0, 0.0)],
        ),
        scene(
            "turn, infinite z",
            &[Step::TurnTo(0.5, 64.0, f64::NEG_INFINITY, 0.0, 0.0)],
        ),
        scene(
            "turn, NaN yaw",
            &[Step::TurnTo(0.5, 64.0, 0.5, f32::NAN, 0.0)],
        ),
        scene(
            "turn, infinite pitch",
            &[Step::TurnTo(0.5, 64.0, 0.5, 0.0, f32::INFINITY)],
        ),
        scene(
            "turn, x beyond the clamp",
            &[Step::TurnTo(5.0E7, 64.0, 0.5, 0.0, 0.0)],
        ),
        scene(
            "turn, y below the clamp",
            &[Step::TurnTo(0.5, -3.0E7, 0.5, 0.0, 0.0)],
        ),
    ]
}

fn move_scenes() -> Vec<Scene> {
    vec![
        scene("walk one block", &[Step::Move(1.0, 0.0, 0.0, GROUND)]),
        scene(
            "walk one block, turned",
            &[Step::Turn(1.0, 0.0, 0.0, 45.0, 10.0, GROUND)],
        ),
        scene(
            "walk one block back",
            &[Step::Move(-1.0, 0.0, -0.5, GROUND)],
        ),
        scene("stand still", &[Step::Move(0.0, 0.0, 0.0, GROUND)]),
        scene(
            "stand still, turned",
            &[Step::Turn(0.0, 0.0, 0.0, -30.0, -45.0, GROUND)],
        ),
        scene("creep a millimetre", &[Step::Move(0.001, 0.0, 0.0, GROUND)]),
        scene(
            "creep four millimetres",
            &[Step::Move(0.004, 0.0, 0.0, GROUND)],
        ),
        scene(
            "creep four millimetres, turned",
            &[Step::Turn(0.0, 0.0, 0.004, 0.0, 0.0, GROUND)],
        ),
        scene("jump", &[Step::Move(0.0, 0.42, 0.0, AIR)]),
        scene(
            "jump, turned",
            &[Step::Turn(0.0, 0.42, 0.0, 10.0, 0.0, AIR)],
        ),
        scene(
            "step up on the ground",
            &[Step::Move(0.3, 0.5, 0.0, GROUND)],
        ),
        scene(
            "step up on the ground, turned",
            &[Step::Turn(0.3, 0.5, 0.0, 0.0, 0.0, GROUND)],
        ),
        Scene {
            sprinting: true,
            ..scene("sprint jump", &[Step::Move(0.3, 0.42, 0.0, AIR)])
        },
        Scene {
            sprinting: true,
            ..scene("sprint", &[Step::Move(1.3, 0.0, 0.0, GROUND)])
        },
        Scene {
            sneaking: true,
            ..scene("sneak", &[Step::Move(0.3, 0.0, 0.0, GROUND)])
        },
        Scene {
            airborne: true,
            ..scene("rise in the air", &[Step::Move(0.0, 0.3, 0.0, AIR)])
        },
        scene("seven blocks", &[Step::Move(7.0, 0.0, 0.0, GROUND)]),
        scene("nine blocks", &[Step::Move(9.0, 0.0, 0.0, GROUND)]),
        scene(
            "nine blocks, turned",
            &[Step::Turn(0.0, 0.0, 9.0, 0.0, 0.0, GROUND)],
        ),
        scene("eight blocks up", &[Step::Move(0.0, 8.0, 0.0, GROUND)]),
        scene(
            "into the next chunk",
            &[
                Step::Move(0.0, 0.0, 7.5, GROUND),
                Step::Move(0.0, 0.0, 7.5, GROUND),
            ],
        ),
        scene(
            "yaw 370 pitch 100",
            &[Step::Turn(0.0, 0.0, 0.0, 370.0, 100.0, GROUND)],
        ),
        scene(
            "yaw -190 pitch -95",
            &[Step::Turn(0.0, 0.0, 0.0, -190.0, -95.0, GROUND)],
        ),
        scene("yaw 725", &[Step::Turn(0.0, 0.0, 0.0, 725.0, 0.0, GROUND)]),
        scene(
            "yaw 179.9",
            &[Step::Turn(0.0, 0.0, 0.0, 179.9, 0.0, GROUND)],
        ),
        Scene {
            gamemode: GameMode::Spectator,
            ..scene("spectator walk", &[Step::Move(1.0, 0.0, 0.0, AIR)])
        },
        Scene {
            gamemode: GameMode::Creative,
            ..scene("creative walk", &[Step::Move(1.0, 0.0, 0.0, GROUND)])
        },
        Scene {
            flying: true,
            airborne: true,
            ..scene("fly", &[Step::Move(1.0, 0.5, 0.0, AIR)])
        },
    ]
}

/// A fall of `height` blocks in two packets, the second landing.
const FALL_TEN: &[Step] = &[
    Step::Move(0.0, -5.0, 0.0, AIR),
    Step::Move(0.0, -5.0, 0.0, GROUND),
];

fn fall_scenes() -> Vec<Scene> {
    vec![
        scene("fall ten blocks", FALL_TEN),
        scene(
            "fall ten blocks, turned",
            &[
                Step::Turn(0.0, -5.0, 0.0, 0.0, 0.0, AIR),
                Step::Turn(0.0, -5.0, 0.0, 0.0, 0.0, GROUND),
            ],
        ),
        scene(
            "fall two blocks",
            &[
                Step::Move(0.0, -1.0, 0.0, AIR),
                Step::Move(0.0, -1.0, 0.0, GROUND),
            ],
        ),
        scene(
            "fall four blocks",
            &[
                Step::Move(0.0, -2.0, 0.0, AIR),
                Step::Move(0.0, -2.0, 0.0, GROUND),
            ],
        ),
        scene(
            "fall ten blocks, not landed",
            &[
                Step::Move(0.0, -5.0, 0.0, AIR),
                Step::Move(0.0, -5.0, 0.0, AIR),
            ],
        ),
        Scene {
            gamemode: GameMode::Creative,
            ..scene("creative fall ten blocks", FALL_TEN)
        },
        Scene {
            flying: true,
            ..scene("flying fall ten blocks", FALL_TEN)
        },
        Scene {
            dead: true,
            ..scene("dead fall ten blocks", FALL_TEN)
        },
        Scene {
            gliding: true,
            airborne: true,
            ..scene("glide and land", &[Step::Move(1.0, -1.0, 0.0, GROUND)])
        },
        Scene {
            gliding: true,
            airborne: true,
            ..scene("glide", &[Step::Move(1.0, -1.0, 0.0, AIR)])
        },
        Scene {
            gliding: true,
            airborne: true,
            ..scene(
                "glide and land, turned",
                &[Step::Turn(1.0, -1.0, 0.0, 0.0, 0.0, GROUND)],
            )
        },
        Scene {
            airborne: true,
            ..scene("land from the air", &[Step::Move(0.0, -0.5, 0.0, GROUND)])
        },
    ]
}

/// Scenes sit 32 chunks apart.
const fn position_of(index: usize) -> BlockPos {
    BlockPos::new(index as i32 * 512 + 200, 64, 8)
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Packet ids that carry a random seed (sounds) or the server's clock.
const RANDOM_PACKETS: [u8; 3] = [1, 118, 119];

fn packets(tap: &mut PacketTap) -> String {
    tap.drain()
        .iter()
        .map(|p| match p.first() {
            Some(id) if RANDOM_PACKETS.contains(id) => format!("{}:id{id}", p.len()),
            _ => format!("{}:{:08x}", p.len(), fnv(p) as u32),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn user_text(user: &Player) -> String {
    let entity = user.get_entity();
    let pos = entity.pos.load();
    let mut stats: Vec<String> = user
        .stats
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .stats
        .iter()
        .filter(|(_, value)| **value != 0)
        .map(|((category, stat), value)| format!("{category}:{stat}={value}"))
        .collect();
    stats.sort();
    let awaiting = user
        .awaiting_teleport
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .map(|(id, at)| format!("{} {:.3},{:.3},{:.3}", id.0, at.x, at.y, at.z));
    let watched = user.watched_section.load();
    format!(
        "pos {:.4},{:.4},{:.4} yaw {:.3} pitch {:.3} ground {} gliding {} fall {:.3} health {:.2} exhaustion {:.3} stats {stats:?} awaiting {awaiting:?} watched {},{} r{}",
        pos.x,
        pos.y,
        pos.z,
        entity.yaw.load(),
        entity.pitch.load(),
        entity.on_ground.load(Ordering::Relaxed),
        entity.is_fall_flying(),
        user.living_entity.fall_distance.load(),
        user.living_entity.health.load(),
        user.hunger_manager.exhaustion.load(),
        watched.center.x,
        watched.center.y,
        watched.view_distance,
    )
}

fn place(player: &Player, at: Vector3<f64>) {
    player.get_entity().set_pos(at);
    let mut config = (**player.config.load()).clone();
    config.view_distance = NonZero::<u8>::MIN.saturating_add(1);
    player.config.store(Arc::new(config));
    player.watched_section.store(Cylindrical::new(
        player.get_entity().chunk_pos.load(),
        NonZero::<u8>::MIN.saturating_add(1),
    ));
}

/// Sets the scene's state on the user.
fn prepare(user: &Player, scene: &Scene) {
    let entity = user.get_entity();
    user.client_loaded.store(!scene.unloaded, Ordering::Relaxed);
    entity.on_ground.store(!scene.airborne, Ordering::Relaxed);
    if scene.awaiting {
        *user
            .awaiting_teleport
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some((VarInt(77), entity.pos.load()));
    }
    user.is_movement_locked
        .store(scene.locked, Ordering::Relaxed);
    if scene.flying {
        user.abilities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .flying = true;
    }
    if scene.dead {
        user.living_entity.health.store(0.0);
    }
    if scene.gliding {
        entity.set_fall_flying(true);
    }
    if scene.sprinting {
        entity.set_sprinting(true);
    }
    if scene.sneaking {
        entity.set_sneaking(true);
    }
    if let Some(long_ago) = Instant::now().checked_sub(Duration::from_secs(600)) {
        user.last_action_time.store(long_ago);
    }
}

fn step(server: &Arc<Server>, user: &Arc<Player>, step: Step) -> Result<(), String> {
    let ClientPlatform::Java(client) = user.client.as_ref() else {
        return Err("not a Java player".into());
    };
    let pos = user.get_entity().pos.load();
    match step {
        Step::Move(dx, dy, dz, collision) => client.handle_position(
            user,
            server,
            &SPlayerPosition {
                position: pos.add(&Vector3::new(dx, dy, dz)),
                collision,
            },
        ),
        Step::Turn(dx, dy, dz, yaw, pitch, collision) => client.handle_position_rotation(
            user,
            server,
            &SPlayerPositionRotation {
                position: pos.add(&Vector3::new(dx, dy, dz)),
                yaw,
                pitch,
                collision,
            },
        ),
        Step::MoveTo(x, y, z) => client.handle_position(
            user,
            server,
            &SPlayerPosition {
                position: Vector3::new(x, y, z),
                collision: GROUND,
            },
        ),
        Step::TurnTo(x, y, z, yaw, pitch) => client.handle_position_rotation(
            user,
            server,
            &SPlayerPositionRotation {
                position: Vector3::new(x, y, z),
                yaw,
                pitch,
                collision: GROUND,
            },
        ),
        Step::Confirm => client.handle_confirm_teleport(
            user,
            &SConfirmTeleport {
                teleport_id: VarInt(user.teleport_id_count.load(Ordering::Relaxed)),
                position: pos,
                yaw: 0.0,
                pitch: 0.0,
            },
        ),
    }
    Ok(())
}

fn run(
    server: &Arc<Server>,
    world: &Arc<World>,
    index: usize,
    scene: &Scene,
) -> Result<String, String> {
    let pos = position_of(index);
    let base = Vector3::new(f64::from(pos.0.x) + 0.5, 64.0, f64::from(pos.0.z) + 0.5);
    let (user, mut user_tap) =
        test_support::java_player_as(world, scene.gamemode, &USER).ok_or("no Java packet tap")?;
    let (watcher, mut watcher_tap) =
        test_support::java_player_as(world, GameMode::Survival, &WATCHER)
            .ok_or("no Java packet tap")?;
    place(&user, base);
    place(&watcher, base.add(&Vector3::new(3.0, 0.0, 0.0)));
    watcher.client_loaded.store(true, Ordering::Relaxed);
    world.add_player(&user)?;
    world.add_player(&watcher)?;
    prepare(&user, scene);
    drop(packets(&mut user_tap));
    drop(packets(&mut watcher_tap));
    for each in scene.steps {
        step(server, &user, *each)?;
    }
    let kicked = match user.client.as_ref() {
        ClientPlatform::Java(client) => client.is_closed(),
        ClientPlatform::Bedrock(_) => false,
    };
    Ok(format!(
        "{}: {} acted {} kicked {kicked} | sent [{}] | watcher [{}]\n",
        scene.name,
        user_text(&user),
        user.last_action_time.load().elapsed() < Duration::from_secs(300),
        packets(&mut user_tap),
        packets(&mut watcher_tap),
    ))
}

fn load_chunks(world: &World, count: usize) {
    for index in 0..count {
        let chunk = position_of(index).chunk_position();
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
}

#[tokio::test(flavor = "multi_thread")]
async fn player_position_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let scenes = scenes();
    load_chunks(&world, scenes.len());
    let mut out = String::new();
    for (index, scene) in scenes.iter().enumerate() {
        out.push_str(&run(&server, &world, index, scene)?);
        for player in world.players.load().iter() {
            world.remove_player(player, false).await;
        }
    }
    server.stop();
    script_golden::bless_or_compare("player_position.txt", &out)
}
