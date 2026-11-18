//! A record of what `BedrockClient::handle_player_action` does, made before the function was
//! split (`tests/golden/bedrock_player_action.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene lays one block (and the same block one step east) in a loaded area of a real
//! server's world, puts a fresh Bedrock breaker (fixed identity, two blocks away, the scene's game
//! mode, items and state) and a fresh Java watcher next to it, then runs the scene's steps: player
//! actions on the block or on its east neighbour, ticks passing, a change of held item. One line
//! per scene: the block left, the items lying around, the breaker's mining state, held and
//! offhand items, exhaustion, `Used`/`Mined` statistics, pose and entity flags, sleep, blocking,
//! life and activity, then the packets of the breaker and the watcher (length and FNV-1a).
//! Not covered: plugins cancelling the glide, interact or bed events (none is loaded).

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use pumpkin_data::block_properties::TntLikeProperties;
use pumpkin_data::data_component_impl::EquipmentSlot;
use pumpkin_data::entity::EntityPose;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::{Block, BlockStateId, Enchantment};
use pumpkin_protocol::bedrock::server::player_action::{PlayerActionType, SPlayerAction};
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::codec::var_ulong::VarULong;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::{GameMode, Hand};
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::EntityBase;
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::player::Player;
use crate::entity::player::statistics::StatisticCategory;
use crate::net::ClientPlatform;
use crate::server::Server;
use crate::world::World;

const BREAKER: Identity = Identity {
    name: "Breaker",
    uuid: Uuid::from_u128(0x0000_00f1),
    entity_id: 9_301,
};
const WATCHER: Identity = Identity {
    name: "Watcher",
    uuid: Uuid::from_u128(0x0000_00f2),
    entity_id: 9_302,
};

#[derive(Clone, Copy)]
enum Kind {
    Stone,
    Air,
    Torch,
    Dirt,
    UnstableTnt,
    Leaves,
}

