//! A record of what `Player::attack` does, made before the function was split
//! (`tests/golden/player_attack.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene puts a fresh attacker (Java or Bedrock, fixed identity, facing yaw 30) on a real
//! server's world with the scene's held item, enchantments, attack charge, sprint, fall and
//! effects, a victim two blocks in front of it (zombie, spider, iron golem, minecart, an
//! invulnerable zombie, or a player) and, when asked, three bystanders around the victim (one
//! inside the sweep box, one beside it and one above it), then calls `attack` once or twice.
//! One line per scene: the victim's health, velocity, fire and last damage, the bystanders'
//! health, the attacker's velocity, fall distance, held item, exhaustion, `Used` statistic,
//! charge ticks and last attack, then the packets of the attacker, a watcher and a victim player
//! (length and FNV-1a of the bytes; random sound and Add Entity packets by id).
//! Not covered: the `knockback` setting of the `pvp` config turned off (every scene uses the defaults).

use std::fmt::Write;
use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use pumpkin_data::Enchantment;
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::potion::Effect;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::{GameMode, Hand};
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use uuid::Uuid;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::living::LivingEntity;
use crate::entity::player::Player;
use crate::entity::player::statistics::StatisticCategory;
use crate::entity::vehicle::minecart::MinecartEntity;
use crate::entity::{Entity, EntityBase};
use crate::world::World;

const ATTACKER: Identity = Identity {
    name: "Attacker",
    uuid: Uuid::from_u128(0x0000_00f1),
    entity_id: 9_301,
};
const WATCHER: Identity = Identity {
    name: "Watcher",
    uuid: Uuid::from_u128(0x0000_00f2),
    entity_id: 9_302,
};
const VICTIM: Identity = Identity {
    name: "Victim",
    uuid: Uuid::from_u128(0x0000_00f3),
    entity_id: 9_303,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Victim {
    Zombie,
    Spider,
    Golem,
    Minecart,
    /// A zombie whose entity is invulnerable: the hit does no damage.
    Invulnerable,
    Player(GameMode),
}

#[derive(Clone, Copy)]
struct Scene {
    name: &'static str,
    bedrock: bool,
    held: Option<&'static Item>,
    enchantments: &'static [(&'static Enchantment, u16)],
    /// Ticks since the attacker's last attack (12 = fully charged for a sword or a fist).
    ticks: u32,
    sprinting: bool,
    /// Off the ground with this fall distance.
    falling: Option<f32>,
    effect: Option<(&'static StatusEffect, u8)>,
    victim: Victim,
    bystanders: bool,
    /// The attacker stands 1.2 blocks from the victim, inside the sweep box.
    beside: bool,
    /// How many times `attack` is called.
    hits: u8,
}

const SCENE: Scene = Scene {
    name: "",
    bedrock: false,
    held: None,
    enchantments: &[],
    ticks: 100,
    sprinting: false,
    falling: None,
    effect: None,
    victim: Victim::Zombie,
    bystanders: false,
    beside: false,
    hits: 1,
};

const fn scene(name: &'static str) -> Scene {
    Scene { name, ..SCENE }
}

const SWORD: Option<&Item> = Some(&Item::DIAMOND_SWORD);
const MACE: Option<&Item> = Some(&Item::MACE);

fn scenes() -> Vec<Scene> {
    let mut scenes = charge_scenes();
    scenes.extend(item_scenes());
    scenes.extend(enchantment_scenes());
    scenes.extend(mace_scenes());
    scenes.extend(victim_scenes());
    scenes
}

