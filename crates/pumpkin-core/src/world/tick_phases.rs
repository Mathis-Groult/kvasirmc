//! The per-tick sequence of a dimension, one function per phase.
//!
//! `World::tick` runs the phases in a fixed order and times each one for the tick profile; the
//! phases that need more than a few lines live here. The statements are the ones the single long
//! function had, in the same order, with the timers started and stopped at the same points.

use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

use kvasir_moblod::Config;
use pumpkin_util::math::boundingbox::BoundingBox;
use pumpkin_util::math::get_section_cord;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::tick::order;
use rayon::prelude::*;
use rustc_hash::FxHashSet;
use tracing::debug;

use super::entity_islands::{self, Islands};
use super::{World, mob_attention};
use crate::block::entities::BlockEntity;
use crate::entity::EntityBase;
use crate::entity::player::Player;
use crate::server::Server;

/// A player's position, expanded body and chunk, taken once per tick.
pub(super) type PlayerSnapshot<'a> = (&'a Arc<Player>, Vector3<f64>, BoundingBox, Vector2<i32>);

/// How long each phase of a tick took, and how many things it ticked.
pub(super) struct TickTimes {
    pub total: std::time::Duration,
    pub prelude: std::time::Duration,
    pub chunks: std::time::Duration,
    pub players: std::time::Duration,
    pub entities: std::time::Duration,
    pub tracking: std::time::Duration,
    pub block_entities: std::time::Duration,
    pub tickets: std::time::Duration,
    pub player_count: usize,
    pub entity_count: usize,
    pub block_entity_count: usize,
}

impl World {
    /// Block updates, active chunks, the environment and raids, each timed on its own.
    pub(super) fn tick_prelude(self: &Arc<Self>, server: &Arc<Server>) {
        let t_prelude = std::time::Instant::now();
        self.flush_block_updates();
        self.flush_synced_block_events();
        server
            .tickprof
            .record(kvasir_tickprof::Phase::PreludeBlocks, t_prelude.elapsed());
        let t_prelude = std::time::Instant::now();
        self.update_active_chunks();
        server.tickprof.record(
            kvasir_tickprof::Phase::PreludeActiveChunks,
            t_prelude.elapsed(),
        );
        let t_prelude = std::time::Instant::now();
        self.tick_environment();
        server.tickprof.record(
            kvasir_tickprof::Phase::PreludeEnvironment,
            t_prelude.elapsed(),
        );
        let t_prelude = std::time::Instant::now();
        let mut raids = {
            let mut guard = self
                .raids
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::take(&mut *guard)
        };
        raids.tick(self);
        {
            let mut guard = self
                .raids
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for (id, raid) in guard.raid_map.drain() {
                raids.raid_map.insert(id, raid);
            }
            raids.next_id = raids.next_id.max(guard.next_id);
            *guard = raids;
        };
        server
            .tickprof
            .record(kvasir_tickprof::Phase::PreludeRaids, t_prelude.elapsed());
    }

