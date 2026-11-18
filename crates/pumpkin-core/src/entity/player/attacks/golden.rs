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

use pumpkin_data::{AttributeModifierSlot, Enchantment};
use std::borrow::Cow;

use pumpkin_config::world::LevelConfig;
use pumpkin_config::{AdvancedConfiguration, BasicConfiguration, TelemetryConfig};
use pumpkin_data::attributes::Attributes;
use pumpkin_data::data_component_impl::{
    AttributeModifiersImpl, EquipmentSlot, Modifier, Operation,
};
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
use crate::data::VanillaData;
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::living::LivingEntity;
use crate::entity::player::Player;
use crate::entity::player::statistics::StatisticCategory;
use crate::entity::vehicle::minecart::MinecartEntity;
use crate::entity::{Entity, EntityBase};
use crate::net::java::PacketTap;
use crate::server::Server;
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
    /// Appended: the held item's attribute modifiers are applied, as the tick does when the
    /// held item changes (a sword then hits for its own damage).
    armed: bool,
    /// Appended: bystanders on every side of the victim and below it instead of the three.
    ring: bool,
    /// Appended: a stick carrying hand-made attribute modifiers.
    custom: Option<Custom>,
    /// Appended: the scene runs on a server whose `pvp.knockback` setting is off.
    no_knockback: bool,
    /// Appended: a second effect.
    also: Option<(&'static StatusEffect, u8)>,
    /// Appended scenes show sound packets without their random seed and end their line with
    /// how many advancements changed.
    extra: bool,
}

/// Hand-made attribute modifiers on a stick (appended scenes).
#[derive(Clone, Copy)]
enum Custom {
    /// Attack speed -3 (add value), then armor +2 (add value).
    SpeedThenArmor,
    /// Attack speed +50 % of the base (multiplied base).
    MultipliedSpeed,
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
    armed: false,
    ring: false,
    custom: None,
    no_knockback: false,
    also: None,
    extra: false,
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
    scenes.extend(armed_scenes());
    scenes.extend(widened_scenes());
    scenes
}

const WIDE: Scene = Scene {
    extra: true,
    ..SCENE
};

const fn wide(name: &'static str) -> Scene {
    Scene { name, ..WIDE }
}

const fn armed(name: &'static str) -> Scene {
    Scene {
        name,
        armed: true,
        ..WIDE
    }
}

/// Appended: the held item's own damage and speed, sweep bystanders all around.
fn armed_scenes() -> Vec<Scene> {
    vec![
        Scene {
            held: SWORD,
            ring: true,
            ..armed("armed sword, full charge, ring")
        },
        Scene {
            held: SWORD,
            enchantments: &[(&Enchantment::SWEEPING_EDGE, 3)],
            ring: true,
            ..armed("armed sweeping edge III sword, ring")
        },
        Scene {
            held: SWORD,
            ticks: 5,
            ..armed("armed sword, half charge")
        },
        Scene {
            held: SWORD,
            falling: Some(1.0),
            ..armed("armed sword while falling")
        },
        Scene {
            held: SWORD,
            effect: Some((&StatusEffect::WEAKNESS, 0)),
            ..armed("armed sword with weakness I")
        },
        Scene {
            held: SWORD,
            effect: Some((&StatusEffect::WEAKNESS, 1)),
            ..armed("armed sword with weakness II")
        },
        Scene {
            held: SWORD,
            effect: Some((&StatusEffect::STRENGTH, 2)),
            also: Some((&StatusEffect::WEAKNESS, 1)),
            ..armed("armed sword with strength III and weakness II")
        },
        Scene {
            held: MACE,
            falling: Some(3.0),
            ..armed("armed mace smash, fall 3")
        },
        Scene {
            held: MACE,
            falling: Some(5.0),
            ..armed("armed mace smash, fall 5")
        },
        Scene {
            held: MACE,
            falling: Some(10.0),
            ..armed("armed mace smash, fall 10")
        },
    ]
}

