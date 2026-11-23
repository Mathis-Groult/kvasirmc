//! A record of what `LivingEntity::on_death` does, made before the function was split
//! (`tests/golden/living_death.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene is built in its own loaded area of a real server's world (stone floor at y 63) with
//! a survival watcher in a corner (its packet queue is read), a victim (zombie, cow, baby cow,
//! pig, creeper, survival player, or a bare living armor stand) and up to two attackers:
//! actor A, a survival player holding nothing, dirt, a looting stick or a fire-aspect stick, and
//! actor B, a zombie or a skeleton. The scene says which of them is the damage source, which the
//! cause and who holds the kill credit (the last player to hurt it with a memory time, an offline
//! player, or the last mob to hurt it), the damage type, the game rules (mob drops, keep
//! inventory, death messages), and whether the victim burns, carries the oozing effect, is dead
//! already, or is missing from the world. Mob victims get a fixed loot seed and drop chances that
//! make equipment drops certain (above 1), impossible (0) or dependent on the looting level
//! alone (0 plus looting 100).
//!
//! `on_death` is called directly (twice in one scene). Recorded: whether the victim is dead, its
//! pose, health, remaining effects, movement input and jumping flag, every entity the death added
//! to the world (items with their stack and block, experience orbs with their amount and exact
//! position, slimes), the statistics of every player in the scene, a player victim's inventory,
//! and what the watcher and the players were sent (sound and Add Entity packets by length only:
//! their bytes are random like in vanilla).
//!
//! Scenes with `trials` kill that many fresh victims one after the other and add up what they
//! dropped: a drop chance of 0 stays 0 only when the looting level is really 0.
//!
//! `KVASIR_DUMP=<file>` appends every scene's text as it is recorded.

use std::fmt::Write;
use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

use pumpkin_data::Block;
use pumpkin_data::Enchantment;
use pumpkin_data::damage::DamageType;
use pumpkin_data::data_component_impl::{EnchantmentsImpl, EquipmentSlot};
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::potion::Effect;
use pumpkin_inventory::Inventory;
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use crate::entity::living::LivingEntity;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::experience_orb::ExperienceOrbEntity;
use crate::entity::item::ItemEntity;
use crate::entity::mob::creeper::CreeperEntity;
use crate::entity::mob::skeleton::skeleton::SkeletonEntity;
use crate::entity::mob::zombie::zombie::ZombieEntity;
use crate::entity::passive::cow::CowEntity;
use crate::entity::passive::pig::PigEntity;
use crate::entity::player::Player;
use crate::entity::{Entity, EntityBase};
use crate::net::java::PacketTap;
use crate::server::Server;
use crate::world::World;

const FLOOR_Y: i32 = 63;
/// Packet ids whose bytes are random like in vanilla: Add Entity (UUID, velocity) and the sound
/// packets (seed).
const RANDOM_PACKETS: [u8; 3] = [1, 118, 119];
/// The level of the looting stick: its 0.01 per level lifts a drop chance of 0 to certain.
const LOOTING_LEVEL: i32 = 100;

#[derive(Clone, Copy)]
enum Victim {
    Zombie,
    Cow,
    BabyCow,
    Pig,
    Creeper,
    Player,
    ArmorStand,
}

/// What actor A, a player, holds in the main hand.
#[derive(Clone, Copy)]
enum Held {
    Empty,
    Dirt,
    Looting,
    FireAspect,
}

#[derive(Clone, Copy)]
enum Mob {
    Zombie,
    Skeleton,
}

/// Which actor a role goes to.
#[derive(Clone, Copy)]
enum Who {
    Nobody,
    Player,
    Mob,
}

/// Who the victim remembers as its last attacker.
#[derive(Clone, Copy)]
enum Credit {
    Nobody,
    Player,
    OfflinePlayer,
    Mob,
}

#[derive(Clone, Copy)]
struct Scene {
    name: &'static str,
    victim: Victim,
    held: Held,
    mob: Mob,
    source: Who,
    cause: Who,
    credit: Credit,
    memory: i32,
    damage: &'static DamageType,
    mob_drops: bool,
    keep_inventory: bool,
    messages: bool,
    burning: bool,
    oozing: bool,
    twice: bool,
    absent: bool,
    trials: u32,
}

