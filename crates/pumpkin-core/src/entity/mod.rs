mod damage_rules;
mod entity_dimensions;
mod entity_state;
mod flags_and_pose;
mod fluids_and_fire;
mod leashes;
mod movement_physics;
mod portals_teleport;
mod position_facing;
mod push;
mod riding;
mod saved_data;
mod step_effects;
mod synced_state;
use crate::{
    entity::item::ItemEntity,
    net::{bedrock::BedrockClient, java::JavaClient},
    server::Server,
    world::{World, portal::PortalProcessor},
};
use arc_swap::ArcSwap;
use bytes::BufMut;
use crossbeam::atomic::AtomicCell;
use living::LivingEntity;
use player::Player;
use pumpkin_data::BlockState;
use pumpkin_data::biome::Biome;
use pumpkin_data::data_component_impl::EquipmentSlot;
use pumpkin_data::entity::EntityStatus;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::packet::CURRENT_MC_VERSION;
use pumpkin_data::tag::{self, Taggable};
use pumpkin_data::tracked_data;
use pumpkin_data::{
    damage::DamageType,
    entity::{EntityPose, EntityType, MovementEmission},
    sound::{Sound, SoundCategory},
};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_protocol::bedrock::client::CAddActor;
use pumpkin_protocol::codec::var_long::VarLong;
use pumpkin_protocol::{
    bedrock::client::set_actor_data::{
        PropertySyncData, SyncedActorDataList, entity_data_flag, entity_data_key,
    },
    codec::var_ulong::VarULong,
    java::client::play::{CSetEntityMetadata, Metadata},
};
use pumpkin_util::math::{
    boundingbox::{BoundingBox, EntityDimensions},
    get_section_cord,
    position::BlockPos,
    vector2::Vector2,
    vector3::Vector3,
};
use pumpkin_util::text::TextComponent;
use pumpkin_util::text::hover::HoverEvent;
use pumpkin_util::version::JavaMinecraftVersion;
use std::collections::HashSet;
use std::sync::{
    Arc,
    atomic::{
        AtomicBool, AtomicI32, AtomicU8, AtomicU32,
        Ordering::{self, Relaxed},
    },
};
use uuid::Uuid;

pub mod ageable;
pub mod ai;
pub mod area_effect_cloud;
pub mod attributes;
pub mod boss;
pub mod breath;
pub mod bucketable;
pub mod custom_sound;
pub mod decoration;
pub mod effect;
pub mod experience_orb;
pub mod falling;
pub mod hunger;
pub mod interaction;
pub mod item;
pub mod item_steerable;
pub mod lightning;
pub mod living;
pub mod marker;
pub mod mob;
pub mod passive;
pub mod player;
pub mod projectile;
pub mod projectile_deflection;
pub mod spawn_util;
pub mod synched_entity_data;
pub mod tnt;
pub mod r#type;
pub mod vehicle;

pub use lightning::LightningBoltEntity;

pub(crate) mod combat;
pub mod predicate;
#[cfg(test)]
mod push_golden;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelayedLeash {
    Entity(Uuid),
    Knot(BlockPos),
}

const LEASH_RESTORE_TIMEOUT: i32 = 100;

/// The maximum number of scoreboard tags an entity can carry, matching Vanilla.
pub const MAX_SCOREBOARD_TAGS: usize = 1024;

/// Push strength per axis in `Entity.push`.
pub(crate) const PUSH_STRENGTH: f64 = 0.05f32 as f64;

const MAX_LOADED_MOTION: f64 = 10.0;
const MAX_HORIZONTAL_POSITION: f64 = 3.000_051_2E7;
const MAX_VERTICAL_POSITION: f64 = 2.0E7;

/// Fluid current speeds of `updateFluidHeightAndDoFluidPushing`.
const WATER_PUSH_SPEED: f64 = 0.014;
const FAST_LAVA_PUSH_SPEED: f64 = 0.007;
const LAVA_PUSH_SPEED: f64 = 0.002_333_333;

/// Speed range of `moveTowardsClosestSpace`.
const PUSH_OUT_MIN_SPEED: f64 = 0.1;
const PUSH_OUT_RANDOM_SPEED: f64 = 0.2;
pub(crate) const MAX_PUSH_OUT_SPEED: f64 = PUSH_OUT_MIN_SPEED + PUSH_OUT_RANDOM_SPEED;

#[must_use]
pub(crate) const fn lava_push_speed(fast_lava: bool) -> f64 {
    if fast_lava {
        FAST_LAVA_PUSH_SPEED
    } else {
        LAVA_PUSH_SPEED
    }
}

/// Speed water and lava currents can add together in one tick.
#[must_use]
pub(crate) const fn max_fluid_push_speed(fast_lava: bool) -> f64 {
    WATER_PUSH_SPEED + lava_push_speed(fast_lava)
}

/// Returns the [`EntityStatus`] that should be broadcast when the given
/// equipment slot breaks.
#[must_use]
pub const fn equipment_break_status(slot: &EquipmentSlot) -> EntityStatus {
    match slot {
        EquipmentSlot::MainHand(_) => EntityStatus::MainhandBreak,
        EquipmentSlot::OffHand(_) => EntityStatus::OffhandBreak,
        EquipmentSlot::Head(_) => EntityStatus::HeadBreak,
        EquipmentSlot::Chest(_) => EntityStatus::ChestBreak,
        EquipmentSlot::Legs(_) => EntityStatus::LegsBreak,
        EquipmentSlot::Feet(_) => EntityStatus::FeetBreak,
        EquipmentSlot::Body(_) => EntityStatus::BodyBreak,
        EquipmentSlot::Saddle(_) => EntityStatus::SaddleBreak,
    }
}

impl dyn EntityBase + '_ {
    /// Inherent on the trait object so both sides can be `&dyn EntityBase`.
    #[must_use]
    pub fn is_allied_to(&self, other: &dyn EntityBase) -> bool {
        self.get_entity().entity_id == other.get_entity().entity_id
            || self.considers_entity_as_ally(other)
            || other.considers_entity_as_ally(self)
    }

    #[must_use]
    pub fn do_teams_allow_damage(&self, other: &dyn EntityBase) -> bool {
        let Some(team) = self.get_team_name() else {
            return true;
        };
        other.get_team_name().as_ref() != Some(&team)
            || self
                .get_team()
                .is_some_and(|team| team.is_allow_friendly_fire())
    }
}

/// Maps Java entity types to Bedrock runtime entity identifiers where the naming differs.
#[must_use]
pub const fn entity_type_to_bedrock_identifier(entity_type: &EntityType) -> &'static str {
    if entity_type.id == EntityType::EXPERIENCE_ORB.id {
        "minecraft:xp_orb"
    } else if entity_type.id == EntityType::FISHING_BOBBER.id {
        "minecraft:fishing_hook"
    } else if entity_type.id == EntityType::EVOKER_FANGS.id {
        "minecraft:evocation_fang"
    } else if entity_type.id == EntityType::TRIDENT.id {
        "minecraft:thrown_trident"
    } else if entity_type.id == EntityType::FIREWORK_ROCKET.id {
        "minecraft:fireworks_rocket"
    } else if entity_type.id == EntityType::EYE_OF_ENDER.id {
        "minecraft:eye_of_ender_signal"
    } else if entity_type.id == EntityType::ZOMBIE_VILLAGER.id {
        "minecraft:zombie_villager_v2"
    } else {
        entity_type.resource_name
    }
}

