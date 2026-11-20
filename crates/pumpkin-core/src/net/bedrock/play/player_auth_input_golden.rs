//! A record of what `BedrockClient::handle_player_auth_input` does, made before the function was
//! split (`tests/golden/bedrock_player_auth_input.txt`; rewrite it on purpose with
//! `KVASIR_BLESS=1`).
//!
//! Each scene puts a fresh Bedrock player (fixed identity, view distance 2, already watching its
//! chunk, standing on the ground) on a scene position with a Java watcher three blocks east and a
//! Bedrock watcher three blocks west, sets the scene's state, then hands the player's client the
//! scene's input packets in order (the position is named relative to where the player stands, as
//! the eye position the client sends). One line per scene: the player's position, rotation, ground
//! flag, pose and sneak, sprint, swim and glide flags, flight, fall distance, health, exhaustion,
//! non-zero statistics and watched chunk, then the packets the player and both watchers were sent
//! (length and FNV-1a of the bytes; inventory slot updates show only as `slot`).
//! Not covered: riding (a passenger's ground flag and the boat paddles) and plugins cancelling
//! the flight toggle (none is loaded).

use std::fmt::Write as _;
use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use pumpkin_data::data_component_impl::EquipmentSlot;
use pumpkin_data::entity::{EntityPose, EntityType};
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::{Block, Enchantment};
use pumpkin_protocol::bedrock::server::player_action::PlayerActionType;
use pumpkin_protocol::bedrock::server::player_auth_input::{
    InputData, PlayerBlockAction, SPlayerAuthInput,
};
use pumpkin_protocol::codec::bitset::Bitset;
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::codec::var_uint::VarUInt;
use pumpkin_protocol::codec::var_ulong::VarULong;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::{GameMode, Hand};
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::player::Player;
use crate::entity::vehicle::boat::BoatEntity;
use crate::entity::{Entity, EntityBase};
use crate::net::ClientPlatform;
use crate::server::Server;
use crate::world::World;

const MOVER: Identity = Identity {
    name: "Mover",
    uuid: Uuid::from_u128(0x0000_0101),
    entity_id: 9_901,
};
const JAVA_WATCHER: Identity = Identity {
    name: "JavaOnlooker",
    uuid: Uuid::from_u128(0x0000_0102),
    entity_id: 9_902,
};
const BEDROCK_WATCHER: Identity = Identity {
    name: "BedrockOnlooker",
    uuid: Uuid::from_u128(0x0000_0103),
    entity_id: 9_903,
};

use InputData as I;
use PlayerActionType as A;

/// One input packet handed to the player's client.
#[derive(Clone, Copy)]
struct Input {
    /// Feet offset from where the player stands now.
    dx: f64,
    dy: f64,
    dz: f64,
    yaw: f32,
    pitch: f32,
    /// The packet's own movement delta (y only).
    delta_y: f32,
    flags: &'static [InputData],
    /// Block actions sent in the same packet.
    actions: &'static [PlayerActionType],
    /// The packet carries a block action list (possibly empty).
    has_actions: bool,
}

const STAY: Input = Input {
    dx: 0.0,
    dy: 0.0,
    dz: 0.0,
    yaw: 0.0,
    pitch: 0.0,
    delta_y: 0.0,
    flags: &[],
    actions: &[],
    has_actions: false,
};

/// What lies under the player's feet (y 63) in the area the scene walks over.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Floor {
    Nothing,
    Stone,
    Water,
}

#[derive(Clone, Copy)]
struct Scene {
    name: &'static str,
    steps: &'static [Input],
    gamemode: GameMode,
    unloaded: bool,
    dead: bool,
    dead_flag_only: bool,
    zero_health_only: bool,
    /// The player is in the air to begin with.
    airborne: bool,
    flying: bool,
    gliding: bool,
    sprinting: bool,
    sneaking: bool,
    swimming: bool,
    /// Wears an elytra on the chest.
    elytra: bool,
    /// Appended: health to start with.
    health: Option<f32>,
    /// Appended: the pose to start in.
    pose: Option<EntityPose>,
    /// Appended: the floor under the walk (its block under the end position is recorded).
    floor: Floor,
    /// Appended: wears Frost Walker boots.
    frost: bool,
    /// Appended: holds a shield in the off hand (blocking is recorded).
    shield: bool,
    /// Appended: is blocking with the shield already.
    blocking: bool,
    /// Appended: sits in a boat.
    boat: bool,
}

