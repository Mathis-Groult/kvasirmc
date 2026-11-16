mod bedrock_player_join;
mod block_entities;
mod block_reads;
mod block_writes;
mod broadcasting;
mod chat_messages;
mod chunk_io;
mod collisions;
mod custom_data;
mod entity_lifecycle;
mod entity_lookup;
mod explosions;
mod fluid_queries;
mod game_events_emit;
mod java_player_join;
mod light_queries;
mod loot_and_raids;
mod neighbour_updates;
mod player_membership;
mod sky_and_environment;
mod sounds_and_particles;
mod tick_phases;
mod time_and_weather;
mod world_settings;
use crate::block::entities::BlockEntity;
use dashmap::DashMap;

/// `KvasirMC`: a concurrent map for integer and position keys, with a fast hash.
///
/// It uses the multiply-and-rotate hash instead of `SipHash`. These maps are read hundreds of times per tick; see
/// `pumpkin_world::level::ChunkMap` for the measurement.
pub type FxDashMap<K, V> = DashMap<K, V, rustc_hash::FxBuildHasher>;
use pumpkin_data::chunk::Biome;
use pumpkin_world::generation::proto_chunk::GenerationCache;
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, RwLock, Weak};

mod active_chunks;
pub mod brightness;
pub mod chunker;
mod entity_index;
mod fluid_states;
mod mob_attention;
pub(crate) use entity_index::EntityIndex;
pub mod entity_islands;
pub mod explosion;
#[cfg(test)]
mod explosion_knockback_tests;
pub mod game_event;
pub mod generation_cache;
pub mod loot;
pub mod map;
mod neighbor_updater;
pub mod portal;
pub mod raid;
pub mod random_sequences;
pub mod stopwatches;
pub mod time;
pub mod villager_poi;

use crate::block::BlockEvent;
use crate::{
    block::registry::BlockRegistry,
    entity::{EntityBase, player::Player, r#type::load_static_entity_from_nbt},
    error::PumpkinError,
    server::Server,
};
use active_chunks::ActiveChunkTracker;
use arc_swap::ArcSwap;
use border::Worldborder;
pub use explosion::{
    BlockInteraction, DefaultExplosionDamageCalculator, Explosion, ExplosionDamageCalculator,
    ExplosionInteraction, SimpleExplosionDamageCalculator,
};
use pumpkin_data::BlockState;
use pumpkin_data::block_rotation::{Mirror, Rotation};
use pumpkin_data::dimension::Dimension;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_protocol::java::client::play::PlayerInfoFlags;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};
use pumpkin_world::level::Level;
use pumpkin_world::world::BlockAccessor;
use pumpkin_world::world::{GetBlockError, WorldPortalExt};
pub use pumpkin_world::{world::BlockFlags, world_info::LevelData};
use scoreboard::Scoreboard;
use time::LevelTime;

pub mod block_placer;
pub mod border;
pub mod bossbar;
pub mod custom_bossbar;
pub mod dragon_fight;
pub mod end_podium;
pub mod entity_tracker;
pub mod environment;
pub mod natural_spawner;
pub mod scoreboard;
pub mod weather;

pub use environment::EnvironmentAttributes;
pub use pumpkin_data::environment_attribute::{Activity, MoonPhase};

use crate::world::natural_spawner::SpawnState;
use uuid::Uuid;
use weather::Weather;

const MAX_LIGHT_LEVEL: u8 = 15;

fn bedrock_chest_block_actor(state_id: BlockStateId, position: BlockPos) -> Option<NbtCompound> {
    crate::block::entities::bedrock::build_chest_actor(position, state_id, None)
}

use rustc_hash::{FxHashMap, FxHashSet};

impl PumpkinError for GetBlockError {
    fn is_kick(&self) -> bool {
        false
    }

    fn severity(&self) -> tracing::Level {
        tracing::Level::WARN
    }

    fn client_kick_reason(&self) -> Option<String> {
        None
    }
}