pub trait EntityBase: Send + Sync + std::any::Any {
    fn write_nbt(&self, nbt: &mut NbtCompound) {
        self.get_entity().write_nbt(nbt);
        if let Some(living) = self.get_living_entity() {
            living.write_living_nbt(nbt);
        }
        if let Some(owner) = self.get_projectile_owner() {
            owner.write_nbt(nbt);
        }
        self.write_custom_nbt(nbt);
        self.get_entity().write_passengers(nbt);
    }

    fn write_custom_nbt(&self, _nbt: &mut NbtCompound) {}

    fn read_nbt_non_mut(&self, nbt: &NbtCompound) {
        self.get_entity().read_nbt_non_mut(nbt);
        if let Some(living) = self.get_living_entity() {
            living.read_living_nbt_non_mut(nbt);
        }
        if let Some(owner) = self.get_projectile_owner() {
            owner.read_nbt(nbt);
        }
        self.read_custom_nbt(nbt);
    }

    fn read_custom_nbt(&self, _nbt: &NbtCompound) {}

    /// Explicit removal of top-level keys, since a missing key on read keeps the field.
    fn remove_nbt(&self, removed: &[Box<str>]) {
        if let Some(owner) = self.get_projectile_owner() {
            owner.remove_nbt(removed);
        }
        self.remove_custom_nbt(removed);
    }

    fn remove_custom_nbt(&self, _removed: &[Box<str>]) {}
    /// Called every tick for this entity.
    ///
    /// The `caller` parameter is a reference to the entity that initiated the tick.
    /// This can be the same entity the method is being called on (`self`),
    /// but in some scenarios (e.g., interactions or events), it might be a different entity.
    ///
    /// The `server` parameter provides access to the game server instance.
    fn tick(&self, caller: &dyn EntityBase, server: &Server) {
        if let Some(living) = self.get_living_entity() {
            living.tick(caller, server);
        } else {
            self.get_entity().tick(caller, server);
        }
    }

    fn get_job_site_pos(&self) -> Option<pumpkin_util::math::position::BlockPos> {
        None
    }

    fn get_home_pos(&self) -> Option<pumpkin_util::math::position::BlockPos> {
        None
    }

    #[must_use]
    fn resolve_dynamic_raw_id(
        &self,
        registry: &str,
        name: &str,
        static_fallback: impl FnOnce(&str) -> Option<u32>,
    ) -> Option<u32>
    where
        Self: Sized,
    {
        self.get_entity()
            .resolve_dynamic_raw_id(registry, name, static_fallback)
    }

    #[must_use]
    fn resolve_dynamic_entry_id(
        &self,
        registry: &str,
        raw_id: u32,
        static_fallback: impl FnOnce(u32) -> String,
    ) -> String
    where
        Self: Sized,
    {
        self.get_entity()
            .resolve_dynamic_entry_id(registry, raw_id, static_fallback)
    }

    fn as_any(&self) -> &dyn std::any::Any
    where
        Self: Sized,
    {
        self
    }

    fn get_item_steerable(&self) -> Option<&dyn crate::entity::item_steerable::ItemSteerable> {
        None
    }

    /// Who shot or threw this, for projectiles.
    fn get_projectile_owner(&self) -> Option<&projectile::owner::ProjectileOwner> {
        None
    }

    /// Vanilla `Projectile.getOwner`.
    fn get_owner_entity(&self) -> Option<Arc<dyn EntityBase>> {
        self.get_projectile_owner()?
            .get(&self.get_entity().world.load())
    }

    fn get_eye_pos(&self) -> Vector3<f64> {
        self.get_entity().get_eye_pos()
    }

    fn get_looking_vector(&self) -> Vector3<f64> {
        let entity = self.get_entity();
        Vector3::from_yaw_pitch(entity.yaw.load(), entity.pitch.load())
    }

    fn get_component(
        &self,
        _component: pumpkin_data::data_component::DataComponent,
    ) -> Option<Box<dyn pumpkin_data::data_component_impl::DataComponentImpl>> {
        None
    }

    fn init_data_tracker(&self) {
        let entity = self.get_entity();

        // If the internal age is negative, it's a baby
        let is_baby = entity.age.load(Ordering::Relaxed) < 0;

        if is_baby {
            let mut bedrock_meta = SyncedActorDataList::new();
            bedrock_meta.set_flag(entity_data_key::FLAGS, entity_data_flag::BABY as u8, true);
            entity.set_synced_data(tracked_data::ageable_mob::DATA_BABY_ID, true);
            entity.send_bedrock_actor_data(&bedrock_meta);
        }
    }
    fn set_variant_name(&self, _name: &str) {}
    fn apply_implicit_component(
        &self,
        _component: pumpkin_data::data_component::DataComponent,
        _value: &str,
    ) {
    }
    fn set_sound_variant_name(&self, _name: &str) {}

    fn teleport(
        &self,
        position: Vector3<f64>,
        yaw: Option<f32>,
        pitch: Option<f32>,
        world: Arc<World>,
    ) {
        self.get_entity().teleport(position, yaw, pitch, &world);
    }

    fn is_pushed_by_fluids(&self) -> bool {
        true
    }

    /// Whether the entity is immune from explosion knockback and damage
    fn is_immune_to_explosion(&self) -> bool {
        false
    }

    fn get_gravity(&self) -> f64 {
        0.0
    }

    /// Extra metadata to accompany this entity when spawned for Bedrock clients (e.g. falling block variant, TNT fuse).
    fn entity_bedrock_spawn_metadata(&self) -> Option<SyncedActorDataList> {
        None
    }

    /// Bedrock network Y above the feet. Endstone `getBaseOffset`.
    fn bedrock_y_offset(&self) -> f64 {
        0.0
    }

    fn bedrock_pos(&self) -> Vector3<f64> {
        self.get_entity()
            .pos
            .load()
            .add_raw(0.0, self.bedrock_y_offset(), 0.0)
    }

    /// What this entity can touch next tick; `None` means a conservative reach.
    fn audited_reach(&self) -> Option<crate::world::entity_islands::AuditedReach> {
        self.get_mob()
            .and_then(|mob| mob.get_mob_entity().audited_reach(mob))
    }

    fn get_mob(&self) -> Option<&dyn mob::Mob> {
        None
    }

    /// Players are tracked by profile name, every other entity by its UUID.
    fn get_scoreboard_name(&self) -> String {
        self.get_player().map_or_else(
            || self.get_entity().entity_uuid.to_string(),
            |player| player.gameprofile.name.clone(),
        )
    }

    fn get_team(&self) -> Option<crate::world::scoreboard::Team> {
        if let Some(player) = self.get_player() {
            return player.get_team();
        }
        let world = self.get_entity().world.load();
        let scoreboard = world
            .scoreboard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if scoreboard.get_teams().is_empty() {
            return None;
        }
        scoreboard
            .get_entity_team(&self.get_scoreboard_name())
            .cloned()
    }

    /// The team name alone, which is all the ally checks need. Worth having because
    /// they run once per candidate of every target search, and a `Team` is expensive
    /// to clone.
    fn get_team_name(&self) -> Option<String> {
        if let Some(player) = self.get_player() {
            return player.get_team_name();
        }
        let world = self.get_entity().world.load();
        let scoreboard = world
            .scoreboard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if scoreboard.get_teams().is_empty() {
            return None;
        }
        scoreboard
            .get_entity_team(&self.get_scoreboard_name())
            .map(|team| team.name.clone())
    }