const SCENE: Scene = Scene {
    name: "",
    steps: &[],
    gamemode: GameMode::Survival,
    unloaded: false,
    dead: false,
    dead_flag_only: false,
    zero_health_only: false,
    airborne: false,
    flying: false,
    gliding: false,
    sprinting: false,
    sneaking: false,
    swimming: false,
    elytra: false,
    health: None,
    pose: None,
    floor: Floor::Nothing,
    frost: false,
    shield: false,
    blocking: false,
    boat: false,
};

const fn scene(name: &'static str, steps: &'static [Input]) -> Scene {
    Scene {
        name,
        steps,
        ..SCENE
    }
}

/// A scene of one packet carrying only these input flags.
macro_rules! flagged {
    ($name:expr, $flags:expr $(,)?) => {
        scene(
            $name,
            &[Input {
                flags: $flags,
                ..STAY
            }],
        )
    };
}

fn scenes() -> Vec<Scene> {
    let mut all = guard_scenes();
    all.extend(move_scenes());
    all.extend(flag_scenes());
    all.extend(widened_scenes());
    all
}

fn guard_scenes() -> Vec<Scene> {
    let walk = &[Input {
        dx: 1.0,
        flags: &[I::StartSprinting],
        ..STAY
    }];
    vec![
        Scene {
            unloaded: true,
            ..scene("client not loaded", walk)
        },
        Scene {
            dead: true,
            ..scene("dead", walk)
        },
        Scene {
            dead_flag_only: true,
            ..scene("dead flag only", walk)
        },
        Scene {
            zero_health_only: true,
            ..scene("zero health only", walk)
        },
    ]
}

fn move_scenes() -> Vec<Scene> {
    vec![
        scene("no change", &[STAY]),
        scene("walk", &[Input { dx: 0.25, ..STAY }]),
        scene(
            "walk, vertical collision going down",
            &[Input {
                dx: 0.25,
                delta_y: -0.08,
                flags: &[I::VerticalCollision],
                ..STAY
            }],
        ),
        scene(
            "walk, vertical collision going up",
            &[Input {
                dx: 0.25,
                delta_y: 0.4,
                flags: &[I::VerticalCollision],
                ..STAY
            }],
        ),
        scene(
            "walk, going down without a collision",
            &[Input {
                dx: 0.25,
                delta_y: -0.08,
                ..STAY
            }],
        ),
        Scene {
            airborne: true,
            ..scene("walk in the air", &[Input { dy: -0.5, ..STAY }])
        },
        scene(
            "turn only",
            &[Input {
                yaw: 90.0,
                pitch: 10.0,
                ..STAY
            }],
        ),
        scene("yaw only", &[Input { yaw: 45.0, ..STAY }]),
        scene(
            "pitch only",
            &[Input {
                pitch: -30.0,
                ..STAY
            }],
        ),
        scene(
            "walk and turn",
            &[Input {
                dz: 0.5,
                yaw: 180.0,
                pitch: 20.0,
                ..STAY
            }],
        ),
        scene("yaw wraps", &[Input { yaw: -90.0, ..STAY }]),
        scene(
            "yaw past a full turn",
            &[Input {
                yaw: 450.0,
                pitch: 90.0,
                ..STAY
            }],
        ),
        scene("seven and a half blocks", &[Input { dx: 7.5, ..STAY }]),
        scene("eight blocks", &[Input { dx: 8.0, ..STAY }]),
        scene(
            "eight blocks and a turn",
            &[Input {
                dz: 8.0,
                yaw: 30.0,
                ..STAY
            }],
        ),
        scene("a fall of ten blocks", &[Input { dy: -10.0, ..STAY }]),
        scene(
            "two steps",
            &[
                Input { dx: 0.3, ..STAY },
                Input {
                    dx: 0.3,
                    yaw: 15.0,
                    ..STAY
                },
            ],
        ),
        Scene {
            sneaking: true,
            ..scene("walk while sneaking", &[Input { dx: 0.2, ..STAY }])
        },
        Scene {
            sprinting: true,
            ..scene("walk while sprinting", &[Input { dx: 0.5, ..STAY }])
        },
    ]
}