/// Represents a Minecraft world, containing entities, players, and the underlying level data.
///
/// Each dimension (Overworld, Nether, End) typically has its own `World`.
///
/// **Key Responsibilities:**
///
/// - Manages the `Level` instance for handling chunk-related operations.
/// - Stores and tracks active `Player` entities within the world.
/// - Provides a central hub for interacting with the world's entities and environment.
#[repr(C)]
pub struct World {
    /// Represents the World's Unique Identifier
    pub uuid: Uuid,
    /// The underlying level, responsible for chunk management and terrain generation.
    pub level: Arc<Level>,
    pub level_info: Arc<ArcSwap<LevelData>>,
    /// A map of active players within the world, keyed by their unique UUID.
    pub players: ArcSwap<Vec<Arc<Player>>>,
    /// A map of active entities within the world, keyed by their unique UUID.
    /// This does not include players.
    pub entities: ArcSwap<Vec<Arc<dyn EntityBase>>>,
    pub(crate) entity_index: Arc<EntityIndex>,
    island_schedule: std::sync::Mutex<entity_islands::Schedule>,
    /// The world's scoreboard, used for tracking scores, objectives, and display information.
    pub scoreboard: std::sync::Mutex<Scoreboard>,
    /// The world's worldborder, defining the playable area and controlling its expansion or contraction.
    pub worldborder: std::sync::Mutex<Worldborder>,
    /// The world's time, including counting ticks for weather, time cycles, and statistics.
    pub level_time: std::sync::Mutex<LevelTime>,
    /// The type of dimension the world is in.
    pub dimension: Dimension,
    pub sea_level: i32,
    pub min_y: i32,
    /// The world's weather, including rain and thunder levels.
    pub weather: std::sync::Mutex<Weather>,
    /// Block Behaviour
    pub block_registry: Arc<BlockRegistry>,
    pub server: Weak<Server>,
    synced_block_event_queue: std::sync::Mutex<Vec<BlockEvent>>,
    /// A map of unsent block changes, keyed by block position.
    unsent_block_changes: std::sync::Mutex<HashMap<BlockPos, BlockStateId>>,
    /// Persisted vanilla POI storage for portal and villager lookups.
    pub portal_poi: std::sync::Mutex<portal::PortalPoiStorage>,
    /// Villager job sites and their current owners.
    pub villager_poi: std::sync::Mutex<villager_poi::VillagerPoiStorage>,
    /// Active raids in this world.
    pub raids: std::sync::Mutex<raid::Raids>,
    /// End Dragon fight manager (only present in `THE_END` dimension).
    pub dragon_fight: Option<std::sync::Mutex<dragon_fight::DragonFight>>,
    pub spawn_state: ArcSwap<SpawnState>,
    pub active_chunks: RwLock<Arc<FxHashSet<Vector2<i32>>>>,
    pub(crate) navigation_change_sequence: std::sync::atomic::AtomicU64,
    /// Entities spawned by ticking entities, with their sequential order.
    tick_spawns: std::sync::Mutex<Vec<(u64, i32)>>,
    active_chunk_tracker: std::sync::Mutex<ActiveChunkTracker>,
    pub forced_chunks: std::sync::Mutex<FxHashSet<Vector2<i32>>>,
    /// Block entities indexed by chunk, so ticking only visits the currently
    /// active chunks instead of scanning every loaded block entity each tick.
    pub block_entities: FxDashMap<Vector2<i32>, FxHashMap<BlockPos, Arc<dyn BlockEntity>>>,
    pending_block_entity_migrations: crossbeam::queue::SegQueue<Vector2<i32>>,
    /// Persistent custom data for the world (matching Bukkit's `PersistentDataHolder`)
    pub custom_data: std::sync::Mutex<NbtCompound>,
    /// Persistent custom data for block entities at specific positions
    pub custom_block_entity_data: DashMap<BlockPos, NbtCompound>,
    /// Entity tracker responsible for tracking entity visibility and sending delta/status packets to watchers.
    pub entity_tracker: entity_tracker::EntityTracker,
}

#[derive(Clone, Copy)]
pub(crate) enum BlockBreakingProgress {
    Start { stage: i32, speed: f32 },
    Update { stage: i32, speed: Option<f32> },
    Stop,
}

