//! A record of what `LivingEntity::damage_with_context` does, made before the function was split
//! (`tests/golden/living_damage.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! The test starts a real [`Server`] (the later half of the function stops without one) and
//! gives thousands of seeded hits to freshly built victims: zombies, cows, iron golems and a
//! living entity of the player's type, with armour, protection enchantments, resistance, fire
//! resistance, absorption, a raised shield, a damaged shield, a hurt cooldown, and attackers
//! that are living entities with an axe, a sword or nothing. After every hit the visible state
//! of the victim and the attacker's shield goes into a running hash; a line is written every
//! 250 hits, so a difference shows where it started, and the counters show which branches the
//! sequence reached.

use std::borrow::Cow;
use std::fmt::Write;
use std::sync::Arc;

use pumpkin_config::{AdvancedConfiguration, BasicConfiguration, TelemetryConfig};
use pumpkin_data::Enchantment;
use pumpkin_data::damage::DamageType;
use pumpkin_data::data_component_impl::{DamageImpl, EnchantmentsImpl, EquipmentSlot};
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::potion::Effect;
use pumpkin_util::Hand;
use pumpkin_util::math::vector3::Vector3;

use crate::data::VanillaData;
use crate::entity::{Entity, EntityBase, living::LivingEntity};
use crate::server::Server;
use crate::world::World;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// FNV-1a over a running state: a hash that does not change with the Rust version.
struct Fnv(u64);