fn flag_scenes() -> Vec<Scene> {
    let mut all = vec![
        flagged!("start sprinting", &[I::StartSprinting]),
        Scene {
            sprinting: true,
            ..flagged!("stop sprinting", &[I::StopSprinting])
        },
        Scene {
            sprinting: true,
            ..flagged!(
                "sprint start and stop together",
                &[I::StartSprinting, I::StopSprinting],
            )
        },
        flagged!(
            "sprint start and stop together, from rest",
            &[I::StopSprinting, I::StartSprinting],
        ),
        flagged!("start sneaking", &[I::StartSneaking]),
        Scene {
            sneaking: true,
            ..flagged!("stop sneaking", &[I::StopSneaking])
        },
        Scene {
            sneaking: true,
            ..flagged!(
                "sneak start and stop together",
                &[I::StartSneaking, I::StopSneaking],
            )
        },
        flagged!("start crawling", &[I::StartCrawling]),
        flagged!("stop crawling", &[I::StopCrawling]),
        flagged!(
            "crawl start and stop together",
            &[I::StartCrawling, I::StopCrawling],
        ),
        flagged!("start swimming", &[I::StartSwimming]),
        Scene {
            swimming: true,
            ..flagged!("stop swimming", &[I::StopSwimming])
        },
        flagged!("start spinning", &[I::StartSpinAttack]),
        flagged!("stop spinning", &[I::StopSpinAttack]),
        flagged!("start gliding without an elytra", &[I::StartGliding]),
        Scene {
            elytra: true,
            airborne: true,
            ..flagged!("start gliding in the air", &[I::StartGliding])
        },
        Scene {
            elytra: true,
            ..flagged!("start gliding on the ground", &[I::StartGliding])
        },
        Scene {
            gliding: true,
            ..flagged!("stop gliding", &[I::StopGliding])
        },
        flagged!("start flying in survival", &[I::StartFlying]),
        Scene {
            gamemode: GameMode::Creative,
            ..flagged!("start flying in creative", &[I::StartFlying])
        },
        Scene {
            gamemode: GameMode::Creative,
            flying: true,
            ..flagged!("start flying while flying", &[I::StartFlying])
        },
        Scene {
            gamemode: GameMode::Creative,
            flying: true,
            ..flagged!("stop flying", &[I::StopFlying])
        },
        Scene {
            gamemode: GameMode::Creative,
            ..flagged!("stop flying while walking", &[I::StopFlying])
        },
        Scene {
            gamemode: GameMode::Creative,
            ..flagged!(
                "flight start and stop together",
                &[I::StartFlying, I::StopFlying],
            )
        },
        flagged!(
            "paddling without a boat",
            &[I::PaddlingLeft, I::PaddlingRight],
        ),
        flagged!("an input flag the handler ignores", &[I::Jumping, I::Up]),
    ];
    all.extend(combined_scenes());
    all
}

fn combined_scenes() -> Vec<Scene> {
    vec![
        scene(
            "sneak and walk in one packet",
            &[Input {
                dx: 0.2,
                flags: &[I::StartSneaking],
                ..STAY
            }],
        ),
        scene(
            "sprint, sneak and swim in one packet",
            &[Input {
                flags: &[I::StartSprinting, I::StartSneaking, I::StartSwimming],
                ..STAY
            }],
        ),
        scene(
            "an empty block action list",
            &[Input {
                has_actions: true,
                ..STAY
            }],
        ),
        scene(
            "block actions: sneak then sprint",
            &[Input {
                has_actions: true,
                actions: &[A::StartSneaking, A::StartSprinting],
                ..STAY
            }],
        ),
        scene(
            "block actions after a walk",
            &[Input {
                dx: 0.4,
                has_actions: true,
                actions: &[A::StartSprinting],
                ..STAY
            }],
        ),
        scene(
            "block action stops what the flag started",
            &[Input {
                flags: &[I::StartSprinting],
                has_actions: true,
                actions: &[A::StopSprinting],
                ..STAY
            }],
        ),
    ]
}

const GROUND: Input = Input {
    flags: &[I::VerticalCollision],
    delta_y: -0.08,
    ..STAY
};

/// Appended after the mutation run of the first scenes.
fn widened_scenes() -> Vec<Scene> {
    let mut all = vec![
        Scene {
            health: Some(0.5),
            ..scene("nearly dead", &[Input { dx: 0.5, ..STAY }])
        },
        scene(
            "vertical collision without vertical movement",
            &[Input {
                dx: 0.25,
                flags: &[I::VerticalCollision],
                ..STAY
            }],
        ),
        Scene {
            sprinting: true,
            ..scene("sprinting on the ground", &[Input { dx: 1.0, ..GROUND }])
        },
        Scene {
            floor: Floor::Stone,
            ..scene("a small step on stone", &[Input { dx: 0.25, ..GROUND }])
        },
        Scene {
            floor: Floor::Stone,
            ..scene("three blocks on stone", &[Input { dx: 3.0, ..GROUND }])
        },
        Scene {
            floor: Floor::Water,
            frost: true,
            ..scene("frost walker over water", &[Input { dx: 1.0, ..GROUND }])
        },
        Scene {
            floor: Floor::Water,
            ..scene("over water without boots", &[Input { dx: 1.0, ..GROUND }])
        },
        Scene {
            floor: Floor::Water,
            frost: true,
            ..scene("frost walker in the air", &[Input { dx: 1.0, ..STAY }])
        },
    ];
    all.extend(posture_scenes());
    all.extend(boat_scenes());
    all
}