    fn considers_entity_as_ally(&self, other: &dyn EntityBase) -> bool {
        if let Some(considered) = self
            .get_mob()
            .and_then(|mob| mob.mob_considers_entity_as_ally(other, None))
        {
            return considered;
        }
        if let Some(tamable) = self.get_mob().and_then(mob::Mob::as_tamable)
            && let Some(considered) = tamable.tamable_considers_entity_as_ally(other)
        {
            return considered;
        }
        self.is_on_same_team(other)
    }

    fn is_on_same_team(&self, other: &dyn EntityBase) -> bool {
        let Some(team) = self.get_team_name() else {
            return false;
        };
        other.get_team_name().is_some_and(|other| other == team)
    }

    fn tick_in_void(&self, _dyn_self: &dyn EntityBase) {
        self.get_entity().remove();
    }

    /// Returns if damage was successful or not
    fn damage(&self, caller: &dyn EntityBase, amount: f32, damage_type: DamageType) -> bool {
        caller.damage_with_context(caller, amount, damage_type, None, None, None)
    }

    fn on_lightning_strike(
        &self,
        caller: &dyn EntityBase,
        lightning: &lightning::LightningBoltEntity,
    ) {
        // Vanilla `Entity.thunderHit` hits entities with 5 damage and sets them on fire for 8 seconds.
        self.set_on_fire_for(8.0);
        let cause = lightning.get_cause();
        self.damage_with_context(
            caller,
            5.0,
            DamageType::LIGHTNING_BOLT,
            None,
            Some(lightning),
            cause.as_deref().map(|p| p as &dyn EntityBase),
        );
    }

    fn is_spectator(&self) -> bool {
        false
    }

    fn can_be_seen_by_anyone(&self) -> bool {
        self.get_living_entity()
            .is_some_and(|living| !self.is_spectator() && living.is_alive())
    }

    fn can_be_seen_as_enemy(&self) -> bool {
        self.get_living_entity()
            .is_some_and(|living| !living.entity.invulnerable.load(Ordering::Relaxed))
            && self.can_be_seen_by_anyone()
    }

    fn dampens_vibrations(&self) -> bool {
        false
    }

    fn is_collidable(&self, _entity: Option<Box<dyn EntityBase>>) -> bool {
        false
    }

    fn can_hit(&self) -> bool {
        false
    }

    fn is_immobile(&self) -> bool {
        self.get_living_entity()
            .is_some_and(LivingEntity::is_dead_or_dying)
    }

    fn get_controlling_passenger(&self) -> Option<Arc<dyn EntityBase>> {
        None
    }

    fn can_control_vehicle(&self) -> bool {
        !self
            .get_entity()
            .entity_type
            .has_tag(&tag::EntityType::MINECRAFT_NON_CONTROLLING_RIDER)
    }

    fn is_flutterer(&self) -> bool {
        false
    }

    fn set_sprinting(&self, is_sprinting: bool) {
        if let Some(living) = self.get_living_entity() {
            living.set_sprinting(is_sprinting);
        } else {
            self.get_entity().set_sprinting(is_sprinting);
        }
    }

    fn get_block_speed_factor(&self) -> f32 {
        self.get_living_entity().map_or_else(
            || self.get_entity().get_block_speed_factor(),
            LivingEntity::get_block_speed_factor,
        )
    }

    /// Vanilla `getSoundSource`.
    fn sound_source(&self) -> SoundCategory {
        self.get_entity().entity_type.sound_source
    }

    /// Vanilla `Entity.playSound`: at the entity, in its sound source, unless it is silent.
    fn play_entity_sound(&self, sound: Sound, volume: f32, pitch: f32) {
        let entity = self.get_entity();
        if !entity.is_silent() {
            entity.world.load().play_sound_fine(
                sound,
                self.sound_source(),
                &entity.pos.load(),
                volume,
                pitch,
            );
        }
    }

    /// Vanilla `getMovementEmission`.
    fn movement_emission(&self) -> MovementEmission {
        self.get_entity().entity_type.movement_emission
    }

    /// Vanilla `nextStep`: the `move_dist` at which the next step sound and event happen.
    fn next_step(&self) -> f32 {
        (self.get_entity().move_dist.load() as i32 + 1) as f32
    }

    /// Vanilla `playStepSound`.
    fn play_step_sound(&self, _pos: &BlockPos, state: &BlockState) {
        self.play_block_step_sound(state);
    }

    /// The base `Entity.playStepSound`: the step sound of the block walked on.
    fn play_block_step_sound(&self, state: &BlockState) {
        let sound_type = state.get_sound_type();
        self.play_entity_sound(
            sound_type.step_sound,
            sound_type.volume * 0.15,
            sound_type.pitch,
        );
    }

    /// Vanilla `getSwimSound`.
    fn get_swim_sound(&self) -> Option<Sound> {
        self.get_entity().entity_type.swim_sound
    }

    /// Vanilla `playSwimSound`.
    fn play_swim_sound(&self, volume: f32) {
        self.play_default_swim_sound(volume);
    }

    /// The base `Entity.playSwimSound`.
    fn play_default_swim_sound(&self, volume: f32) {
        if let Some(sound) = self.get_swim_sound() {
            let pitch = (rand::random::<f32>() - rand::random::<f32>()).mul_add(0.4, 1.0);
            self.play_entity_sound(sound, volume, pitch);
        }
    }

    /// Vanilla `getSwimSplashSound`.
    fn get_swim_splash_sound(&self) -> Option<Sound> {
        self.get_entity().entity_type.swim_splash_sound
    }

    /// Vanilla `getSwimHighSpeedSplashSound`.
    fn get_swim_high_speed_splash_sound(&self) -> Option<Sound> {
        self.get_entity().entity_type.swim_high_speed_splash_sound
    }

    /// Vanilla `doWaterSplashEffect`, run when the entity enters water.
    fn do_water_splash_effect(&self, caller: &dyn EntityBase) {
        self.get_entity().water_splash_effect(caller);
    }

    /// Vanilla `LivingEntity.getFallSounds`: the small and big fall damage sounds.
    fn get_fall_sounds(&self) -> (Sound, Sound) {
        let entity_type = self.get_entity().entity_type;
        (
            entity_type
                .small_fall_sound
                .unwrap_or(Sound::EntityGenericSmallFall),
            entity_type
                .big_fall_sound
                .unwrap_or(Sound::EntityGenericBigFall),
        )
    }

    /// Vanilla `isFlapping`: whether moving through the air plays `on_flap` and emits `FLAP`.
    fn is_flapping(&self) -> bool {
        false
    }

    /// Vanilla `onFlap`.
    fn on_flap(&self) {}

    /// Vanilla `isOnRails`: rail-riding entities step even when not on the ground.
    fn is_on_rails(&self) -> bool {
        false
    }

    /// Custom Y-axis velocity drag multiplier applied during `travel_in_air`.
    /// Bats return `Some(0.6)` to match vanilla's `travel()` override.
    fn get_y_velocity_drag(&self) -> Option<f64> {
        None
    }