impl PartialEq for World {
    fn eq(&self, other: &Self) -> bool {
        self.uuid == other.uuid
    }
}

impl Eq for World {}

impl World {
    const TAB_LIST_ADD_FLAGS: u8 = PlayerInfoFlags::ADD_PLAYER.bits()
        | PlayerInfoFlags::UPDATE_LISTED.bits()
        | PlayerInfoFlags::UPDATE_GAME_MODE.bits()
        | PlayerInfoFlags::UPDATE_LATENCY.bits()
        | PlayerInfoFlags::UPDATE_LIST_PRIORITY.bits()
        | PlayerInfoFlags::UPDATE_HAT.bits();

    pub fn tick(self: &Arc<Self>, server: &Arc<Server>) {
        let start = std::time::Instant::now();

        self.tick_prelude(server);

        let prelude_elapsed = start.elapsed();
        let t_chunks = std::time::Instant::now();
        self.tick_chunks(&server.tickprof);
        let chunk_elapsed = t_chunks.elapsed();

        let players = self.players.load();
        let player_count = players.len();
        let players_cache = tick_phases::player_snapshots(&players);

        let t_players = std::time::Instant::now();
        players.par_iter().for_each(|player| {
            player.tick(server);
        });
        let player_elapsed = t_players.elapsed();

        let entities_to_tick = self.entities.load();
        let entity_count = entities_to_tick.len();
        let active_chunks = self
            .active_chunks
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let entity_elapsed = self.tick_entities(
            server,
            &players,
            &players_cache,
            &entities_to_tick,
            &active_chunks,
        );

        let t_tracking = std::time::Instant::now();
        self.entity_tracker.update_all(self);
        let tracking_elapsed = t_tracking.elapsed();

        let block_entities = self.collect_block_entities(&active_chunks);
        let block_entity_count = block_entities.len();
        let block_entity_elapsed = self.tick_block_entities(&block_entities);

        let tickets_elapsed = self.send_chunk_ticket_changes();

        if let Some(ref fight_mutex) = self.dragon_fight {
            dragon_fight::DragonFight::tick(fight_mutex, self);
        }

        tick_phases::record_tick(
            server,
            &tick_phases::TickTimes {
                total: start.elapsed(),
                prelude: prelude_elapsed,
                chunks: chunk_elapsed,
                players: player_elapsed,
                entities: entity_elapsed,
                tracking: tracking_elapsed,
                block_entities: block_entity_elapsed,
                tickets: tickets_elapsed,
                player_count,
                entity_count,
                block_entity_count,
            },
        );
    }

    /* ItemScatterer.java */
    /* End ItemScatterer.java */
}

impl BlockAccessor for World {
    fn get_block(&self, position: &BlockPos) -> &'static Block {
        self.get_block_state_id_if_loaded(position)
            .map_or(&Block::AIR, Block::from_state_id)
    }
    fn get_block_state(&self, position: &BlockPos) -> &'static BlockState {
        self.get_block_state_id_if_loaded(position)
            .map_or(Block::AIR.default_state, BlockState::from_id)
    }

    fn get_block_state_id(&self, position: &BlockPos) -> BlockStateId {
        self.get_block_state_id_if_loaded(position)
            .unwrap_or(Block::AIR.default_state.id)
    }

    fn get_block_and_state(&self, position: &BlockPos) -> (&'static Block, &'static BlockState) {
        let id = self
            .get_block_state_id_if_loaded(position)
            .unwrap_or(Block::AIR.default_state.id);
        BlockState::from_id_with_block(id)
    }
}

fn bedrock_block_breaking_rate(speed: f32) -> i32 {
    (speed.clamp(0.0, 1.0) * f32::from(u16::MAX)) as i32
}

pub struct WorldPortal(pub Arc<World>);

// Pure Beauty :cap:
impl WorldPortalExt for WorldPortal {
    fn can_place_at(
        &self,
        block: &pumpkin_data::Block,
        state: &BlockState,
        block_accessor: &dyn BlockAccessor,
        block_pos: &BlockPos,
    ) -> bool {
        self.0.block_registry.can_place_at(
            None,
            None,
            block_accessor,
            None,
            block,
            state,
            block_pos,
            None,
            None,
        )
    }