fn posture_scenes() -> Vec<Scene> {
    vec![
        Scene {
            shield: true,
            ..flagged!("sneak with a shield", &[I::StartSneaking])
        },
        Scene {
            shield: true,
            blocking: true,
            sneaking: true,
            ..flagged!("stop sneaking while blocking", &[I::StopSneaking])
        },
        Scene {
            shield: true,
            blocking: true,
            ..flagged!("start sneaking while blocking", &[I::StartSneaking])
        },
        Scene {
            shield: true,
            blocking: true,
            ..flagged!("start sneaking, shield gone", &[I::StartSneaking])
        },
        Scene {
            pose: Some(EntityPose::Swimming),
            ..flagged!("stop crawling from the crawl pose", &[I::StopCrawling])
        },
        Scene {
            pose: Some(EntityPose::Swimming),
            swimming: true,
            ..flagged!("stop swimming from the swim pose", &[I::StopSwimming])
        },
        Scene {
            pose: Some(EntityPose::SpinAttack),
            ..flagged!("stop spinning from the spin pose", &[I::StopSpinAttack])
        },
    ]
}

fn boat_scenes() -> Vec<Scene> {
    vec![
        Scene {
            boat: true,
            ..flagged!("riding, paddling left", &[I::PaddlingLeft])
        },
        Scene {
            boat: true,
            ..flagged!("riding, paddling right", &[I::PaddlingRight])
        },
        Scene {
            boat: true,
            ..flagged!(
                "riding, paddling both",
                &[I::PaddlingLeft, I::PaddlingRight]
            )
        },
        Scene {
            boat: true,
            ..flagged!("riding, not paddling", &[I::Up])
        },
        Scene {
            boat: true,
            ..scene("riding, ground flag", &[Input { dx: 0.25, ..GROUND }])
        },
    ]
}

