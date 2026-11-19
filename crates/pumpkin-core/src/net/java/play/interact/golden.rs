//! A record of what `JavaClient::handle_interact` does, made before the function was split
//! (`tests/golden/interact.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene spawns one target (a cow, a baby cow, a sheep, an allay, a cod, or a second player)
//! with a fixed id in a loaded area of a real server's world, puts a fresh user (fixed identity,
//! the scene's game mode and items, two blocks from the target) and a fresh watcher next to it,
//! then hands the user's client one interact packet (action, target id, hand, sneaking, target
//! position). One line per scene: the target's health, age and custom name, the items and
//! entities lying around, the user's sneaking state, camera target, held and offhand items,
//! `Used`/`Broken` statistics and the advancements whose progress changed, then the packets of
//! the user, the watcher and a player target (length and FNV-1a of the bytes; random packets by
//! id).
//! Not covered: plugins cancelling the interact events (none is loaded).

use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use pumpkin_config::world::LevelConfig;
use pumpkin_config::{AdvancedConfiguration, BasicConfiguration, TelemetryConfig};
use pumpkin_data::data_component_impl::EquipmentSlot;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::java::server::play::SInteract;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::{GameMode, Hand};
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use uuid::Uuid;

use crate::command::commands::script_golden::{self, TestResult};
use crate::data::VanillaData;
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::mob::piglin::PiglinEntity;
use crate::entity::passive::allay::AllayEntity;
use crate::entity::passive::armadillo::ArmadilloEntity;
use crate::entity::passive::cod::CodEntity;
use crate::entity::passive::cow::CowEntity;
use crate::entity::passive::sheep::SheepEntity;
use crate::entity::passive::sniffer::SnifferEntity;
use crate::entity::player::Player;
use crate::entity::player::statistics::StatisticCategory;
use crate::entity::{Entity, EntityBase};
use crate::net::ClientPlatform;
use crate::net::java::PacketTap;
use crate::server::Server;
use crate::world::World;

const USER: Identity = Identity {
    name: "User",
    uuid: Uuid::from_u128(0x0000_00f5),
    entity_id: 9_401,
};
const WATCHER: Identity = Identity {
    name: "Watcher",
    uuid: Uuid::from_u128(0x0000_00f6),
    entity_id: 9_402,
};
const VICTIM: Identity = Identity {
    name: "Victim",
    uuid: Uuid::from_u128(0x0000_00f7),
    entity_id: 9_403,
};

const INTERACT: i32 = 0;
const ATTACK: i32 = 1;
const INTERACT_AT: i32 = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Cow,
    BabyCow,
    Sheep,
    Allay,
    Cod,
    Player(GameMode),
    DeadPlayer,
    /// The packet names an id no entity has.
    Missing,
    /// The packet names the user itself.
    Itself,
    /// Appended targets.
    Sniffer,
    BabySniffer,
    Armadillo,
    Piglin,
    /// A survival player with half a heart.
    WoundedPlayer,
}

