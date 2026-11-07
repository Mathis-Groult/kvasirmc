//! Spawning, removing, loading and saving entities.

use super::{World, is_saved_as_passenger, merge_entity_records, saved_chunk_pos};
use crate::entity::{
    Entity, EntityBase, RemovalReason,
    player::Player,
    r#type::{load_entity_from_nbt, self_and_passengers},
};
use crate::world::natural_spawner::{SpawnState, spawn_for_chunk};
use pumpkin_data::entity::MobCategory;
use pumpkin_data::{Block, entity::EntityType};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::chunk::ChunkData;
use pumpkin_world::chunk::ChunkHeightmapType::MotionBlocking;
use pumpkin_world::chunk::io::Dirtiable;
use pumpkin_world::tick::order;
use rand::{RngExt, rng};
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::atomic::Ordering::Relaxed;
use tracing::{debug, trace};

impl World {
    /// Writes `entities` into the saved data of the chunks they are in. A live chunk is
    /// rebuilt from scratch, so `snapshot_chunks` lists the live chunks that must be rewritten
    /// even when nothing is left in them; a chunk that never went live keeps its records.
    pub(super) async fn save_entities_by_chunk(
        &self,
        entities: &[Arc<dyn EntityBase>],
        snapshot_chunks: impl IntoIterator<Item = Vector2<i32>>,
    ) {
        let mut groups: FxHashMap<Vector2<i32>, Vec<NbtCompound>> = FxHashMap::default();
        for entity in entities {
            let base_entity = entity.get_entity();
            if base_entity.is_removed() || is_saved_as_passenger(entity.as_ref()) {
                continue;
            }
            let mut nbt = NbtCompound::new();
            entity.write_nbt(&mut nbt);
            groups
                .entry(base_entity.chunk_pos.load())
                .or_default()
                .push(nbt);
        }
        for pos in snapshot_chunks {
            groups.entry(pos).or_default();
        }

        for (pos, records) in groups {
            let chunk = if records.is_empty() {
                let Some(chunk) = self.level.get_entity_chunk_sync(&pos) else {
                    continue;
                };
                chunk
            } else {
                self.level.get_entity_chunk(pos).await
            };
            let live = chunk.live.load(Relaxed);
            if !live && records.is_empty() {
                continue;
            }
            let mut data = chunk
                .data
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            merge_entity_records(&mut data, live, records);
            drop(data);
            chunk.mark_dirty(true);
        }
    }

    pub fn tick_spawning_chunk(
        self: &Arc<Self>,
        chunk_pos: Vector2<i32>,
        chunk: &Arc<ChunkData>,
        spawn_list: &Vec<&'static MobCategory>,
        spawn_state: &Arc<SpawnState>,
    ) {
        // this.level.tickThunder(chunk);
        //TODO check in simulation distance
        let (is_raining, is_thundering) = (self.is_raining(), self.is_thundering());

        if is_raining && is_thundering && rng().random_range(0..100_000) == 0 {
            let rand_value = rng().random::<i32>() >> 2;
            let delta = Vector3::new(rand_value & 15, rand_value >> 16 & 15, rand_value >> 8 & 15);
            let random_pos = Vector3::new(
                chunk_pos.x << 4,
                chunk
                    .heightmap
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(
                        MotionBlocking,
                        chunk_pos.x << 4,
                        chunk_pos.y << 4,
                        self.min_y,
                    ),
                chunk_pos.y << 4,
            )
            .add(&delta);
            // TODO this.getBrightness(LightLayer.SKY, blockPos) >= 15;
            // TODO heightmap

            // TODO findLightningRod(blockPos)
            // TODO encapsulatingFullBlocks
            if true {
                // TODO biome.getPrecipitationAt(pos, this.getSeaLevel()) == Biome.Precipitation.RAIN
                // TODO this.getCurrentDifficultyAt(blockPos);
                if rng().random::<f32>() < 0.0675
                    && self.get_block(&random_pos.to_block_pos().down()) != &Block::LIGHTNING_ROD
                {
                    let entity = Entity::new(
                        self.clone(),
                        random_pos.to_f64(),
                        &EntityType::SKELETON_HORSE,
                    );
                    self.spawn_entity_non_save(Arc::new(entity));
                }
                let entity = Entity::new(
                    self.clone(),
                    random_pos.to_f64().add_raw(0.5, 0., 0.5),
                    &EntityType::LIGHTNING_BOLT,
                );
                self.spawn_entity_non_save(Arc::new(entity));
            }
        }

        if spawn_list.is_empty() {
            return;
        }
        // TODO this.level.canSpawnEntitiesInChunk(chunkPos)
        let entities = spawn_for_chunk(
            self,
            chunk_pos,
            chunk,
            spawn_state,
            spawn_list,
            is_thundering,
        );
        for entity in entities {
            self.spawn_entity_non_save(entity);
        }
    }

