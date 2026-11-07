//! Loading, generating, saving and unloading chunks.

use super::active_chunks::{ActiveChunkTracker, ActivePlayerArea};
use super::border::Worldborder;
use super::scoreboard::Scoreboard;
use super::time::LevelTime;
use super::weather::Weather;
use super::{
    FxDashMap, World, dragon_fight, entity_tracker, natural_spawner, portal, raid, villager_poi,
};
use crate::block::RandomTickArgs;
use crate::world::natural_spawner::SpawnState;
use crate::{
    block::{OnScheduledTickArgs, registry::BlockRegistry},
    entity::EntityBase,
    server::Server,
};
use arc_swap::ArcSwap;
use dashmap::DashMap;
use pumpkin_data::dimension::Dimension;
use pumpkin_data::noise_settings::NoiseSettings;
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::{Difficulty, math::position::BlockPos};
use pumpkin_world::level::Level;
use pumpkin_world::world_info::LevelData;
use rand::rng;
use rand::seq::SliceRandom;
use rayon::prelude::*;
use rustc_hash::FxHashSet;
use std::collections::HashMap;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, RwLock, Weak};
use tracing::error;
use uuid::Uuid;

impl World {
    #[must_use]
    pub fn load(
        level: Arc<Level>,
        level_info: Arc<ArcSwap<LevelData>>,
        dimension: Dimension,
        block_registry: Arc<BlockRegistry>,
        server: Weak<Server>,
    ) -> Self {
        // TODO
        let generation_settings = NoiseSettings::from_dimension(&dimension);

        // Load portal POI from disk (PoiStorage::new automatically loads from disk if files exist)
        let portal_poi = portal::PortalPoiStorage::new(level.level_folder.poi_folder.clone());
        let dragon_fight = (dimension.minecraft_name == Dimension::THE_END.minecraft_name)
            .then(|| std::sync::Mutex::new(dragon_fight::DragonFight::new()));

        let custom_data_path = level
            .level_folder
            .root_folder
            .join("pumpkin_custom_data.nbt");
        let custom_data = if custom_data_path.exists()
            && let Ok(bytes) = std::fs::read(&custom_data_path)
            && let Ok(nbt) = pumpkin_nbt::Nbt::read_unnamed(
                &mut pumpkin_nbt::deserializer::NbtReadHelperJava::new(&mut std::io::Cursor::new(
                    bytes,
                )),
            ) {
            nbt.root_tag
        } else {
            NbtCompound::new()
        };

        if let Some(server_arc) = server.upgrade() {
            let _ = level.rt_handle.set(server_arc.runtime.clone());
        }

        Self {
            uuid: Uuid::new_v4(),
            level,
            level_info,
            players: ArcSwap::new(Arc::new(Vec::new())),
            entities: ArcSwap::new(Arc::new(Vec::new())),
            entity_index: Arc::default(),
            island_schedule: std::sync::Mutex::default(),
            scoreboard: std::sync::Mutex::new(Scoreboard::default()),
            worldborder: std::sync::Mutex::new(Worldborder::new(
                0.0,
                0.0,
                5.999_996_8E7,
                0,
                5,
                300,
            )),
            level_time: std::sync::Mutex::new(LevelTime::new()),
            dimension,
            weather: std::sync::Mutex::new(Weather::new()),
            block_registry,
            sea_level: generation_settings.sea_level,
            min_y: i32::from(generation_settings.shape.min_y),
            synced_block_event_queue: std::sync::Mutex::new(Vec::new()),
            unsent_block_changes: std::sync::Mutex::new(HashMap::new()),
            portal_poi: std::sync::Mutex::new(portal_poi),
            villager_poi: std::sync::Mutex::new(villager_poi::VillagerPoiStorage::default()),
            raids: std::sync::Mutex::new(raid::Raids::default()),
            dragon_fight,
            spawn_state: ArcSwap::new(Arc::new(SpawnState::empty())),
            active_chunks: RwLock::new(Arc::default()),
            navigation_change_sequence: std::sync::atomic::AtomicU64::new(0),
            tick_spawns: std::sync::Mutex::new(Vec::new()),
            active_chunk_tracker: std::sync::Mutex::new(ActiveChunkTracker::default()),
            forced_chunks: std::sync::Mutex::new(FxHashSet::default()),
            server,
            block_entities: FxDashMap::default(),
            pending_block_entity_migrations: crossbeam::queue::SegQueue::new(),
            custom_data: std::sync::Mutex::new(custom_data),
            custom_block_entity_data: DashMap::new(),
            entity_tracker: entity_tracker::EntityTracker::new(),
        }
    }

