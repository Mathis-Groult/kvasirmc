mod living_attackers;
mod living_attributes;
mod living_damage;
mod living_death;
mod living_effects;
mod living_equipment;
mod living_falling;
mod living_health;
mod living_save;
mod living_sounds;
mod living_tick;
mod living_travel;
#[cfg(test)]
mod tick_golden;
#[cfg(test)]
mod use_tests;
use kvasir_tickprof::parts::Part;
use pumpkin_data::particle::Particle;
use pumpkin_data::potion::Effect;
use pumpkin_data::tracked_data;
use pumpkin_inventory::build_equipment_slots;
use pumpkin_util::Hand;
use pumpkin_util::math::position::BlockPos;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::atomic::{
    AtomicBool, AtomicI32, AtomicU8,
    Ordering::{Relaxed, SeqCst},
};
use uuid::Uuid;

use super::{Entity, EntityBase};
use crate::entity::attributes::AttributeInstance;
use crate::entity::combat::CombatTracker;
use crate::server::Server;
use crossbeam::atomic::AtomicCell;
use pumpkin_data::AttributeModifierSlot;
use pumpkin_data::Block;
use pumpkin_data::attributes::Attributes;
use pumpkin_data::damage::DamageType;
use pumpkin_data::data_component_impl::{EnchantmentsImpl, EquipmentSlot};
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_inventory::entity_equipment::EntityEquipment;
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::{
    java::client::play::MetadataSerializer,
    ser::{NetworkWriteExt, WritingError},
};
use pumpkin_util::math::vector3::Vector3;
use std::sync::RwLock;

#[derive(Clone, Copy)]
struct LastHurtByMob {
    uuid: Uuid,
    entity_id: i32,
}

/// Represents a living entity within the game world.
///
/// This struct encapsulates the core properties and behaviors of living entities, including players, mobs, and other creatures.
#[repr(C)]
pub struct LivingEntity {
    /// The underlying entity object, providing basic entity information and functionality.
    pub entity: Entity,
    /// Tracks the remaining time until the entity can regenerate health.
    pub hurt_cooldown: AtomicI32,
    /// Stores the amount of damage the entity last received.
    pub last_damage_taken: AtomicCell<f32>,
    /// The current health level of the entity.
    pub health: AtomicCell<f32>,
    /// The current absorption (yellow hearts) on the entity.
    pub absorption: AtomicCell<f32>,
    pub item_use_time: AtomicI32,
    pub item_in_use: std::sync::Mutex<Option<ItemStack>>,
    pub active_hand: std::sync::Mutex<Option<Hand>>,
    pub recent_kinetic_enemies: std::sync::Mutex<FxHashMap<i32, i32>>,
    pub death_time: AtomicU8,
    /// Indicates whether the entity is dead. (`on_death` called)
    pub dead: AtomicBool,
    /// The distance the entity has been falling.
    pub fall_distance: AtomicCell<f32>,
    pub active_effects: std::sync::Mutex<FxHashMap<&'static StatusEffect, Effect>>,
    pub entity_equipment: Arc<std::sync::Mutex<EntityEquipment>>,
    pub equipment_drop_chances: Arc<std::sync::Mutex<FxHashMap<EquipmentSlot, f32>>>,
    pub movement_input: AtomicCell<Vector3<f64>>,
    speed: AtomicCell<f32>,
    pub equipment_slots: Arc<FxHashMap<usize, EquipmentSlot>>,

    pub jumping: AtomicBool,

    pub jumping_cooldown: AtomicU8,

    pub climbing: AtomicBool,

    /// The position where the entity was last climbing, used for death messages
    pub climbing_pos: AtomicCell<Option<BlockPos>>,

    last_hurt_by_mob: AtomicCell<Option<LastHurtByMob>>,
    last_hurt_by_mob_timestamp: AtomicI32,
    last_damage_type: std::sync::Mutex<Option<DamageType>>,
    /// Entity id of the last damage source's causing entity, 0 when it had none.
    last_damage_attacker: AtomicI32,
    last_damage_stamp: std::sync::atomic::AtomicI64,

    /// The entity ID of the entity this living entity last attacked.
    pub last_attacking_id: AtomicI32,
    /// The tick at which this entity last attacked something (entity age).
    pub last_attack_time: AtomicI32,

    /// Tracks combat entries, assisted falls, kill credit, and death messages.
    pub combat_tracker: std::sync::Mutex<CombatTracker>,

    pub last_hurt_by_player: AtomicCell<Option<Uuid>>,
    pub last_hurt_by_player_memory_time: AtomicI32,
    pub no_action_time: AtomicI32,

    water_movement_speed_multiplier: f32,
    livings_flags: AtomicU8,

    /// The last block position the entity occupied, used to trigger location changed effects.
    pub last_block_pos: AtomicCell<Option<BlockPos>>,

    /// The attributes of the entity
    pub attributes: RwLock<FxHashMap<u8, AttributeInstance>>,
    /// Modifier ids applied from the current item in each equipment slot.
    /// Used to remove them on unequip without the previous stack.
    equipment_attribute_modifier_ids: std::sync::Mutex<FxHashMap<EquipmentSlot, Vec<(u8, String)>>>,
}

#[derive(Clone)]
struct EffectParticle {
    particle_id: VarInt,
    color: i32,
}

#[derive(Clone)]
struct EffectParticles(Vec<EffectParticle>);

impl MetadataSerializer for EffectParticles {
    fn write_metadata(
        &self,
        writer: &mut impl std::io::Write,
        _version: &pumpkin_util::version::JavaMinecraftVersion,
    ) -> Result<(), WritingError> {
        let count = i32::try_from(self.0.len())
            .map_err(|_| WritingError::Message("Too many effect particles".into()))?;
        writer.write_var_int(&VarInt(count))?;
        for particle in &self.0 {
            writer.write_var_int(&particle.particle_id)?;
            writer.write_i32(particle.color)?;
        }
        Ok(())
    }
}

impl EffectParticle {
    const fn from_effect(effect: &Effect) -> Self {
        Self {
            particle_id: VarInt(Particle::EntityEffect as i32),
            color: (((if effect.ambient { 38 } else { 255 }) as u32) << 24
                | effect.effect_type.color as u32) as i32,
        }
    }
}