const fn position_of(index: usize) -> BlockPos {
    BlockPos::new(index as i32 * 512 + 200, 64, 8)
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// What a Bedrock player was sent. Inventory slot updates (game packet 50, after the 0xfe batch
/// byte and a one-byte length) show only as `slot`: they carry a stack id that depends on the
/// tests run before.
fn bedrock_sent(sent: &[bytes::Bytes]) -> String {
    sent.iter()
        .map(|p| {
            if p.get(2) == Some(&50) {
                "slot".to_string()
            } else {
                format!("{}:{:08x}", p.len(), fnv(p) as u32)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Java packet ids whose bytes are random like in vanilla: Add Entity and the two sound packets.
const RANDOM_PACKETS: [u8; 3] = [1, 118, 119];

/// A frost walker's disk changes many blocks, which Bedrock sends in the hash order of their
/// positions: those scenes record the packets sorted.
fn unordered_if(unordered: bool, sent: String) -> String {
    if !unordered {
        return sent;
    }
    let mut packets: Vec<&str> = sent.split(' ').collect();
    packets.sort_unstable();
    packets.join(" ")
}

/// The Java Update Section Blocks packet id: its entries come in the order of a hash set.
const SECTION_BLOCKS: u8 = 0x56;

/// The packet with the varlong entries of an Update Section Blocks packet sorted, so the
/// record does not depend on the hash order of the changed positions.
fn sorted_section_blocks(p: &[u8]) -> Vec<u8> {
    // id, section position (long), entry count (varint, one byte under 128), entries.
    let mut at = 1 + 8;
    let count = usize::from(p[at]);
    at += 1;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let start = at;
        while p[at] & 0x80 != 0 {
            at += 1;
        }
        at += 1;
        entries.push(p[start..at].to_vec());
    }
    entries.sort();
    let mut out = p[..10].to_vec();
    out.extend(entries.concat());
    out.extend_from_slice(&p[at..]);
    out
}

fn java_sent(sent: &[bytes::Bytes]) -> String {
    sent.iter()
        .map(|p| match p.first() {
            Some(&SECTION_BLOCKS) => {
                let sorted = sorted_section_blocks(p);
                format!("{}:{:08x}", sorted.len(), fnv(&sorted) as u32)
            }
            Some(id) if RANDOM_PACKETS.contains(id) => format!("{}:id{id}", p.len()),
            _ => format!("{}:{:08x}", p.len(), fnv(p) as u32),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn mover_text(mover: &Player) -> String {
    let entity = mover.get_entity();
    let pos = entity.pos.load();
    let mut stats: Vec<String> = mover
        .stats
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .stats
        .iter()
        .filter(|(_, value)| **value != 0)
        .map(|((category, stat), value)| format!("{category}:{stat}={value}"))
        .collect();
    stats.sort();
    let flying = mover
        .abilities
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .flying;
    let watched = mover.watched_section.load();
    format!(
        "pos {:.4},{:.4},{:.4} yaw {:.3} pitch {:.3} ground {} pose {} sneaking {} sprinting {} swimming {} gliding {} flying {flying} fall {:.3} health {:.2} exhaustion {:.3} stats {stats:?} watched {},{} r{}",
        pos.x,
        pos.y,
        pos.z,
        entity.yaw.load(),
        entity.pitch.load(),
        entity.on_ground.load(Ordering::Relaxed),
        entity.pose.load() as i32,
        entity.is_sneaking(),
        entity.is_sprinting(),
        entity.is_swimming(),
        entity.is_fall_flying(),
        mover.living_entity.fall_distance.load(),
        mover.living_entity.health.load(),
        mover.hunger_manager.exhaustion.load(),
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

/// Sets the scene's state on the mover.
fn prepare(mover: &Player, scene: &Scene) {
    let entity = mover.get_entity();
    mover
        .client_loaded
        .store(!scene.unloaded, Ordering::Relaxed);
    entity.on_ground.store(!scene.airborne, Ordering::Relaxed);
    if scene.dead || scene.dead_flag_only {
        mover.living_entity.dead.store(true, Ordering::Relaxed);
    }
    if scene.dead || scene.zero_health_only {
        mover.living_entity.health.store(0.0);
    }
    if scene.flying {
        mover
            .abilities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .flying = true;
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
    if scene.swimming {
        entity.set_swimming(true);
    }
    if let Some(health) = scene.health {
        mover.living_entity.health.store(health);
    }
    if let Some(pose) = scene.pose {
        entity.set_pose(pose);
    }
    if scene.shield {
        mover
            .inventory()
            .set_stack_in_hand(Hand::Left, ItemStack::new(1, &Item::SHIELD));
    }
    if scene.blocking {
        mover
            .living_entity
            .set_active_hand(Hand::Left, ItemStack::new(1, &Item::SHIELD), 72_000);
    }
    if scene.frost {
        let mut boots = ItemStack::new(1, &Item::DIAMOND_BOOTS);
        boots.add_enchantment(&Enchantment::FROST_WALKER, 1);
        mover
            .inventory()
            .entity_equipment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .put(&EquipmentSlot::FEET, boots);
    }
    if scene.elytra {
        mover
            .inventory()
            .entity_equipment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .put(&EquipmentSlot::CHEST, ItemStack::new(1, &Item::ELYTRA));
    }
}

/// Lays the scene's floor at y 63 under and around the walk.
fn lay_floor(world: &Arc<World>, base: Vector3<f64>, floor: Floor) {
    let state = match floor {
        Floor::Nothing => return,
        Floor::Stone => Block::STONE.default_state.id,
        Floor::Water => Block::WATER.default_state.id,
    };
    let (x, z) = (base.x.floor() as i32, base.z.floor() as i32);
    for dx in -1..=6 {
        for dz in -1..=1 {
            world.set_block_state(
                &BlockPos::new(x + dx, 63, z + dz),
                state,
                BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
            );
        }
    }
}

/// Appended scenes only: the block under the end position and whether the shield blocks.
fn extra_text(world: &World, mover: &Player, scene: &Scene) -> String {
    let mut text = String::new();
    if scene.floor != Floor::Nothing {
        let at = mover.get_entity().pos.load();
        let under = BlockPos::new(at.x.floor() as i32, 63, at.z.floor() as i32);
        let _ = write!(text, " | under {}", world.get_block(&under).name);
    }
    if scene.shield {
        let _ = write!(text, " | blocking {}", mover.living_entity.is_blocking());
    }
    text
}

fn packet_of(mover: &Player, input: &Input) -> SPlayerAuthInput {
    let entity = mover.get_entity();
    let feet = entity
        .pos
        .load()
        .add(&Vector3::new(input.dx, input.dy, input.dz));
    let mut input_data = Bitset::<66>::default();
    for flag in input.flags {
        input_data.set(*flag as usize, true);
    }
    let block_actions = input.has_actions.then(|| {
        input
            .actions
            .iter()
            .map(|action| PlayerBlockAction {
                action: *action,
                block_pos: BlockPos::ZERO,
                face: VarInt(1),
            })
            .collect()
    });
    SPlayerAuthInput {
        pitch: input.pitch,
        yaw: input.yaw,
        position: Vector3::new(
            feet.x as f32,
            feet.y as f32 + entity.entity_type.eye_height,
            feet.z as f32,
        ),
        move_vec: Vector2::new(0.0, 0.0),
        head_yaw: input.yaw,
        input_data,
        input_mode: VarUInt(0),
        play_mode: VarUInt(0),
        interaction_model: VarInt(0),
        interact_pitch: 0.0,
        interact_yaw: 0.0,
        tick: VarULong(0),
        delta: Vector3::new(0.0, input.delta_y, 0.0),
        block_actions,
        item_interaction: None,
        item_stack_request: None,
        vehicle_rotation: None,
        vehicle_unique_id: None,
        analog_move: Vector2::new(0.0, 0.0),
        camera_orientation: Vector3::new(0.0, 0.0, 0.0),
        raw_move: Vector2::new(0.0, 0.0),
    }
}

async fn run(
    server: &Arc<Server>,
    world: &Arc<World>,
    index: usize,
    scene: &Scene,
) -> Result<String, String> {
    let pos = position_of(index);
    let base = Vector3::new(f64::from(pos.0.x) + 0.5, 64.0, f64::from(pos.0.z) + 0.5);
    let (mover, mut mover_tap) =
        test_support::bedrock_player_as(world, scene.gamemode, &MOVER).await?;
    let (java, mut java_tap) =
        test_support::java_player_as(world, GameMode::Survival, &JAVA_WATCHER)
            .ok_or("no Java packet tap")?;
    let (bedrock, mut bedrock_tap) =
        test_support::bedrock_player_as(world, GameMode::Survival, &BEDROCK_WATCHER).await?;
    place(&mover, base);
    place(&java, base.add(&Vector3::new(3.0, 0.0, 0.0)));
    place(&bedrock, base.add(&Vector3::new(-3.0, 0.0, 0.0)));
    java.client_loaded.store(true, Ordering::Relaxed);
    bedrock.client_loaded.store(true, Ordering::Relaxed);
    world.add_player(&mover)?;
    world.add_player(&java)?;
    world.add_player(&bedrock)?;
    lay_floor(world, base, scene.floor);
    prepare(&mover, scene);
    if scene.boat {
        // A fixed id: the global counter would make the packet bytes depend on test order.
        let mut boat = Entity::new(world.clone(), base, &EntityType::OAK_BOAT);
        boat.entity_id = 9_904;
        let vehicle: Arc<dyn EntityBase> = Arc::new(BoatEntity::new(boat));
        vehicle
            .get_entity()
            .add_passenger_silently(Arc::clone(&vehicle), mover.clone());
    }
    drop(mover_tap.drain());
    drop(java_tap.drain());
    drop(bedrock_tap.drain());
    let ClientPlatform::Bedrock(client) = mover.client.as_ref() else {
        return Err("the mover is not a Bedrock player".into());
    };
    for input in scene.steps {
        client.handle_player_auth_input(&mover, packet_of(&mover, input), server);
    }
    Ok(format!(
        "{}: {} | sent [{}] | java watcher [{}] | bedrock watcher [{}]{}\n",
        scene.name,
        mover_text(&mover),
        unordered_if(scene.frost, bedrock_sent(&mover_tap.drain())),
        java_sent(&java_tap.drain()),
        unordered_if(scene.frost, bedrock_sent(&bedrock_tap.drain())),
        extra_text(world, &mover, scene),
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
async fn bedrock_player_auth_input_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let scenes = scenes();
    load_chunks(&world, scenes.len());
    let mut out = String::new();
    for (index, scene) in scenes.iter().enumerate() {
        out.push_str(&run(&server, &world, index, scene).await?);
        for player in world.players.load().iter() {
            world.remove_player(player, false).await;
        }
    }
    server.stop();
    script_golden::bless_or_compare("bedrock_player_auth_input.txt", &out)
}