const BASE: Scene = Scene {
    name: "",
    victim: Victim::Zombie,
    held: Held::Empty,
    mob: Mob::Zombie,
    source: Who::Nobody,
    cause: Who::Nobody,
    credit: Credit::Nobody,
    memory: 0,
    damage: &DamageType::FALL,
    mob_drops: true,
    keep_inventory: false,
    messages: true,
    burning: false,
    oozing: false,
    twice: false,
    absent: false,
    trials: 1,
};

/// The player kills in melee and holds the credit.
const MELEE: Scene = Scene {
    source: Who::Player,
    cause: Who::Player,
    credit: Credit::Player,
    memory: 100,
    damage: &DamageType::PLAYER_ATTACK,
    ..BASE
};

const SCENES: &[Scene] = &[
    Scene {
        name: "absent",
        absent: true,
        ..MELEE
    },
    Scene {
        name: "fall",
        ..BASE
    },
    Scene {
        name: "fall_twice",
        twice: true,
        ..BASE
    },
    Scene {
        name: "melee_looting",
        held: Held::Looting,
        ..MELEE
    },
    Scene {
        name: "melee_empty_hand",
        ..MELEE
    },
    Scene {
        name: "melee_dirt",
        held: Held::Dirt,
        ..MELEE
    },
    Scene {
        name: "melee_no_mob_drops",
        held: Held::Looting,
        mob_drops: false,
        ..MELEE
    },
    Scene {
        name: "melee_memory_zero",
        memory: 0,
        ..MELEE
    },
    Scene {
        name: "melee_memory_one",
        memory: 1,
        ..MELEE
    },
    Scene {
        name: "melee_oozing_burning",
        oozing: true,
        burning: true,
        ..MELEE
    },
    Scene {
        name: "source_player_only",
        held: Held::Looting,
        cause: Who::Nobody,
        ..MELEE
    },
    Scene {
        name: "cause_player_only",
        held: Held::Looting,
        source: Who::Nobody,
        ..MELEE
    },
    Scene {
        name: "credit_without_attacker",
        source: Who::Nobody,
        cause: Who::Nobody,
        damage: &DamageType::FALL,
        ..MELEE
    },
    Scene {
        name: "credit_offline_player",
        credit: Credit::OfflinePlayer,
        ..MELEE
    },
    Scene {
        name: "mob_source_player_cause",
        source: Who::Mob,
        held: Held::Looting,
        ..MELEE
    },
    Scene {
        name: "player_source_mob_cause",
        cause: Who::Mob,
        ..MELEE
    },
    Scene {
        name: "mob_melee",
        source: Who::Mob,
        cause: Who::Mob,
        credit: Credit::Mob,
        damage: &DamageType::MOB_ATTACK,
        ..BASE
    },
    Scene {
        name: "mob_melee_credit_nobody",
        source: Who::Mob,
        cause: Who::Mob,
        damage: &DamageType::MOB_ATTACK,
        ..BASE
    },
    Scene {
        name: "creeper_skeleton",
        victim: Victim::Creeper,
        mob: Mob::Skeleton,
        source: Who::Mob,
        cause: Who::Mob,
        damage: &DamageType::ARROW,
        ..BASE
    },
    Scene {
        name: "creeper_melee",
        victim: Victim::Creeper,
        held: Held::Looting,
        ..MELEE
    },
    Scene {
        name: "cow_fire_aspect",
        victim: Victim::Cow,
        held: Held::FireAspect,
        ..MELEE
    },
    Scene {
        name: "cow_burning_fall",
        victim: Victim::Cow,
        burning: true,
        ..BASE
    },
    Scene {
        name: "baby_cow_melee",
        victim: Victim::BabyCow,
        held: Held::Looting,
        ..MELEE
    },
    Scene {
        name: "pig_melee",
        victim: Victim::Pig,
        held: Held::Dirt,
        ..MELEE
    },
    Scene {
        name: "armor_stand_melee",
        victim: Victim::ArmorStand,
        ..MELEE
    },
    Scene {
        name: "player_melee",
        victim: Victim::Player,
        mob_drops: false,
        ..MELEE
    },
    Scene {
        name: "player_melee_keep_inventory",
        victim: Victim::Player,
        keep_inventory: true,
        ..MELEE
    },
    Scene {
        name: "player_fall_messages_off",
        victim: Victim::Player,
        messages: false,
        ..BASE
    },
    Scene {
        name: "player_mob_melee",
        victim: Victim::Player,
        source: Who::Mob,
        cause: Who::Mob,
        credit: Credit::Mob,
        damage: &DamageType::MOB_ATTACK,
        oozing: true,
        ..BASE
    },
    Scene {
        name: "trials_nothing_held",
        trials: 120,
        ..BASE
    },
    Scene {
        name: "trials_dirt",
        trials: 120,
        held: Held::Dirt,
        ..MELEE
    },
    Scene {
        name: "trials_looting",
        trials: 5,
        held: Held::Looting,
        ..MELEE
    },
];