fn is_allowed_by_team_rules(
    own_team: Option<&crate::world::scoreboard::Team>,
    their_team: Option<&crate::world::scoreboard::Team>,
) -> bool {
    use crate::world::scoreboard::CollisionRule;

    let own_rule = own_team.map_or(CollisionRule::Always, |team| team.collision_rule);
    let their_rule = their_team.map_or(CollisionRule::Always, |team| team.collision_rule);

    if own_rule == CollisionRule::Never || their_rule == CollisionRule::Never {
        return false;
    }

    let same_team = own_team
        .zip(their_team)
        .is_some_and(|(own, their)| own.name == their.name);

    if (own_rule == CollisionRule::PushOwnTeam || their_rule == CollisionRule::PushOwnTeam)
        && same_team
    {
        return false;
    }

    (own_rule != CollisionRule::PushOtherTeams && their_rule != CollisionRule::PushOtherTeams)
        || same_team
}

/// Who an instant effect came from: the potion or cloud, and who threw or placed it.
#[derive(Clone, Copy)]
pub struct EffectSource<'a> {
    pub direct: &'a dyn EntityBase,
    pub owner: Option<&'a dyn EntityBase>,
}

impl LivingEntity {
    const USING_ITEM_FLAG: u8 = 1;
    const OFF_HAND_ACTIVE_FLAG: u8 = 2;
    const RANDOM_TELEPORT_ATTEMPTS: usize = 16;
    #[expect(dead_code)]
    const USING_RIPTIDE_FLAG: u8 = 4;

    const PREVENT_AREA_FALL_DAMAGE_BLOCKS: [&'static Block; 4] = [
        &Block::COBWEB,
        &Block::LADDER,
        &Block::POWDER_SNOW,
        &Block::SLIME_BLOCK,
    ];

    #[must_use]
    pub fn is_baby(&self) -> bool {
        self.entity.age.load(Relaxed) < 0
    }

    pub fn new(mut entity: Entity) -> Self {
        entity.blocks_building = true;
        let water_movement_speed_multiplier = if entity.entity_type == &EntityType::POLAR_BEAR {
            0.98
        } else if entity.entity_type == &EntityType::SKELETON_HORSE {
            0.96
        } else {
            0.8
        };
        let mut max_health: f32 = 20.0; // Overridden by attribute base below
        Self {
            // Populate local attribute instances from the default registry and get initial vars
            attributes: {
                let mut m = FxHashMap::default();

                for (attr, base) in entity.entity_type.attributes {
                    if attr.id == Attributes::MAX_HEALTH.id {
                        max_health = *base as f32;
                    }
                    m.insert(attr.id, AttributeInstance::new(*base));
                }
                std::sync::RwLock::new(m)
            },
            health: AtomicCell::new(max_health), // Initial health value from attributes
            entity,
            hurt_cooldown: AtomicI32::new(0),
            last_damage_taken: AtomicCell::new(0.0),
            absorption: AtomicCell::new(0.0),
            fall_distance: AtomicCell::new(0.0),
            death_time: AtomicU8::new(0),
            dead: AtomicBool::new(false),
            item_use_time: AtomicI32::new(0),
            item_in_use: std::sync::Mutex::new(None),
            active_hand: std::sync::Mutex::new(None),
            recent_kinetic_enemies: std::sync::Mutex::new(FxHashMap::default()),
            livings_flags: AtomicU8::new(0),
            active_effects: std::sync::Mutex::new(FxHashMap::default()),
            entity_equipment: Arc::new(std::sync::Mutex::new(EntityEquipment::new())),
            equipment_drop_chances: Arc::new(std::sync::Mutex::new(FxHashMap::default())),
            equipment_slots: Arc::new(build_equipment_slots()),
            jumping: AtomicBool::new(false),
            jumping_cooldown: AtomicU8::new(0),
            climbing: AtomicBool::new(false),
            climbing_pos: AtomicCell::new(None),
            last_hurt_by_mob: AtomicCell::new(None),
            last_hurt_by_mob_timestamp: AtomicI32::new(0),
            last_damage_type: std::sync::Mutex::new(None),
            last_damage_attacker: AtomicI32::new(0),
            last_damage_stamp: std::sync::atomic::AtomicI64::new(0),
            last_attacking_id: AtomicI32::new(0),
            last_attack_time: AtomicI32::new(0),
            combat_tracker: std::sync::Mutex::new(CombatTracker::new()),
            last_hurt_by_player: AtomicCell::new(None),
            last_hurt_by_player_memory_time: AtomicI32::new(0),
            no_action_time: AtomicI32::new(0),
            movement_input: AtomicCell::new(Vector3::default()),
            speed: AtomicCell::new(0.0),
            water_movement_speed_multiplier,
            last_block_pos: AtomicCell::new(None),
            equipment_attribute_modifier_ids: std::sync::Mutex::new(FxHashMap::default()),
        }
    }

    pub const fn entity_id(&self) -> i32 {
        self.entity.entity_id
    }

    pub fn reset_state(&self) {
        self.entity.reset_state();

        // Restore to maximum health for this entity type
        let max_health = self.get_max_health();
        self.set_health(max_health);
        // Clear any absorption
        self.absorption.store(0.0);
        // Send health metadata
        self.entity
            .set_synced_data(tracked_data::living_entity::DATA_HEALTH_ID, max_health);

        self.reset_effects_and_attributes();

        // Give a short grace period of invulnerability after respawn
        self.hurt_cooldown.store(20, Relaxed);
        self.last_damage_taken.store(0f32);

        self.entity.portal_cooldown.store(0, Relaxed);
        *self
            .entity
            .portal_manager
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;

        // Clear fall/fire state
        self.fall_distance.store(0f32);
        self.death_time.store(0, Relaxed);
        self.entity.extinguish();
        self.entity.fire_ticks.store(0, Relaxed);

        // Clear velocity and movement input to remove persisted momentum
        self.entity.velocity.store(Vector3::default());
        self.entity.velocity_dirty.store(true, SeqCst);
        self.movement_input.store(Vector3::default());
        self.jumping.store(false, Relaxed);

        // If this LivingEntity corresponds to a Player, reset their hunger manager
        let world = self.entity.world.load();
        if let Some(player) = world.get_player_by_id(self.entity.entity_id) {
            player.hunger_manager.restart();
        }

        self.dead.store(false, Relaxed);
    }