/// Appended: charge edges, hand-made modifiers, the overkill advancement, the no-damage sound
/// and a server without knockback.
fn widened_scenes() -> Vec<Scene> {
    vec![
        Scene {
            held: Some(&Item::STICK),
            ticks: 2,
            ..wide("stick, two ticks")
        },
        Scene {
            held: Some(&Item::COPPER_SPEAR),
            ticks: 11,
            ..wide("spear at eleven ticks")
        },
        Scene {
            held: Some(&Item::COPPER_SPEAR),
            ticks: 13,
            ..wide("spear at thirteen ticks")
        },
        Scene {
            held: Some(&Item::STICK),
            custom: Some(Custom::SpeedThenArmor),
            ticks: 2,
            ..wide("stick with speed then armor modifiers, two ticks")
        },
        Scene {
            held: Some(&Item::STICK),
            custom: Some(Custom::MultipliedSpeed),
            ticks: 2,
            ..wide("stick with a multiplied speed modifier, two ticks")
        },
        Scene {
            held: MACE,
            enchantments: &[(&Enchantment::DENSITY, 1)],
            falling: Some(49.5),
            victim: Victim::Golem,
            ..wide("density I mace smash for exactly 100")
        },
        Scene {
            held: MACE,
            enchantments: &[(&Enchantment::DENSITY, 1)],
            falling: Some(49.0),
            victim: Victim::Golem,
            ..wide("density I mace smash for 99")
        },
        Scene {
            effect: Some((&StatusEffect::STRENGTH, 33)),
            victim: Victim::Golem,
            ..wide("fist with strength XXXIV for 103")
        },
        Scene {
            held: SWORD,
            victim: Victim::Invulnerable,
            ..wide("sword on an invulnerable zombie, sounds")
        },
        Scene {
            sprinting: true,
            no_knockback: true,
            ..wide("fist while sprinting, knockback off")
        },
        Scene {
            held: SWORD,
            enchantments: &[(&Enchantment::KNOCKBACK, 2)],
            no_knockback: true,
            ..wide("knockback II sword, knockback off")
        },
    ]
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

/// Like [`packets`], for appended scenes: sound packets are hashed without their trailing
/// random pitch and seed (4 + 8 bytes), so the sound itself shows.
fn packets_unseeded(sent: &[bytes::Bytes]) -> String {
    sent.iter()
        .map(|p| match p.first() {
            Some(1) => format!("{}:id1", p.len()),
            Some(id @ (118 | 119)) if p.len() > 12 => {
                format!("{}:id{id}:{:08x}", p.len(), fnv(&p[..p.len() - 12]) as u32)
            }
            _ => format!("{}:{:08x}", p.len(), fnv(p) as u32),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// What the attacker was sent. A Bedrock attacker's inventory slot updates (game packet 50,
/// after the 0xfe batch byte and a one-byte length) show only as `slot`: they carry a stack id
/// that depends on the tests run before.
fn attacker_sent(sent: &[bytes::Bytes], scene: &Scene) -> String {
    if scene.bedrock {
        return sent
            .iter()
            .map(|p| {
                if p.get(2) == Some(&50) {
                    "slot".to_string()
                } else {
                    format!("{}:{:08x}", p.len(), fnv(p) as u32)
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
    }
    sent_text(sent, true, scene.extra)
}

fn sent_text(sent: &[bytes::Bytes], masked: bool, extra: bool) -> String {
    if extra {
        packets_unseeded(sent)
    } else {
        packets(sent, masked)
    }
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

fn custom_modifiers(custom: Custom) -> AttributeModifiersImpl {
    let speed = |amount, operation| Modifier {
        r#type: &Attributes::ATTACK_SPEED,
        id: "minecraft:base_attack_speed",
        amount,
        operation,
        slot: AttributeModifierSlot::MainHand,
    };
    let modifiers = match custom {
        Custom::SpeedThenArmor => vec![
            speed(-3.0, Operation::AddValue),
            Modifier {
                r#type: &Attributes::ARMOR,
                id: "minecraft:golden_test_armor",
                amount: 2.0,
                operation: Operation::AddValue,
                slot: AttributeModifierSlot::MainHand,
            },
        ],
        Custom::MultipliedSpeed => vec![speed(0.5, Operation::AddMultipliedBase)],
    };
    AttributeModifiersImpl {
        attribute_modifiers: Cow::Owned(modifiers),
    }
}

/// Gives the attacker the scene's item, charge, motion, effect and facing.
fn equip(attacker: &Player, scene: &Scene) {
    if let Some(item) = scene.held {
        let mut stack = ItemStack::new(1, item);
        for (enchantment, level) in scene.enchantments {
            stack.add_enchantment(enchantment, *level);
        }
        if let Some(custom) = scene.custom {
            stack.set_data_component(custom_modifiers(custom));
        }
        if scene.armed {
            attacker
                .living_entity
                .apply_and_send_equipment_attribute_modifiers(&[(
                    EquipmentSlot::MAIN_HAND,
                    stack.clone(),
                )]);
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
    for (effect_type, amplifier) in scene.effect.into_iter().chain(scene.also) {
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

/// Appended: bystanders just inside the sweep box on the -x, -z and +z sides, and one below
/// whose top is inside the box only because the box reaches half a block down.
fn ring(
    world: &Arc<World>,
    index: usize,
    victim_at: Vector3<f64>,
) -> Result<Vec<Arc<dyn EntityBase>>, String> {
    let offsets = [
        Vector3::new(-1.1, 0.0, 0.0),
        Vector3::new(0.0, 0.0, -1.1),
        Vector3::new(0.0, 0.0, 1.1),
        Vector3::new(0.0, -2.2, 0.0),
    ];
    let mut found = Vec::new();
    for (slot, offset) in (4..).zip(offsets) {
        found.push(mob(
            world,
            index,
            slot,
            &EntityType::ZOMBIE,
            victim_at.add(&offset),
        )?);
    }
    Ok(found)
}

/// The scene's victim, with the packet tap of a victim player.
fn victim_of(
    world: &Arc<World>,
    index: usize,
    scene: &Scene,
    victim_at: Vector3<f64>,
) -> Result<(Arc<dyn EntityBase>, Option<PacketTap>), String> {
    let victim: Arc<dyn EntityBase> = match scene.victim {
        Victim::Player(gamemode) => {
            let (player, tap) = test_support::java_player_as(world, gamemode, &VICTIM)
                .ok_or("no Java packet tap")?;
            place(&player, victim_at);
            world.add_player(&player)?;
            return Ok((player, Some(tap)));
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
    Ok((victim, None))
}

/// The scene's bystanders: the ring, the three, or none.
fn bystanders_of(
    world: &Arc<World>,
    index: usize,
    scene: &Scene,
    victim_at: Vector3<f64>,
) -> Result<Vec<Arc<dyn EntityBase>>, String> {
    if scene.ring {
        return ring(world, index, victim_at);
    }
    if !scene.bystanders {
        return Ok(Vec::new());
    }
    let offsets = [
        Vector3::new(0.9, 0.0, 0.3),
        Vector3::new(2.5, 0.0, 0.0),
        Vector3::new(0.0, 1.2, 0.0),
    ];
    let mut found = Vec::new();
    for (slot, offset) in (1..).zip(offsets) {
        found.push(mob(
            world,
            index,
            slot,
            &EntityType::ZOMBIE,
            victim_at.add(&offset),
        )?);
    }
    Ok(found)
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
    if scene.extra {
        // As the server's login does, so that awarded criteria are recorded.
        attacker
            .advancements
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .player = Arc::downgrade(&attacker);
    }
    let (watcher, mut watcher_tap) =
        test_support::java_player_as(world, GameMode::Survival, &WATCHER)
            .ok_or("no Java packet tap")?;
    place(&watcher, at.add(&Vector3::new(-6.0, 0.0, 0.0)));
    let (victim, mut victim_tap) = victim_of(world, index, scene, victim_at)?;
    let bystanders = bystanders_of(world, index, scene, victim_at)?;
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
        attacker_sent(&attacker_drain(), scene),
        sent_text(&watcher_tap.drain(), true, scene.extra),
    );
    if let Some(tap) = victim_tap.as_mut() {
        let _ = write!(
            line,
            " | victim [{}]",
            sent_text(&tap.drain(), true, scene.extra)
        );
    }
    if scene.extra {
        let changed = attacker
            .advancements
            .try_lock()
            .map_or(usize::MAX, |advancements| {
                advancements.progress_changed.len()
            });
        let _ = write!(line, " | advancements changed {changed}");
    }
    line.push('\n');
    Ok(line)
}

/// Appended: a real server like [`test_support::server`] whose `pvp.knockback` setting is off.
async fn server_without_knockback(
    folder: &tempfile::TempDir,
) -> Result<Arc<Server>, Box<dyn std::error::Error>> {
    let mut basic = BasicConfiguration {
        default_level_name: folder.path().join("world").to_string_lossy().into_owned(),
        ..BasicConfiguration::default()
    };
    basic.seed.0 = 20_261_112;
    let mut advanced = AdvancedConfiguration {
        world: LevelConfig {
            stronghold_search_at_load: false,
            ..LevelConfig::default()
        },
        ..AdvancedConfiguration::default()
    };
    advanced.pvp.knockback = false;
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

#[tokio::test(flavor = "multi_thread")]
async fn player_attack_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let other_folder = tempfile::tempdir()?;
    let no_knockback = server_without_knockback(&other_folder).await?;
    let no_knockback_world = no_knockback.worlds.load()[0].clone();
    let scenes = scenes();
    load_chunks(&world, scenes.len());
    load_chunks(&no_knockback_world, scenes.len());
    let mut out = String::new();
    for (index, scene) in scenes.iter().enumerate() {
        let world = if scene.no_knockback {
            &no_knockback_world
        } else {
            &world
        };
        out.push_str(&run(world, index, scene).await?);
        for player in world.players.load().iter() {
            world.remove_player(player, false).await;
        }
    }
    server.stop();
    no_knockback.stop();
    script_golden::bless_or_compare("player_attack.txt", &out)
}