fn charge_scenes() -> Vec<Scene> {
    vec![
        scene("fist, full charge"),
        Scene {
            ticks: 0,
            ..scene("fist, just attacked")
        },
        Scene {
            ticks: 5,
            ..scene("fist, half charge")
        },
        Scene {
            ticks: 11,
            ..scene("fist, nearly charged")
        },
        Scene {
            held: Some(&Item::STICK),
            ticks: 5,
            ..scene("stick, five ticks")
        },
        Scene {
            held: SWORD,
            bystanders: true,
            ..scene("sword, full charge, bystanders")
        },
        Scene {
            held: SWORD,
            bystanders: true,
            beside: true,
            ..scene("sword sweep with the attacker beside the victim")
        },
        Scene {
            held: SWORD,
            ticks: 0,
            bystanders: true,
            ..scene("sword, just attacked, bystanders")
        },
        Scene {
            held: SWORD,
            ticks: 10,
            bystanders: true,
            ..scene("sword at ten ticks, bystanders")
        },
        Scene {
            sprinting: true,
            ..scene("fist while sprinting")
        },
        Scene {
            held: SWORD,
            sprinting: true,
            bystanders: true,
            ..scene("sword while sprinting, bystanders")
        },
        Scene {
            falling: Some(1.0),
            ..scene("fist while falling")
        },
        Scene {
            held: SWORD,
            falling: Some(1.0),
            bystanders: true,
            ..scene("sword while falling, bystanders")
        },
        Scene {
            falling: Some(0.0),
            ..scene("fist in the air, not falling")
        },
        Scene {
            sprinting: true,
            falling: Some(1.0),
            ..scene("fist while sprinting and falling")
        },
        Scene {
            ticks: 0,
            falling: Some(1.0),
            ..scene("fist while falling, just attacked")
        },
    ]
}

fn item_scenes() -> Vec<Scene> {
    vec![
        Scene {
            held: Some(&Item::IRON_AXE),
            ..scene("iron axe")
        },
        Scene {
            held: Some(&Item::IRON_AXE),
            ticks: 12,
            ..scene("iron axe at twelve ticks")
        },
        Scene {
            held: Some(&Item::SHEARS),
            ..scene("shears")
        },
        Scene {
            held: Some(&Item::COPPER_SPEAR),
            ..scene("spear, full charge")
        },
        Scene {
            held: Some(&Item::COPPER_SPEAR),
            ticks: 0,
            ..scene("spear, just attacked")
        },
        Scene {
            bedrock: true,
            held: Some(&Item::COPPER_SPEAR),
            ticks: 0,
            ..scene("bedrock spear, just attacked")
        },
        Scene {
            bedrock: true,
            held: SWORD,
            ticks: 0,
            bystanders: true,
            ..scene("bedrock sword, just attacked, bystanders")
        },
        Scene {
            bedrock: true,
            falling: Some(1.0),
            ..scene("bedrock fist while falling")
        },
    ]
}

fn enchantment_scenes() -> Vec<Scene> {
    vec![
        Scene {
            held: SWORD,
            enchantments: &[(&Enchantment::SHARPNESS, 5)],
            ..scene("sharpness V sword")
        },
        Scene {
            held: SWORD,
            enchantments: &[(&Enchantment::SHARPNESS, 5)],
            ticks: 5,
            ..scene("sharpness V sword, half charge")
        },
        Scene {
            held: SWORD,
            enchantments: &[(&Enchantment::SMITE, 3)],
            ..scene("smite III sword on a zombie")
        },
        Scene {
            held: SWORD,
            enchantments: &[(&Enchantment::SMITE, 3)],
            victim: Victim::Spider,
            ..scene("smite III sword on a spider")
        },
        Scene {
            held: SWORD,
            enchantments: &[(&Enchantment::BANE_OF_ARTHROPODS, 2)],
            victim: Victim::Spider,
            ..scene("bane II sword on a spider")
        },
        Scene {
            held: SWORD,
            enchantments: &[(&Enchantment::SWEEPING_EDGE, 3)],
            bystanders: true,
            ..scene("sweeping edge III, bystanders")
        },
        Scene {
            held: SWORD,
            enchantments: &[
                (&Enchantment::SWEEPING_EDGE, 1),
                (&Enchantment::SHARPNESS, 2),
            ],
            bystanders: true,
            ..scene("sweeping edge I and sharpness II, bystanders")
        },
        Scene {
            held: SWORD,
            enchantments: &[(&Enchantment::KNOCKBACK, 2)],
            ..scene("knockback II sword")
        },
        Scene {
            held: Some(&Item::STICK),
            enchantments: &[(&Enchantment::KNOCKBACK, 1)],
            sprinting: true,
            ..scene("knockback I stick while sprinting")
        },
        Scene {
            held: SWORD,
            enchantments: &[(&Enchantment::FIRE_ASPECT, 2)],
            ..scene("fire aspect II sword")
        },
        Scene {
            held: SWORD,
            enchantments: &[(&Enchantment::FIRE_ASPECT, 1)],
            victim: Victim::Invulnerable,
            ..scene("fire aspect I sword on an invulnerable zombie")
        },
        Scene {
            effect: Some((&StatusEffect::STRENGTH, 1)),
            ..scene("fist with strength II")
        },
        Scene {
            effect: Some((&StatusEffect::WEAKNESS, 0)),
            ..scene("fist with weakness I")
        },
        Scene {
            held: SWORD,
            effect: Some((&StatusEffect::WEAKNESS, 0)),
            ..scene("sword with weakness I")
        },
    ]
}