    pub fn update_active_chunks(&self) {
        let sim_dist = self.server.upgrade().map_or(10, |s| {
            s.advanced_config.networking.java.simulation_distance.get()
        }) as i32;
        let players = self.players.load();
        let forced_chunks = self
            .forced_chunks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let mut tracker = self
            .active_chunk_tracker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut active_chunks = self
            .active_chunks
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut newly_active = Vec::new();
        let mut migrations = Vec::new();
        let mut current_players = FxHashSet::default();

        let spectators_generate_chunks =
            self.level_info.load().game_rules.spectators_generate_chunks;

        for player in players.iter() {
            if player.is_spectator() && !spectators_generate_chunks {
                continue;
            }
            let id = player.gameprofile.id;
            current_players.insert(id);
            tracker.update_player(
                id,
                ActivePlayerArea {
                    center: player.get_entity().chunk_pos.load(),
                    simulation_distance: sim_dist,
                },
                Arc::make_mut(&mut active_chunks),
                &mut newly_active,
            );
        }
        let removed_players: Vec<_> = tracker
            .players
            .keys()
            .filter(|id| !current_players.contains(id))
            .copied()
            .collect();
        for id in removed_players {
            tracker.remove_player(id, Arc::make_mut(&mut active_chunks));
        }
        tracker.sync_forced_chunks(
            &forced_chunks,
            Arc::make_mut(&mut active_chunks),
            &mut newly_active,
        );

        for pos in newly_active {
            if self.level.is_chunk_loaded(&pos) && tracker.loaded_active_chunks.insert(pos) {
                migrations.push(pos);
            }
        }
        for change in self.level.loaded_chunk_changes() {
            match change {
                pumpkin_world::level::LoadedChunkChange::Loaded(pos) => {
                    if active_chunks.contains(&pos)
                        && self.level.is_chunk_loaded(&pos)
                        && tracker.loaded_active_chunks.insert(pos)
                    {
                        migrations.push(pos);
                    }
                }
                pumpkin_world::level::LoadedChunkChange::Unloaded(pos) => {
                    if !self.level.is_chunk_loaded(&pos) {
                        tracker.loaded_active_chunks.remove(&pos);
                    }
                }
            }
        }
        let mut pending_migrations = FxHashSet::default();
        while let Some(pos) = self.pending_block_entity_migrations.pop() {
            pending_migrations.insert(pos);
        }
        for pos in pending_migrations {
            if active_chunks.contains(&pos) && self.level.is_chunk_loaded(&pos) {
                migrations.push(pos);
            }
        }
        let spawnable_chunks = tracker.loaded_active_chunks.len() as i32;
        drop(active_chunks);
        drop(tracker);
        for pos in migrations {
            self.migrate_pending_block_entities(pos);
        }

        self.spawn_state.store(Arc::new(SpawnState::new(
            spawnable_chunks,
            &self.entities,
            self,
        )));
    }

    pub async fn shutdown(&self) {
        let entities = self.entities.load_full();
        self.save_entities_by_chunk(&entities, self.level.live_entity_chunk_positions())
            .await;

        let chunks: Vec<Vector2<i32>> = self
            .block_entities
            .iter()
            .map(|chunk_block_entities| *chunk_block_entities.key())
            .collect();
        for chunk_pos in chunks {
            self.save_block_entities(chunk_pos);
        }

        // Save portal POI to disk
        let save_result = self
            .portal_poi
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .save_all();
        if let Err(e) = save_result {
            error!("Failed to save portal POI: {e}");
        }

        self.level.shutdown().await;
    }