#[derive(Clone, Copy)]
struct Scene {
    name: &'static str,
    target: Target,
    action: i32,
    gamemode: GameMode,
    held: Option<(&'static Item, u8)>,
    offhand: Option<(&'static Item, u8)>,
    /// The held tool has one use left.
    worn: bool,
    /// The held item carries a custom name.
    named: bool,
    /// `None` = no hand field in the packet.
    hand: Option<i32>,
    sneaking: bool,
    /// The user sneaks before the packet arrives.
    was_sneaking: bool,
    /// The client has not sent `PlayerLoaded`.
    unloaded: bool,
    /// The target stands ten blocks away (out of reach).
    far: bool,
    /// Appended: the target is in reach only with the 1.0 margin.
    margin: bool,
    /// Appended scenes end their line with whether the packet counted as activity and whether
    /// the user's connection was closed (kicked).
    extra: bool,
    /// Appended: the user wears a golden helmet.
    golden_helmet: bool,
    /// Appended: the scene runs on a second server whose `pvp.protect_creative` is off.
    unprotected: bool,
}

const SCENE: Scene = Scene {
    name: "",
    target: Target::Cow,
    action: INTERACT,
    gamemode: GameMode::Survival,
    held: None,
    offhand: None,
    worn: false,
    named: false,
    hand: Some(0),
    sneaking: false,
    was_sneaking: false,
    unloaded: false,
    far: false,
    margin: false,
    extra: false,
    golden_helmet: false,
    unprotected: false,
};

const fn scene(name: &'static str, target: Target, action: i32) -> Scene {
    Scene {
        name,
        target,
        action,
        ..SCENE
    }
}

fn scenes() -> Vec<Scene> {
    let mut scenes = guard_scenes();
    scenes.extend(attack_scenes());
    scenes.extend(interact_scenes());
    scenes.extend(widened_scenes());
    scenes.extend(widened_scenes_late());
    scenes
}

const LATE: Scene = Scene {
    extra: true,
    ..SCENE
};

const fn late(name: &'static str, target: Target, action: i32) -> Scene {
    Scene {
        name,
        target,
        action,
        ..LATE
    }
}

/// Scenes appended in the second widening (the arm bodies, judged on the split helpers).
fn widened_scenes_late() -> Vec<Scene> {
    vec![
        late(
            "attack a player with half a heart",
            Target::WoundedPlayer,
            ATTACK,
        ),
        Scene {
            held: Some((&Item::WHEAT, 1)),
            ..late("the last wheat on a cow", Target::Cow, INTERACT)
        },
        Scene {
            held: Some((&Item::TORCHFLOWER_SEEDS, 2)),
            ..late(
                "torchflower seeds on a baby sniffer",
                Target::BabySniffer,
                INTERACT,
            )
        },
        Scene {
            held: Some((&Item::TORCHFLOWER_SEEDS, 2)),
            ..late("torchflower seeds on a sniffer", Target::Sniffer, INTERACT)
        },
        Scene {
            held: Some((&Item::BRUSH, 1)),
            ..late("brush on an armadillo", Target::Armadillo, INTERACT)
        },
        Scene {
            held: Some((&Item::GOLD_INGOT, 2)),
            ..late("gold ingot on a piglin", Target::Piglin, INTERACT)
        },
        Scene {
            held: Some((&Item::GOLD_INGOT, 2)),
            golden_helmet: true,
            ..late(
                "gold ingot on a piglin, golden helmet worn",
                Target::Piglin,
                INTERACT,
            )
        },
        Scene {
            held: Some((&Item::NAME_TAG, 2)),
            named: true,
            ..late(
                "named name tag on a cow, advancements",
                Target::Cow,
                INTERACT,
            )
        },
        Scene {
            unprotected: true,
            ..late(
                "attack a creative player, creative not protected",
                Target::Player(GameMode::Creative),
                ATTACK,
            )
        },
    ]
}

const WIDE: Scene = Scene {
    extra: true,
    ..SCENE
};

const fn wide(name: &'static str, target: Target, action: i32) -> Scene {
    Scene {
        name,
        target,
        action,
        ..WIDE
    }
}

/// Scenes appended in the first widening.
fn widened_scenes() -> Vec<Scene> {
    vec![
        wide("invalid action, kick", Target::Cow, 5),
        wide("attack itself, kick", Target::Itself, ATTACK),
        wide("attack a missing entity, kick", Target::Missing, ATTACK),
        wide(
            "interact with a missing entity, no kick",
            Target::Missing,
            INTERACT,
        ),
        Scene {
            held: Some((&Item::BUCKET, 1)),
            ..wide("bucket on a cow, activity", Target::Cow, INTERACT)
        },
        Scene {
            margin: true,
            held: Some((&Item::BUCKET, 1)),
            ..wide(
                "bucket on a cow just inside the reach margin",
                Target::Cow,
                INTERACT,
            )
        },
        Scene {
            margin: true,
            ..wide(
                "attack a cow just inside the reach margin",
                Target::Cow,
                ATTACK,
            )
        },
    ]
}

fn guard_scenes() -> Vec<Scene> {
    vec![
        Scene {
            unloaded: true,
            held: Some((&Item::BUCKET, 1)),
            ..scene("client not loaded", Target::Cow, INTERACT)
        },
        Scene {
            held: Some((&Item::BUCKET, 1)),
            ..scene("invalid action", Target::Cow, 5)
        },
        Scene {
            sneaking: true,
            ..scene("sneaking flag on, invalid action", Target::Cow, 5)
        },
        Scene {
            was_sneaking: true,
            ..scene("sneaking flag off, interact", Target::Cow, INTERACT)
        },
        scene("attack a missing entity", Target::Missing, ATTACK),
        scene("interact with a missing entity", Target::Missing, INTERACT),
        Scene {
            far: true,
            held: Some((&Item::BUCKET, 1)),
            ..scene("interact out of reach", Target::Cow, INTERACT)
        },
        Scene {
            far: true,
            ..scene("attack out of reach", Target::Cow, ATTACK)
        },
        Scene {
            gamemode: GameMode::Spectator,
            ..scene("spectator interacts with a cow", Target::Cow, INTERACT)
        },
        Scene {
            gamemode: GameMode::Spectator,
            ..scene("spectator attacks a cow", Target::Cow, ATTACK)
        },
    ]
}

fn attack_scenes() -> Vec<Scene> {
    vec![
        scene("attack a cow", Target::Cow, ATTACK),
        scene("attack itself", Target::Itself, ATTACK),
        scene(
            "attack a survival player",
            Target::Player(GameMode::Survival),
            ATTACK,
        ),
        scene(
            "attack a creative player",
            Target::Player(GameMode::Creative),
            ATTACK,
        ),
        scene("attack a dead player", Target::DeadPlayer, ATTACK),
        Scene {
            held: Some((&Item::IRON_SWORD, 1)),
            ..scene("attack a cow with a sword", Target::Cow, ATTACK)
        },
    ]
}

fn interact_scenes() -> Vec<Scene> {
    vec![
        scene("empty hand on a cow", Target::Cow, INTERACT),
        Scene {
            held: Some((&Item::BUCKET, 1)),
            ..scene("bucket on a cow", Target::Cow, INTERACT)
        },
        Scene {
            held: Some((&Item::BUCKET, 3)),
            ..scene("one of three buckets on a cow", Target::Cow, INTERACT)
        },
        Scene {
            held: Some((&Item::BUCKET, 1)),
            ..scene("bucket on a baby cow", Target::BabyCow, INTERACT)
        },
        Scene {
            held: Some((&Item::WHEAT, 4)),
            ..scene("wheat on a cow", Target::Cow, INTERACT)
        },
        Scene {
            held: Some((&Item::WHEAT, 4)),
            ..scene("wheat on a baby cow", Target::BabyCow, INTERACT)
        },
        Scene {
            held: Some((&Item::NAME_TAG, 2)),
            named: true,
            ..scene("named name tag on a cow", Target::Cow, INTERACT)
        },
        Scene {
            held: Some((&Item::NAME_TAG, 2)),
            ..scene("blank name tag on a cow", Target::Cow, INTERACT)
        },
        Scene {
            held: Some((&Item::RED_DYE, 2)),
            ..scene("red dye on a sheep", Target::Sheep, INTERACT)
        },
        Scene {
            held: Some((&Item::SHEARS, 1)),
            ..scene("shears on a sheep", Target::Sheep, INTERACT)
        },
        Scene {
            held: Some((&Item::SHEARS, 1)),
            worn: true,
            ..scene("worn shears on a sheep (break)", Target::Sheep, INTERACT)
        },
        Scene {
            held: Some((&Item::APPLE, 2)),
            ..scene("apple on an allay", Target::Allay, INTERACT)
        },
        Scene {
            offhand: Some((&Item::APPLE, 2)),
            hand: Some(1),
            ..scene("offhand apple on an allay", Target::Allay, INTERACT)
        },
        Scene {
            held: Some((&Item::WATER_BUCKET, 1)),
            ..scene("water bucket on a cod", Target::Cod, INTERACT)
        },
        Scene {
            offhand: Some((&Item::BUCKET, 1)),
            hand: Some(1),
            ..scene("offhand bucket on a cow", Target::Cow, INTERACT)
        },
        Scene {
            held: Some((&Item::BUCKET, 1)),
            hand: Some(3),
            ..scene("invalid hand on a cow", Target::Cow, INTERACT)
        },
        Scene {
            held: Some((&Item::BUCKET, 1)),
            hand: None,
            ..scene("no hand field on a cow", Target::Cow, INTERACT)
        },
        Scene {
            held: Some((&Item::BUCKET, 1)),
            ..scene("bucket on a cow, at a position", Target::Cow, INTERACT_AT)
        },
        Scene {
            held: Some((&Item::BUCKET, 1)),
            hand: Some(1),
            ..scene(
                "bucket held, offhand at a position on a cow",
                Target::Cow,
                INTERACT_AT,
            )
        },
        Scene {
            held: Some((&Item::BUCKET, 1)),
            ..scene(
                "bucket on a survival player",
                Target::Player(GameMode::Survival),
                INTERACT,
            )
        },
        Scene {
            gamemode: GameMode::Creative,
            held: Some((&Item::BUCKET, 1)),
            ..scene("bucket on a cow in creative", Target::Cow, INTERACT)
        },
    ]
}

const fn position_of(index: usize) -> BlockPos {
    BlockPos::new(index as i32 * 16 + 200, 64, 8)
}

const fn target_id(index: usize) -> i32 {
    9_500 + index as i32
}

fn mob(
    world: &Arc<World>,
    index: usize,
    kind: &'static EntityType,
    at: Vector3<f64>,
) -> Result<Arc<dyn EntityBase>, String> {
    let id = target_id(index);
    let entity = Entity::from_uuid_with_id(
        id,
        Uuid::from_u128(0x6d6f_6200_0000 + id as u128),
        world.clone(),
        at,
        kind,
    );
    let mob: Arc<dyn EntityBase> = if kind == &EntityType::SNIFFER {
        SnifferEntity::new(entity)
    } else if kind == &EntityType::ARMADILLO {
        ArmadilloEntity::new(entity)
    } else if kind == &EntityType::PIGLIN {
        PiglinEntity::new(entity)
    } else if kind == &EntityType::SHEEP {
        SheepEntity::new(entity)
    } else if kind == &EntityType::ALLAY {
        AllayEntity::new(entity)
    } else if kind == &EntityType::COD {
        CodEntity::new(entity)
    } else {
        CowEntity::new(entity)
    };
    if world.spawn_entity(mob.clone()) {
        Ok(mob)
    } else {
        Err(format!("mob {id} not spawned"))
    }
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Packet ids whose bytes are random like in vanilla: Add Entity (UUID, velocity) and the two
/// sound packets (seed).
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

/// Items (each name once: shearing drops a random count) and other entities (type) within six
/// blocks of `pos`, sorted, players left out.
fn things_near(world: &World, pos: BlockPos) -> Vec<String> {
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
                    item.get_item_stack()
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .item
                        .registry_key
                        .to_string()
                },
            )
        })
        .collect();
    found.sort();
    // Shearing drops one to three wool items at random: show each kind once.
    found.dedup();
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