fn mace_scenes() -> Vec<Scene> {
    vec![
        Scene {
            held: MACE,
            ..scene("mace on the ground")
        },
        Scene {
            held: MACE,
            falling: Some(1.5),
            ..scene("mace falling 1.5")
        },
        Scene {
            held: MACE,
            falling: Some(3.0),
            ..scene("mace smash, fall 3")
        },
        Scene {
            held: MACE,
            falling: Some(10.0),
            ..scene("mace smash, fall 10")
        },
        Scene {
            held: MACE,
            falling: Some(5.0),
            ticks: 0,
            ..scene("mace smash, fall 5, just attacked")
        },
        Scene {
            held: MACE,
            falling: Some(70.0),
            victim: Victim::Golem,
            ..scene("mace smash, fall 70 on an iron golem")
        },
        Scene {
            held: MACE,
            enchantments: &[(&Enchantment::DENSITY, 3)],
            falling: Some(4.0),
            ..scene("density III mace smash, fall 4")
        },
        Scene {
            held: MACE,
            enchantments: &[(&Enchantment::WIND_BURST, 2)],
            falling: Some(4.0),
            ..scene("wind burst II mace smash, fall 4")
        },
        Scene {
            held: MACE,
            enchantments: &[(&Enchantment::WIND_BURST, 1)],
            falling: Some(4.0),
            victim: Victim::Invulnerable,
            ..scene("wind burst I mace smash on an invulnerable zombie")
        },
    ]
}

fn victim_scenes() -> Vec<Scene> {
    vec![
        Scene {
            victim: Victim::Golem,
            ..scene("fist on an iron golem")
        },
        Scene {
            held: SWORD,
            sprinting: true,
            victim: Victim::Golem,
            ..scene("sword while sprinting on an iron golem")
        },
        Scene {
            victim: Victim::Minecart,
            ..scene("fist on a minecart")
        },
        Scene {
            held: SWORD,
            bystanders: true,
            victim: Victim::Minecart,
            ..scene("sword on a minecart, bystanders")
        },
        Scene {
            held: SWORD,
            falling: Some(1.0),
            victim: Victim::Minecart,
            ..scene("sword while falling on a minecart")
        },
        Scene {
            held: SWORD,
            victim: Victim::Invulnerable,
            ..scene("sword on an invulnerable zombie")
        },
        Scene {
            held: SWORD,
            victim: Victim::Player(GameMode::Survival),
            ..scene("sword on a survival player")
        },
        Scene {
            held: SWORD,
            sprinting: true,
            victim: Victim::Player(GameMode::Survival),
            ..scene("sword while sprinting on a survival player")
        },
        Scene {
            held: SWORD,
            victim: Victim::Player(GameMode::Creative),
            ..scene("sword on a creative player")
        },
        Scene {
            held: SWORD,
            hits: 2,
            ..scene("sword twice")
        },
        Scene {
            hits: 2,
            ..scene("fist twice")
        },
    ]
}

const fn position_of(index: usize) -> BlockPos {
    BlockPos::new(index as i32 * 16 + 200, 64, 40)
}