    // NOTE: This function doesn't actually await on anything, it just spawns two tokio tasks
    /// IMPORTANT: Chunks have to be non-empty
    pub(super) fn spawn_world_entity_chunks(
        self: &Arc<Self>,
        player: Arc<Player>,
        chunks: Vec<Vector2<i32>>,
    ) {
        #[cfg(debug_assertions)]
        let inst = std::time::Instant::now();

        // Note: `chunks` originates from `Cylindrical::changed_chunks`, which is
        // already ordered from closest to farthest from center by the precompiled
        // cylindrical chunk view LUT. No re-sorting needed.

        let mut entity_receiver = self.level.receive_entity_chunks(chunks);
        let level = self.level.clone();
        let world = self.clone();

        player.clone().spawn_task(async move {
            'main: loop {
                let recv_result = tokio::select! {
                    () = player.client.await_close_interrupt() => {
                        debug!("Canceling player packet processing");
                        None
                    },
                    recv_result = entity_receiver.recv() => {
                        recv_result
                    }
                };

                let Some((chunk_weak, first_load)) = recv_result else {
                    break;
                };

                let Some(chunk) = chunk_weak.upgrade() else {
                    continue;
                };

                let position = Vector2::new(chunk.x, chunk.z);

                if !level.is_chunk_watched(&position) {
                    // No longer watched: don't make its entities live. Leave the
                    // serialized data untouched so the normal unload path persists
                    // it as-is (nothing went live, so there is nothing to save).
                    trace!(
                        "Received entity chunk {:?}, but it is no longer watched; leaving it for the unload path",
                        &position
                    );
                    continue 'main;
                }

                if first_load {
                    // First watcher: consume the serialized entities and make them
                    // live. The live entity list becomes the single source of
                    // truth, so the chunk's NBT is taken (cleared) to avoid keeping
                    // a duplicate copy that would be re-appended on the next unload
                    // and doubled on every reload.
                    let entity_nbts = std::mem::take(
                        &mut *chunk
                            .data
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner),
                    );
                    chunk.live.store(true, Relaxed);
                    for entity_nbt in &entity_nbts {
                        let Some(root) =
                            load_entity_from_nbt(entity_nbt, Vector3::default(), &world)
                        else {
                            continue;
                        };
                        for entity in self_and_passengers(&root) {
                            match entity.get_mob() {
                                Some(mob) => mob.mob_init_data_tracker(),
                                None => entity.init_data_tracker(),
                            }

                            // Clear velocity so the client does not replay the drop
                            // animation; residual velocity from the original drop is
                            // stale data.
                            entity.get_entity().velocity.store(Vector3::default());

                            // UUID-dedupes if another watcher already loaded this entity.
                            // Tracker owns pairing (spawn packets + vehicle restore).
                            world.add_entity_silent(entity.clone());
                            player.try_restore_vehicle(&entity);
                        }
                    }
                } else {
                    // Already live for other watchers: pair this player now so
                    // spawn packets and vehicle restore do not wait on a tracker tick.
                    world
                        .entity_tracker
                        .update_player_chunks(&player, &world, &[position]);
                }
            }

            #[cfg(debug_assertions)]
            debug!("Chunks queued after {}ms", inst.elapsed().as_millis());
        });
    }

    #[expect(clippy::needless_pass_by_value)]
    pub fn spawn_entity_non_save(&self, entity: Arc<dyn EntityBase>) {
        let _base_entity = entity.get_entity();
        self.entity_tracker.add_entity(&entity, self);
        self.spawn_state.load().add_entity(self, entity.as_ref());

        self.entities.rcu(|current_entities| {
            let mut new_entities = (**current_entities).clone();
            new_entities.push(entity.clone());
            new_entities
        });
        self.record_tick_spawn(entity.get_entity().entity_id);
        self.add_pending_riders(&entity);
    }

    /// Records the sequential order of an entity spawned while an entity ticks.
    pub(super) fn record_tick_spawn(&self, entity_id: i32) {
        if let Some(order) = order::next_entity_order() {
            self.tick_spawns
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((order, entity_id));
        }
    }

    /// Moves entities spawned by islands into sequential order, which is their next tick order.
    pub(super) fn order_tick_spawns(&self) {
        let mut spawns = std::mem::take(
            &mut *self
                .tick_spawns
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        if spawns.len() < 2 {
            return;
        }
        spawns.sort_unstable();
        let rank: FxHashMap<i32, usize> = spawns
            .iter()
            .enumerate()
            .map(|(rank, (_, entity_id))| (*entity_id, rank))
            .collect();
        self.entities.rcu(|current_entities| {
            let (mut spawned, mut kept): (Vec<_>, Vec<_>) = current_entities
                .iter()
                .cloned()
                .partition(|entity| rank.contains_key(&entity.get_entity().entity_id));
            spawned.sort_by_key(|entity| rank[&entity.get_entity().entity_id]);
            kept.append(&mut spawned);
            kept
        });
    }

    /// Adds the riders a mob queued while being finalized (vanilla `addFreshEntityWithPassengers`).
    pub(super) fn add_pending_riders(&self, vehicle: &Arc<dyn EntityBase>) {
        let Some(mob) = vehicle.get_mob() else {
            return;
        };
        for rider in mob.get_mob_entity().take_pending_riders() {
            rider.init_data_tracker();
            self.add_entity_silent(rider.clone());
            vehicle.get_entity().add_passenger(vehicle.clone(), rider);
        }
    }

    /// Returns `false` when a plugin cancels the [`EntitySpawnEvent`].
    ///
    /// [`EntitySpawnEvent`]: crate::plugin::api::events::entity::entity_spawn::EntitySpawnEvent
    pub fn spawn_entity(self: &Arc<Self>, entity: Arc<dyn EntityBase>) -> bool {
        let mut event = crate::plugin::api::events::entity::entity_spawn::EntitySpawnEvent::new(
            entity.get_entity().entity_id,
            entity.get_entity().entity_type.id.to_string(),
            entity.get_entity().pos.load(),
            self.clone(),
        );
        if let Some(server) = self.server.upgrade() {
            server.plugin_manager.fire_blocking(&server, &mut event);
        }
        if event.cancelled {
            return false;
        }

        entity.init_data_tracker();
        self.add_entity_silent(entity);
        true
    }

    /// Fires [`CreatureSpawnEvent`], then spawns the entity; `false` if either event is cancelled.
    ///
    /// [`CreatureSpawnEvent`]: crate::plugin::api::events::entity::creature_spawn::CreatureSpawnEvent
    pub fn spawn_creature(
        self: &Arc<Self>,
        entity: Arc<dyn EntityBase>,
        reason: crate::plugin::api::events::entity::creature_spawn::CreatureSpawnReason,
        player: Option<Arc<Player>>,
    ) -> bool {
        let base = entity.get_entity();
        let mut event = crate::plugin::api::events::entity::creature_spawn::CreatureSpawnEvent::new(
            base.entity_id,
            base.entity_type.resource_name.to_string(),
            base.pos.load(),
            self.clone(),
            reason,
            player,
        );
        if let Some(server) = self.server.upgrade() {
            server.plugin_manager.fire_blocking(&server, &mut event);
        }
        if event.cancelled {
            return false;
        }
        self.spawn_entity(entity)
    }

    #[expect(clippy::needless_pass_by_value)]
    pub fn add_entity_silent(&self, entity: Arc<dyn EntityBase>) {
        let base_entity = entity.get_entity();

        // Guard against duplicate entities with the same UUID.
        // This can happen when chunk entity data is loaded while the entity
        // already exists in the world (e.g. another player is still tracking it).
        let already_exists = self
            .entities
            .load()
            .iter()
            .any(|e| e.get_entity().entity_uuid == base_entity.entity_uuid);
        if already_exists {
            return;
        }

        // The entity stays live-only: it is written to its chunk's saved data on
        // unload (see `save_entities_by_chunk`), never at spawn, so it can't be both live and
        // serialized at once (which would double it on the next reload).
        self.spawn_state.load().add_entity(self, entity.as_ref());
        self.entity_tracker.add_entity(&entity, self);

        self.entities.rcu(|current_entities| {
            let mut new_entities = (**current_entities).clone();
            new_entities.push(entity.clone());
            new_entities
        });
        self.record_tick_spawn(entity.get_entity().entity_id);
        self.add_pending_riders(&entity);
    }

    pub fn remove_entity(&self, entity: &dyn EntityBase) {
        // Entity::remove passes its base projection; retain the registered living owner.
        let registered = self.get_entity_by_id(entity.get_entity().entity_id);
        let entity = registered.as_deref().unwrap_or(entity);
        let base_entity = entity.get_entity();
        if base_entity
            .removal_reason
            .compare_exchange(None, Some(RemovalReason::Discarded))
            .is_err()
        {
            return;
        }
        base_entity.removed.store(true, Ordering::Release);

        self.spawn_state.load().remove_entity(self, entity);
        self.entity_tracker.remove_entity(entity, self);
        self.entities.rcu(|current_entities| {
            let mut new_entities = (**current_entities).clone();
            new_entities.retain(|e| e.get_entity().entity_uuid != base_entity.entity_uuid);
            new_entities
        });
        if let Some(mob) = entity.get_mob() {
            mob.get_mob_entity().finish_removal(mob);
        }
    }

    pub async fn remove_entities_in_chunks(
        &self,
        chunks: impl IntoIterator<Item = impl std::borrow::Borrow<Vector2<i32>>>,
    ) {
        let chunks_set: FxHashSet<_> = chunks.into_iter().map(|c| *c.borrow()).collect();
        if chunks_set.is_empty() {
            return;
        }
        let mut entities_to_remove = Vec::new();

        self.entities.rcu(|current_entities| {
            entities_to_remove.clear();
            let mut new_entities = (**current_entities).clone();
            new_entities.retain(|entity| {
                let pos = saved_chunk_pos(entity.as_ref());
                if chunks_set.contains(&pos) {
                    entities_to_remove.push(entity.clone());
                    false
                } else {
                    true
                }
            });
            new_entities
        });

        self.save_entities_by_chunk(&entities_to_remove, chunks_set.iter().copied())
            .await;

        for entity in entities_to_remove {
            let base = entity.get_entity();
            if base
                .removal_reason
                .compare_exchange(None, Some(RemovalReason::UnloadedToChunk))
                .is_ok()
            {
                base.removed.store(true, Ordering::Release);
            }
            self.entity_tracker.remove_entity(entity.as_ref(), self);
            self.spawn_state.load().remove_entity(self, entity.as_ref());
            if let Some(mob) = entity.get_mob() {
                mob.get_mob_entity().finish_removal(mob);
            }
        }

        for chunk_pos in &chunks_set {
            self.save_block_entities(*chunk_pos);
            self.block_entities.remove(chunk_pos);
        }
    }

    pub fn strike_lightning(self: &Arc<Self>, pos: Vector3<f64>, effect_only: bool) {
        use pumpkin_data::entity::EntityType;
        use uuid::Uuid;
        let server_ref = self.server.upgrade();
        if let Some(server_ref) = server_ref {
            let mut event =
                crate::plugin::api::events::world::lightning_strike::LightningStrikeEvent::new(
                    pos,
                    effect_only,
                );
            server_ref
                .plugin_manager
                .fire_blocking(&server_ref, &mut event);
            if event.cancelled {
                return;
            }
        }

        let lightning = crate::entity::r#type::from_type(
            &EntityType::LIGHTNING_BOLT,
            pos,
            self,
            Uuid::new_v4(),
        );

        if let Some(bolt) = lightning
            .cast_any()
            .downcast_ref::<crate::entity::lightning::LightningBoltEntity>()
        {
            bolt.set_visual_only(effect_only);
        }

        self.spawn_entity(lightning);
    }

    pub fn load_entities(&self, chunk_pos: Vector2<i32>, entity_count: usize) {
        let mut load_event =
            crate::plugin::api::events::world::entities_load::EntitiesLoadEvent::new(
                chunk_pos,
                entity_count,
            );
        if let Some(server) = self.server.upgrade() {
            server
                .plugin_manager
                .fire_blocking(&server, &mut load_event);
        }
    }

    pub fn unload_entities(&self, chunk_pos: Vector2<i32>, entity_count: usize) {
        let mut unload_event =
            crate::plugin::api::events::world::entities_unload::EntitiesUnloadEvent::new(
                chunk_pos,
                entity_count,
            );
        if let Some(server) = self.server.upgrade() {
            server
                .plugin_manager
                .fire_blocking(&server, &mut unload_event);
        }
    }
}