fn target_text(target: Option<&Arc<dyn EntityBase>>) -> String {
    let Some(target) = target else {
        return "none".to_string();
    };
    let entity = target.get_entity();
    format!(
        "health {:?} age {} name {:?}",
        target.get_living_entity().map(|l| l.health.load()),
        entity.age.load(Ordering::Relaxed),
        entity
            .custom_name
            .load()
            .as_ref()
            .as_ref()
            .map(|name| format!("{name:?}")),
    )
}

fn user_text(player: &Player, used: &Item) -> String {
    let inventory = player.inventory();
    let stats = player
        .stats
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut changed: Vec<String> = player
        .advancements
        .try_lock()
        .map(|advancements| {
            advancements
                .progress_changed
                .iter()
                .map(|advancement| advancement.id.to_string())
                .collect()
        })
        .unwrap_or_default();
    changed.sort();
    format!(
        "sneaking {} camera {:?} held {} offhand {} used {} broken {} advancements {changed:?}",
        player.get_entity().is_sneaking(),
        player.camera_target_id.load(),
        stack_text(&inventory.held_item()),
        stack_text(&inventory.off_hand_item()),
        stats.get(StatisticCategory::Used, used.id as i32),
        stats.get(StatisticCategory::Broken, used.id as i32),
    )
}