    pub fn is_player(&self) -> bool {
        let world = self.entity.world.load();
        world.get_player_by_id(self.entity.entity_id).is_some()
    }
}

impl LivingEntity {}

impl EntityBase for LivingEntity {
    fn damage_with_context(
        &self,
        caller: &dyn EntityBase,
        amount: f32,
        damage_type: DamageType,
        position: Option<Vector3<f64>>,
        source: Option<&dyn EntityBase>,
        cause: Option<&dyn EntityBase>,
    ) -> bool {
        self.damage_with_context(caller, amount, damage_type, position, source, cause)
    }

    fn tick_in_void(&self, dyn_self: &dyn EntityBase) {
        dyn_self.damage(dyn_self, 4.0, DamageType::OUT_OF_WORLD);
    }

    fn get_gravity(&self) -> f64 {
        self.get_attribute_value(&Attributes::GRAVITY)
    }

    /// Advances the living entity by one tick: base entity tick, movement and
    /// physics while alive (still applied during the 20-tick death animation so
    /// knockback lands), velocity coalescing, status effects, void damage, and
    /// death-animation completion.
    fn tick(&self, caller: &dyn EntityBase, server: &Server) {
        Part::BaseTick.time(|| self.entity.tick(caller, server));
        self.tick_last_hurt_by_mob();
        Part::MobBaseTick.time(|| {
            if let Some(mob) = caller.get_mob() {
                mob.tick_ambient_sound();
                mob.base_tick();
            }
        });

        // Only tick movement if the entity is alive. This prevents a dead "corpse"
        // from continuing to be simulated (accumulating fall_distance/velocity).
        // We allow movement during death animation (20 ticks) so knockback is applied.
        let is_alive = !self.dead.load(Relaxed) && self.health.load() > 0.0;
        self.tick_motion(caller, is_alive);

        // Coalesce velocity sends to once per tick.
        if self.entity.velocity_dirty.swap(false, Ordering::SeqCst) {
            self.entity.send_velocity();
        }

        self.step_on_supporting_blocks(caller);

        let current_block_pos = self.entity.block_pos.load();
        if is_alive && self.last_block_pos.load() != Some(current_block_pos) {
            self.last_block_pos.store(Some(current_block_pos));
            self.on_changed_block(caller, current_block_pos);
        }

        Part::Effects.time(|| self.tick_effects());

        self.tick_item_use(caller, server);
        self.finish_item_in_use(caller);
        self.tick_hurt_counters();

        self.refresh_dirty_attributes(caller);

        self.tick_death();
    }

    fn get_entity(&self) -> &Entity {
        &self.entity
    }

    fn get_living_entity(&self) -> Option<&LivingEntity> {
        Some(self)
    }

    fn is_pushable(&self) -> bool {
        self.health.load() > 0.0 && !self.dead.load(Relaxed)
    }

    fn cast_any(&self) -> &dyn std::any::Any {
        self
    }
}

const LAST_HURT_BY_PLAYER_MEMORY_TIME: i32 = 100;

pub const SPEED_MODIFIER_SPRINTING_ID: &str = "minecraft:sprinting";
pub const SPEED_MODIFIER_SPRINTING_AMOUNT: f64 = 0.300_000_011_920_928_96;

impl LivingEntity {}

fn random_teleport_coordinate(center: f64, diameter: f32, random: f64) -> f64 {
    center + (random - 0.5) * f64::from(diameter)
}

pub(crate) fn attributes_by_id(id: u8) -> Option<&'static Attributes> {
    Attributes::ALL.iter().find(|attr| attr.id == id)
}

fn push_unique_attribute(touched: &mut Vec<Attributes>, attr: &Attributes) {
    if !touched.iter().any(|existing| existing.id == attr.id) {
        touched.push(attr.clone());
    }
}

pub(crate) const fn attribute_modifier_slot_matches(
    modifier_slot: &AttributeModifierSlot,
    equipment_slot: &EquipmentSlot,
) -> bool {
    match modifier_slot {
        AttributeModifierSlot::Any => true,
        AttributeModifierSlot::MainHand => matches!(equipment_slot, EquipmentSlot::MainHand(_)),
        AttributeModifierSlot::OffHand => matches!(equipment_slot, EquipmentSlot::OffHand(_)),
        AttributeModifierSlot::Hand => {
            matches!(
                equipment_slot,
                EquipmentSlot::MainHand(_) | EquipmentSlot::OffHand(_)
            )
        }
        AttributeModifierSlot::Feet => matches!(equipment_slot, EquipmentSlot::Feet(_)),
        AttributeModifierSlot::Legs => matches!(equipment_slot, EquipmentSlot::Legs(_)),
        AttributeModifierSlot::Chest => matches!(equipment_slot, EquipmentSlot::Chest(_)),
        AttributeModifierSlot::Head => matches!(equipment_slot, EquipmentSlot::Head(_)),
        AttributeModifierSlot::Armor => matches!(
            equipment_slot,
            EquipmentSlot::Feet(_)
                | EquipmentSlot::Legs(_)
                | EquipmentSlot::Chest(_)
                | EquipmentSlot::Head(_)
        ),
        AttributeModifierSlot::Body => matches!(equipment_slot, EquipmentSlot::Body(_)),
        AttributeModifierSlot::Saddle => matches!(equipment_slot, EquipmentSlot::Saddle(_)),
    }
}

/// Mirrors vanilla's strict `random < probability` consume-effect gate.
const fn consume_effect_probability_applies(probability: f32, random: f32) -> bool {
    random < probability
}

#[cfg(test)]
mod consumable_effect_tests {
    use super::{consume_effect_probability_applies, random_teleport_coordinate};

    #[test]
    fn consumable_effect_probability_matches_vanilla_strict_threshold() {
        assert!(!consume_effect_probability_applies(0.0, 0.0));
        assert!(consume_effect_probability_applies(1.0, 0.999));
        assert!(consume_effect_probability_applies(0.5, 0.499));
        assert!(!consume_effect_probability_applies(0.5, 0.5));
    }