/// Entity ids of one scene: fixed, so that the packets that carry them are the same in every
/// run.
struct Ids {
    base: i32,
}

impl Ids {
    const fn watcher(&self) -> i32 {
        self.base
    }

    const fn actor(&self) -> i32 {
        self.base + 1
    }

    const fn mob(&self) -> i32 {
        self.base + 2
    }

    const fn victim(&self, trial: u32) -> i32 {
        self.base + 10 + trial as i32
    }
}

/// FNV-1a: a hash that does not change with the Rust version.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

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

fn place_block(world: &Arc<World>, position: BlockPos, block: &Block) {
    world.set_block_state(
        &position,
        block.default_state.id,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

struct Area<'a> {
    world: &'a Arc<World>,
    chunk_x: i32,
    x: i32,
    z: i32,
}

impl Area<'_> {
    fn load(&self) {
        for cx in self.chunk_x..self.chunk_x + 4 {
            for cz in 0..4 {
                self.world.level.loaded_chunks.insert(
                    Vector2::new(cx, cz),
                    pumpkin_world::chunk::ChunkData::empty_sync(cx, cz),
                );
            }
        }
        for dx in -6..=6 {
            for dz in -6..=6 {
                place_block(
                    self.world,
                    BlockPos::new(self.x + dx, FLOOR_Y, self.z + dz),
                    &Block::STONE,
                );
            }
        }
    }

    fn unload(&self) {
        for entity in self.world.entities.load().iter() {
            self.world.remove_entity(entity.as_ref());
        }
        for cx in self.chunk_x..self.chunk_x + 4 {
            for cz in 0..4 {
                self.world.level.loaded_chunks.remove(&Vector2::new(cx, cz));
            }
        }
    }

    fn at(&self, dx: f64) -> Vector3<f64> {
        Vector3::new(
            f64::from(self.x) + 0.5 + dx,
            f64::from(FLOOR_Y + 1),
            f64::from(self.z) + 0.5,
        )
    }
}

fn set_rules(world: &Arc<World>, scene: &Scene) {
    let mut level = (**world.level_info.load()).clone();
    level.game_rules.mob_drops = scene.mob_drops;
    level.game_rules.keep_inventory = scene.keep_inventory;
    level.game_rules.show_death_messages = scene.messages;
    world.level_info.store(Arc::new(level));
}

fn enchanted(item: &'static Item, enchantment: &'static Enchantment, level: i32) -> ItemStack {
    let mut stack = ItemStack::new(1, item);
    stack.set_data_component(EnchantmentsImpl {
        enchantment: std::borrow::Cow::Owned(vec![(enchantment, level)]),
    });
    stack
}

type Watched = (Arc<Player>, PacketTap);

fn player_at(
    world: &Arc<World>,
    area: &Area<'_>,
    name: &'static str,
    id: i32,
    dx: f64,
) -> Result<Watched, String> {
    let who = Identity {
        name,
        uuid: Uuid::from_u128(0x4465_6174_6800_0000 + id as u128),
        entity_id: id,
    };
    let (player, tap) = test_support::java_player_as(world, GameMode::Survival, &who)
        .ok_or("the test player's client no longer owned its outgoing queue")?;
    player.get_entity().set_pos(area.at(dx));
    player.get_entity().on_ground.store(true, Relaxed);
    player.watched_section.store(Cylindrical::new(
        Vector2::new((area.x + 12) >> 4, (area.z + 12) >> 4),
        NonZero::<u8>::MIN.saturating_add(3),
    ));
    world.add_player(&player)?;
    Ok((player, tap))
}

fn hold(actor: &Player, held: Held) {
    let stack = match held {
        Held::Empty => return,
        Held::Dirt => ItemStack::new(3, &Item::DIRT),
        Held::Looting => enchanted(&Item::STICK, &Enchantment::LOOTING, LOOTING_LEVEL),
        Held::FireAspect => enchanted(&Item::STICK, &Enchantment::FIRE_ASPECT, 2),
    };
    actor.inventory.set_held_item(stack);
}