    fn mirror(&self, block: &Block, state_id: BlockStateId, mirror: Mirror) -> &'static BlockState {
        self.0.block_registry.mirror(block, state_id, mirror)
    }

    fn rotate(
        &self,
        block: &Block,
        state_id: BlockStateId,
        rotation: Rotation,
    ) -> &'static BlockState {
        self.0.block_registry.rotate(block, state_id, rotation)
    }

    fn spawn_mobs_for_chunk_generation(
        &self,
        cache: &mut dyn GenerationCache,
        biome: &'static Biome,
        chunk_x: i32,
        chunk_z: i32,
    ) {
        natural_spawner::spawn_mobs_for_chunk_generation(&self.0, cache, biome, chunk_x, chunk_z);
    }

    fn spawn_structure_entities(&self, entities: Vec<NbtCompound>) {
        for mut nbt in entities {
            nbt.child_tags.remove("UUID");
            if let Some(entity) = load_static_entity_from_nbt(&nbt, Vector3::default(), &self.0) {
                self.0.spawn_entity(entity);
            }
        }
    }
}

struct CubicCurve {
    a: f32,
    b: f32,
    c: f32,
}

impl CubicCurve {
    fn new(v1: f32, v2: f32) -> Self {
        Self {
            a: 3.0 * v1 - 3.0 * v2 + 1.0,
            b: -6.0 * v1 + 3.0 * v2,
            c: 3.0 * v1,
        }
    }

    fn sample(&self, t: f32) -> f32 {
        ((self.a * t + self.b) * t + self.c) * t
    }

    fn sample_gradient(&self, t: f32) -> f32 {
        (3.0 * self.a * t + 2.0 * self.b) * t + self.c
    }
}

/// Calculates the celestial (sun) angle fraction in `[0.0, 1.0]`.
/// Matches vanilla 26.2 `EnvironmentAttributes.SUN_ANGLE` easing with `symmetricCubicBezier(0.362, 0.241)`.
#[must_use]
pub fn calculate_celestial_angle(time_of_day: i64) -> f32 {
    let ticks = time_of_day.rem_euclid(24000);
    let alpha = if ticks < 6000 {
        (ticks + 18000) as f32 / 24000.0
    } else {
        (ticks - 6000) as f32 / 24000.0
    };

    let x_curve = CubicCurve::new(0.362, 0.638);
    let y_curve = CubicCurve::new(0.241, 0.759);

    let mut t = alpha;
    let mut solved = false;
    for _ in 0..4 {
        let error = x_curve.sample(t) - alpha;
        if error.abs() < 1e-5 {
            solved = true;
            break;
        }
        let gradient = x_curve.sample_gradient(t);
        if gradient < 1e-5 {
            break;
        }
        t -= (error / gradient).clamp(-0.25, 0.25);
    }

    if !solved {
        let mut t0 = 0.0f32;
        let mut t1 = 1.0f32;
        for _ in 0..64 {
            if t0 >= t1 {
                break;
            }
            let error = x_curve.sample(t) - alpha;
            if error.abs() < 1e-5 {
                break;
            }
            if error < 0.0 {
                t0 = t;
            } else {
                t1 = t;
            }
            t = f32::midpoint(t1, t0);
        }
    }

    y_curve.sample(t)
}

fn saving_vehicle(entity: &dyn EntityBase) -> Option<Arc<dyn EntityBase>> {
    entity
        .get_entity()
        .get_vehicle()
        .filter(|vehicle| vehicle.get_player().is_none() && !vehicle.get_entity().is_removed())
}

fn is_saved_as_passenger(entity: &dyn EntityBase) -> bool {
    saving_vehicle(entity).is_some()
}

fn saved_chunk_pos(entity: &dyn EntityBase) -> Vector2<i32> {
    saving_vehicle(entity).map_or_else(
        || entity.get_entity().chunk_pos.load(),
        |vehicle| saved_chunk_pos(vehicle.as_ref()),
    )
}