    #[test]
    fn random_teleport_coordinate_uses_full_diameter() {
        assert_eq!(random_teleport_coordinate(10.0, 16.0, 0.0), 2.0);
        assert_eq!(random_teleport_coordinate(10.0, 16.0, 0.5), 10.0);
        assert_eq!(random_teleport_coordinate(10.0, 16.0, 1.0), 18.0);
    }
}
/// Returns `true` if `damage_type` is in `#minecraft:bypasses_armor` (1.21.11).
/// These sources bypass armor entirely (fall, drown, freeze, etc.).
pub(crate) const fn bypasses_armor_durability(damage_type: &DamageType) -> bool {
    // Bitmask lookup: O(1) with two instructions (shift + AND), no array scan.
    // DamageType IDs can exceed 31; use u64 for sufficient range.
    // TODO: Make data-driven once the data pack system can handle it without performance regressions.
    // Compile-time assertions: ensure all bypassing types fit in u64 bitmask.
    const _: () = assert!(
        DamageType::FALL.id < 64
            && DamageType::FLY_INTO_WALL.id < 64
            && DamageType::ON_FIRE.id < 64
            && DamageType::IN_WALL.id < 64
            && DamageType::CRAMMING.id < 64
            && DamageType::DROWN.id < 64
            && DamageType::GENERIC.id < 64
            && DamageType::WITHER.id < 64
            && DamageType::DRAGON_BREATH.id < 64
            && DamageType::STARVE.id < 64
            && DamageType::ENDER_PEARL.id < 64
            && DamageType::FREEZE.id < 64
            && DamageType::STALAGMITE.id < 64
            && DamageType::MAGIC.id < 64
            && DamageType::INDIRECT_MAGIC.id < 64
            && DamageType::OUT_OF_WORLD.id < 64
            && DamageType::GENERIC_KILL.id < 64
            && DamageType::SONIC_BOOM.id < 64
            && DamageType::OUTSIDE_BORDER.id < 64,
        "One or more bypass DamageType IDs exceed u64 bitmask width (>= 64)"
    );
    const BYPASS_MASK: u64 = (1u64 << DamageType::FALL.id)
        | (1u64 << DamageType::FLY_INTO_WALL.id)
        | (1u64 << DamageType::ON_FIRE.id)
        | (1u64 << DamageType::IN_WALL.id)
        | (1u64 << DamageType::CRAMMING.id)
        | (1u64 << DamageType::DROWN.id)
        | (1u64 << DamageType::GENERIC.id)
        | (1u64 << DamageType::WITHER.id)
        | (1u64 << DamageType::DRAGON_BREATH.id)
        | (1u64 << DamageType::STARVE.id)
        | (1u64 << DamageType::ENDER_PEARL.id)
        | (1u64 << DamageType::FREEZE.id)
        | (1u64 << DamageType::STALAGMITE.id)
        | (1u64 << DamageType::MAGIC.id)
        | (1u64 << DamageType::INDIRECT_MAGIC.id)
        | (1u64 << DamageType::OUT_OF_WORLD.id)
        | (1u64 << DamageType::GENERIC_KILL.id)
        | (1u64 << DamageType::SONIC_BOOM.id)
        | (1u64 << DamageType::OUTSIDE_BORDER.id);
    (damage_type.id < 64) && ((BYPASS_MASK >> damage_type.id) & 1 == 1)
}