const fn mob_id(index: usize, slot: i32) -> i32 {
    60_000 + index as i32 * 8 + slot
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Packet ids whose bytes are random like in vanilla: Add Entity (UUID) and the two sound
/// packets (seed).
const RANDOM_PACKETS: [u8; 3] = [1, 118, 119];

fn packets(sent: &[bytes::Bytes], masked: bool) -> String {
    sent.iter()
        .map(|p| match p.first() {
            Some(id) if masked && RANDOM_PACKETS.contains(id) => format!("{}:id{id}", p.len()),
            _ => format!("{}:{:08x}", p.len(), fnv(p) as u32),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn vector(v: Vector3<f64>) -> String {
    format!("{:.6},{:.6},{:.6}", v.x, v.y, v.z)
}

fn mob(
    world: &Arc<World>,
    index: usize,
    slot: i32,
    kind: &'static EntityType,
    at: Vector3<f64>,
) -> Result<Arc<dyn EntityBase>, String> {
    let id = mob_id(index, slot);
    let entity = Entity::from_uuid_with_id(
        id,
        Uuid::from_u128(0x6d6f_6200_0000 + id as u128),
        world.clone(),
        at,
        kind,
    );
    let entity: Arc<dyn EntityBase> = if kind == &EntityType::MINECART {
        Arc::new(MinecartEntity::new(entity))
    } else {
        Arc::new(LivingEntity::new(entity))
    };
    if world.spawn_entity(entity.clone()) {
        Ok(entity)
    } else {
        Err(format!("mob {id} not spawned"))
    }
}

fn place(player: &Player, at: Vector3<f64>) {
    player.get_entity().set_pos(at);
    player.watched_section.store(Cylindrical::new(
        BlockPos::floored_v(at).chunk_position(),
        NonZero::<u8>::MIN.saturating_add(1),
    ));
}

/// Gives the attacker the scene's item, charge, motion, effect and facing.
fn equip(attacker: &Player, scene: &Scene) {
    if let Some(item) = scene.held {
        let mut stack = ItemStack::new(1, item);
        for (enchantment, level) in scene.enchantments {
            stack.add_enchantment(enchantment, *level);
        }
        attacker.inventory().set_stack_in_hand(Hand::Right, stack);
    }
    attacker
        .last_attacked_ticks
        .store(scene.ticks, Ordering::Relaxed);
    let entity = attacker.get_entity();
    entity.yaw.store(30.0);
    entity.tick_count.store(37, Ordering::Relaxed);
    entity.velocity.store(Vector3::new(0.25, -0.125, 0.5));
    entity.set_sprinting(scene.sprinting);
    entity
        .on_ground
        .store(scene.falling.is_none(), Ordering::Relaxed);
    attacker
        .living_entity
        .fall_distance
        .store(scene.falling.unwrap_or(0.0));
    if let Some((effect_type, amplifier)) = scene.effect {
        attacker.living_entity.add_effect(Effect {
            effect_type,
            duration: 2000,
            amplifier,
            ambient: false,
            show_particles: false,
            show_icon: false,
            blend: false,
        });
    }
}

fn victim_text(victim: &Arc<dyn EntityBase>) -> String {
    let entity = victim.get_entity();
    let mut text = format!(
        "velocity {} fire {} removed {}",
        vector(entity.velocity.load()),
        entity.fire_ticks.load(Ordering::Relaxed),
        entity.removed.load(Ordering::Relaxed),
    );
    if let Some(living) = victim.get_living_entity() {
        let _ = write!(
            text,
            " health {} last damage {} hurt cooldown {}",
            living.health.load(),
            living.last_damage_taken.load(),
            living.hurt_cooldown.load(Ordering::Relaxed),
        );
    }
    text
}

fn attacker_text(attacker: &Player, used: &'static Item) -> String {
    let living = &attacker.living_entity;
    let stats = attacker
        .stats
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let held = attacker.inventory().held_item();
    format!(
        "velocity {} fall {} held {}x{} dmg {} exhaustion {} used {} ticks {} last attacking {} at {}",
        vector(attacker.get_entity().velocity.load()),
        living.fall_distance.load(),
        held.item.registry_key,
        held.item_count,
        held.get_damage(),
        attacker.hunger_manager.exhaustion.load(),
        stats.get(StatisticCategory::Used, used.id as i32),
        attacker.last_attacked_ticks.load(Ordering::Relaxed),
        living.last_attacking_id.load(Ordering::Relaxed),
        living.last_attack_time.load(Ordering::Relaxed),
    )
}

/// The attacker as a Java or a Bedrock player, with a function that drains what it was sent.
type Drain = Box<dyn FnMut() -> Vec<bytes::Bytes>>;

async fn attacker_of(world: &Arc<World>, scene: &Scene) -> Result<(Arc<Player>, Drain), String> {
    if scene.bedrock {
        let (player, mut tap) =
            test_support::bedrock_player_as(world, GameMode::Survival, &ATTACKER).await?;
        Ok((player, Box::new(move || tap.drain())))
    } else {
        let (player, mut tap) = test_support::java_player_as(world, GameMode::Survival, &ATTACKER)
            .ok_or("no Java packet tap")?;
        Ok((player, Box::new(move || tap.drain())))
    }
}

async fn run(world: &Arc<World>, index: usize, scene: &Scene) -> Result<String, String> {
    let base = position_of(index);
    let at = Vector3::new(f64::from(base.0.x) + 0.5, 64.0, f64::from(base.0.z) + 0.5);
    let victim_at = at.add(&Vector3::new(0.0, 0.0, 2.0));
    let (attacker, mut attacker_drain) = attacker_of(world, scene).await?;
    let attacker_at = if scene.beside {
        at.add(&Vector3::new(0.0, 0.0, 0.8))
    } else {
        at
    };
    place(&attacker, attacker_at);
    equip(&attacker, scene);
    let (watcher, mut watcher_tap) =
        test_support::java_player_as(world, GameMode::Survival, &WATCHER)
            .ok_or("no Java packet tap")?;
    place(&watcher, at.add(&Vector3::new(-6.0, 0.0, 0.0)));
    let mut victim_tap = None;
    let victim: Arc<dyn EntityBase> = match scene.victim {
        Victim::Player(gamemode) => {
            let (player, tap) = test_support::java_player_as(world, gamemode, &VICTIM)
                .ok_or("no Java packet tap")?;
            place(&player, victim_at);
            world.add_player(&player)?;
            victim_tap = Some(tap);
            player
        }
        Victim::Zombie => mob(world, index, 0, &EntityType::ZOMBIE, victim_at)?,
        Victim::Spider => mob(world, index, 0, &EntityType::SPIDER, victim_at)?,
        Victim::Golem => mob(world, index, 0, &EntityType::IRON_GOLEM, victim_at)?,
        Victim::Minecart => mob(world, index, 0, &EntityType::MINECART, victim_at)?,
        Victim::Invulnerable => {
            let zombie = mob(world, index, 0, &EntityType::ZOMBIE, victim_at)?;
            zombie
                .get_entity()
                .invulnerable
                .store(true, Ordering::Relaxed);
            zombie
        }
    };
    let bystanders = if scene.bystanders {
        vec![
            mob(
                world,
                index,
                1,
                &EntityType::ZOMBIE,
                victim_at.add(&Vector3::new(0.9, 0.0, 0.3)),
            )?,
            mob(
                world,
                index,
                2,
                &EntityType::ZOMBIE,
                victim_at.add(&Vector3::new(2.5, 0.0, 0.0)),
            )?,
            mob(
                world,
                index,
                3,
                &EntityType::ZOMBIE,
                victim_at.add(&Vector3::new(0.0, 1.2, 0.0)),
            )?,
        ]
    } else {
        Vec::new()
    };
    world.add_player(&attacker)?;
    world.add_player(&watcher)?;
    drop(attacker_drain());
    drop(watcher_tap.drain());
    if let Some(tap) = victim_tap.as_mut() {
        drop(tap.drain());
    }
    let used = attacker.inventory().held_item().item;
    for _ in 0..scene.hits {
        attacker.attack(&victim);
    }
    let others: Vec<String> = bystanders
        .iter()
        .map(|b| {
            b.get_living_entity()
                .map_or_else(|| "-".to_string(), |l| l.health.load().to_string())
        })
        .collect();
    let mut line = format!(
        "{}: victim [{}] bystanders {others:?} | attacker [{}] | sent [{}] | watcher [{}]",
        scene.name,
        victim_text(&victim),
        attacker_text(&attacker, used),
        packets(&attacker_drain(), !scene.bedrock),
        packets(&watcher_tap.drain(), true),
    );
    if let Some(tap) = victim_tap.as_mut() {
        let _ = write!(line, " | victim [{}]", packets(&tap.drain(), true));
    }
    line.push('\n');
    Ok(line)
}

#[tokio::test(flavor = "multi_thread")]
async fn player_attack_is_the_same() -> TestResult {
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
        out.push_str(&run(&world, index, scene).await?);
        for player in world.players.load().iter() {
            world.remove_player(player, false).await;
        }
    }
    server.stop();
    script_golden::bless_or_compare("player_attack.txt", &out)
}