/// The attacking mob. It is not added to the world: a zombie that ticks there walks up and
/// picks up the drops.
fn spawn_mob(area: &Area<'_>, world: &Arc<World>, ids: &Ids, kind: Mob) -> Arc<dyn EntityBase> {
    let uuid = Uuid::from_u128(0x4465_6174_6800_0100);
    let (entity_type, at) = match kind {
        Mob::Zombie => (&EntityType::ZOMBIE, area.at(2.0)),
        Mob::Skeleton => (&EntityType::SKELETON, area.at(2.0)),
    };
    let entity = Entity::from_uuid_with_id(ids.mob(), uuid, world.clone(), at, entity_type);
    match kind {
        Mob::Zombie => ZombieEntity::new(entity),
        Mob::Skeleton => SkeletonEntity::new(entity),
    }
}

fn equip(living: &LivingEntity) {
    let gear = [
        (EquipmentSlot::HEAD, &Item::IRON_HELMET, 2.0),
        (EquipmentSlot::LEGS, &Item::STONE, 0.0),
        (EquipmentSlot::MAIN_HAND, &Item::STICK, 2.0),
    ];
    for (slot, item, chance) in gear {
        living
            .entity_equipment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .put(&slot, ItemStack::new(1, item));
        living
            .equipment_drop_chances
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(slot, chance);
    }
}

fn spawn_victim(
    world: &Arc<World>,
    area: &Area<'_>,
    scene: &Scene,
    id: i32,
) -> Result<(Arc<dyn EntityBase>, Option<PacketTap>), String> {
    let uuid = Uuid::from_u128(0x4465_6174_6800_0200 + id as u128);
    let at = area.at(-2.0);
    let entity = Entity::from_uuid_with_id(id, uuid, world.clone(), at, victim_type(scene.victim));
    let victim: Arc<dyn EntityBase> = match scene.victim {
        Victim::Zombie => ZombieEntity::new(entity),
        Victim::Cow => CowEntity::new(entity),
        Victim::BabyCow => {
            entity.age.store(-100, Relaxed);
            CowEntity::new(entity)
        }
        Victim::Pig => PigEntity::new(entity),
        Victim::Creeper => CreeperEntity::new(entity),
        Victim::ArmorStand => Arc::new(LivingEntity::new(entity)),
        Victim::Player => {
            let (player, tap) = player_at(world, area, "DeathVictim", id, -2.0)?;
            player.experience_level.store(5, Relaxed);
            player
                .inventory
                .set_stack(0, ItemStack::new(3, &Item::DIRT));
            player
                .inventory
                .set_stack(9, ItemStack::new(64, &Item::STONE));
            return Ok((player, Some(tap)));
        }
    };
    if let Some(mob) = victim.get_mob() {
        mob.get_mob_entity()
            .death_loot_table_seed
            .store(0x5eed_0000 + i64::from(id), Relaxed);
    }
    if !scene.absent {
        world.spawn_entity(victim.clone());
    }
    Ok((victim, None))
}

const fn victim_type(victim: Victim) -> &'static EntityType {
    match victim {
        Victim::Zombie => &EntityType::ZOMBIE,
        Victim::Cow | Victim::BabyCow => &EntityType::COW,
        Victim::Pig => &EntityType::PIG,
        Victim::Creeper => &EntityType::CREEPER,
        Victim::Player => &EntityType::PLAYER,
        Victim::ArmorStand => &EntityType::ARMOR_STAND,
    }
}

fn effect(effect_type: &'static StatusEffect, amplifier: u8) -> Effect {
    Effect {
        effect_type,
        duration: 600,
        amplifier,
        ambient: false,
        show_particles: true,
        show_icon: true,
        blend: false,
    }
}

