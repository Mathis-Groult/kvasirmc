//! The scheduler thread's loop (`GenerationSchedule::work`), phase by phase.

use std::mem::swap;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::thread;
use std::time::Duration;

use slotmap::Key;
use tracing::{debug, info, warn};

use super::{GenerationSchedule, TaskHeapNode};
use crate::chunk_system::ChunkPos;
use crate::chunk_system::chunk_state::{Chunk, StagedChunkEnum};
use crate::chunk_system::dag::{EdgeKey, Node, NodeKey};
use crate::chunk_system::generation_cache::{Cache, SurfaceBiomeNeighborhood};
use crate::level::Level;

impl GenerationSchedule {
    /// Unload, save and shutdown requests from the level; true when the loop must end.
    pub(super) fn handle_level_flags(&mut self, level: &Arc<Level>) -> bool {
        if level.should_unload.swap(false, Relaxed) {
            self.garbage_collect_dependencies();
            self.process_unload_queue();
        }
        if level.should_save.swap(false, Relaxed) {
            self.save_all_chunk(false);
        }
        if level.shut_down_chunk_system.load(Relaxed) {
            info!("Saving chunks before shutdown...");
            self.garbage_collect_dependencies();
            self.process_unload_queue();
            self.save_all_chunk(true);
            return true;
        }
        false
    }

    /// Takes the latest level change and runs the once-a-second unload.
    pub(super) fn resort_and_unload_periodically(&mut self) {
        // 1. Get latest world state (player moves, etc)
        if self.resort_work(self.send_level.get()) {
            self.garbage_collect_dependencies();
            self.check_waiting_tasks();
        }

        // Process unload queue periodically (every 1 second) to batch writes together
        // and act as a brief memory cache if a player walks back into the chunk.
        // This must run even when the queue is empty: `garbage_collect_dependencies`
        // is what puts stale dependency holders into the queue in the first place.
        if self.last_unload.elapsed() >= std::time::Duration::from_secs(1) {
            self.garbage_collect_dependencies();
            self.process_unload_queue();
            self.check_waiting_tasks();
            self.last_unload = std::time::Instant::now();
        }
    }

    /// Hands queued tasks out until the queue is empty or the in-flight limit is reached.
    pub(super) fn dispatch_ready_tasks(&mut self, level: &Arc<Level>) {
        // 4. Process ready tasks in the queue (up to max_in_flight)
        let mut io_batch = Vec::with_capacity(16);
        'out2: while let Some(task) = self.queue.pop() {
            if level.shut_down_chunk_system.load(Relaxed) {
                self.queue.push(task);
                info!("Shutdown detected during task processing, saving chunks...");
                self.save_all_chunk(true);
                break 'out2;
            }

            if self.running_task_count >= self.max_in_flight {
                self.queue.push(task);
                break 'out2;
            }

            // Briefly check for high-priority results or world changes to avoid stalling
            while let Ok((pos, data)) = self.recv_chunk.try_recv() {
                self.receive_chunk(pos, data);
                if self.resort_work(self.send_level.get()) {
                    // If world state changed, we MUST re-sort before continuing
                    self.garbage_collect_dependencies();
                    self.check_waiting_tasks();
                    self.queue.push(task);
                    self.queue_dirty = true;
                    break 'out2;
                }
            }

            if self.dispatch_task(&task, level, &mut io_batch).is_break() {
                break 'out2;
            }
        }