    #[expect(clippy::too_many_lines)]
    pub fn tick_chunks(self: &Arc<Self>, profile: &kvasir_tickprof::TickProfile) {
        use kvasir_tickprof::Phase;
        const BATCH_SIZE: usize = 32;
        const INHABITED_TIME_BATCH_SIZE: usize = 1024;
        let random_tick_speed = self.level_info.load().game_rules.random_tick_speed;

        let mut stage = std::time::Instant::now();
        let active_chunks = self
            .active_chunks
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let tick_data = self.level.get_tick_data(&active_chunks, random_tick_speed);
        profile.record(Phase::ChunkTickData, stage.elapsed());
        stage = std::time::Instant::now();

        // 1. Parallel Block Ticks via Rayon
        let world = self.clone();
        tick_data
            .block_ticks
            .par_chunks(BATCH_SIZE)
            .for_each(|batch| {
                let world = world.clone();
                for scheduled_tick in batch {
                    let pos = scheduled_tick.position;
                    let block = world.get_block(&pos);
                    if let Some(pumpkin_block) = world.block_registry.get_pumpkin_block(block.id) {
                        pumpkin_block.on_scheduled_tick(OnScheduledTickArgs {
                            world: &world,
                            block,
                            position: &pos,
                        });
                    }
                }
            });

        // 2. Parallel Fluid Ticks via Rayon
        let world = self.clone();
        tick_data
            .fluid_ticks
            .par_chunks(BATCH_SIZE)
            .for_each(|batch| {
                let world = world.clone();
                for scheduled_tick in batch {
                    let pos = scheduled_tick.position;
                    let fluid = world.get_fluid(&pos);
                    if let Some(pumpkin_fluid) = world.block_registry.get_pumpkin_fluid(fluid.id) {
                        pumpkin_fluid.on_scheduled_tick(&world, fluid, &pos);
                    }
                }
            });

        profile.record(Phase::ChunkScheduled, stage.elapsed());
        stage = std::time::Instant::now();

        // 3. Parallel Random Ticks via Rayon
        let world = self.clone();
        tick_data
            .random_ticks
            .par_chunks(BATCH_SIZE)
            .for_each(|batch| {
                let world = world.clone();
                for scheduled_tick in batch {
                    let pos = scheduled_tick.position;
                    let (block, fluid) =
                        match (scheduled_tick.tick_block, scheduled_tick.tick_fluid) {
                            (true, true) => {
                                let (b, f) = world.get_block_and_fluid(&pos);
                                (Some(b), Some(f))
                            }
                            (true, false) => (Some(world.get_block(&pos)), None),
                            (false, true) => (None, Some(world.get_fluid(&pos))),
                            (false, false) => (None, None),
                        };

                    if let Some(block) = block
                        && let Some(pumpkin_block) =
                            world.block_registry.get_pumpkin_block(block.id)
                    {
                        pumpkin_block.random_tick(RandomTickArgs {
                            world: &world,
                            block,
                            position: &pos,
                        });
                    }

                    if let Some(fluid) = fluid
                        && let Some(pumpkin_fluid) =
                            world.block_registry.get_pumpkin_fluid(fluid.id)
                    {
                        pumpkin_fluid.random_tick(fluid, &world, &pos);
                    }
                }
            });

        profile.record(Phase::ChunkRandom, stage.elapsed());
        stage = std::time::Instant::now();

        // 4. Calculate Spawn List (Sequential setup)
        let spawn_state = self.spawn_state.load();
        let (spawn_mobs, spawn_monsters, peaceful) = {
            let lock = self.level_info.load();
            (
                lock.game_rules.spawn_mobs,
                lock.game_rules.spawn_monsters,
                lock.difficulty == Difficulty::Peaceful,
            )
        };
        let spawn_passives = self.get_time_of_day() % 400 == 0;
        let spawn_enemies = !peaceful && spawn_monsters && spawn_mobs;
        let spawn_passives = spawn_passives && spawn_mobs;

        let spawn_list = Arc::new(natural_spawner::get_filtered_spawning_categories(
            &spawn_state,
            spawn_mobs,
            spawn_enemies,
            spawn_passives,
        ));

        // 5. Parallel Chunk Spawners via Rayon
        if !spawn_list.is_empty() {
            let mut spawning_chunks = Vec::new();
            for pos in active_chunks.iter() {
                if let Some(chunk) = self.level.read_chunk_sync(pos, std::clone::Clone::clone) {
                    spawning_chunks.push((*pos, chunk));
                }
            }

            spawning_chunks.shuffle(&mut rng());

            let world = self.clone();
            spawning_chunks.par_chunks(8).for_each(|batch| {
                let world = world.clone();
                let s_list = spawn_list.clone();
                let s_state = spawn_state.clone();
                for (pos, chunk) in batch {
                    world.tick_spawning_chunk(*pos, chunk, &s_list, &s_state);
                }
            });
        }

        profile.record(Phase::ChunkSpawning, stage.elapsed());
        stage = std::time::Instant::now();

        // Batch these cheap lookups and atomic increments to avoid waking Rayon
        // workers for tiny tasks every tick, while retaining parallelism for large sets.
        let loaded_chunks = self.level.loaded_chunks.clone();
        let active_chunks_vec: Vec<_> = active_chunks.iter().copied().collect();
        active_chunks_vec
            .par_iter()
            .with_min_len(INHABITED_TIME_BATCH_SIZE)
            .for_each(|pos| {
                if let Some(chunk) = loaded_chunks.get(pos) {
                    chunk.inhabited_time.fetch_add(1, Relaxed);
                }
            });
        profile.record(Phase::ChunkInhabited, stage.elapsed());
    }