/// A live chunk's records were already spawned, so `fresh` replaces them. Otherwise the
/// records are the only copy of unspawned entities, so they stay and are only replaced by UUID.
fn merge_entity_records(data: &mut Vec<NbtCompound>, live: bool, fresh: Vec<NbtCompound>) {
    if live {
        *data = fresh;
        return;
    }

    for record in fresh {
        if let Some(uuid) = record.get_uuid("UUID") {
            data.retain(|existing| existing.get_uuid("UUID") != Some(uuid));
        }
        data.push(record);
    }
}

fn block_entity_nbt(block_entity: &dyn BlockEntity) -> NbtCompound {
    let mut nbt = NbtCompound::new();
    block_entity.write_internal(&mut nbt);
    nbt
}

#[cfg(test)]
mod tests {
    use pumpkin_data::{
        Block,
        block_properties::{ChestLikeProperties, ChestType, HorizontalFacing, WaterLikeProperties},
        fluid::Fluid,
    };
    use pumpkin_nbt::compound::NbtCompound;
    use pumpkin_util::math::position::BlockPos;
    use uuid::Uuid;

    use super::{
        World, bedrock_block_breaking_rate, bedrock_chest_block_actor, merge_entity_records,
    };

    fn record(uuid: Option<Uuid>, id: &str) -> NbtCompound {
        let mut nbt = NbtCompound::new();
        nbt.put_string("id", id.to_string());
        if let Some(uuid) = uuid {
            nbt.put_uuid("UUID", uuid);
        }
        nbt
    }

    fn ids(data: &[NbtCompound]) -> Vec<&str> {
        data.iter()
            .filter_map(|record| record.get_string("id"))
            .collect()
    }

    #[test]
    fn merge_entity_records_replaces_everything_in_a_live_chunk() {
        let stale = Uuid::from_u128(1);
        let fresh = Uuid::from_u128(2);
        let mut data = vec![record(Some(stale), "minecraft:piglin")];

        merge_entity_records(
            &mut data,
            true,
            vec![record(Some(fresh), "minecraft:zombie")],
        );

        assert_eq!(ids(&data), ["minecraft:zombie"]);
        assert_eq!(data[0].get_uuid("UUID"), Some(fresh));
    }

    #[test]
    fn merge_entity_records_empties_a_live_chunk_with_no_entities_left() {
        let mut data = vec![
            record(Some(Uuid::from_u128(1)), "minecraft:piglin"),
            record(Some(Uuid::from_u128(2)), "minecraft:zombie"),
        ];

        merge_entity_records(&mut data, true, Vec::new());

        assert!(data.is_empty());
    }

    #[test]
    fn merge_entity_records_keeps_unspawned_records_of_a_dormant_chunk() {
        let dormant = Uuid::from_u128(1);
        let mut data = vec![record(Some(dormant), "minecraft:piglin")];

        merge_entity_records(
            &mut data,
            false,
            vec![record(Some(Uuid::from_u128(2)), "minecraft:zombie")],
        );

        assert_eq!(ids(&data), ["minecraft:piglin", "minecraft:zombie"]);
    }

    #[test]
    fn merge_entity_records_replaces_a_same_uuid_record_of_a_dormant_chunk() {
        let shared = Uuid::from_u128(1);
        let mut data = vec![
            record(Some(shared), "minecraft:piglin"),
            record(Some(Uuid::from_u128(2)), "minecraft:zombie"),
        ];

        merge_entity_records(
            &mut data,
            false,
            vec![record(Some(shared), "minecraft:hoglin")],
        );

        assert_eq!(ids(&data), ["minecraft:zombie", "minecraft:hoglin"]);
        assert_eq!(
            data.iter()
                .filter(|r| r.get_uuid("UUID") == Some(shared))
                .count(),
            1
        );
    }

    #[test]
    fn merge_entity_records_never_matches_a_record_without_a_uuid() {
        let mut data = vec![record(None, "minecraft:piglin")];

        merge_entity_records(&mut data, false, vec![record(None, "minecraft:zombie")]);

        assert_eq!(ids(&data), ["minecraft:piglin", "minecraft:zombie"]);
    }