    /// Ticks every entity in an active, loaded chunk and returns how long that took.
    pub(super) fn tick_entities(
        self: &Arc<Self>,
        server: &Arc<Server>,
        players: &[Arc<Player>],
        players_cache: &[PlayerSnapshot<'_>],
        entities_to_tick: &[Arc<dyn EntityBase>],
        active_chunks: &Arc<FxHashSet<Vector2<i32>>>,
    ) -> std::time::Duration {
        let level_for_entities = self.level.clone();

        let t_entities = std::time::Instant::now();
        let tickable: Vec<_> = entities_to_tick
            .par_iter()
            .filter_map(|entity| {
                let entity_pos = entity.get_entity().pos.load();
                let entity_chunk = Vector2::new(
                    get_section_cord(entity_pos.x.floor() as i32),
                    get_section_cord(entity_pos.z.floor() as i32),
                );
                if !active_chunks.contains(&entity_chunk) {
                    return None;
                }
                if !level_for_entities.is_chunk_loaded(&entity_chunk) {
                    return None;
                }
                Some((entity, entity_chunk))
            })
            .collect();

        server
            .tickprof
            .record(kvasir_tickprof::Phase::EntityFilter, t_entities.elapsed());
        let (islands, first_order) =
            self.plan_entity_islands(server, players, &tickable, entities_to_tick);
        let t_ticking = std::time::Instant::now();

        let run = EntityTickRun::new(self, server, &tickable, players_cache, first_order);
        run.execute(islands);
        server
            .tickprof
            .record(kvasir_tickprof::Phase::EntityTicking, t_ticking.elapsed());
        self.order_tick_spawns();
        t_entities.elapsed()
    }

    /// Works out which entities can tick on different threads (the islands) and reserves their
    /// tick order numbers. Returns no islands when ticking in list order is as good.
    fn plan_entity_islands(
        self: &Arc<Self>,
        server: &Arc<Server>,
        players: &[Arc<Player>],
        tickable: &[(&Arc<dyn EntityBase>, Vector2<i32>)],
        entities_to_tick: &[Arc<dyn EntityBase>],
    ) -> (Option<Islands>, u64) {
        let t_planning = std::time::Instant::now();
        // Entities that can affect each other tick in list order; separate islands run in
        // parallel.
        let islands_config = &server.advanced_config.world.entity_islands;
        // Islands exist so that separate groups of entities can tick on different threads. On a
        // one-thread pool (the default tick mode, see `kvasir-tickmode`) that cannot happen, so
        // the planning pass would be pure cost: entities then tick in list order, as the plan
        // would have made them.
        let plan_islands = rayon::current_num_threads() > 1
            && !entity_islands::plugins_observe_entity_ticks(server)
            && self
                .island_schedule
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .should_plan(tickable.len());
        let islands = if plan_islands {
            let islands = (|| {
                let reports: Vec<_> = tickable
                    .par_iter()
                    .map(|(entity, _)| {
                        entity_islands::EntityReach::of(entity.as_ref(), islands_config)
                    })
                    .collect();
                let block_write_checks: Vec<_> = reports
                    .iter()
                    .zip(tickable)
                    .map(|(report, (entity, _))| report.block_write_check(entity.as_ref()))
                    .collect();
                let mut reaches = entity_islands::resolve(reports, islands_config)?;
                entity_islands::prune_block_writes(self, &mut reaches, &block_write_checks)?;
                let nodes: Vec<_> = tickable
                    .iter()
                    .map(|(entity, _)| entity.get_entity().entity_id)
                    .zip(reaches)
                    .collect();
                let ticking: FxHashSet<i32> = nodes.iter().map(|(id, _)| *id).collect();
                let idle_entities: Vec<_> = entities_to_tick
                    .iter()
                    .map(|entity| entity.get_entity())
                    .filter(|entity| !ticking.contains(&entity.entity_id))
                    .map(|entity| (entity.entity_id, entity.bounding_box.load()))
                    .collect();
                let player_bodies: Vec<_> = players
                    .iter()
                    .map(|player| {
                        let entity = player.get_entity();
                        (entity.entity_id, entity.bounding_box.load())
                    })
                    .collect();
                let mut islands =
                    entity_islands::partition(&nodes, &idle_entities, &player_bodies)?;
                // Start the largest islands first.
                islands
                    .islands
                    .sort_by_key(|island| std::cmp::Reverse(island.len()));
                Some(islands)
            })();
            let parallel = self
                .island_schedule
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .record_plan(islands.as_ref().map_or(tickable.len(), |islands| {
                    islands.islands.first().map_or(0, Vec::len)
                }));
            islands.filter(|_| parallel)
        } else {
            None
        };
        let first_order = self.level.reserve_entity_tick_orders(tickable.len());
        server
            .tickprof
            .record(kvasir_tickprof::Phase::EntityPlanning, t_planning.elapsed());
        (islands, first_order)
    }

    /// The block entities of the active chunks.
    pub(crate) fn collect_block_entities(
        &self,
        active_chunks: &Arc<FxHashSet<Vector2<i32>>>,
    ) -> Vec<Arc<dyn BlockEntity>> {
        let mut block_entities: Vec<Arc<dyn BlockEntity>> = Vec::new();
        if self.block_entities.len() < active_chunks.len() {
            for chunk_block_entities in &self.block_entities {
                if active_chunks.contains(chunk_block_entities.key()) {
                    block_entities.extend(chunk_block_entities.values().cloned());
                }
            }
        } else {
            for chunk_pos in active_chunks.iter() {
                if let Some(chunk_block_entities) = self.block_entities.get(chunk_pos) {
                    block_entities.extend(chunk_block_entities.values().cloned());
                }
            }
        }
        block_entities
    }

    /// Ticks the block entities in batches and returns how long that took.
    pub(crate) fn tick_block_entities(
        self: &Arc<Self>,
        block_entities: &[Arc<dyn BlockEntity>],
    ) -> std::time::Duration {
        let t_be = std::time::Instant::now();
        block_entities.par_chunks(16).for_each(|batch| {
            for be in batch {
                be.tick(self);
            }
        });
        // Drained after all ticks, so changes (hopper -> chest) land in the same tick.
        self.flush_comparator_updates(block_entities);
        t_be.elapsed()
    }

    /// Sends the chunk loading changes of the tick and returns how long that took.
    pub(super) fn send_chunk_ticket_changes(&self) -> std::time::Duration {
        let t_tickets = std::time::Instant::now();
        self.level
            .chunk_loading
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .send_change();
        t_tickets.elapsed()
    }
}

/// Where every player is at the start of the tick.
pub(super) fn player_snapshots(players: &[Arc<Player>]) -> Vec<PlayerSnapshot<'_>> {
    players
        .par_iter()
        .map(|player| {
            let entity = player.get_entity();
            let pos = entity.pos.load();
            let bb = entity.bounding_box.load().expand(1.0, 0.5, 1.0);
            let chunk_pos = Vector2::new(
                get_section_cord(pos.x.floor() as i32),
                get_section_cord(pos.z.floor() as i32),
            );
            (player, pos, bb, chunk_pos)
        })
        .collect()
}