fn java(player: &Player) -> Option<&crate::net::java::JavaClient> {
    match player.client.as_ref() {
        ClientPlatform::Java(client) => Some(client),
        ClientPlatform::Bedrock(_) => None,
    }
}

fn place(player: &Player, at: Vector3<f64>, pos: BlockPos) {
    player.get_entity().set_pos(at);
    player.watched_section.store(Cylindrical::new(
        pos.chunk_position(),
        NonZero::<u8>::MIN.saturating_add(1),
    ));
}

fn stack_of(item: &'static Item, count: u8, scene: &Scene) -> ItemStack {
    let mut stack = ItemStack::new(count, item);
    if scene.worn
        && let Some(max) = stack.get_max_damage()
    {
        stack.set_damage(max - 1);
    }
    if scene.named {
        stack.set_custom_name("Daisy".to_string());
    }
    stack
}

/// Gives the user the scene's items, sneaking and recorded advancements.
fn equip(user: &Arc<Player>, scene: &Scene) {
    let inventory = user.inventory();
    if let Some((item, count)) = scene.held {
        inventory.set_stack_in_hand(Hand::Right, stack_of(item, count, scene));
    }
    if let Some((item, count)) = scene.offhand {
        inventory.set_stack_in_hand(Hand::Left, stack_of(item, count, scene));
    }
    if scene.was_sneaking {
        user.get_entity().set_sneaking(true);
    }
    if scene.golden_helmet {
        user.living_entity
            .entity_equipment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .put(
                &EquipmentSlot::HEAD,
                ItemStack::new(1, &Item::GOLDEN_HELMET),
            );
    }
    if scene.extra
        && let Some(long_ago) = Instant::now().checked_sub(Duration::from_secs(600))
    {
        user.last_action_time.store(long_ago);
    }
    // As the server's login does, so that awarded criteria are recorded.
    user.advancements
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .player = Arc::downgrade(user);
}