    fn java_spawn_metadata(&self, version: JavaMinecraftVersion) -> Option<Box<[u8]>> {
        if version < JavaMinecraftVersion::V_1_9 {
            let entity = self.get_entity();
            let shared_flags = entity.flags.load(Ordering::Relaxed);
            return (shared_flags != 0).then(|| {
                // (0 << 5) | 0 = 0 (type: byte, index: 0 flags), value, 127 (terminator)
                Box::<[u8]>::from([0x00u8, shared_flags as u8, 127u8])
            });
        }
        self.get_mob().map_or_else(
            || {
                let entity = self.get_entity();
                let shared_flags = entity.flags.load(Ordering::Relaxed);
                (shared_flags != 0).then(|| {
                    let mut buf = Vec::new();
                    let _ = Metadata::new(
                        pumpkin_data::tracked_data::entity::DATA_SHARED_FLAGS_ID,
                        shared_flags,
                    )
                    .write(&mut buf, &version);
                    buf.put_u8(255);
                    buf.into_boxed_slice()
                })
            },
            |mob| mob.mob_java_spawn_metadata(version),
        )
    }

    fn send_bedrock_spawn_packet(&self, client: &BedrockClient) {
        let entity = self.get_entity();
        let runtime_id = entity.entity_id as u64;
        let identifier = self
            .get_mob()
            .and_then(mob::Mob::mob_bedrock_identifier)
            .unwrap_or_else(|| entity_type_to_bedrock_identifier(entity.entity_type));
        let mut metadata = entity.bedrock_metadata();
        if let Some(extra_meta) = self.entity_bedrock_spawn_metadata() {
            metadata.0.extend(extra_meta.0);
        }
        if let Some(mob) = self.get_mob()
            && let Some(mob_metadata) = mob.mob_bedrock_spawn_metadata()
        {
            metadata.0.extend(mob_metadata.0);
        }
        let packet = CAddActor {
            target_actor_id: VarLong(runtime_id as i64),
            target_runtime_id: VarULong(runtime_id),
            actor_type: identifier.to_string(),
            position: self.bedrock_pos().to_f32_lossy(),
            velocity: entity.velocity.load().to_f32_lossy(),
            rotation: Vector2::new(entity.pitch.load(), entity.yaw.load()),
            y_head_rotation: entity.head_yaw.load(),
            y_body_rotation: entity.body_yaw.load(),
            attributes_list: Vec::new(),
            actor_data: metadata,
            // TODO: Bedrock `climate_variant` property for pig/cow/chicken (`TemperatureVariantAnimal`)
            // empty -> client picks its own variant.
            synced_properties: PropertySyncData {
                int_entries_list: std::collections::HashMap::new(),
                float_entries_list: std::collections::HashMap::new(),
            },
            actor_links: Vec::new(),
            passenger_of_block: None,
        };
        if let Ok(data) = client.serialize_packet(&packet) {
            client.try_enqueue_packet(data);
        }
    }

    fn send_java_spawn_packet(&self, client: &JavaClient) {
        let entity = self.get_entity();
        let metadata = self.java_spawn_metadata(CURRENT_MC_VERSION);
        let spawn_packet = entity.create_spawn_packet();
        if let Ok(data) = client.serialize_packet(&spawn_packet) {
            client.try_enqueue_packet(data);
        }
        if let Some(meta) = metadata {
            let meta_packet = CSetEntityMetadata::new(entity.entity_id.into(), meta);
            if let Ok(meta_data) = client.serialize_packet(&meta_packet) {
                client.try_enqueue_packet(meta_data);
            }
        }
        let holder = entity
            .leashed_to
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(|holder| holder.get_entity().entity_id);
        if let Some(holder_id) = holder {
            let link_packet = pumpkin_protocol::java::client::play::CSetEntityLink::new(
                entity.entity_id,
                holder_id,
                true,
            );
            if let Ok(link_data) = client.serialize_packet(&link_packet) {
                client.try_enqueue_packet(link_data);
            }
        }
    }

    fn damage_with_context(
        &self,
        caller: &dyn EntityBase,
        amount: f32,
        damage_type: DamageType,
        position: Option<Vector3<f64>>,
        source: Option<&dyn EntityBase>,
        cause: Option<&dyn EntityBase>,
    ) -> bool {
        if let Some(living) = caller.get_living_entity() {
            return living.damage_with_context(
                caller,
                amount,
                damage_type,
                position,
                source,
                cause,
            );
        }
        false
    }

    /// Called when a player right-clicks this entity with an item.
    /// Called when a player right-clicks this entity with an item.
    /// Returns true if the interaction was handled.
    fn interact(&self, _player: &Arc<Player>, _item_stack: &mut ItemStack) -> bool {
        false
    }

    fn interact_with_hand(
        &self,
        _player: &Arc<Player>,
        _item_stack: &mut ItemStack,
        _hand: pumpkin_util::Hand,
    ) -> Option<bool> {
        None
    }

    fn set_on_fire_for(&self, seconds: f32) {
        let entity = self.get_entity();
        // Exclude fire-immune entities (ex. certain items) from burn damage
        if !entity.is_fire_immune() {
            self.set_on_fire_for_ticks((seconds * 20.0).floor() as u32);
        }
    }

    fn set_on_fire_for_ticks(&self, ticks: u32) {
        let entity = self.get_entity();
        let mut event = crate::plugin::api::events::entity::entity_combust::EntityCombustEvent::new(
            entity.entity_id,
            ticks as f32 / 20.0,
        );
        if let Some(server) = entity.world.load().server.upgrade() {
            server.plugin_manager.fire_blocking(&server, &mut event);
            if event.cancelled {
                return;
            }
        }
        if entity.fire_ticks.load(Ordering::Relaxed) < ticks as i32 {
            entity.fire_ticks.store(ticks as i32, Ordering::Relaxed);
        }
        // TODO: defrost
    }

    /// Called when a player collides with an entity
    fn on_player_collision(&self, _player: &Arc<Player>) {}

    fn is_passenger(&self) -> bool {
        self.get_entity().has_vehicle()
    }

    fn is_vehicle(&self) -> bool {
        self.get_entity().has_passengers()
    }

    fn has_passenger(&self, other: &dyn EntityBase) -> bool {
        self.get_entity()
            .has_passenger(other.get_entity().entity_id)
    }

    fn move_entity(&self, caller: &dyn EntityBase, motion: Vector3<f64>) {
        self.get_entity().move_entity(caller, motion);
    }

    fn is_pushable(&self) -> bool {
        false
    }