    #[test]
    fn merge_entity_records_is_idempotent_on_a_dormant_chunk() {
        let uuid = Uuid::from_u128(1);
        let mut data = Vec::new();

        for _ in 0..3 {
            merge_entity_records(
                &mut data,
                false,
                vec![record(Some(uuid), "minecraft:piglin")],
            );
        }

        assert_eq!(ids(&data), ["minecraft:piglin"]);
    }

    #[test]
    fn liquid_block_states_preserve_source_flow_and_falling_depths() {
        // Liquid block level and fluid amount are different: all falling levels
        // resolve to amount 8, rather than wrapping through the fluid state array.
        let amounts = [8, 7, 6, 5, 4, 3, 2, 1, 8, 8, 8, 8, 8, 8, 8, 8];
        for (block, expected_fluid) in [
            (&Block::WATER, &Fluid::FLOWING_WATER),
            (&Block::LAVA, &Fluid::FLOWING_LAVA),
        ] {
            for (level, amount) in amounts.into_iter().enumerate() {
                let id = WaterLikeProperties { level: level as u8 }.to_state_id(block);
                let (fluid, state) = World::fluid_state_from_block_state(id);
                assert_eq!(fluid.id, expected_fluid.id);
                assert_eq!(state.level, amount);
                assert!((state.height - f32::from(amount) / 9.0).abs() < f32::EPSILON);
                assert_eq!(state.is_source, level == 0);
                assert_eq!(state.is_still, level == 0);
                assert_eq!(state.falling, level >= 8);
                assert!(!state.is_empty);
                assert_eq!(
                    state.block_state_id,
                    WaterLikeProperties {
                        level: level.min(8) as u8
                    }
                    .to_state_id(block)
                );
            }
        }
    }

    #[test]
    fn aquatic_and_waterlogged_blocks_contain_source_water() {
        let wet_stairs = Block::OAK_STAIRS
            .set_waterlogged(Block::OAK_STAIRS.default_state.id, true)
            .unwrap();
        for id in [
            Block::KELP.default_state.id,
            Block::KELP_PLANT.default_state.id,
            Block::SEAGRASS.default_state.id,
            Block::TALL_SEAGRASS.default_state.id,
            Block::BUBBLE_COLUMN.default_state.id,
            wet_stairs,
        ] {
            let (fluid, state) = World::fluid_state_from_block_state(id);
            assert!(fluid.matches_type(&Fluid::WATER));
            assert!(state.is_source && state.is_still && !state.is_empty && !state.falling);
            assert_eq!(state.level, 8);
            assert!((state.height - 8.0 / 9.0).abs() < f32::EPSILON);
        }

        for block in [&Block::AIR, &Block::OAK_STAIRS, &Block::WATER_CAULDRON] {
            let (fluid, state) = World::fluid_state_from_block_state(block.default_state.id);
            assert_eq!(fluid.id, Fluid::EMPTY.id);
            assert!(state.is_empty);
            assert_eq!(state.height, 0.0);
        }
    }

    #[test]
    fn a_shapeless_block_never_stops_a_ray() {
        // The ray always starts inside a block, usually air, and that block must not
        // count as a hit or every raycast would stop where it began.
        let pos = BlockPos::new(10, 64, 10);
        let from = pumpkin_util::math::vector3::Vector3::new(10.5, 64.5, 10.5);
        let to = pumpkin_util::math::vector3::Vector3::new(20.5, 64.5, 10.5);

        assert!(
            super::World::clip_outline_shapes(Block::AIR.default_state, &pos, from, to).is_none()
        );
        assert!(
            super::World::clip_outline_shapes(Block::STONE.default_state, &pos, from, to).is_some()
        );
    }

    #[test]
    fn bedrock_block_breaking_rate_uses_progress_per_tick() {
        assert_eq!(bedrock_block_breaking_rate(0.0), 0);
        assert_eq!(bedrock_block_breaking_rate(1.0 / 30.0), 2_184);
        assert_eq!(bedrock_block_breaking_rate(1.0), 65_535);
    }