/// A scene's target, a player target's packet tap and the entity id the packet names.
type Targeted = (Option<Arc<dyn EntityBase>>, Option<PacketTap>, i32);

/// The scene's target (and a player target's packet tap) and the entity id the packet names.
fn target_of(
    world: &Arc<World>,
    index: usize,
    scene: &Scene,
    at: Vector3<f64>,
) -> Result<Targeted, String> {
    let pos = position_of(index);
    let kind = match scene.target {
        Target::Player(gamemode) => return player_target(world, gamemode, at, pos, false),
        Target::DeadPlayer => return player_target(world, GameMode::Survival, at, pos, true),
        Target::Missing => return Ok((None, None, target_id(index))),
        Target::Itself => return Ok((None, None, USER.entity_id)),
        Target::Cow | Target::BabyCow => &EntityType::COW,
        Target::Sheep => &EntityType::SHEEP,
        Target::Allay => &EntityType::ALLAY,
        Target::Cod => &EntityType::COD,
        Target::Sniffer | Target::BabySniffer => &EntityType::SNIFFER,
        Target::Armadillo => &EntityType::ARMADILLO,
        Target::Piglin => &EntityType::PIGLIN,
        Target::WoundedPlayer => {
            let found = player_target(world, GameMode::Survival, at, pos, false)?;
            if let Some(victim) = found.0.as_ref().and_then(|t| t.get_living_entity()) {
                victim.health.store(0.5);
            }
            return Ok(found);
        }
    };
    let mob = mob(world, index, kind, at)?;
    if matches!(scene.target, Target::BabyCow | Target::BabySniffer) {
        mob.get_entity().age.store(-24_000, Ordering::Relaxed);
    }
    Ok((Some(mob), None, target_id(index)))
}