    fn push(&self, entity: &dyn EntityBase) {
        let self_entity = self.get_entity();
        let other_entity = entity.get_entity();

        if self_entity.no_physics.load(Ordering::Relaxed)
            || other_entity.no_physics.load(Ordering::Relaxed)
        {
            return;
        }

        if self_entity.has_passenger(other_entity.entity_id)
            || other_entity.has_passenger(self_entity.entity_id)
        {
            return;
        }

        let mut dx = other_entity.pos.load().x - self_entity.pos.load().x;
        let mut dz = other_entity.pos.load().z - self_entity.pos.load().z;
        let mut d = dx.abs().max(dz.abs());
        if d >= 0.01 {
            d = d.sqrt();
            dx /= d;
            dz /= d;
            let mut d2 = 1.0 / d;
            if d2 > 1.0 {
                d2 = 1.0;
            }
            dx *= d2;
            dz *= d2;
            dx *= PUSH_STRENGTH;
            dz *= PUSH_STRENGTH;

            if !self_entity.has_passengers() && self.is_pushable() {
                let mut vel = self_entity.velocity.load();
                vel.x -= dx;
                vel.z -= dz;
                self_entity.velocity.store(vel);
                self_entity.velocity_dirty.store(true, Ordering::SeqCst);
            }

            if !other_entity.has_passengers() && entity.is_pushable() {
                let mut vel = other_entity.velocity.load();
                vel.x += dx;
                vel.z += dz;
                other_entity.velocity.store(vel);
                other_entity.velocity_dirty.store(true, Ordering::SeqCst);
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn push_entities(&self, dyn_self: &dyn EntityBase) -> bool {
        let self_entity = self.get_entity();
        let entity_bb = self_entity.bounding_box.load();

        if !self.is_pushable() {
            return false;
        }

        let world = self_entity.world.load();

        let is_rideable_minecart = self_entity.entity_type.id == EntityType::MINECART.id;

        if push::is_minecart_type(self_entity.entity_type.id) {
            let velocity = self_entity.velocity.load();
            let push = push::MinecartPush {
                is_rideable_minecart,
                is_vehicle: self.is_vehicle(),
                speed_squared: velocity.x * velocity.x + velocity.z * velocity.z,
            };
            push::minecart_push_and_pickup(self, dyn_self, &world, &entity_bb, push)
        } else {
            push::push_overlapping(dyn_self, self_entity, &world, &entity_bb);

            false
        }
    }

    fn on_hit(&self, _hit: crate::entity::projectile::ProjectileHit) {}

    fn set_paddle_state(&self, _left: bool, _right: bool) {}

    fn is_in_love(&self) -> bool {
        false
    }

    fn is_breeding_ready(&self) -> bool {
        false
    }

    fn reset_love(&self) {}

    fn set_breeding_cooldown(&self, _ticks: i32) {}

    fn is_panicking(&self) -> bool {
        false
    }

    fn get_entity(&self) -> &Entity;

    fn get_living_entity(&self) -> Option<&LivingEntity>;

    fn cast_any(&self) -> &dyn std::any::Any;

    fn get_item_entity(&self) -> Option<&ItemEntity> {
        None
    }

    fn get_player(&self) -> Option<&Player> {
        None
    }

    /// Should return the name of the entity without click or hover events.
    fn get_name(&self) -> TextComponent {
        let entity = self.get_entity();
        entity
            .custom_name
            .load()
            .as_ref()
            .clone()
            .unwrap_or(TextComponent::translate_cross(
                format!("entity.minecraft.{}", entity.entity_type.resource_name),
                format!("entity.minecraft.{}", entity.entity_type.resource_name),
                [],
            ))
    }

    fn get_display_name(&self) -> TextComponent {
        // TODO: team color
        let entity = self.get_entity();
        let mut name =
            entity
                .custom_name
                .load()
                .as_ref()
                .clone()
                .unwrap_or(TextComponent::translate_cross(
                    format!("entity.minecraft.{}", entity.entity_type.resource_name),
                    format!("entity.minecraft.{}", entity.entity_type.resource_name),
                    [],
                ));
        let name_clone = name.clone();
        name = name.hover_event(HoverEvent::show_entity(
            entity.entity_uuid.to_string(),
            entity.entity_type.resource_name.into(),
            Some(name_clone),
        ));
        name = name.insertion(entity.entity_uuid.to_string());
        name
    }

    /// Kills the Entity.
    fn kill(&self, caller: &dyn EntityBase) {
        if self.get_living_entity().is_some() {
            caller.damage(caller, f32::MAX, DamageType::GENERIC_KILL);
        } else {
            // TODO this should be removed once all entities are implemented
            self.get_entity().remove();
        }
    }

    fn get_experience_reward(&self, _killer: Option<&dyn EntityBase>) -> u32 {
        0
    }

    fn should_drop_loot(&self) -> bool {
        self.get_living_entity()
            .is_none_or(LivingEntity::should_drop_loot)
    }

    fn should_drop_experience(&self) -> bool {
        self.get_living_entity()
            .is_none_or(LivingEntity::should_drop_experience)
    }

    fn get_base_experience_reward(&self) -> u32 {
        0
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum RemovalReason {
    Killed,
    Discarded,
    UnloadedToChunk,
    UnloadedWithPlayer,
    ChangedDimension,
}

impl RemovalReason {
    #[must_use]
    pub const fn should_destroy(&self) -> bool {
        match self {
            Self::Killed | Self::Discarded => true,
            Self::UnloadedToChunk | Self::UnloadedWithPlayer | Self::ChangedDimension => false,
        }
    }

    #[must_use]
    pub const fn should_save(&self) -> bool {
        match self {
            Self::Killed | Self::Discarded | Self::UnloadedWithPlayer | Self::ChangedDimension => {
                false
            }
            Self::UnloadedToChunk => true,
        }
    }
}

// IMPORTANT: have that 1 and not 0 because fetch_add returns previous value and 0 would be invalid
static CURRENT_ID: AtomicI32 = AtomicI32::new(1);

/// Position, motion and packed rotation for spawn packets.
struct SpawnState {
    pos: Vector3<f64>,
    velocity: Vector3<f64>,
    pitch: u8,
    yaw: u8,
    head_yaw: u8,
}

/// Represents a non-living Entity (e.g. Item, Egg, Snowball...)
#[repr(C)]
pub struct Entity {
    /// A unique identifier for the entity
    pub entity_id: i32,
    /// A persistent, unique identifier for the entity
    pub entity_uuid: uuid::Uuid,
    /// The type of entity (e.g., player, zombie, item)
    pub entity_type: &'static EntityType,
    /// The world in which the entity exists.
    /// Uses `ArcSwap` to allow atomic updates when changing dimensions.
    pub world: ArcSwap<World>,
    /// The entity's current position in the world
    pub pos: AtomicCell<Vector3<f64>>,
    /// The last known position of the entity.
    pub last_pos: AtomicCell<Vector3<f64>>,
    /// The last movement vector
    pub movement: AtomicCell<Vector3<f64>>,
    /// The entity's position rounded to the nearest block coordinates
    pub block_pos: AtomicCell<BlockPos>,
    /// The block supporting the entity
    pub supporting_block_pos: AtomicCell<Option<BlockPos>>,
    /// The chunk coordinates of the entity's current position
    pub chunk_pos: AtomicCell<Vector2<i32>>,
    /// Indicates whether the entity is sneaking
    pub sneaking: AtomicBool,
    /// Indicates whether the entity is sprinting
    pub sprinting: AtomicBool,
    /// Indicates whether the entity is swimming
    pub swimming: AtomicBool,
    /// Indicates whether the entity is invisible
    pub invisible: AtomicBool,
    /// Indicates whether the entity is glowing
    pub glowing: AtomicBool,
    pub has_glowing_tag: AtomicBool,
    pub glowing_from_effect: AtomicBool,
    /// Indicates whether the entity is flying due to a fall
    pub fall_flying: AtomicBool,
    /// The entity's current velocity vector, aka knockback
    pub velocity: AtomicCell<Vector3<f64>>,
    /// Tracks a horizontal collision
    pub horizontal_collision: AtomicBool,
    pub vertical_collision: AtomicBool,
    /// Indicates whether the entity is on the ground (may not always be accurate).
    pub on_ground: AtomicBool,
    /// Indicates whether the entity is touching water
    pub touching_water: AtomicBool,
    /// Indicates the fluid height
    pub water_height: AtomicCell<f64>,
    /// Indicates whether the entity is touching lava
    pub touching_lava: AtomicBool,
    /// Indicates the fluid height
    pub lava_height: AtomicCell<f64>,
    /// The entity's yaw rotation (horizontal rotation) ← →
    pub yaw: AtomicCell<f32>,
    /// The entity's head yaw rotation (horizontal rotation of the head)
    pub head_yaw: AtomicCell<f32>,
    /// The entity's body yaw rotation (horizontal rotation of the body)
    pub body_yaw: AtomicCell<f32>,
    /// The entity's pitch rotation (vertical rotation) ↑ ↓
    pub pitch: AtomicCell<f32>,
    /// The entity's current pose (e.g., standing, sitting, swimming).
    pub pose: AtomicCell<EntityPose>,
    /// The bounding box of an entity (hitbox)
    pub bounding_box: EntityBoundingBox,
    ///The size (width and height) of the bounding box
    pub entity_dimension: AtomicCell<EntityDimensions>,
    default_dimensions: AtomicCell<Option<EntityDimensions>>,
    /// Whether this entity is invulnerable to all damage
    pub invulnerable: AtomicBool,
    /// List of damage types this entity is immune to
    pub damage_immunities: std::sync::Mutex<Vec<DamageType>>,
    // Whether the entity is immune to fire (to disable visual fire and fire damage)
    pub fire_immune: AtomicBool,
    pub fire_ticks: AtomicI32,
    pub has_visual_fire: AtomicBool,
    /// The number of ticks the entity has been frozen (in powder snow)
    /// Max is 140 ticks (7 seconds). Increases by 1/tick in powder snow, decreases by 2/tick outside.
    pub frozen_ticks: AtomicI32,
    /// Set during block-collision processing when the entity is touching powder snow.
    pub is_in_powder_snow: AtomicBool,
    /// True if the entity was in powder snow during the previous tick.
    pub was_in_powder_snow: AtomicBool,
    pub removal_reason: AtomicCell<Option<RemovalReason>>,
    // The passengers that entity has
    pub passengers: std::sync::Mutex<Vec<Arc<dyn EntityBase>>>,
    /// The vehicle that entity is in
    pub vehicle: std::sync::Mutex<Option<Arc<dyn EntityBase>>>,
    /// The entity this entity is attached/leashed to (if any)
    pub leashed_to: std::sync::Mutex<Option<Arc<dyn EntityBase>>>,
    pub delayed_leash: std::sync::Mutex<Option<DelayedLeash>>,
    /// Cooldown before entity can mount again after dismounting
    pub riding_cooldown: AtomicI32,
    /// Vanilla `Entity.tickCount`: ticks this entity has been ticked.
    pub tick_count: AtomicI32,
    /// Vanilla `moveDist`, `flyDist` and `nextStep`, which pace step sounds and movement game events.
    pub move_dist: AtomicCell<f32>,
    pub fly_dist: AtomicCell<f32>,
    pub next_step: AtomicCell<f32>,
    crystal_sound_intensity: AtomicCell<f32>,
    last_crystal_sound_play_tick: AtomicI32,
    /// Vanilla `AgeableMob.age`. Negative values indicate a baby.
    pub age: AtomicI32,

    pub current_biome: ArcSwap<&'static Biome>,
    pub last_biome_update_pos: AtomicCell<BlockPos>,

    pub portal_cooldown: AtomicU32,

    pub portal_manager: std::sync::Mutex<Option<PortalProcessor>>,
    /// Custom name for the entity
    pub custom_name: ArcSwap<Option<TextComponent>>,
    /// Indicates whether the entity's custom name is visible
    pub custom_name_visible: AtomicBool,
    pub blocks_building: bool,
    pub silent: AtomicBool,
    pub air_supply: AtomicI32,
    pub has_no_gravity: AtomicBool,
    /// Scoreboard tags attached to this entity, managed with `/tag`.
    /// Vanilla allows at most [`MAX_SCOREBOARD_TAGS`] tags per entity.
    pub scoreboard_tags: std::sync::Mutex<HashSet<String>>,
    /// The data send in the Entity Spawn packet
    pub data: AtomicI32,
    /// Stores entity boolean flags (on fire, sneaking, invisible, glowing, etc.)
    pub flags: std::sync::atomic::AtomicI8,
    /// Stores Bedrock-specific entity boolean flags (bit 0-63)
    pub bedrock_flags: std::sync::atomic::AtomicI64,
    /// Stores more Bedrock-specific entity boolean flags (bit 0-63)
    pub bedrock_flags_two: std::sync::atomic::AtomicI64,
    /// If true, the entity bypasses physics, collisions, and block effects (e.g. spectator, markers, display entities)
    pub no_physics: AtomicBool,
    /// Vanilla `firstTick`: true until the first base tick has run.
    pub first_tick: AtomicBool,
    pub synched_data: synched_entity_data::SynchedEntityData,
    /// Multiplies movement for one tick before being reset
    pub movement_multiplier: AtomicCell<Vector3<f64>>,
    /// Vanilla `needsSync`: tracker resyncs position and velocity of entities
    pub velocity_dirty: AtomicBool,
    /// Set when an Entity is to be removed but could still be referenced
    pub removed: AtomicBool,
    /// The last sent yaw value (encoded as u8) for change detection
    pub last_sent_yaw: AtomicU8,
    /// The last sent pitch value (encoded as u8) for change detection
    pub last_sent_pitch: AtomicU8,
    /// Delta base of the move packets. Vanilla `ServerEntity.positionCodec`.
    pub last_sent_pos: AtomicCell<Vector3<f64>>,
    /// Cache for the last sent head yaw byte
    pub last_sent_head_yaw: AtomicU8,
    /// Velocity last put on the wire. Vanilla `ServerEntity.lastSentMovement`.
    pub last_sent_velocity: AtomicCell<Vector3<f64>>,
    /// Persistent custom data container for plugins
    pub custom_data: std::sync::Mutex<NbtCompound>,
}

/// Entity bounds that keep the world's spatial queries current when written.
pub struct EntityBoundingBox {
    value: AtomicCell<BoundingBox>,
    index: std::sync::RwLock<std::sync::Weak<crate::world::EntityIndex>>,
    indexed: bool,
    entity_id: i32,
}

impl EntityBoundingBox {
    fn new(world: &World, entity_id: i32, entity_type: &EntityType, bounds: BoundingBox) -> Self {
        let index = if entity_type == &EntityType::PLAYER {
            std::sync::Weak::new()
        } else {
            Arc::downgrade(&world.entity_index)
        };
        Self {
            value: AtomicCell::new(bounds),
            index: std::sync::RwLock::new(index),
            indexed: entity_type != &EntityType::PLAYER,
            entity_id,
        }
    }

    /// Returns the current bounds.
    #[must_use]
    pub fn load(&self) -> BoundingBox {
        self.value.load()
    }

    /// Publishes bounds and their spatial index membership together.
    pub fn store(&self, bounds: BoundingBox) {
        if !self.indexed {
            self.value.store(bounds);
            return;
        }
        let index = self
            .index
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(index) = index.upgrade() {
            index.set_bounds(self.entity_id, &self.value, bounds);
        } else {
            self.value.store(bounds);
        }
    }

    fn set_index(&self, index: &Arc<crate::world::EntityIndex>) {
        let mut owner = self
            .index
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *owner = Arc::downgrade(index);
        index.set_bounds(self.entity_id, &self.value, self.value.load());
    }
}

impl Entity {
    #[must_use]
    pub fn get_server(&self) -> Option<std::sync::Arc<Server>> {
        self.world.load().server.upgrade()
    }

    #[must_use]
    pub fn resolve_dynamic_raw_id(
        &self,
        registry: &str,
        name: &str,
        static_fallback: impl FnOnce(&str) -> Option<u32>,
    ) -> Option<u32> {
        self.get_server()
            .and_then(|s| {
                s.datapack_manager
                    .get_dynamic_registry_raw_id(registry, name)
            })
            .or_else(|| static_fallback(name))
    }

    #[must_use]
    pub fn resolve_dynamic_entry_id(
        &self,
        registry: &str,
        raw_id: u32,
        static_fallback: impl FnOnce(u32) -> String,
    ) -> String {
        self.get_server()
            .and_then(|s| {
                s.datapack_manager
                    .get_dynamic_registry_entry_id(registry, raw_id)
            })
            .unwrap_or_else(|| static_fallback(raw_id))
    }

    pub fn new(
        world: Arc<World>,
        position: Vector3<f64>,
        entity_type: &'static EntityType,
    ) -> Self {
        Self::from_uuid(Uuid::new_v4(), world, position, entity_type)
    }

    pub fn reserve_ids(count: i32) -> i32 {
        CURRENT_ID.fetch_add(count, Relaxed)
    }

    pub fn from_uuid(
        entity_uuid: uuid::Uuid,
        world: Arc<World>,
        position: Vector3<f64>,
        entity_type: &'static EntityType,
    ) -> Self {
        Self::from_uuid_with_id(
            CURRENT_ID.fetch_add(1, Relaxed),
            entity_uuid,
            world,
            position,
            entity_type,
        )
    }

    pub fn from_uuid_with_id(
        entity_id: i32,
        entity_uuid: uuid::Uuid,
        world: Arc<World>,
        position: Vector3<f64>,
        entity_type: &'static EntityType,
    ) -> Self {
        let floor_x = position.x.floor() as i32;
        let floor_y = position.y.floor() as i32;
        let floor_z = position.z.floor() as i32;

        let bounding_box_size = Self::type_dimensions(entity_type);

        let current_biome = world
            .level
            .get_rough_biome(&BlockPos::new(floor_x, floor_y, floor_z));

        let bounding_box = EntityBoundingBox::new(
            &world,
            entity_id,
            entity_type,
            BoundingBox::new_from_pos(position.x, position.y, position.z, &bounding_box_size),
        );

        Self {
            entity_id,
            entity_uuid,
            entity_type,
            on_ground: AtomicBool::new(false),
            touching_water: AtomicBool::new(false),
            water_height: AtomicCell::new(0.0),
            touching_lava: AtomicBool::new(false),
            lava_height: AtomicCell::new(0.0),
            horizontal_collision: AtomicBool::new(false),
            vertical_collision: AtomicBool::new(false),
            pos: AtomicCell::new(position),
            last_pos: AtomicCell::new(position),
            movement: AtomicCell::new(Vector3::default()),
            block_pos: AtomicCell::new(BlockPos(Vector3::new(floor_x, floor_y, floor_z))),
            supporting_block_pos: AtomicCell::new(None),
            chunk_pos: AtomicCell::new(Vector2::new(
                get_section_cord(floor_x),
                get_section_cord(floor_z),
            )),
            sneaking: AtomicBool::new(false),
            swimming: AtomicBool::new(false),
            invisible: AtomicBool::new(false),
            glowing: AtomicBool::new(false),
            has_glowing_tag: AtomicBool::new(false),
            glowing_from_effect: AtomicBool::new(false),
            world: ArcSwap::new(world),
            sprinting: AtomicBool::new(false),
            fall_flying: AtomicBool::new(false),
            yaw: AtomicCell::new(0.0),
            head_yaw: AtomicCell::new(0.0),
            body_yaw: AtomicCell::new(0.0),
            pitch: AtomicCell::new(0.0),
            velocity: AtomicCell::new(Vector3::new(0.0, 0.0, 0.0)),
            pose: AtomicCell::new(EntityPose::Standing),
            bounding_box,
            entity_dimension: AtomicCell::new(bounding_box_size),
            default_dimensions: AtomicCell::new(None),
            invulnerable: AtomicBool::new(false),
            damage_immunities: std::sync::Mutex::new(Vec::new()),
            data: AtomicI32::new(0),
            flags: std::sync::atomic::AtomicI8::new(0),
            bedrock_flags: std::sync::atomic::AtomicI64::new(0),
            bedrock_flags_two: std::sync::atomic::AtomicI64::new(0),
            fire_immune: AtomicBool::new(false),
            fire_ticks: AtomicI32::new(-1),
            has_visual_fire: AtomicBool::new(false),
            frozen_ticks: AtomicI32::new(0),
            is_in_powder_snow: AtomicBool::new(false),
            was_in_powder_snow: AtomicBool::new(false),
            removal_reason: AtomicCell::new(None),
            passengers: std::sync::Mutex::new(Vec::new()),
            vehicle: std::sync::Mutex::new(None),
            leashed_to: std::sync::Mutex::new(None),
            delayed_leash: std::sync::Mutex::new(None),

            riding_cooldown: AtomicI32::new(0),
            tick_count: AtomicI32::new(0),
            move_dist: AtomicCell::new(0.0),
            fly_dist: AtomicCell::new(0.0),
            next_step: AtomicCell::new(1.0),
            crystal_sound_intensity: AtomicCell::new(0.0),
            last_crystal_sound_play_tick: AtomicI32::new(0),
            age: AtomicI32::new(0),
            current_biome: ArcSwap::new(Arc::new(current_biome)),
            last_biome_update_pos: AtomicCell::new(BlockPos::new(floor_x, floor_y, floor_z)),
            portal_cooldown: AtomicU32::new(0),
            portal_manager: std::sync::Mutex::new(None),
            custom_name: ArcSwap::new(Arc::new(None)),
            custom_name_visible: AtomicBool::new(false),
            blocks_building: false,
            silent: AtomicBool::new(false),
            air_supply: AtomicI32::new(breath::MAX_AIR),
            has_no_gravity: AtomicBool::new(false),
            scoreboard_tags: std::sync::Mutex::new(HashSet::new()),
            no_physics: AtomicBool::new(false),
            first_tick: AtomicBool::new(true),
            synched_data: synched_entity_data::SynchedEntityData::new(),
            movement_multiplier: AtomicCell::new(Vector3::default()),
            velocity_dirty: AtomicBool::new(true),
            removed: AtomicBool::new(false),
            last_sent_yaw: AtomicU8::new(0),
            last_sent_pitch: AtomicU8::new(0),
            last_sent_head_yaw: AtomicU8::new(0),
            last_sent_pos: AtomicCell::new(position),
            last_sent_velocity: AtomicCell::new(Vector3::default()),
            custom_data: std::sync::Mutex::new(NbtCompound::new()),
        }
    }

    // Part of LivingEntity.tickMovement() in yarn

    // updateWaterState() in yarn

    // Entity.updateVelocity in yarn

    // Entity.movementInputToVelocity in yarn

    // Move by a delta, adjust for collisions, and send

    /// Maximum freeze ticks (7 seconds at 20 tps)
    pub const MAX_FROZEN_TICKS: i32 = 140;

    /// Freeze damage is dealt every 40 ticks when fully frozen
    const FREEZE_DAMAGE_INTERVAL: i32 = 40;

    pub const LEASH_SNAP_DISTANCE: f64 = 12.0;
    pub const LEASH_ELASTIC_DISTANCE: f64 = 6.0;
}

impl Entity {}

impl EntityBase for Entity {
    fn tick(&self, caller: &dyn EntityBase, _server: &Server) {
        // Recomputed during movement/block-collision handling in the same tick.
        let was_in_powder_snow = self.is_in_powder_snow.load(Ordering::Relaxed);
        self.was_in_powder_snow
            .store(was_in_powder_snow, Ordering::Relaxed);
        self.is_in_powder_snow.store(false, Ordering::Relaxed);

        self.update_last_pos();
        self.tick_portal(caller);
        kvasir_tickprof::parts::Part::FluidState.time(|| self.update_fluid_state(caller));
        self.check_out_of_world(caller);
        let fire_ticks = self.fire_ticks.load(Ordering::Relaxed);

        // Check for fire immunity (or if the specific entity is)
        let is_immune = self.is_fire_immune();
        if fire_ticks > 0 {
            if is_immune {
                self.fire_ticks.store(fire_ticks - 4, Ordering::Relaxed);
                if self.fire_ticks.load(Ordering::Relaxed) < 0 {
                    self.extinguish();
                }
            } else {
                // lava deals its own damage.
                if fire_ticks % 20 == 0 && !self.touching_lava.load(Ordering::SeqCst) {
                    caller.damage(caller, 1.0, DamageType::ON_FIRE);
                }

                self.fire_ticks.store(fire_ticks - 1, Ordering::Relaxed);
            }
        }

        // Check if visual fire should be sent
        let should_render_fire = self.fire_ticks.load(Ordering::Relaxed) > 0 && !is_immune;
        self.set_on_fire(should_render_fire);

        let riding_cooldown = self.riding_cooldown.load(Ordering::Relaxed);
        if riding_cooldown > 0 {
            self.riding_cooldown
                .store(riding_cooldown - 1, Ordering::Relaxed);
        }
        self.first_tick.store(false, Ordering::Relaxed);
    }

    fn get_entity(&self) -> &Entity {
        self
    }

    fn get_living_entity(&self) -> Option<&LivingEntity> {
        None
    }

    fn cast_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl<T: EntityBase + ?Sized> NBTStorage for T {
    fn write_nbt(&self, nbt: &mut NbtCompound) {
        EntityBase::write_nbt(self, nbt);
    }

    fn read_nbt_non_mut(&self, nbt: &NbtCompound) {
        EntityBase::read_nbt_non_mut(self, nbt);
    }
}

pub trait NBTStorage: Send + Sync {
    fn write_nbt(&self, _nbt: &mut NbtCompound) {}

    fn read_nbt(&mut self, nbt: &mut NbtCompound) {
        self.read_nbt_non_mut(nbt);
    }

    fn read_nbt_non_mut(&self, _nbt: &NbtCompound) {}
}

pub trait NBTStorageInit: Send + Sync + Sized {
    fn create_from_nbt(_nbt: &mut NbtCompound) -> Option<Self> {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Represents various entity flags that are sent in entity metadata.
///
/// These flags are used by the client to modify the rendering of entities based on their current state.
///
/// **Purpose:**
///
/// This enum provides a more type-safe and readable way to represent entity flags compared to using raw integer values.
pub enum Flag {
    /// Indicates if the entity is on fire.
    OnFire = 0,
    /// Indicates if the entity is sneaking.
    Sneaking = 1,
    /// Indicates if the entity is sprinting.
    Sprinting = 3,
    /// Indicates if the entity is swimming.
    Swimming = 4,
    /// Indicates if the entity is invisible.
    Invisible = 5,
    /// Indicates if the entity is glowing.
    Glowing = 6,
    /// Indicates if the entity is flying due to a fall.
    FallFlying = 7,
}

impl Flag {
    #[must_use]
    pub const fn to_bedrock(&self) -> Option<u32> {
        match self {
            Self::OnFire => Some(entity_data_flag::ON_FIRE),
            Self::Sneaking => Some(entity_data_flag::SNEAKING),
            Self::Sprinting => Some(entity_data_flag::SPRINTING),
            Self::Swimming => Some(entity_data_flag::SWIMMING),
            Self::Invisible => Some(entity_data_flag::INVISIBLE),
            Self::FallFlying => Some(entity_data_flag::GLIDING),
            Self::Glowing => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equipment_break_status_maps_all_slots() {
        // Status bytes from vanilla EntityEvent: mainhand=47, offhand=48,
        // head=49, chest=50, legs=51, feet=52, body=65, saddle=68.
        let cases: &[(&EquipmentSlot, u8)] = &[
            (&EquipmentSlot::MAIN_HAND, EntityStatus::MainhandBreak as u8),
            (&EquipmentSlot::OFF_HAND, EntityStatus::OffhandBreak as u8),
            (&EquipmentSlot::HEAD, EntityStatus::HeadBreak as u8),
            (&EquipmentSlot::CHEST, EntityStatus::ChestBreak as u8),
            (&EquipmentSlot::LEGS, EntityStatus::LegsBreak as u8),
            (&EquipmentSlot::FEET, EntityStatus::FeetBreak as u8),
            (&EquipmentSlot::BODY, EntityStatus::BodyBreak as u8),
            (&EquipmentSlot::SADDLE, EntityStatus::SaddleBreak as u8),
        ];
        for (i, (slot, expected)) in cases.iter().enumerate() {
            assert_eq!(
                equipment_break_status(slot) as u8,
                *expected,
                "status mismatch at index {i}"
            );
        }
    }

    #[test]
    fn entity_type_bedrock_identifier_mappings() {
        assert_eq!(
            entity_type_to_bedrock_identifier(&EntityType::EXPERIENCE_ORB),
            "minecraft:xp_orb"
        );
        assert_eq!(
            entity_type_to_bedrock_identifier(&EntityType::FISHING_BOBBER),
            "minecraft:fishing_hook"
        );
        assert_eq!(
            entity_type_to_bedrock_identifier(&EntityType::EVOKER_FANGS),
            "minecraft:evocation_fang"
        );
        assert_eq!(
            entity_type_to_bedrock_identifier(&EntityType::TRIDENT),
            "minecraft:thrown_trident"
        );
        assert_eq!(
            entity_type_to_bedrock_identifier(&EntityType::FIREWORK_ROCKET),
            "minecraft:fireworks_rocket"
        );
        assert_eq!(
            entity_type_to_bedrock_identifier(&EntityType::EYE_OF_ENDER),
            "minecraft:eye_of_ender_signal"
        );
        assert_eq!(
            entity_type_to_bedrock_identifier(&EntityType::ZOMBIE_VILLAGER),
            "minecraft:zombie_villager_v2"
        );
        assert_eq!(entity_type_to_bedrock_identifier(&EntityType::COW), "cow");
    }
}