    #[test]
    fn bedrock_double_chest_block_actor_identifies_pair_and_lead() {
        let position = BlockPos::new(5, 64, 7);
        let properties = ChestLikeProperties {
            facing: HorizontalFacing::North,
            r#type: ChestType::Right,
            waterlogged: false,
        };
        let actor =
            bedrock_chest_block_actor(properties.to_state_id(&Block::CHEST), position).unwrap();

        assert_eq!(actor.get_int("pairx"), Some(4));
        assert_eq!(actor.get_int("pairz"), Some(7));
        assert_eq!(actor.get_bool("pairlead"), Some(true));
    }

    #[test]
    fn game_rules_registry() {
        use pumpkin_data::game_rules::{GameRule, GameRuleRegistry, GameRuleValue};

        let mut registry = GameRuleRegistry::default();
        match registry.get(&GameRule::KeepInventory) {
            GameRuleValue::Bool(v) => assert!(!v),
            GameRuleValue::Int(_) => panic!("expected bool"),
        }

        match registry.get_mut(&GameRule::KeepInventory) {
            GameRuleValue::Bool(v) => *v = true,
            GameRuleValue::Int(_) => panic!("expected bool"),
        }

        match registry.get(&GameRule::KeepInventory) {
            GameRuleValue::Bool(v) => assert!(v),
            GameRuleValue::Int(_) => panic!("expected bool"),
        }

        match registry.get(&GameRule::RandomTickSpeed) {
            GameRuleValue::Int(v) => assert_eq!(*v, 3),
            GameRuleValue::Bool(_) => panic!("expected int"),
        }

        match registry.get_mut(&GameRule::RandomTickSpeed) {
            GameRuleValue::Int(v) => *v = 20,
            GameRuleValue::Bool(_) => panic!("expected int"),
        }

        match registry.get(&GameRule::RandomTickSpeed) {
            GameRuleValue::Int(v) => assert_eq!(*v, 20),
            GameRuleValue::Bool(_) => panic!("expected int"),
        }
    }