fn player_target(
    world: &Arc<World>,
    gamemode: GameMode,
    at: Vector3<f64>,
    pos: BlockPos,
    dead: bool,
) -> Result<Targeted, String> {
    let (victim, tap) =
        test_support::java_player_as(world, gamemode, &VICTIM).ok_or("no Java packet tap")?;
    place(&victim, at, pos);
    if dead {
        victim.living_entity.health.store(0.0);
    }
    world.add_player(&victim)?;
    Ok((Some(victim), Some(tap), VICTIM.entity_id))
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
    let away = if scene.far {
        10.0
    } else if scene.margin {
        3.95
    } else {
        2.0
    };
    place(&user, base.add(&Vector3::new(0.0, 0.0, away)), pos);
    place(&watcher, base.add(&Vector3::new(3.0, 0.0, 0.0)), pos);
    user.client_loaded.store(!scene.unloaded, Ordering::Relaxed);
    equip(&user, scene);
    world.add_player(&user)?;
    world.add_player(&watcher)?;
    let (target, mut target_tap, id) = target_of(world, index, scene, base)?;
    let client = java(&user).ok_or("the user is not a Java player")?;
    let used = if scene.hand == Some(1) {
        user.inventory().off_hand_item().item
    } else {
        user.inventory().held_item().item
    };
    drop(packets(&mut user_tap));
    drop(packets(&mut watcher_tap));
    if let Some(tap) = target_tap.as_mut() {
        drop(packets(tap));
    }
    let packet = SInteract {
        entity_id: VarInt(id),
        r#type: VarInt(scene.action),
        target_position: (scene.action == INTERACT_AT).then(|| Vector3::new(0.1, 0.9, -0.2)),
        hand: scene.hand.map(VarInt),
        sneaking: scene.sneaking,
    };
    client.handle_interact(&user, &packet, server);
    let target_sent = target_tap.as_mut().map(packets);
    let mut line = format!(
        "{}: target [{}] {:?} | user [{}] | sent [{}] | watcher [{}] | target sent {target_sent:?}",
        scene.name,
        target_text(target.as_ref()),
        things_near(world, pos),
        user_text(&user, used),
        packets(&mut user_tap),
        packets(&mut watcher_tap),
    );
    if scene.extra {
        line = format!(
            "{line} | acted {} kicked {}",
            user.last_action_time.load().elapsed() < Duration::from_secs(300),
            client.is_closed(),
        );
    }
    Ok(line + "\n")
}

fn load_chunks(world: &World, count: usize) {
    for index in 0..count {
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
}

/// Appended: a real server like [`test_support::server`] whose `pvp.protect_creative` is off.
async fn server_without_creative_protection(
    folder: &tempfile::TempDir,
) -> Result<Arc<Server>, Box<dyn std::error::Error>> {
    let mut basic = BasicConfiguration {
        default_level_name: folder.path().join("world").to_string_lossy().into_owned(),
        ..BasicConfiguration::default()
    };
    basic.seed.0 = 20_261_119;
    let mut advanced = AdvancedConfiguration {
        world: LevelConfig {
            stronghold_search_at_load: false,
            ..LevelConfig::default()
        },
        ..AdvancedConfiguration::default()
    };
    advanced.pvp.protect_creative = false;
    let telemetry = TelemetryConfig {
        enabled: false,
        ..TelemetryConfig::default()
    };
    let vanilla = VanillaData {
        banned_ip_list: std::sync::RwLock::default(),
        banned_player_list: std::sync::RwLock::default(),
        operator_config: std::sync::RwLock::default(),
        user_cache: std::sync::RwLock::default(),
        whitelist_config: std::sync::RwLock::default(),
    };
    Ok(Server::new(basic, advanced, telemetry, vanilla, Vec::new()).await?)
}

#[tokio::test(flavor = "multi_thread")]
async fn interact_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let other_folder = tempfile::tempdir()?;
    let unprotected = server_without_creative_protection(&other_folder).await?;
    let other_world = unprotected.worlds.load()[0].clone();
    let scenes = scenes();
    load_chunks(&world, scenes.len());
    load_chunks(&other_world, scenes.len());
    let mut out = String::new();
    for (index, scene) in scenes.iter().enumerate() {
        let (server, world) = if scene.unprotected {
            (&unprotected, &other_world)
        } else {
            (&server, &world)
        };
        out.push_str(&run(server, world, index, scene)?);
        for player in world.players.load().iter() {
            world.remove_player(player, false).await;
        }
    }
    server.stop();
    unprotected.stop();
    script_golden::bless_or_compare("interact.txt", &out)
}