impl Fnv {
    fn text(&mut self, text: &str) {
        for byte in text.bytes() {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
}

/// A fixed pseudo-random sequence (64-bit linear congruential generator).
struct Sequence(u64);

impl Sequence {
    fn next(&mut self, below: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % below
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.next(items.len() as u64) as usize]
    }
}

async fn server(folder: &tempfile::TempDir) -> Arc<Server> {
    let basic = BasicConfiguration {
        default_level_name: folder.path().join("world").to_string_lossy().into_owned(),
        ..BasicConfiguration::default()
    };
    let telemetry = TelemetryConfig {
        enabled: false,
        ..TelemetryConfig::default()
    };
    let vanilla = VanillaData {
        banned_ip_list: Default::default(),
        banned_player_list: Default::default(),
        operator_config: Default::default(),
        user_cache: Default::default(),
        whitelist_config: Default::default(),
    };
    Server::new(
        basic,
        AdvancedConfiguration::default(),
        telemetry,
        vanilla,
        Vec::new(),
    )
    .await
    .expect("a server starts on an empty folder")
}

fn living(world: &Arc<World>, kind: &'static EntityType, x: f64, z: f64) -> LivingEntity {
    LivingEntity::new(Entity::new(
        Arc::clone(world),
        Vector3::new(x, 64.0, z),
        kind,
    ))
}

const DAMAGE_TYPES: [DamageType; 22] = [
    DamageType::GENERIC,
    DamageType::IN_FIRE,
    DamageType::LAVA,
    DamageType::ON_FIRE,
    DamageType::FREEZE,
    DamageType::CACTUS,
    DamageType::SWEET_BERRY_BUSH,
    DamageType::CAMPFIRE,
    DamageType::HOT_FLOOR,
    DamageType::STALAGMITE,
    DamageType::MOB_ATTACK,
    DamageType::PLAYER_ATTACK,
    DamageType::ARROW,
    DamageType::OUT_OF_WORLD,
    DamageType::GENERIC_KILL,
    DamageType::WIND_CHARGE,
    DamageType::FALL,
    DamageType::MAGIC,
    DamageType::PLAYER_EXPLOSION,
    DamageType::FIREBALL,
    DamageType::THORNS,
    DamageType::SONIC_BOOM,
];

const AMOUNTS: [f32; 11] = [-3.0, 0.0, 0.5, 1.0, 2.0, 3.5, 4.0, 7.0, 12.0, 20.0, 100.0];

/// Puts armour, enchantments, effects and a shield on a victim.
fn dress(victim: &LivingEntity, sequence: &mut Sequence) {
    let pieces = [
        (EquipmentSlot::HEAD, &Item::DIAMOND_HELMET),
        (EquipmentSlot::CHEST, &Item::IRON_CHESTPLATE),
        (EquipmentSlot::LEGS, &Item::NETHERITE_LEGGINGS),
        (EquipmentSlot::FEET, &Item::LEATHER_BOOTS),
    ];
    for (slot, item) in pieces {
        if sequence.next(3) == 0 {
            continue;
        }
        let mut stack = ItemStack::new(1, item);
        if sequence.next(2) == 0 {
            let enchantment = if sequence.next(2) == 0 {
                &Enchantment::PROTECTION
            } else {
                &Enchantment::FIRE_PROTECTION
            };
            let level = sequence.next(4) as i32 + 1;
            stack.set_data_component(EnchantmentsImpl {
                enchantment: Cow::Owned(vec![(enchantment, level)]),
            });
        }
        victim
            .entity_equipment
            .lock()
            .expect("equipment lock")
            .put(&slot, stack);
    }
    if sequence.next(3) == 0 {
        victim.add_effect(Effect {
            effect_type: &StatusEffect::RESISTANCE,
            duration: 2000,
            amplifier: sequence.next(4) as u8,
            ambient: false,
            show_particles: false,
            show_icon: false,
            blend: false,
        });
    }
    if sequence.next(4) == 0 {
        victim.add_effect(Effect {
            effect_type: &StatusEffect::FIRE_RESISTANCE,
            duration: 2000,
            amplifier: 0,
            ambient: false,
            show_particles: false,
            show_icon: false,
            blend: false,
        });
    }
    raise_shield(victim, sequence);
}

/// A shield in one hand, raised long enough to block, or just raised, or worn out.
fn raise_shield(victim: &LivingEntity, sequence: &mut Sequence) {
    if sequence.next(3) != 0 {
        return;
    }
    let (hand, slot) = if sequence.next(2) == 0 {
        (Hand::Left, EquipmentSlot::OFF_HAND)
    } else {
        (Hand::Right, EquipmentSlot::MAIN_HAND)
    };
    let mut shield = ItemStack::new(1, &Item::SHIELD);
    if sequence.next(3) == 0 {
        shield.set_data_component(DamageImpl { damage: 335 });
    }
    victim
        .entity_equipment
        .lock()
        .expect("equipment lock")
        .put(&slot, shield.clone());
    let duration = sequence.pick(&[0, 0, 71_998, 71_990]);
    victim.set_active_hand(hand, shield, duration);
}

fn victim(world: &Arc<World>, sequence: &mut Sequence) -> LivingEntity {
    let kind = sequence.pick(&[
        &EntityType::ZOMBIE,
        &EntityType::COW,
        &EntityType::IRON_GOLEM,
        &EntityType::PLAYER,
        &EntityType::ZOMBIE,
    ]);
    let victim = living(world, kind, 0.5, 0.5);
    victim.set_health(sequence.pick(&[1.0, 3.0, 6.0, 10.0, 20.0, 20.0]));
    victim
        .absorption
        .store(sequence.pick(&[0.0, 0.0, 2.0, 4.0]));
    victim.hurt_cooldown.store(
        sequence.pick(&[0, 0, 5, 15, 20]),
        std::sync::atomic::Ordering::Relaxed,
    );
    victim
        .last_damage_taken
        .store(sequence.pick(&[0.0, 3.0, 6.0]));
    victim
        .entity
        .yaw
        .store(sequence.pick(&[0.0, 90.0, 180.0, 270.0]));
    dress(&victim, sequence);
    victim
}

/// A living entity that hits: bare handed, with an axe (which disables shields) or a sword.
fn attacker(world: &Arc<World>, sequence: &mut Sequence) -> LivingEntity {
    let kind = sequence.pick(&[
        &EntityType::ZOMBIE,
        &EntityType::SKELETON,
        &EntityType::WARDEN,
    ]);
    let attacker = living(world, kind, 2.5, 0.5);
    match sequence.next(3) {
        0 => {}
        1 => {
            attacker
                .entity_equipment
                .lock()
                .expect("equipment lock")
                .put(
                    &EquipmentSlot::MAIN_HAND,
                    ItemStack::new(1, &Item::DIAMOND_AXE),
                );
        }
        _ => {
            attacker
                .entity_equipment
                .lock()
                .expect("equipment lock")
                .put(
                    &EquipmentSlot::MAIN_HAND,
                    ItemStack::new(1, &Item::DIAMOND_SWORD),
                );
        }
    }
    attacker
}

/// What a hit leaves behind, as text. Entity ids are left out because they depend on how many
/// entities other tests made.
fn state(victim: &LivingEntity, hit: bool) -> String {
    use std::sync::atomic::Ordering::Relaxed;
    let mut out = String::new();
    let equipment = victim.entity_equipment.lock().expect("equipment lock");
    let slots = [
        EquipmentSlot::MAIN_HAND,
        EquipmentSlot::OFF_HAND,
        EquipmentSlot::HEAD,
        EquipmentSlot::CHEST,
    ];
    let items: Vec<String> = slots
        .iter()
        .map(|slot| {
            let stack = equipment.get(slot);
            format!("{}:{}", stack.item.registry_key, stack.get_damage())
        })
        .collect();
    let killer = victim
        .combat_tracker
        .lock()
        .expect("tracker lock")
        .get_killer_entry()
        .map(|entry| {
            (
                entry.damage_type.id,
                entry.damage.to_bits(),
                entry.attacker_is_living,
            )
        });
    let velocity = victim.entity.velocity.load();
    let _ = write!(
        out,
        "hit={hit} health={} abs={} cooldown={} last={} dead={} idle={} type={:?} attacker={} \
         velocity=({:.5},{:.5},{:.5}) hand={:?} use={} items=[{}] killer={killer:?}",
        victim.health.load(),
        victim.absorption.load(),
        victim.hurt_cooldown.load(Relaxed),
        victim.last_damage_taken.load(),
        victim.dead.load(Relaxed),
        victim.no_action_time.load(Relaxed),
        victim.last_damage_type.lock().expect("type lock"),
        victim.last_damage_attacker.load(Relaxed) != 0,
        velocity.x,
        velocity.y,
        velocity.z,
        victim.active_hand.lock().expect("hand lock"),
        victim.item_use_time.load(Relaxed),
        items.join(","),
    );
    out
}

/// Runs the hits and returns the report.
fn run(world: &Arc<World>) -> String {
    let mut sequence = Sequence(0x6b_7661_7369);
    let mut hash = Fnv(0xcbf2_9ce4_8422_2325);
    let mut report = String::new();
    let (mut landed, mut refused, mut cases) = (0_u32, 0_u32, 0_u32);
    for case in 0..6_000_u32 {
        let victim = victim(world, &mut sequence);
        let first = attacker(world, &mut sequence);
        let second = attacker(world, &mut sequence);
        for _ in 0..=sequence.next(3) {
            let amount = sequence.pick(&AMOUNTS);
            let damage_type = sequence.pick(&DAMAGE_TYPES);
            let position = match sequence.next(3) {
                0 => None,
                1 => Some(Vector3::new(2.5, 64.0, 0.5)),
                _ => Some(Vector3::new(-2.5, 64.0, 0.5)),
            };
            let (source, cause): (Option<&dyn EntityBase>, Option<&dyn EntityBase>) =
                match sequence.next(5) {
                    0 | 1 => (None, None),
                    2 => (Some(&first), None),
                    3 => (Some(&first), Some(&second)),
                    _ => (None, Some(&second)),
                };
            let hit =
                victim.damage_with_context(&victim, amount, damage_type, position, source, cause);
            if hit {
                landed += 1;
            } else {
                refused += 1;
            }
            cases += 1;
            hash.text(&format!("{case}:{}", state(&victim, hit)));
        }
        if case % 250 == 249 {
            let _ = writeln!(report, "case {case}: {:016x}", hash.0);
        }
    }
    let _ = writeln!(report, "hits {cases}: landed {landed}, refused {refused}");
    report
}

#[tokio::test(flavor = "multi_thread")]
async fn living_damage() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = server(&folder).await;
    let world = server
        .worlds
        .load()
        .first()
        .cloned()
        .ok_or("the server has no world")?;
    let now = run(&world);
    server.stop();

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("living_damage.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, now);
    Ok(())
}