/// What the entity tick of one dimension works with.
struct EntityTickRun<'a> {
    world: &'a World,
    server: &'a Server,
    tickable: &'a [(&'a Arc<dyn EntityBase>, Vector2<i32>)],
    players_cache: &'a [PlayerSnapshot<'a>],
    first_order: u64,
    lod: Config,
    motion_counted: bool,
    crowd_ranks: Vec<u32>,
    player_positions: Vec<Vector3<f64>>,
}

impl<'a> EntityTickRun<'a> {
    fn new(
        world: &'a World,
        server: &'a Server,
        tickable: &'a [(&'a Arc<dyn EntityBase>, Vector2<i32>)],
        players_cache: &'a [PlayerSnapshot<'a>],
        first_order: u64,
    ) -> Self {
        let lod = server.advanced_config.world.mob_lod.sanitised();
        let motion_counted = kvasir_tickprof::motion::is_enabled();
        // With the feature off the ranks are only needed to tell which tier a mob would be in.
        let crowd_ranks = if lod.enabled || motion_counted {
            let ticking: Vec<_> = tickable.iter().map(|(e, c)| (*e, *c)).collect();
            mob_attention::crowd_ranks(&lod, &ticking)
        } else {
            vec![0; tickable.len()]
        };
        let player_positions: Vec<Vector3<f64>> = players_cache.iter().map(|p| p.1).collect();
        Self {
            world,
            server,
            tickable,
            players_cache,
            first_order,
            lod,
            motion_counted,
            crowd_ranks,
            player_positions,
        }
    }

    /// Ticks the entity at `index` of the tickable list and tells the players it touches.
    fn tick_entity(&self, index: usize) {
        let (entity, entity_chunk) = &self.tickable[index];
        order::set_entity_order(Some(
            self.first_order + index as u64 * order::ORDERS_PER_ENTITY,
        ));

        let tier = self.world.attend_to_mob(
            &self.lod,
            entity.as_ref(),
            self.crowd_ranks[index],
            &self.player_positions,
        );
        let before =
            (self.motion_counted && tier.is_some()).then(|| entity.get_entity().pos.load());
        entity.get_entity().tick_count.fetch_add(1, Relaxed);
        let t_kind = self
            .server
            .tickprof
            .kinds
            .is_enabled()
            .then(std::time::Instant::now);
        entity.tick(entity.as_ref(), self.server);
        if let Some(t_kind) = t_kind {
            self.server.tickprof.kinds.record(
                usize::from(entity.get_entity().entity_type.id),
                t_kind.elapsed(),
            );
        }

        let entity_inner = entity.get_entity();
        let entity_pos = entity_inner.pos.load();
        if let (Some(before), Some(tier)) = (before, tier) {
            let moved = entity_pos - before;
            kvasir_tickprof::motion::record(tier.index(), moved.length(), moved.y);
        }
        let entity_bb = entity_inner.bounding_box.load();

        for (player, player_pos, player_bb, player_chunk) in self.players_cache {
            if player.living_entity.health.load() > 0.0
                && !player.is_spectator()
                && (player_chunk.x - entity_chunk.x).abs() <= 1
                && (player_chunk.y - entity_chunk.y).abs() <= 1
                && (player_pos.x - entity_pos.x).abs() < 5.0
                && (player_pos.y - entity_pos.y).abs() < 5.0
                && (player_pos.z - entity_pos.z).abs() < 5.0
                && player_bb.intersects(&entity_bb)
            {
                entity.on_player_collision(player);
            }
        }
    }

    /// Ticks all of them: islands on separate threads, or in list order.
    fn execute(&self, islands: Option<Islands>) {
        if let Some(islands) = islands {
            islands
                .islands
                .par_iter()
                .with_max_len(1)
                .for_each(|island| {
                    for &index in island {
                        #[cfg(debug_assertions)]
                        entity_islands::enter(
                            &islands,
                            self.tickable[index].0.get_entity().entity_id,
                        );
                        self.tick_entity(index);
                    }
                    order::set_entity_order(None);
                    #[cfg(debug_assertions)]
                    entity_islands::leave();
                });
        } else {
            for index in 0..self.tickable.len() {
                self.tick_entity(index);
            }
            order::set_entity_order(None);
        }
    }
}

/// Writes the timings of a tick to the tick profile and logs a slow tick.
pub(super) fn record_tick(server: &Server, times: &TickTimes) {
    let total_elapsed = times.total;
    {
        use kvasir_tickprof::Phase;
        let profile = &server.tickprof;
        profile.record(Phase::World, total_elapsed);
        if times.player_count == 0 {
            profile.record(Phase::EmptyWorld, total_elapsed);
        }
        profile.record(Phase::Prelude, times.prelude);
        profile.record(Phase::Chunks, times.chunks);
        profile.record(Phase::Players, times.players);
        profile.record(Phase::Entities, times.entities);
        profile.record(Phase::EntityTracking, times.tracking);
        profile.record(Phase::BlockEntities, times.block_entities);
        profile.record(Phase::ChunkTickets, times.tickets);
    };
    if total_elapsed.as_millis() > 50 {
        debug!(
            "Slow Tick [{}ms]: Chunks: {:?} | Players({}): {:?} | Entities({}): {:?} | Block Entities({}): {:?}",
            total_elapsed.as_millis(),
            times.chunks,
            times.player_count,
            times.players,
            times.entity_count,
            times.entities,
            times.block_entity_count,
            times.block_entities,
        );
    }
}