    #[tokio::test]
    async fn listener_collision_changes_in_ticking_chunks_invalidate_nearby_routes()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::entity::{
            Entity,
            ai::{
                pathfinder::{Navigator, node::Node, path::Path},
                test_support::world,
            },
            passive::cow::CowEntity,
        };
        use pumpkin_data::entity::EntityType;
        use pumpkin_util::math::{vector2::Vector2, vector3::Vector3};
        use pumpkin_world::{chunk::ChunkData, world::BlockFlags};
        use std::sync::Arc;

        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        world
            .level_time
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .world_age = 21;
        let chunk = Vector2::new(0, 0);
        world
            .level
            .loaded_chunks
            .insert(chunk, ChunkData::empty_sync(0, 0));
        let cow = CowEntity::new(Entity::new(
            world.clone(),
            Vector3::new(0.5, 64.0, 0.5),
            &EntityType::COW,
        ));
        cow.mob_entity.set_no_ai(true);
        let mut route = Path::new(
            vec![
                Node::new(BlockPos::new(0, 64, 0)),
                Node::new(BlockPos::new(4, 64, 0)),
                Node::new(BlockPos::new(8, 64, 0)),
            ],
            BlockPos::new(8, 64, 0),
            true,
        );
        route.set_next_node_index(1);
        {
            let mut nav = cow
                .mob_entity
                .navigator
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *nav = Navigator::flying();
            nav.move_to_path(Some(route), 1.0, &cow.mob_entity.living_entity)
        };
        world.entities.store(Arc::new(vec![cow.clone()]));
        let next_node_index = || {
            cow.mob_entity
                .navigator
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get_path()
                .map(Path::get_next_node_index)
        };
        let near = BlockPos::new(4, 64, 0);
        let flags = BlockFlags::NOTIFY_LISTENERS
            | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK
            | BlockFlags::UPDATE_KNOWN_SHAPE;
        world.set_block_state(&near, Block::STONE.default_state.id, flags);
        assert_eq!(next_node_index(), Some(1));
        world
            .forced_chunks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(chunk);
        world.update_active_chunks();
        world.set_block_state(&near, Block::DIRT.default_state.id, flags);
        world.set_block_state(
            &near,
            Block::AIR.default_state.id,
            flags & !BlockFlags::NOTIFY_LISTENERS,
        );
        world.set_block_state(
            &BlockPos::new(15, 64, 0),
            Block::STONE.default_state.id,
            flags,
        );
        assert_eq!(next_node_index(), Some(1));
        world.set_block_state(&near, Block::STONE.default_state.id, flags);
        let index = next_node_index();
        world.level.shutdown().await;
        assert_eq!(index, Some(0));
        assert!(cow.mob_entity.is_no_ai());
        Ok(())
    }
    #[tokio::test]
    async fn removal_clears_brain_memories_after_unload_serialization()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::entity::{
            Entity, EntityBase, RemovalReason,
            ai::{
                brain::memory::{GlobalPos, types},
                test_support::world,
            },
            passive::cow::CowEntity,
        };
        use pumpkin_data::{dimension::Dimension, entity::EntityType};
        use pumpkin_util::math::{vector2::Vector2, vector3::Vector3};
        use std::sync::Arc;
        for unload in [false, true] {
            let folder = tempfile::tempdir()?;
            let world = world(&folder);
            let cow = CowEntity::new(Entity::new(
                world.clone(),
                Vector3::new(0.5, 64.0, 0.5),
                &EntityType::COW,
            ));
            cow.mob_entity.with_brain(cow.as_ref(), |brain| {
                brain.register_memory(types::HOME.id());
                brain.set(
                    types::HOME,
                    GlobalPos::new(&Dimension::OVERWORLD, BlockPos::new(3, 64, 4)),
                );
            });
            world.entities.store(Arc::new(vec![cow.clone()]));
            if unload {
                world.remove_entities_in_chunks([Vector2::new(0, 0)]).await;
                let chunk = world.level.get_entity_chunk(Vector2::new(0, 0)).await;
                let saved_home = {
                    let records = chunk
                        .data
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    records
                        .first()
                        .and_then(|record| record.get_compound("Brain"))
                        .and_then(|brain| brain.get_compound("memories"))
                        .is_some_and(|memories| memories.get(types::HOME.id().name()).is_some())
                };
                assert!(saved_home);
                assert!(
                    cow.get_entity().removal_reason.load() == Some(RemovalReason::UnloadedToChunk)
                );
                cow.get_entity().remove();
                assert!(
                    cow.get_entity().removal_reason.load() == Some(RemovalReason::UnloadedToChunk)
                );
            } else {
                cow.get_entity().remove();
            }
            let retained_memory = cow.mob_entity.with_brain(cow.as_ref(), |brain| {
                brain.has_memory_value(types::HOME.id())
            });
            assert!(!retained_memory);
            assert!(cow.get_entity().is_removed());
            assert!(world.get_entity_by_id(cow.get_entity().entity_id).is_none());
            world.level.shutdown().await;
        }
        Ok(())
    }
    #[tokio::test]
    async fn removing_a_wither_clears_its_bossbar_registration_without_dropping_loot()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::entity::{
            Entity, EntityBase, ai::test_support::world, boss::wither::WitherEntity,
        };
        use pumpkin_data::entity::EntityType;
        use pumpkin_util::math::vector3::Vector3;
        use std::sync::Arc;
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        let wither = WitherEntity::new(Entity::new(
            world.clone(),
            Vector3::new(0.0, 64.0, 0.0),
            &EntityType::WITHER,
        ));
        wither
            .bossbar_players
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(uuid::Uuid::new_v4());
        world.entities.store(Arc::new(vec![wither.clone()]));
        wither.get_entity().remove();
        let empty = wither
            .bossbar_players
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty();
        let no_entities = world.entities.load().is_empty();
        world.level.shutdown().await;
        assert!(empty);
        assert!(no_entities);
        Ok(())
    }
}