    pub async fn unload(self: &Arc<Self>) {
        let mut event =
            crate::plugin::api::events::world::world_load::WorldUnloadEvent::new(self.clone());
        if let Some(server) = self.server.upgrade() {
            server.plugin_manager.fire(&server, &mut event).await;
        }
    }

    pub async fn save(&self) {
        let entities = self.entities.load_full();
        self.save_entities_by_chunk(&entities, self.level.live_entity_chunk_positions())
            .await;

        let chunks: Vec<Vector2<i32>> = self
            .block_entities
            .iter()
            .map(|chunk_block_entities| *chunk_block_entities.key())
            .collect();
        for chunk_pos in chunks {
            self.save_block_entities(chunk_pos);
        }

        if let Ok(mut portal_poi) = self.portal_poi.try_lock() {
            let _ = portal_poi.save_all();
        }

        {
            let custom_data = self
                .custom_data
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !custom_data.is_empty() {
                let custom_data_path = self
                    .level
                    .level_folder
                    .root_folder
                    .join("pumpkin_custom_data.nbt");
                let nbt = pumpkin_nbt::Nbt::from(custom_data.clone());
                let _ = std::fs::write(custom_data_path, nbt.write());
            }
        }

        self.level
            .should_save
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.level.level_channel.notify();

        let mut save_event = crate::plugin::api::events::world::world_save::WorldSaveEvent::new(
            format!("{:?}", self.dimension),
        );
        if let Some(server) = self.server.upgrade() {
            server.plugin_manager.fire(&server, &mut save_event).await;
        }
    }

    pub fn populate_chunk(&self, chunk_pos: Vector2<i32>) {
        let mut populate_event =
            crate::plugin::api::events::world::chunk_populate::ChunkPopulateEvent::new(chunk_pos);
        if let Some(server) = self.server.upgrade() {
            server
                .plugin_manager
                .fire_blocking(&server, &mut populate_event);
        }
    }

    pub fn unload_chunk(&self, chunk_pos: Vector2<i32>) {
        let mut unload_event =
            crate::plugin::api::events::world::chunk_unload::ChunkUnloadEvent::new(chunk_pos);
        if let Some(server) = self.server.upgrade() {
            server
                .plugin_manager
                .fire_blocking(&server, &mut unload_event);
        }
    }

    pub fn async_structure_generate(
        &self,
        world_name: String,
        structure_name: String,
        pos: BlockPos,
    ) {
        let mut event = crate::plugin::api::events::world::async_structure_generate::AsyncStructureGenerateEvent::new(
            world_name,
            structure_name,
            pos,
        );
        if let Some(server) = self.server.upgrade() {
            server.plugin_manager.fire_blocking(&server, &mut event);
        }
    }

    pub fn async_structure_spawn(&self, world_name: String, structure_name: String, pos: BlockPos) {
        let mut event =
            crate::plugin::api::events::world::async_structure_spawn::AsyncStructureSpawnEvent::new(
                world_name,
                structure_name,
                pos,
            );
        if let Some(server) = self.server.upgrade() {
            server.plugin_manager.fire_blocking(&server, &mut event);
        }
    }
}