pub(crate) fn prevents_equipment_drop(stack: &ItemStack) -> bool {
    stack
        .get_data_component::<EnchantmentsImpl>()
        .is_some_and(|enchantments| {
            enchantments.enchantment.iter().any(|(enchantment, level)| {
                *level > 0 && enchantment.effects.prevent_equipment_drop
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumpkin_data::sound::Sound;

    // ── bypasses_armor_durability ─────────────────────────────────────

    /// Every member of `minecraft:bypasses_armor` (1.21.11) must return `true`.
    #[test]
    fn bypasses_armor_durability_returns_true_for_tag_members() {
        // Exact contents of the minecraft:bypasses_armor tag in 1.21.11.
        let bypassing: &[DamageType] = &[
            DamageType::ON_FIRE,
            DamageType::IN_WALL,
            DamageType::CRAMMING,
            DamageType::DROWN,
            DamageType::FLY_INTO_WALL,
            DamageType::GENERIC,
            DamageType::WITHER,
            DamageType::DRAGON_BREATH,
            DamageType::STARVE,
            DamageType::FALL,
            DamageType::ENDER_PEARL,
            DamageType::FREEZE,
            DamageType::STALAGMITE,
            DamageType::MAGIC,
            DamageType::INDIRECT_MAGIC,
            DamageType::OUT_OF_WORLD,
            DamageType::GENERIC_KILL,
            DamageType::SONIC_BOOM,
            DamageType::OUTSIDE_BORDER,
        ];
        for dt in bypassing {
            assert!(
                bypasses_armor_durability(dt),
                "{} should bypass armor durability",
                dt.message_id
            );
        }
    }

    /// Physical/combat damage types must NOT bypass armor durability.
    #[test]
    fn bypasses_armor_durability_returns_false_for_physical_sources() {
        let physical: &[DamageType] = &[
            DamageType::MOB_ATTACK,
            DamageType::PLAYER_ATTACK,
            DamageType::ARROW,
            DamageType::CACTUS,
            DamageType::SWEET_BERRY_BUSH,
            DamageType::LAVA,
            DamageType::EXPLOSION,
            DamageType::PLAYER_EXPLOSION,
            DamageType::LIGHTNING_BOLT,
            DamageType::FIREBALL,
            DamageType::THORNS,
            DamageType::TRIDENT,
        ];
        for dt in physical {
            assert!(
                !bypasses_armor_durability(dt),
                "{} should NOT bypass armor durability",
                dt.message_id
            );
        }
    }

    #[test]
    fn hurt_sound_for_entity_uses_zombie_family_sounds() {
        let cases = [
            (&EntityType::ZOMBIE, Sound::EntityZombieHurt),
            (&EntityType::DROWNED, Sound::EntityDrownedHurt),
            (&EntityType::HUSK, Sound::EntityHuskHurt),
            (
                &EntityType::ZOMBIE_VILLAGER,
                Sound::EntityZombieVillagerHurt,
            ),
        ];

        for (entity_type, expected) in cases {
            assert_eq!(LivingEntity::hurt_sound_for_entity(entity_type), expected);
        }
    }

    #[test]
    fn hurt_sound_for_entity_uses_enderman_hurt_sound() {
        assert_eq!(
            LivingEntity::hurt_sound_for_entity(&EntityType::ENDERMAN),
            Sound::EntityEndermanHurt
        );
    }

    #[test]
    fn hurt_sound_for_entity_uses_skeleton_family_sounds() {
        let cases = [
            (&EntityType::SKELETON, Sound::EntitySkeletonHurt),
            (&EntityType::BOGGED, Sound::EntityBoggedHurt),
            (&EntityType::PARCHED, Sound::EntityParchedHurt),
            (
                &EntityType::WITHER_SKELETON,
                Sound::EntityWitherSkeletonHurt,
            ),
            (&EntityType::STRAY, Sound::EntityStrayHurt),
        ];

        for (entity_type, expected) in cases {
            assert_eq!(LivingEntity::hurt_sound_for_entity(entity_type), expected);
        }
    }

    #[test]
    fn hurt_sound_for_entity_defaults_to_generic_hurt() {
        assert_eq!(
            LivingEntity::hurt_sound_for_entity(&EntityType::ITEM),
            Sound::EntityGenericHurt
        );
    }

    #[test]
    fn regeneration_particle_metadata_uses_vanilla_argb_color() {
        let effect = Effect {
            effect_type: &StatusEffect::REGENERATION,
            duration: 200,
            amplifier: 0,
            ambient: false,
            show_particles: true,
            show_icon: true,
            blend: false,
        };
        let metadata = pumpkin_protocol::java::client::play::Metadata::new(
            tracked_data::living_entity::EFFECT_PARTICLES,
            EffectParticles(vec![EffectParticle::from_effect(&effect)]),
        );
        let mut bytes = Vec::new();

        metadata
            .write(
                &mut bytes,
                &pumpkin_util::version::JavaMinecraftVersion::V_26_3,
            )
            .unwrap();

        assert_eq!(bytes, [10, 17, 1, 28, 0xff, 0xcd, 0x5c, 0xab]);
    }
}

#[cfg(test)]
mod ai_step_tests {
    use std::error::Error;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use crate::entity::Entity;
    use crate::entity::ai::control::move_control::MoveControl;
    use crate::entity::ai::control::{Control, MoveControlTrait};
    use crate::entity::ai::goal::Goal;
    use crate::entity::ai::pathfinder::node::Node;
    use crate::entity::ai::pathfinder::path::Path;
    use crate::entity::ai::pathfinder::{NavigationTarget, Navigator, NavigatorGoal};
    use crate::entity::ai::test_support::world;
    use crate::entity::mob::{Mob, MobEntity};
    use crate::world::World;
    use pumpkin_data::{attributes::Attributes, entity::EntityType};
    use pumpkin_util::math::position::BlockPos;
    use pumpkin_util::math::vector3::Vector3;

    type TestResult = Result<(), Box<dyn Error>>;

    struct ProbeMob {
        mob: MobEntity,
        immobile: bool,
    }

    impl Mob for ProbeMob {
        fn get_mob_entity(&self) -> &MobEntity {
            &self.mob
        }

        fn is_mob_immobile(&self) -> bool {
            self.immobile || self.mob.living_entity.is_dead_or_dying()
        }
    }

    #[derive(Default)]
    struct Observation {
        velocity: Vector3<f64>,
        input: Vector3<f64>,
    }

    struct ObserveGoal(Arc<Mutex<Observation>>);

    impl Goal for ObserveGoal {
        fn can_start(&mut self, mob: &dyn Mob) -> bool {
            let living = &mob.get_mob_entity().living_entity;
            *self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Observation {
                velocity: living.entity.velocity.load(),
                input: living.movement_input.load(),
            };
            false
        }
    }

    struct ProbeControl(Arc<AtomicUsize>);

    struct TargetControl(Arc<Mutex<Option<NavigationTarget>>>);

    impl Control for TargetControl {}

    impl MoveControlTrait for TargetControl {
        fn tick(&mut self, _mob: &dyn Mob) {}

        fn set_wanted_position(&mut self, x: f64, y: f64, z: f64, speed_modifier: f64) {
            *self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(NavigationTarget {
                position: Vector3::new(x, y, z),
                speed_modifier,
            });
        }
    }

    struct JumpOnceGoal(bool);

    impl Goal for JumpOnceGoal {
        fn can_start(&mut self, _mob: &dyn Mob) -> bool {
            self.0
        }

        fn start(&mut self, mob: &dyn Mob) {
            self.0 = false;
            mob.get_mob_entity()
                .jump_control
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .jump();
        }
    }

    impl Control for ProbeControl {}

    impl MoveControlTrait for ProbeControl {
        fn tick(&mut self, mob: &dyn Mob) {
            self.0.fetch_add(1, Ordering::Relaxed);
            let living = &mob.get_mob_entity().living_entity;
            living.entity.velocity.store(Vector3::new(0.002, 0.0, 0.0));
            living.movement_input.store(Vector3::new(0.0, 0.0, 0.5));
        }
    }

    fn probe(world: Arc<World>, calls: Arc<AtomicUsize>) -> ProbeMob {
        let mob = MobEntity::new(Entity::new(
            world,
            Vector3::new(0.0, 64.0, 0.0),
            &EntityType::COW,
        ));
        *mob.move_control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Box::new(ProbeControl(calls));
        ProbeMob {
            mob,
            immobile: false,
        }
    }

    #[tokio::test]
    async fn follow_range_updates_resize_the_search_budget_even_when_reverted() -> TestResult {
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        let mob = probe(world.clone(), Arc::new(AtomicUsize::new(0)));
        let living = &mob.mob.living_entity;
        *mob.mob
            .navigator
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Navigator::flying();
        let reaches_target = || {
            mob.mob
                .navigator
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .create_path(living, Vector3::new(8.0, 64.0, 0.0), 0)
                .is_some_and(|path| path.can_reach())
        };
        living.set_attribute_base(&Attributes::FOLLOW_RANGE, 0.0);
        let initial = reaches_target();
        living.set_attribute_base(&Attributes::FOLLOW_RANGE, 16.0);
        living.set_attribute_base(&Attributes::FOLLOW_RANGE, 0.0);
        living.refresh_dirty_attributes(&mob);
        let updated = reaches_target();
        world.level.shutdown().await;
        assert!(!initial);
        assert!(updated);
        Ok(())
    }

    #[tokio::test]
    async fn ai_step_preserves_controller_motion_after_input_preparation() -> TestResult {
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        let calls = Arc::new(AtomicUsize::new(0));
        let mob = probe(world.clone(), calls.clone());
        let observation = Arc::new(Mutex::new(Observation::default()));
        mob.mob
            .goals_selector
            .add_goal(0, Box::new(ObserveGoal(observation.clone())));
        let living = &mob.mob.living_entity;
        living.entity.velocity.store(Vector3::new(0.001, 0.0, 0.0));
        living.movement_input.store(Vector3::new(0.0, 0.25, 0.5));

        living.tick_movement(&mob);
        world.level.shutdown().await;

        assert_eq!(calls.load(Ordering::Relaxed), 1);
        let observed = observation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(observed.velocity.x, 0.0);
        assert_eq!(observed.input.y, 0.25);
        assert!((observed.input.z - 0.49).abs() < f64::EPSILON);
        assert!((living.entity.pos.load().x - 0.002).abs() < f64::EPSILON);
        assert_eq!(living.movement_input.load().z, 0.5);
        Ok(())
    }

    #[tokio::test]
    async fn no_ai_skips_controller_and_travel() -> TestResult {
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        let calls = Arc::new(AtomicUsize::new(0));
        let mob = probe(world.clone(), calls.clone());
        mob.mob.set_no_ai(true);
        let living = &mob.mob.living_entity;
        let position = living.entity.pos.load();
        living.entity.velocity.store(Vector3::new(0.1, 0.0, 0.0));

        living.tick_movement(&mob);
        world.level.shutdown().await;

        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert_eq!(living.entity.pos.load(), position);
        Ok(())
    }

    #[tokio::test]
    async fn goal_jump_survives_navigation_finish_and_expires_next_step() -> TestResult {
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        let mob = probe(world.clone(), Arc::new(AtomicUsize::new(0)));
        mob.mob
            .goals_selector
            .add_goal(0, Box::new(JumpOnceGoal(true)));
        let living = &mob.mob.living_entity;
        let position = living.entity.pos.load();
        mob.mob
            .navigator
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .set_progress(NavigatorGoal::new(position, position, 1.0));
        living.entity.on_ground.store(true, Ordering::Relaxed);

        living.tick_movement(&mob);
        let first_jumping = living.jumping.load(Ordering::Relaxed);
        let first_position = living.entity.pos.load();
        living.tick_movement(&mob);
        world.level.shutdown().await;

        assert!(first_jumping);
        assert!(first_position.y > position.y);
        assert!(!living.jumping.load(Ordering::Relaxed));
        assert_eq!(living.jumping_cooldown.load(Ordering::Relaxed), 0);
        Ok(())
    }

    #[tokio::test]
    async fn move_control_jump_is_applied_in_the_same_step() -> TestResult {
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        let mob = probe(world.clone(), Arc::new(AtomicUsize::new(0)));
        let living = &mob.mob.living_entity;
        let position = living.entity.pos.load();
        let mut control = MoveControl::default();
        control.set_wanted_position(position.x, position.y + 2.0, position.z, 1.0);
        *mob.mob
            .move_control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Box::new(control);
        living.entity.on_ground.store(true, Ordering::Relaxed);

        living.tick_movement(&mob);
        let first_jumping = living.jumping.load(Ordering::Relaxed);
        let first_position = living.entity.pos.load();
        living.tick_movement(&mob);
        world.level.shutdown().await;

        assert!(first_jumping);
        assert!(first_position.y > position.y);
        assert!(!living.jumping.load(Ordering::Relaxed));
        Ok(())
    }

    #[tokio::test]
    async fn navigation_hands_the_next_waypoint_to_the_installed_control() -> TestResult {
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        let mob = probe(world.clone(), Arc::new(AtomicUsize::new(0)));
        let target = Arc::new(Mutex::new(None));
        *mob.mob
            .move_control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Box::new(TargetControl(target.clone()));
        let living = &mob.mob.living_entity;
        living.entity.pos.store(Vector3::new(0.5, 64.0, 0.5));
        living.entity.last_pos.store(living.entity.pos.load());
        living.entity.on_ground.store(true, Ordering::Relaxed);
        let destination = Vector3::new(2.5, 64.0, 0.5);
        {
            let mut navigator = mob
                .mob
                .navigator
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            navigator.set_progress(NavigatorGoal::new(
                living.entity.pos.load(),
                destination,
                1.5,
            ));
            navigator.move_to_path(
                Some(Path::new(
                    vec![
                        Node::new(BlockPos::new(0, 64, 0)),
                        Node::new(BlockPos::new(2, 64, 0)),
                    ],
                    BlockPos::new(2, 64, 0),
                    true,
                )),
                1.5,
                living,
            );
        };

        living.tick_movement(&mob);
        world.level.shutdown().await;

        assert_eq!(
            target
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_ref()
                .map(|target| (target.position, target.speed_modifier)),
            Some((destination, 1.5))
        );
        assert_eq!(living.entity.yaw.load(), 0.0);
        assert_eq!(living.movement_input.load().z, 0.0);
        Ok(())
    }

    #[tokio::test]
    async fn finishing_navigation_stops_the_default_controller() -> TestResult {
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        let mob = probe(world.clone(), Arc::new(AtomicUsize::new(0)));
        *mob.mob
            .move_control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Box::new(MoveControl::default());
        let living = &mob.mob.living_entity;
        living.entity.pos.store(Vector3::new(0.5, 64.0, 0.5));
        living.entity.last_pos.store(living.entity.pos.load());
        living.entity.on_ground.store(true, Ordering::Relaxed);
        {
            let mut navigator = mob
                .mob
                .navigator
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            navigator.set_progress(NavigatorGoal::new(
                living.entity.pos.load(),
                Vector3::new(2.5, 64.0, 0.5),
                1.0,
            ));
            navigator.move_to_path(
                Some(Path::new(
                    vec![Node::new(BlockPos::new(2, 64, 0))],
                    BlockPos::new(2, 64, 0),
                    true,
                )),
                1.0,
                living,
            );
        };

        living.tick_movement(&mob);
        let moving_input = living.movement_input.load().z;
        mob.mob
            .navigator
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .stop();
        living.tick_movement(&mob);
        world.level.shutdown().await;

        assert!(moving_input > 0.0);
        assert_eq!(living.movement_input.load().z, 0.0);
        Ok(())
    }

    #[tokio::test]
    async fn dead_ai_step_clears_input_without_discarding_knockback() -> TestResult {
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        let mut results = Vec::new();
        for dead_flag in [false, true] {
            let calls = Arc::new(AtomicUsize::new(0));
            let mob = probe(world.clone(), calls.clone());
            let living = &mob.mob.living_entity;
            if dead_flag {
                living.dead.store(true, Ordering::Relaxed);
            } else {
                living.health.store(0.0);
            }
            living.entity.velocity.store(Vector3::new(0.1, 0.0, 0.0));
            living.movement_input.store(Vector3::new(0.5, 0.25, 0.5));
            living.jumping.store(true, Ordering::Relaxed);

            living.tick_movement(&mob);
            results.push((
                calls.load(Ordering::Relaxed),
                living.movement_input.load(),
                living.jumping.load(Ordering::Relaxed),
                living.entity.pos.load().x,
            ));
        }
        world.level.shutdown().await;

        for (calls, input, jumping, position_x) in results {
            assert_eq!(calls, 0);
            assert_eq!(input, Vector3::new(0.0, 0.25, 0.0));
            assert!(!jumping);
            assert!(position_x > 0.05);
        }
        Ok(())
    }
    #[tokio::test]
    async fn ground_control_speed_reaches_travel_without_losing_its_modifier() -> TestResult {
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        let mob = probe(world.clone(), Arc::new(AtomicUsize::new(0)));
        let living = &mob.mob.living_entity;
        living.set_attribute_base(&Attributes::MOVEMENT_SPEED, 0.4);
        living.entity.on_ground.store(true, Ordering::Relaxed);
        living.entity.set_has_no_gravity(true);
        living.movement_input.store(Vector3::new(0.3, 0.25, 0.0));
        let mut control = MoveControl::default();
        control.set_wanted_position(0.0, 64.0, 8.0, 0.5);
        control.tick(&mob);
        assert_eq!(living.movement_input.load().x, 0.3);
        assert_eq!(living.movement_input.load().y, 0.25);
        living
            .movement_input
            .store(Vector3::new(0.0, 0.0, living.movement_input.load().z));
        living.travel_in_air(&mob);
        let moved = living.entity.pos.load().z;
        world.level.shutdown().await;
        assert!((moved - 0.04).abs() < 1.0e-8);
        Ok(())
    }

    #[tokio::test]
    async fn strafe_checks_live_collision_before_publishing_original_inputs() -> TestResult {
        use pumpkin_data::Block;
        use pumpkin_util::math::vector2::Vector2;
        use pumpkin_world::{chunk::ChunkData, world::BlockFlags};
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        world
            .level
            .loaded_chunks
            .insert(Vector2::new(0, 0), ChunkData::empty_sync(0, 0));
        let mob = probe(world.clone(), Arc::new(AtomicUsize::new(0)));
        let living = &mob.mob.living_entity;
        living.entity.set_pos(Vector3::new(0.5, 64.0, 0.5));
        living.set_attribute_base(&Attributes::MOVEMENT_SPEED, 1.0);
        living.movement_input.store(Vector3::new(0.0, 0.25, 0.0));
        let flags = BlockFlags::SKIP_BLOCK_ADDED_CALLBACK | BlockFlags::UPDATE_KNOWN_SHAPE;
        world.set_block_state(
            &BlockPos::new(0, 63, 0),
            Block::STONE.default_state.id,
            flags,
        );
        let mut control = MoveControl::default();
        control.strafe(0.5, 0.5);
        control.tick(&mob);
        assert_eq!(living.movement_input.load(), Vector3::new(0.5, 0.25, 0.5));
        world.set_block_state(
            &BlockPos::new(0, 64, 0),
            Block::STONE.default_state.id,
            flags,
        );
        control.strafe(0.5, 0.5);
        control.tick(&mob);
        let input = living.movement_input.load();
        world.level.shutdown().await;
        assert_eq!(input, Vector3::new(0.0, 0.25, 1.0));
        Ok(())
    }

    #[tokio::test]
    async fn flying_control_preserves_strafe_and_clears_only_its_owned_axes() -> TestResult {
        use crate::entity::ai::control::flying_move_control::FlyingMoveControl;
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        let mob = probe(world.clone(), Arc::new(AtomicUsize::new(0)));
        let living = &mob.mob.living_entity;
        living.set_attribute_base(&Attributes::FLYING_SPEED, 0.8);
        living.movement_input.store(Vector3::new(0.3, 0.25, 0.0));
        let mut control = FlyingMoveControl::new(20, true);
        control.set_wanted_position(0.0, 65.0, 8.0, 0.5);
        control.tick(&mob);
        let moving = living.movement_input.load();
        assert_eq!(moving.x, 0.3);
        assert!((moving.y - 0.4).abs() < 1.0e-7);
        assert_eq!(moving.y, moving.z);
        control.tick(&mob);
        let idle = living.movement_input.load();
        world.level.shutdown().await;
        assert_eq!(idle, Vector3::new(0.3, 0.0, 0.0));
        Ok(())
    }

    #[tokio::test]
    async fn dolphin_water_travel_consumes_control_speed_and_unscaled_direction() -> TestResult {
        use crate::entity::passive::dolphin::DolphinEntity;
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        let dolphin = DolphinEntity::new(Entity::new(
            world.clone(),
            Vector3::new(0.0, 64.0, 0.0),
            &EntityType::DOLPHIN,
        ));
        let living = &dolphin.mob_entity.living_entity;
        living.set_attribute_base(&Attributes::MOVEMENT_SPEED, 0.4);
        living.entity.touching_water.store(true, Ordering::Relaxed);
        {
            let mut navigator = dolphin
                .mob_entity
                .navigator
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            navigator.move_to_path(
                Some(Path::new(
                    vec![Node::new(BlockPos::new(0, 64, 8))],
                    BlockPos::new(0, 64, 8),
                    true,
                )),
                0.5,
                living,
            )
        };
        let input = {
            let mut control = dolphin
                .mob_entity
                .move_control
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            control.set_wanted_position(0.0, 64.0, 8.0, 0.5);
            control.tick(&*dolphin);
            living.movement_input.load()
        };
        living.travel_in_fluid(&*dolphin, true);
        let position = living.entity.pos.load();
        let velocity = living.entity.velocity.load();
        world.level.loaded_chunks.insert(
            pumpkin_util::math::vector2::Vector2::new(0, 0),
            pumpkin_world::chunk::ChunkData::empty_sync(0, 0),
        );
        world.set_block_state(
            &BlockPos::new(0, 64, 1),
            pumpkin_data::Block::STONE.default_state.id,
            pumpkin_world::world::BlockFlags::SKIP_BLOCK_ADDED_CALLBACK
                | pumpkin_world::world::BlockFlags::UPDATE_KNOWN_SHAPE,
        );
        living.entity.set_pos(Vector3::new(0.5, 64.0, 0.5));
        living.entity.velocity.store(Vector3::new(0.0, 0.0, 1.0));
        living.travel_in_fluid(&*dolphin, true);
        let wall_velocity = living.entity.velocity.load();
        let wall_collision = living.entity.horizontal_collision.load(Ordering::Relaxed);
        world.level.shutdown().await;
        assert!(wall_collision);
        assert!(wall_velocity.y < 0.0);
        assert!((input.z - 0.2).abs() < 1.0e-7);
        assert!((position.z - 0.0008).abs() < 1.0e-9);
        assert!((velocity.z - 0.00072).abs() < 1.0e-9);
        assert!((velocity.y + 0.0005).abs() < 1.0e-9);
        Ok(())
    }
    #[tokio::test]
    async fn zero_air_drag_preserves_controller_velocity_in_travel() -> TestResult {
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        let mob = probe(world.clone(), Arc::new(AtomicUsize::new(0)));
        let living = &mob.mob.living_entity;
        living.set_attribute_base(&Attributes::AIR_DRAG_MODIFIER, 0.0);
        living.entity.set_has_no_gravity(true);
        living.entity.velocity.store(Vector3::new(1.0, 1.0, 1.0));
        living.travel_in_air(&mob);
        let velocity = living.entity.velocity.load();
        world.level.shutdown().await;
        assert_eq!(velocity, Vector3::new(1.0, 1.0, 1.0));
        Ok(())
    }

    #[tokio::test]
    async fn installed_bee_look_control_pauses_while_angry() -> TestResult {
        use crate::entity::passive::bee::BeeEntity;
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        let bee = BeeEntity::new(Entity::new(
            world.clone(),
            Vector3::new(0.0, 64.0, 0.0),
            &EntityType::BEE,
        ));
        let entity = &bee.mob_entity.living_entity.entity;
        entity.pitch.store(30.0);
        bee.neutral_data
            .anger_end_time
            .store(100, Ordering::Relaxed);
        let angry_pitch = {
            let mut look = bee
                .mob_entity
                .look_control
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            look.tick(&*bee);
            entity.pitch.load()
        };
        assert_eq!(angry_pitch, 30.0);
        bee.neutral_data.anger_end_time.store(-1, Ordering::Relaxed);
        let pitch = {
            let mut look = bee
                .mob_entity
                .look_control
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            look.tick(&*bee);
            entity.pitch.load()
        };
        world.level.shutdown().await;
        assert_eq!(pitch, 0.0);
        Ok(())
    }
    #[tokio::test]
    async fn immobile_living_mobs_skip_ai_without_discarding_knockback() -> TestResult {
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        let calls = Arc::new(AtomicUsize::new(0));
        let mut mob = probe(world.clone(), calls.clone());
        mob.immobile = true;
        let living = &mob.mob.living_entity;
        living.movement_input.store(Vector3::new(0.5, 0.25, 0.5));
        living.jumping.store(true, Ordering::Relaxed);
        living.entity.velocity.store(Vector3::new(0.1, 0.0, 0.0));
        living.tick_movement(&mob);
        world.level.shutdown().await;
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert!(!living.is_dead_or_dying());
        assert!(!living.jumping.load(Ordering::Relaxed));
        assert_eq!(living.movement_input.load(), Vector3::new(0.0, 0.25, 0.0));
        assert!(living.entity.pos.load().x > 0.05);
        Ok(())
    }

    #[tokio::test]
    async fn no_ai_pauses_species_decisions_while_age_still_advances() -> TestResult {
        use crate::entity::{ageable::AgeableMob, passive::bee::BeeEntity};
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        let bee = BeeEntity::new(Entity::new(
            world.clone(),
            Vector3::new(0.0, 64.0, 0.0),
            &EntityType::BEE,
        ));
        bee.set_age(-20);
        bee.set_has_stung(true);
        bee.time_since_sting.store(100, Ordering::Relaxed);
        bee.mob_entity.set_no_ai(true);
        bee.mob_tick(&*bee);
        bee.mob_entity.living_entity.tick_movement(&*bee);
        assert_eq!(bee.get_age(), -19);
        assert_eq!(bee.time_since_sting.load(Ordering::Relaxed), 100);
        bee.mob_entity.set_no_ai(false);
        bee.mob_tick(&*bee);
        bee.mob_entity.living_entity.tick_movement(&*bee);
        let age = bee.get_age();
        let sting = bee.time_since_sting.load(Ordering::Relaxed);
        world.level.shutdown().await;
        assert_eq!(age, -18);
        assert_eq!(sting, 101);
        Ok(())
    }

    #[tokio::test]
    async fn ageable_mobs_grow_once_per_ai_step_without_tick_count() -> TestResult {
        use crate::entity::{ageable::AgeableMob, passive::cow::CowEntity};
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        let cow = CowEntity::new(Entity::new(
            world.clone(),
            Vector3::new(0.0, 64.0, 0.0),
            &EntityType::COW,
        ));
        cow.set_age(-20);
        let entity = &cow.mob_entity.living_entity.entity;
        entity.tick_count.store(7, Ordering::Relaxed);
        cow.mob_entity.living_entity.tick_movement(&*cow);
        let age = cow.get_age();
        let tick_count = entity.tick_count.load(Ordering::Relaxed);
        world.level.shutdown().await;
        assert_eq!(age, -19);
        assert_eq!(tick_count, 7);
        Ok(())
    }
}