/// The victim's state before the death: it is moving, jumping, hurt and under effects.
fn prepare(
    victim: &Arc<dyn EntityBase>,
    actor: &Arc<Player>,
    mob: &Arc<dyn EntityBase>,
    scene: &Scene,
) {
    let Some(living) = victim.get_living_entity() else {
        return;
    };
    // Only now: a mob that ticks in the live world fits itself out on its first tick.
    equip(living);
    living.movement_input.store(Vector3::new(0.5, 0.0, -0.25));
    living.jumping.store(true, Relaxed);
    living.add_effect(effect(&StatusEffect::SPEED, 1));
    if scene.oozing {
        living.add_effect(effect(&StatusEffect::OOZING, 0));
    }
    if scene.burning {
        living.entity.fire_ticks.store(100, Relaxed);
    }
    living
        .last_hurt_by_player_memory_time
        .store(scene.memory, Relaxed);
    match scene.credit {
        Credit::Nobody => {}
        Credit::Player => living.last_hurt_by_player.store(Some(actor.gameprofile.id)),
        Credit::OfflinePlayer => living
            .last_hurt_by_player
            .store(Some(Uuid::from_u128(0x00ff_11ae))),
        Credit::Mob => living.set_last_hurt_by_mob(Some(mob.as_ref())),
    }
    if let Some(player) = victim.get_player() {
        player.set_stat(
            crate::entity::player::statistics::StatisticCategory::Custom,
            crate::entity::player::statistics::CustomStatistic::TimeSinceDeath as i32,
            9,
        );
        player.set_stat(
            crate::entity::player::statistics::StatisticCategory::Custom,
            crate::entity::player::statistics::CustomStatistic::TimeSinceRest as i32,
            7,
        );
    }
}

fn role<'a>(
    who: Who,
    actor: &'a Arc<Player>,
    mob: &'a Arc<dyn EntityBase>,
) -> Option<&'a dyn EntityBase> {
    match who {
        Who::Nobody => None,
        Who::Player => Some(actor.as_ref()),
        Who::Mob => Some(mob.as_ref()),
    }
}