        // Flush any remaining IO batch
        if !io_batch.is_empty()
            && self
                .io_read
                .blocking_send(std::mem::take(&mut io_batch))
                .is_err()
        {
            info!("IO read thread closed, saving remaining chunks...");
            self.save_all_chunk(true);
        }
    }

    /// One popped task: dropped, parked, or started (IO read or generation).
    fn dispatch_task(
        &mut self,
        task: &TaskHeapNode,
        level: &Arc<Level>,
        io_batch: &mut Vec<ChunkPos>,
    ) -> ControlFlow<()> {
        if let Some(node) = self.graph.nodes.get_mut(task.1) {
            node.in_queue = false;
            if node.in_degree != 0 {
                return ControlFlow::Continue(());
            }
            node.in_flight = true;
            let node = node.clone();

            if self.drop_if_not_needed(task, &node) {
                return ControlFlow::Continue(());
            }

            if node.stage == StagedChunkEnum::Empty {
                self.start_io_read(&node, io_batch)
            } else {
                self.start_generation(task, &node, level, io_batch)
            }
        } else {
            ControlFlow::Continue(())
        }
    }

    /// A task whose stage the chunk already reached, or that nothing needs any more, is dropped.
    fn drop_if_not_needed(&mut self, task: &TaskHeapNode, node: &Node) -> bool {
        // A chunk can be advanced as part of a neighboring task's write cache.
        // In that case its queued node may survive even though the returned
        // ProtoChunk has already reached this stage. Dispatching the stale node
        // would run the same stage twice and trip ProtoChunk's stage invariant.
        let actual_stage = self
            .chunk_map
            .get(&node.pos)
            .and_then(|holder| holder.chunk.as_ref())
            .map(Chunk::get_stage_id);
        if actual_stage.is_some_and(|stage| stage >= node.stage as u8) {
            if let Some(holder) = self.chunk_map.get_mut(&node.pos) {
                holder.current_stage = holder
                    .current_stage
                    .max(StagedChunkEnum::from(actual_stage.expect("checked above")));
                let task_slot = &mut holder.tasks[node.stage as usize];
                if *task_slot == task.1 {
                    *task_slot = NodeKey::null();
                }
            }
            self.waiting_for_chunks.remove(&task.1);
            self.drop_node(task.1);
            return true;
        }

        // Cancel/drop task if chunk is out of range or no longer needed by any target/dependency
        let effective_target = self
            .chunk_map
            .get(&node.pos)
            .map_or(StagedChunkEnum::None, |h| {
                h.target_stage.max(h.dependency_stage)
            });

        if node.stage > effective_target {
            if let Some(holder) = self.chunk_map.get_mut(&node.pos) {
                let task_slot = &mut holder.tasks[node.stage as usize];
                if *task_slot == task.1 {
                    *task_slot = NodeKey::null();
                }
            }
            self.waiting_for_chunks.remove(&task.1);
            self.drop_node(task.1);
            return true;
        }
        false
    }

    /// An Empty task: the chunk is occupied and its position joins the IO read batch.
    fn start_io_read(&mut self, node: &Node, io_batch: &mut Vec<ChunkPos>) -> ControlFlow<()> {
        self.running_task_count += 1;
        let holder = self.chunk_map.get_mut(&node.pos).expect("holder exists");
        debug_assert!(holder.occupied.is_null());
        debug_assert_eq!(holder.current_stage, StagedChunkEnum::None);
        let occupy = self.graph.nodes.insert(Node::new(
            ChunkPos::new(i32::MAX, i32::MAX),
            StagedChunkEnum::None,
        ));
        let effective_target = holder.target_stage.max(holder.dependency_stage);
        for i in (holder.current_stage as usize + 1)..=(effective_target as usize) {
            self.graph.add_edge(occupy, holder.tasks[i]);
        }
        holder.occupied = occupy;

        io_batch.push(node.pos);
        if io_batch.len() >= 16
            && self
                .io_read
                .blocking_send(std::mem::take(io_batch))
                .is_err()
        {
            info!("IO read thread closed, saving remaining chunks...");
            self.save_all_chunk(true);
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    }

    /// A generation task: waits if its read area is not ready, else takes the write area into a
    /// cache and runs the stage on the generation pool.
    fn start_generation(
        &mut self,
        task: &TaskHeapNode,
        node: &Node,
        level: &Arc<Level>,
        io_batch: &mut Vec<ChunkPos>,
    ) -> ControlFlow<()> {
        // Send any pending IO batch before starting generation
        if !io_batch.is_empty()
            && self
                .io_read
                .blocking_send(std::mem::take(io_batch))
                .is_err()
        {
            info!("IO read thread closed, saving remaining chunks...");
            self.save_all_chunk(true);
            return ControlFlow::Break(());
        }

        let write_radius = node.stage.get_write_radius();

        // Pre-validate that every chunk in the read area has its data present
        // (and higher-priority overlapping Features tasks have finished)
        // before we snapshot neighbors or swap the write area out.
        if !self.is_task_ready(node.pos, node.stage) {
            if let Some(n) = self.graph.nodes.get_mut(task.1) {
                n.in_queue = false;
                n.in_flight = false;
            }
            self.waiting_for_chunks.insert(task.1);
            // Close the TOCTOU window: the chunk we're waiting for may
            // have arrived in the recv_chunk drain that happened earlier
            // in this same loop iteration, before this task was parked.
            // If so, check_waiting_tasks() will immediately re-queue it
            // so it isn't stranded with running_task_count==0.
            self.check_waiting_tasks();
            return ControlFlow::Continue(());
        }

        let mut cache = Cache::new(
            node.pos.x - write_radius,
            node.pos.y - write_radius,
            write_radius << 1 | 1,
        );

        if node.stage == StagedChunkEnum::Surface {
            self.add_surface_biomes(node, &mut cache);
        }

        self.take_write_area(node, write_radius, &mut cache);

        self.running_task_count += 1;
        let pos = node.pos;
        let stage = node.stage;
        let send_chunk = self.send_chunk.clone();
        let level = level.clone();

        self.generation_pool.spawn(move || {
            let result =
                crate::chunk_system::worker_logic::run_generation(pos, cache, stage, &level);
            let _ = send_chunk.send((pos, result));
        });
        ControlFlow::Continue(())
    }

    /// The Surface stage reads the biomes of the 3 x 3 chunks around it.
    fn add_surface_biomes(&self, node: &Node, cache: &mut Cache) {
        let mut neighborhood = SurfaceBiomeNeighborhood::new(node.pos.x, node.pos.y);
        for dx in -1..=1 {
            for dz in -1..=1 {
                let holder = self
                    .chunk_map
                    .get(&node.pos.add_raw(dx, dz))
                    .expect("surface biome dependency holder exists");
                let chunk = holder
                    .chunk
                    .as_ref()
                    .expect("surface biome dependency is available");
                assert!(
                    neighborhood.push_chunk(chunk),
                    "surface biome dependency has an incomplete palette"
                );
            }
        }
        cache.set_surface_biomes(neighborhood);
    }

    /// Moves every chunk of the write area into the cache and marks it occupied by a new node
    /// that inherits the live `occupied_by` edges (dead ones are unlinked on the way).
    fn take_write_area(&mut self, node: &Node, write_radius: i32, cache: &mut Cache) {
        let occupy = self.graph.nodes.insert(Node::new(
            ChunkPos::new(i32::MAX, i32::MAX),
            StagedChunkEnum::None,
        ));

        for dx in -write_radius..=write_radius {
            for dy in -write_radius..=write_radius {
                let new_pos = node.pos.add_raw(dx, dy);
                let holder = self.chunk_map.get_mut(&new_pos).expect("holder exists");
                let mut tmp = None;
                swap(&mut tmp, &mut holder.chunk);
                let Some(tmp) = tmp else {
                    panic!(
                        "Missing chunk for position {:?} while processing generation task for {:?} stage {:?}",
                        new_pos, node.pos, node.stage
                    )
                };
                match tmp {
                    Chunk::Level(chunk) => {
                        cache.chunks.push(Chunk::Level(chunk));
                    }
                    Chunk::Proto(chunk) => {
                        cache.chunks.push(Chunk::Proto(chunk));
                    }
                }

                debug_assert!(holder.occupied.is_null());

                let mut cur_edge = holder.occupied_by;
                let mut prev_edge = EdgeKey::null();
                let mut change_head = None;
                while !cur_edge.is_null() {
                    let edge = self.graph.edges.get(cur_edge).expect("edge exists");
                    if self.graph.nodes.contains_key(edge.to) {
                        prev_edge = cur_edge;
                        cur_edge = edge.next;
                        self.graph.add_edge(occupy, edge.to);
                    } else {
                        let next = edge.next;
                        self.graph.edges.remove(cur_edge);
                        cur_edge = next;
                        if prev_edge.is_null() {
                            change_head = Some(next);
                        } else {
                            self.graph
                                .edges
                                .get_mut(prev_edge)
                                .expect("edge exists")
                                .next = next;
                        }
                    }
                }
                if let Some(next) = change_head {
                    holder.occupied_by = next;
                }

                holder.occupied = occupy;
            }
        }
    }

    /// Waits for a worker result or a level change; restores stranded tasks when nothing runs.
    /// True when the result channel is closed and the loop must end.
    pub(super) fn wait_for_work(&mut self, level: &Arc<Level>) -> bool {
        // 5. Wait for work or results
        if self.queue.is_empty() {
            if self.running_task_count > 0 {
                match self.recv_chunk.recv_timeout(Duration::from_millis(5)) {
                    Ok((pos, data)) => {
                        self.receive_chunk(pos, data);
                        if self.resort_work(self.send_level.get()) {
                            self.garbage_collect_dependencies();
                            self.check_waiting_tasks();
                        }
                    }
                    Err(crossbeam::channel::RecvTimeoutError::Timeout) => {
                        // Periodically check LevelChannel for new requests
                        if self.resort_work(self.send_level.get()) {
                            self.garbage_collect_dependencies();
                            self.check_waiting_tasks();
                        }
                    }
                    Err(crossbeam::channel::RecvTimeoutError::Disconnected) => return true,
                }
            } else {
                // No tasks in flight, check for any unblocked waiting tasks or stranded ready tasks
                self.check_waiting_tasks();
                let restored = if self.queue.is_empty() {
                    Self::restore_ready_tasks(
                        &mut self.graph,
                        &mut self.queue,
                        &self.chunk_map,
                        &self.last_level,
                        &self.last_high_priority,
                        &self.waiting_for_chunks,
                    )
                } else {
                    0
                };
                if restored > 0 || !self.queue.is_empty() {
                    if restored > 0 {
                        debug!(
                            "Restored {restored} stranded ready chunk tasks to generation queue"
                        );
                    }
                    if self.queue_dirty {
                        self.sort_queue();
                        self.queue_dirty = false;
                    }
                    return false;
                }
                debug_assert!(self.debug_check());
                debug_assert_eq!(self.running_task_count, 0);
                if self.resort_work(self.send_level.wait_and_get(level)) {
                    self.garbage_collect_dependencies();
                    self.check_waiting_tasks();
                }
            }
            if self.queue_dirty {
                self.sort_queue();
                self.queue_dirty = false;
            }
        } else if self.running_task_count >= self.max_in_flight {
            // Queue has tasks, but we are at maximum in-flight capacity.
            // Wait for an in-flight worker to finish instead of busy-spinning.
            match self.recv_chunk.recv_timeout(Duration::from_millis(5)) {
                Ok((pos, data)) => {
                    self.receive_chunk(pos, data);
                    if self.resort_work(self.send_level.get()) {
                        self.garbage_collect_dependencies();
                        self.check_waiting_tasks();
                    }
                }
                Err(crossbeam::channel::RecvTimeoutError::Timeout) => {
                    if self.resort_work(self.send_level.get()) {
                        self.garbage_collect_dependencies();
                        self.check_waiting_tasks();
                    }
                }
                Err(crossbeam::channel::RecvTimeoutError::Disconnected) => return true,
            }
            if self.queue_dirty {
                self.sort_queue();
                self.queue_dirty = false;
            }
        }
        false
    }

    /// After the loop: waits up to 5 s for running tasks, cancels the rest, closes the write IO.
    pub(super) fn drain_and_close(mut self) {
        info!(
            "schedule: waiting for {} generation tasks to finish",
            self.running_task_count
        );
        let mut wait_iterations = 0;
        let max_wait_iterations = 100; // 5 seconds max wait
        while self.running_task_count > 0 && wait_iterations < max_wait_iterations {
            if let Ok((pos, data)) = self.recv_chunk.try_recv() {
                self.receive_chunk(pos, data);
                wait_iterations = 0;
            } else {
                wait_iterations += 1;
                if wait_iterations % 20 == 0 {
                    warn!(
                        "Still waiting for {} tasks to complete (waited {}ms)",
                        self.running_task_count,
                        wait_iterations * 50
                    );
                }
                thread::sleep(Duration::from_millis(50));
            }
        }

        if self.running_task_count > 0 {
            warn!(
                "Cancelling {} in-flight generation tasks",
                self.running_task_count
            );
            let mut nodes_to_drop = Vec::new();

            for holder in self.chunk_map.values_mut() {
                for task in &mut holder.tasks {
                    if !task.is_null() {
                        self.waiting_for_chunks.remove(task);
                        nodes_to_drop.push(*task);
                        *task = NodeKey::null();
                    }
                }

                if !holder.occupied.is_null()
                    && let Some(node) = self.graph.nodes.get(holder.occupied)
                    && node.pos.x == i32::MAX
                    && node.pos.y == i32::MAX
                {
                    nodes_to_drop.push(holder.occupied);
                    holder.occupied = NodeKey::null();
                }

                self.graph.drop_edge_chain(holder.occupied_by);
                holder.occupied_by = EdgeKey::null();
            }

            for node_key in nodes_to_drop {
                self.drop_node(node_key);
            }

            self.running_task_count = 0;
        }

        drop(self.io_write);

        let unreleased_count = self.graph.nodes.len();
        if unreleased_count > 0 {
            warn!(
                "Cleaning up {} unreleased nodes from incomplete tasks",
                unreleased_count
            );
        }
        self.graph.edges.clear();
    }
}