#[derive(Clone, Copy)]
enum Step {
    /// A player action on the scene's block.
    Act(PlayerActionType),
    /// A player action on the block one step east.
    East(PlayerActionType),
    /// The breaker's tick counter moves on.
    Ticks(i32),
    /// The breaker now holds this item.
    Hold(&'static Item),
}

use PlayerActionType as A;
use Step::{Act, East, Hold, Ticks};

#[derive(Clone, Copy)]
struct Scene {
    name: &'static str,
    kind: Kind,
    gamemode: GameMode,
    held: Option<(&'static Item, u8)>,
    offhand: Option<&'static Item>,
    /// Worn on the chest.
    chest: Option<&'static Item>,
    /// The client has not finished loading.
    unloaded: bool,
    /// The breaker stands eight blocks away (out of reach).
    far: bool,
    /// The breaker stands 5.5 blocks away: inside reach only with the 1.0 margin.
    margin: bool,
    /// The breaker is dead (health 0).
    dead: bool,
    /// Only the dead flag is set; health stays 20.
    dead_flag_only: bool,
    /// Health is 0 but the dead flag is not set.
    zero_health_only: bool,
    /// The breaker stands on the ground.
    grounded: bool,
    /// The held item has Efficiency V.
    efficient: bool,
    /// The breaker is asleep in a bed at the scene's block.
    asleep: bool,
    /// The breaker is sneaking, sprinting and swimming already.
    flagged: bool,
    /// The breaker is gliding already.
    gliding: bool,
    /// The breaker is blocking with its shield already.
    blocking: bool,
    steps: &'static [Step],
}

const SCENE: Scene = Scene {
    name: "",
    kind: Kind::Stone,
    gamemode: GameMode::Survival,
    held: None,
    offhand: None,
    chest: None,
    unloaded: false,
    far: false,
    margin: false,
    dead: false,
    dead_flag_only: false,
    zero_health_only: false,
    grounded: true,
    efficient: false,
    asleep: false,
    flagged: false,
    gliding: false,
    blocking: false,
    steps: &[Act(A::StartDestroyBlock)],
};

const fn scene(name: &'static str, steps: &'static [Step]) -> Scene {
    Scene {
        name,
        steps,
        ..SCENE
    }
}

const PICKAXE: Option<(&Item, u8)> = Some((&Item::IRON_PICKAXE, 1));

fn scenes() -> Vec<Scene> {
    let mut scenes = gate_scenes();
    scenes.extend(start_scenes());
    scenes.extend(predict_scenes());
    scenes.extend(other_scenes());
    scenes
}

/// The guards before the match: loading, death, reach.
fn gate_scenes() -> Vec<Scene> {
    vec![
        Scene {
            unloaded: true,
            ..scene("client not loaded, start", &[Act(A::StartDestroyBlock)])
        },
        Scene {
            dead: true,
            ..scene("dead, start", &[Act(A::StartDestroyBlock)])
        },
        Scene {
            dead_flag_only: true,
            ..scene("dead flag only, start", &[Act(A::StartDestroyBlock)])
        },
        Scene {
            zero_health_only: true,
            ..scene("zero health only, start", &[Act(A::StartDestroyBlock)])
        },
        Scene {
            dead: true,
            ..scene("dead, sneak", &[Act(A::StartSneaking)])
        },
        Scene {
            far: true,
            ..scene("start out of reach", &[Act(A::StartDestroyBlock)])
        },
        Scene {
            margin: true,
            ..scene(
                "start just inside the reach margin",
                &[Act(A::StartDestroyBlock)],
            )
        },
        Scene {
            far: true,
            held: PICKAXE,
            ..scene(
                "predict out of reach",
                &[
                    Act(A::StartDestroyBlock),
                    Ticks(20),
                    Act(A::PredictDestroyBlock),
                ],
            )
        },
        Scene {
            margin: true,
            held: PICKAXE,
            ..scene(
                "predict just inside the reach margin",
                &[
                    Act(A::StartDestroyBlock),
                    Ticks(20),
                    Act(A::PredictDestroyBlock),
                ],
            )
        },
    ]
}

/// Start, creative and continue destroy.
#[expect(clippy::too_many_lines)]
fn start_scenes() -> Vec<Scene> {
    vec![
        Scene {
            gamemode: GameMode::Creative,
            ..scene("start in creative", &[Act(A::StartDestroyBlock)])
        },
        Scene {
            gamemode: GameMode::Creative,
            ..scene("creative destroy", &[Act(A::CreativeDestroyBlock)])
        },
        Scene {
            gamemode: GameMode::Creative,
            kind: Kind::Air,
            ..scene("creative destroy on air", &[Act(A::CreativeDestroyBlock)])
        },
        Scene {
            gamemode: GameMode::Creative,
            ..scene(
                "creative while mining elsewhere",
                &[East(A::ContinueDestroyBlock), Act(A::StartDestroyBlock)],
            )
        },
        scene("start on stone by hand", &[Act(A::StartDestroyBlock)]),
        Scene {
            held: PICKAXE,
            ..scene(
                "start on stone with a pickaxe",
                &[Act(A::StartDestroyBlock)],
            )
        },
        Scene {
            held: PICKAXE,
            grounded: false,
            ..scene(
                "start on stone with a pickaxe, in the air",
                &[Act(A::StartDestroyBlock)],
            )
        },
        Scene {
            kind: Kind::Air,
            ..scene("start on air", &[Act(A::StartDestroyBlock)])
        },
        Scene {
            kind: Kind::Torch,
            ..scene("start on a torch (instant)", &[Act(A::StartDestroyBlock)])
        },
        Scene {
            kind: Kind::Torch,
            held: PICKAXE,
            ..scene(
                "start on a torch holding a pickaxe (instant, no wear)",
                &[Act(A::StartDestroyBlock)],
            )
        },
        Scene {
            kind: Kind::Leaves,
            held: Some((&Item::SHEARS, 1)),
            ..scene(
                "start on leaves with shears (instant, wear)",
                &[Act(A::StartDestroyBlock)],
            )
        },
        Scene {
            kind: Kind::Dirt,
            held: Some((&Item::IRON_SHOVEL, 1)),
            efficient: true,
            ..scene(
                "start on dirt with an efficiency V shovel (instant)",
                &[Act(A::StartDestroyBlock)],
            )
        },
        Scene {
            kind: Kind::UnstableTnt,
            ..scene(
                "start on unstable tnt (instant)",
                &[Act(A::StartDestroyBlock)],
            )
        },
        Scene {
            gamemode: GameMode::Spectator,
            ..scene("start as spectator", &[Act(A::StartDestroyBlock)])
        },
        Scene {
            gamemode: GameMode::Adventure,
            ..scene("start in adventure", &[Act(A::StartDestroyBlock)])
        },
        scene(
            "start twice",
            &[
                Act(A::StartDestroyBlock),
                Ticks(3),
                Act(A::StartDestroyBlock),
            ],
        ),
        scene(
            "start, continue",
            &[
                Act(A::StartDestroyBlock),
                Ticks(3),
                Act(A::ContinueDestroyBlock),
            ],
        ),
        scene("continue without start", &[Act(A::ContinueDestroyBlock)]),
        scene(
            "start, switch to a pickaxe, continue",
            &[
                Act(A::StartDestroyBlock),
                Ticks(3),
                Hold(&Item::IRON_PICKAXE),
                Act(A::ContinueDestroyBlock),
            ],
        ),
        scene(
            "start east, then start here",
            &[
                East(A::StartDestroyBlock),
                Ticks(3),
                Act(A::StartDestroyBlock),
            ],
        ),
        scene(
            "start east, switch to an efficiency-free shovel, start here",
            &[
                East(A::StartDestroyBlock),
                Hold(&Item::IRON_SHOVEL),
                Act(A::StartDestroyBlock),
            ],
        ),
        Scene {
            kind: Kind::Torch,
            ..scene(
                "start east on a torch, then here",
                &[East(A::StartDestroyBlock), Act(A::StartDestroyBlock)],
            )
        },
    ]
}

/// Predict and stop destroy.
#[expect(clippy::too_many_lines)]
fn predict_scenes() -> Vec<Scene> {
    vec![
        scene("predict without start", &[Act(A::PredictDestroyBlock)]),
        scene("stop without start", &[Act(A::StopDestroyBlock)]),
        Scene {
            held: PICKAXE,
            ..scene(
                "start, 20 ticks, predict with a pickaxe",
                &[
                    Act(A::StartDestroyBlock),
                    Ticks(20),
                    Act(A::PredictDestroyBlock),
                ],
            )
        },
        Scene {
            held: PICKAXE,
            ..scene(
                "start, 1 tick, predict with a pickaxe (too soon)",
                &[
                    Act(A::StartDestroyBlock),
                    Ticks(1),
                    Act(A::PredictDestroyBlock),
                ],
            )
        },
        Scene {
            held: PICKAXE,
            ..scene(
                "start, 3 ticks, predict with a pickaxe",
                &[
                    Act(A::StartDestroyBlock),
                    Ticks(3),
                    Act(A::PredictDestroyBlock),
                ],
            )
        },
        Scene {
            held: PICKAXE,
            ..scene(
                "start, 4 ticks, predict with a pickaxe",
                &[
                    Act(A::StartDestroyBlock),
                    Ticks(4),
                    Act(A::PredictDestroyBlock),
                ],
            )
        },
        Scene {
            held: PICKAXE,
            ..scene(
                "start, 5 ticks, predict with a pickaxe",
                &[
                    Act(A::StartDestroyBlock),
                    Ticks(5),
                    Act(A::PredictDestroyBlock),
                ],
            )
        },
        Scene {
            held: PICKAXE,
            ..scene(
                "start, 20 ticks, stop with a pickaxe",
                &[
                    Act(A::StartDestroyBlock),
                    Ticks(20),
                    Act(A::StopDestroyBlock),
                ],
            )
        },
        Scene {
            held: PICKAXE,
            ..scene(
                "start, 1 tick, stop with a pickaxe",
                &[
                    Act(A::StartDestroyBlock),
                    Ticks(1),
                    Act(A::StopDestroyBlock),
                ],
            )
        },
        scene(
            "start, 200 ticks, predict by hand (no drops)",
            &[
                Act(A::StartDestroyBlock),
                Ticks(200),
                Act(A::PredictDestroyBlock),
            ],
        ),
        Scene {
            kind: Kind::Dirt,
            ..scene(
                "dirt by hand: start, 200 ticks, predict",
                &[
                    Act(A::StartDestroyBlock),
                    Ticks(200),
                    Act(A::PredictDestroyBlock),
                ],
            )
        },
        Scene {
            held: PICKAXE,
            ..scene(
                "start east, 20 ticks, predict here",
                &[
                    East(A::StartDestroyBlock),
                    Ticks(20),
                    Act(A::PredictDestroyBlock),
                ],
            )
        },
        Scene {
            held: PICKAXE,
            ..scene(
                "start, 20 ticks, predict east",
                &[
                    Act(A::StartDestroyBlock),
                    Ticks(20),
                    East(A::PredictDestroyBlock),
                ],
            )
        },
        Scene {
            held: PICKAXE,
            ..scene(
                "start, abort, 20 ticks, predict",
                &[
                    Act(A::StartDestroyBlock),
                    Act(A::AbortDestroyBlock),
                    Ticks(20),
                    Act(A::PredictDestroyBlock),
                ],
            )
        },
        Scene {
            gamemode: GameMode::Creative,
            ..scene("predict in creative", &[Act(A::PredictDestroyBlock)])
        },
        Scene {
            gamemode: GameMode::Creative,
            ..scene(
                "stop in creative while mining",
                &[East(A::StartDestroyBlock), Act(A::StopDestroyBlock)],
            )
        },
        Scene {
            kind: Kind::Air,
            ..scene(
                "stop on air while mining east",
                &[East(A::StartDestroyBlock), Act(A::StopDestroyBlock)],
            )
        },
        Scene {
            kind: Kind::Air,
            ..scene(
                "predict on air while mining east",
                &[East(A::StartDestroyBlock), Act(A::PredictDestroyBlock)],
            )
        },
        Scene {
            held: PICKAXE,
            ..scene(
                "start, 20 ticks, switch to bare hand, predict",
                &[
                    Act(A::StartDestroyBlock),
                    Ticks(20),
                    Hold(&Item::AIR),
                    Act(A::PredictDestroyBlock),
                ],
            )
        },
    ]
}

/// The other actions.
#[expect(clippy::too_many_lines)]
fn other_scenes() -> Vec<Scene> {
    vec![
        scene(
            "start, crack",
            &[Act(A::StartDestroyBlock), Act(A::CrackBlock)],
        ),
        scene(
            "start, abort",
            &[Act(A::StartDestroyBlock), Act(A::AbortDestroyBlock)],
        ),
        Scene {
            held: Some((&Item::APPLE, 5)),
            ..scene("drop item", &[Act(A::DropItem)])
        },
        Scene {
            chest: Some(&Item::ELYTRA),
            grounded: false,
            ..scene(
                "start gliding with an elytra in the air",
                &[Act(A::StartGliding)],
            )
        },
        Scene {
            chest: Some(&Item::ELYTRA),
            ..scene(
                "start gliding with an elytra on the ground",
                &[Act(A::StartGliding)],
            )
        },
        Scene {
            grounded: false,
            ..scene("start gliding without an elytra", &[Act(A::StartGliding)])
        },
        Scene {
            chest: Some(&Item::ELYTRA),
            grounded: false,
            gliding: true,
            ..scene("stop gliding", &[Act(A::StopGliding)])
        },
        scene("stop gliding while not gliding", &[Act(A::StopGliding)]),
        scene("start swimming", &[Act(A::StartSwimming)]),
        Scene {
            flagged: true,
            ..scene("stop swimming", &[Act(A::StopSwimming)])
        },
        scene("start spin attack", &[Act(A::StartSpinAttack)]),
        scene(
            "start, stop spin attack",
            &[Act(A::StartSpinAttack), Act(A::StopSpinAttack)],
        ),
        scene("start sprinting", &[Act(A::StartSprinting)]),
        Scene {
            flagged: true,
            ..scene("stop sprinting", &[Act(A::StopSprinting)])
        },
        scene("start sneaking", &[Act(A::StartSneaking)]),
        Scene {
            offhand: Some(&Item::SHIELD),
            ..scene("start sneaking with a shield", &[Act(A::StartSneaking)])
        },
        Scene {
            held: Some((&Item::SHIELD, 1)),
            ..scene(
                "start sneaking with a shield in the main hand",
                &[Act(A::StartSneaking)],
            )
        },
        Scene {
            flagged: true,
            ..scene("stop sneaking", &[Act(A::StopSneaking)])
        },
        Scene {
            flagged: true,
            offhand: Some(&Item::SHIELD),
            blocking: true,
            ..scene("stop sneaking while blocking", &[Act(A::StopSneaking)])
        },
        scene("start crawling", &[Act(A::StartCrawling)]),
        scene(
            "start, stop crawling",
            &[Act(A::StartCrawling), Act(A::StopCrawling)],
        ),
        Scene {
            asleep: true,
            ..scene("stop sleeping", &[Act(A::StopSleeping)])
        },
        scene("stop sleeping while awake", &[Act(A::StopSleeping)]),
        scene("respawn while alive", &[Act(A::Respawn)]),
        Scene {
            dead: true,
            ..scene("respawn while dead", &[Act(A::Respawn)])
        },
        Scene {
            dead_flag_only: true,
            ..scene("respawn with the dead flag only", &[Act(A::Respawn)])
        },
        Scene {
            zero_health_only: true,
            ..scene("respawn with zero health only", &[Act(A::Respawn)])
        },
        scene("start jump (unhandled)", &[Act(A::StartJump)]),
        scene("missed swing (unhandled)", &[Act(A::MissedSwing)]),
    ]
}

const fn position_of(index: usize) -> BlockPos {
    BlockPos::new(index as i32 * 16 + 200, 64, 8)
}

fn put(world: &Arc<World>, pos: &BlockPos, state: BlockStateId) {
    world.set_block_state(
        pos,
        state,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

fn lay(world: &Arc<World>, pos: BlockPos, kind: Kind) {
    let state = match kind {
        Kind::Stone => Block::STONE.default_state.id,
        Kind::Air => Block::AIR.default_state.id,
        Kind::Torch => Block::TORCH.default_state.id,
        Kind::Dirt => Block::DIRT.default_state.id,
        Kind::UnstableTnt => TntLikeProperties { unstable: true }.to_state_id(&Block::TNT),
        Kind::Leaves => Block::OAK_LEAVES.default_state.id,
    };
    for x in -3..=9 {
        for z in -3..=9 {
            put(
                world,
                &BlockPos::new(pos.0.x + x, 63, pos.0.z + z),
                Block::STONE.default_state.id,
            );
        }
    }
    put(world, &pos, state);
    put(world, &pos.east(), state);
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// What the Bedrock breaker was sent. Inventory slot updates (game packet 50, after the 0xfe
/// batch byte and a one-byte length) show only as `slot`: they carry a stack id that depends on
/// the tests run before.
fn breaker_sent(sent: &[bytes::Bytes]) -> String {
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

/// Java packet ids whose bytes are random like in vanilla: Add Entity (UUID, TNT velocity) and
/// the two sound packets (seed).
const RANDOM_PACKETS: [u8; 3] = [1, 118, 119];

fn watcher_sent(sent: &[bytes::Bytes]) -> String {
    sent.iter()
        .map(|p| match p.first() {
            Some(id) if RANDOM_PACKETS.contains(id) => format!("{}:id{id}", p.len()),
            _ => format!("{}:{:08x}", p.len(), fnv(p) as u32),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Items (name x count, sorted) and other non-player entities lying within six blocks of `pos`.
fn around(world: &World, pos: BlockPos) -> Vec<String> {
    let mut found: Vec<String> = world
        .entities
        .load()
        .iter()
        .filter(|entity| {
            let at = entity.get_entity().pos.load();
            (at.x - f64::from(pos.0.x)).abs() < 6.0 && (at.z - f64::from(pos.0.z)).abs() < 6.0
        })
        .map(|entity| {
            entity.get_item_entity().map_or_else(
                || entity.get_entity().entity_type.resource_name.to_string(),
                |item| {
                    let stack = item
                        .get_item_stack()
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clone();
                    format!("{}x{}", stack.item.registry_key, stack.item_count)
                },
            )
        })
        .collect();
    found.sort();
    found
}

fn stack_text(stack: &ItemStack) -> String {
    if stack.is_empty() {
        return "-".to_string();
    }
    format!(
        "{}x{} dmg {}",
        stack.item.registry_key,
        stack.item_count,
        stack.get_damage()
    )
}

fn mining_text(player: &Player, block: &Block) -> String {
    let inventory = player.inventory();
    let item = inventory.held_item();
    let stats = player
        .stats
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    format!(
        "mining {} at {:?} stage {} speed {:08x} start {} held {} offhand {} exhaustion {} used {} mined {}",
        player.mining.load(Ordering::Relaxed),
        *player
            .mining_pos
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        player.current_block_destroy_stage.load(Ordering::Relaxed),
        player.current_block_breaking_speed.load(Ordering::Relaxed),
        player.start_mining_time.load(Ordering::Relaxed),
        stack_text(&item),
        stack_text(&inventory.off_hand_item()),
        player.hunger_manager.exhaustion.load(),
        stats.get(StatisticCategory::Used, item.item.id as i32),
        stats.get(StatisticCategory::Mined, block.id.as_u16() as i32),
    )
}

fn body_text(player: &Player) -> String {
    let entity = player.get_entity();
    format!(
        "pose {} sneaking {} sprinting {} swimming {} gliding {} asleep {} since {:?} blocking {} dead {} health {} acted {}",
        entity.pose.load() as i32,
        entity.is_sneaking(),
        entity.is_sprinting(),
        entity.is_swimming(),
        entity.is_fall_flying(),
        player.is_sleeping(),
        player.sleeping_since.load(),
        player.living_entity.is_blocking(),
        player.living_entity.dead.load(Ordering::Relaxed),
        player.living_entity.health.load(),
        player.last_action_time.load().elapsed() < Duration::from_secs(300),
    )
}

fn place(player: &Player, at: Vector3<f64>, pos: BlockPos) {
    player.get_entity().set_pos(at);
    player.watched_section.store(Cylindrical::new(
        pos.chunk_position(),
        std::num::NonZero::<u8>::MIN.saturating_add(1),
    ));
}

/// Gives the breaker the scene's items and state.
fn equip(breaker: &Player, scene: &Scene, pos: BlockPos) {
    let inventory = breaker.inventory();
    if let Some((item, count)) = scene.held {
        let mut stack = ItemStack::new(count, item);
        if scene.efficient {
            stack.add_enchantment(&Enchantment::EFFICIENCY, 5);
        }
        inventory.set_stack_in_hand(Hand::Right, stack);
    }
    if let Some(item) = scene.offhand {
        inventory.set_stack_in_hand(Hand::Left, ItemStack::new(1, item));
    }
    if let Some(item) = scene.chest {
        inventory
            .entity_equipment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .put(&EquipmentSlot::CHEST, ItemStack::new(1, item));
    }
    let entity = breaker.get_entity();
    entity.on_ground.store(scene.grounded, Ordering::Relaxed);
    if scene.dead || scene.dead_flag_only {
        breaker.living_entity.dead.store(true, Ordering::Relaxed);
    }
    if scene.dead || scene.zero_health_only {
        breaker.living_entity.health.store(0.0);
    }
    if scene.asleep {
        breaker.sleeping_bed_pos.store(Some(pos));
        breaker.sleeping_since.store(Some(40));
        entity.set_pose(EntityPose::Sleeping);
    }
    if scene.flagged {
        entity.set_sneaking(true);
        entity.set_sprinting(true);
        entity.set_swimming(true);
    }
    if scene.gliding {
        entity.set_fall_flying(true);
    }
    if scene.blocking {
        breaker
            .living_entity
            .set_active_hand(Hand::Left, ItemStack::new(1, &Item::SHIELD), 72_000);
    }
    breaker.tick_counter.store(37, Ordering::Relaxed);
    if let Some(long_ago) = Instant::now().checked_sub(Duration::from_secs(600)) {
        breaker.last_action_time.store(long_ago);
    }
}

fn bedrock(player: &Player) -> Option<&crate::net::bedrock::BedrockClient> {
    match player.client.as_ref() {
        ClientPlatform::Bedrock(client) => Some(client),
        ClientPlatform::Java(_) => None,
    }
}

fn action(kind: PlayerActionType, at: BlockPos) -> SPlayerAction {
    SPlayerAction {
        player_runtime_id: VarULong(u64::from(BREAKER.entity_id.unsigned_abs())),
        action: kind,
        block_position: at,
        result_pos: at,
        face: VarInt(1),
    }
}

async fn run(
    server: &Arc<Server>,
    world: &Arc<World>,
    index: usize,
    scene: &Scene,
) -> Result<String, String> {
    let pos = position_of(index);
    lay(world, pos, scene.kind);
    let block = Block::from_state_id(world.get_block_state_id(&pos));
    let (breaker, mut breaker_tap) =
        test_support::bedrock_player_as(world, scene.gamemode, &BREAKER).await?;
    let (watcher, mut watcher_tap) =
        test_support::java_player_as(world, GameMode::Survival, &WATCHER)
            .ok_or("no Java packet tap")?;
    let away = if scene.far {
        8.0
    } else if scene.margin {
        5.5
    } else {
        2.0
    };
    place(
        &breaker,
        Vector3::new(
            f64::from(pos.0.x) + 0.5,
            64.0,
            f64::from(pos.0.z) + 0.5 + away,
        ),
        pos,
    );
    place(
        &watcher,
        Vector3::new(f64::from(pos.0.x) + 3.5, 64.0, f64::from(pos.0.z) + 0.5),
        pos,
    );
    breaker
        .client_loaded
        .store(!scene.unloaded, Ordering::Relaxed);
    equip(&breaker, scene, pos);
    world.add_player(&breaker)?;
    world.add_player(&watcher)?;
    let client = bedrock(&breaker).ok_or("the breaker is not a Bedrock player")?;
    drop(breaker_tap.drain());
    drop(watcher_tap.drain());
    for step in scene.steps {
        match *step {
            Act(kind) => client.handle_player_action(&breaker, server, &action(kind, pos)),
            East(kind) => {
                client.handle_player_action(&breaker, server, &action(kind, pos.east()));
            }
            Ticks(n) => {
                breaker.tick_counter.fetch_add(n, Ordering::Relaxed);
            }
            Hold(item) => breaker
                .inventory()
                .set_stack_in_hand(Hand::Right, ItemStack::new(1, item)),
        }
    }
    if scene.steps.iter().any(|s| matches!(s, Act(A::Respawn))) {
        // The respawn runs as a task of the client: wait for it (or two seconds).
        for _ in 0..200 {
            if !breaker.living_entity.dead.load(Ordering::Relaxed)
                && breaker.living_entity.health.load() > 0.0
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    let line = format!(
        "{}: left {} east {} around {:?} {} {} | breaker [{}] | watcher [{}]\n",
        scene.name,
        world.get_block(&pos).name,
        world.get_block(&pos.east()).name,
        around(world, pos),
        mining_text(&breaker, block),
        body_text(&breaker),
        breaker_sent(&breaker_tap.drain()),
        watcher_sent(&watcher_tap.drain()),
    );
    Ok(line)
}

#[tokio::test(flavor = "multi_thread")]
async fn bedrock_player_action_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let scenes = scenes();
    for index in 0..scenes.len() {
        let chunk = position_of(index).chunk_position();
        for dx in -1..=1 {
            for dz in -1..=1 {
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
        out.push_str(&run(&server, &world, index, scene).await?);
        for player in world.players.load().iter() {
            world.remove_player(player, false).await;
        }
    }
    server.stop();
    script_golden::bless_or_compare("bedrock_player_action.txt", &out)
}