fn stats_text(player: &Player) -> String {
    let stats = player
        .stats
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut entries: Vec<_> = stats.stats.iter().collect();
    entries.sort_unstable();
    entries
        .iter()
        .map(|((category, stat), value)| format!("{category}:{stat}={value}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn stack_text(stack: &ItemStack) -> String {
    format!("{}x{}", stack.item.registry_key, stack.item_count)
}

fn drop_text(entity: &Arc<dyn EntityBase>) -> String {
    let base = entity.get_entity();
    let any = entity.cast_any();
    if let Some(item) = any.downcast_ref::<ItemEntity>() {
        let stack = item
            .get_item_stack()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        return format!(
            "item {} at {:?}",
            stack_text(&stack),
            base.block_pos.load().0
        );
    }
    if let Some(orb) = any.downcast_ref::<ExperienceOrbEntity>() {
        return format!("orb {} at {:?}", orb.amount(), base.pos.load());
    }
    format!(
        "{} at {:?}",
        base.entity_type.resource_name,
        base.block_pos.load().0
    )
}

/// What the death added to the world, sorted, and removed so that the next trial starts clean.
fn drops(world: &Arc<World>, keep: &[i32]) -> Vec<String> {
    let mut found = Vec::new();
    for entity in world.entities.load().iter() {
        if keep.contains(&entity.get_entity().entity_id) {
            continue;
        }
        found.push(drop_text(entity));
        world.remove_entity(entity.as_ref());
    }
    found.sort();
    found
}

fn victim_text(victim: &Arc<dyn EntityBase>) -> String {
    let Some(living) = victim.get_living_entity() else {
        return String::from("no living entity");
    };
    let mut text = format!(
        "dead {} pose {} health {:?} input {:?} jumping {} fire {} removed {} effects",
        living.dead.load(Relaxed),
        living.entity.pose.load() as i32,
        living.health.load(),
        living.movement_input.load(),
        living.jumping.load(Relaxed),
        living.entity.fire_ticks.load(Relaxed),
        living.entity.is_removed(),
    );
    let effects = living
        .active_effects
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut effects: Vec<_> = effects.values().collect();
    effects.sort_by_key(|effect| effect.effect_type.id);
    for effect in effects {
        let _ = write!(
            text,
            " {}:{}:{}",
            effect.effect_type.minecraft_name, effect.amplifier, effect.duration
        );
    }
    text
}

fn inventory_text(player: &Player) -> String {
    let mut text = String::new();
    for slot in 0..player.inventory.size() {
        let stack = player.inventory.get_slot(slot);
        if !stack.is_empty() {
            let _ = write!(text, " {slot}={}", stack_text(&stack));
        }
    }
    text
}

struct Cast {
    watcher: Watched,
    actor: Watched,
    mob: Arc<dyn EntityBase>,
}

fn kill(
    scene: &Scene,
    cast: &mut Cast,
    victim: &Arc<dyn EntityBase>,
    victim_tap: &mut Option<PacketTap>,
    report: &mut String,
) {
    let Some(living) = victim.get_living_entity() else {
        return;
    };
    let source = role(scene.source, &cast.actor.0, &cast.mob);
    let cause = role(scene.cause, &cast.actor.0, &cast.mob);
    drop(packets(&mut cast.watcher.1));
    drop(packets(&mut cast.actor.1));
    if let Some(tap) = victim_tap.as_mut() {
        drop(packets(tap));
    }
    living.on_death(*scene.damage, source, cause);
    if scene.twice {
        living.on_death(*scene.damage, source, cause);
    }
    let _ = writeln!(report, "{} victim: {}", scene.name, victim_text(victim));
    let _ = writeln!(
        report,
        "{} watcher: {}",
        scene.name,
        packets(&mut cast.watcher.1)
    );
    let _ = writeln!(
        report,
        "{} actor: {}",
        scene.name,
        packets(&mut cast.actor.1)
    );
    if let (Some(tap), Some(player)) = (victim_tap.as_mut(), victim.get_player()) {
        let _ = writeln!(
            report,
            "{} victim player: stats {} inventory{} packets {}",
            scene.name,
            stats_text(player),
            inventory_text(player),
            packets(tap)
        );
    }
}

/// Item entities that lie together merge in the world while the scene runs, so over many trials
/// only the total per item is stable.
fn totals(found: &[String]) -> Vec<String> {
    let mut sums: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    for text in found {
        let (kind, count) = text
            .split(" at ")
            .next()
            .and_then(|head| head.rsplit_once('x'))
            .and_then(|(kind, count)| Some((kind, count.parse::<u32>().ok()?)))
            .unwrap_or((text.as_str(), 1));
        *sums.entry(kind.to_owned()).or_default() += count;
    }
    sums.into_iter().map(|(kind, sum)| format!("{kind} total {sum}")).collect()
}

fn run_scene(
    server: &Arc<Server>,
    world: &Arc<World>,
    index: usize,
    scene: &Scene,
    report: &mut String,
) -> TestResult {
    let area = Area {
        world,
        chunk_x: index as i32 * 4,
        x: index as i32 * 64 + 24,
        z: 24,
    };
    area.load();
    set_rules(world, scene);
    let ids = Ids {
        base: 80_000 + index as i32 * 1_000,
    };
    let watcher = player_at(world, &area, "DeathWatch", ids.watcher(), 4.0)?;
    let actor = player_at(world, &area, "DeathActor", ids.actor(), 0.0)?;
    hold(&actor.0, scene.held);
    let mob = spawn_mob(&area, world, &ids, scene.mob);
    let mut cast = Cast {
        watcher,
        actor,
        mob,
    };
    let mut found: Vec<String> = Vec::new();
    for trial in 0..scene.trials {
        let (victim, mut tap) = spawn_victim(world, &area, scene, ids.victim(trial))?;
        prepare(&victim, &cast.actor.0, &cast.mob, scene);
        let keep = [ids.watcher(), ids.actor(), ids.mob(), ids.victim(trial)];
        drop(drops(world, &keep));
        kill(scene, &mut cast, &victim, &mut tap, report);
        if scene.trials > 1 {
            // The world ticks on its own: let its item merging finish before the drops are read.
            std::thread::sleep(std::time::Duration::from_millis(120));
        }
        found.extend(drops(world, &keep));
        if !scene.absent && trial + 1 < scene.trials {
            world.remove_entity(victim.as_ref());
        }
    }
    found.sort();
    if scene.trials > 1 {
        found = totals(&found);
    }
    let _ = writeln!(
        report,
        "{} dropped {}: {}",
        scene.name,
        found.len(),
        found.join("; ")
    );
    let _ = writeln!(
        report,
        "{} actor player: stats {}",
        scene.name,
        stats_text(&cast.actor.0)
    );
    if let Some(dump) = std::env::var_os("KVASIR_DUMP") {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dump)?;
        writeln!(file, "{} done", scene.name)?;
    }
    let _ = server;
    area.unload();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn living_death() -> TestResult {
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
    script_golden::bless_or_compare("living_death.txt", &report)
}
