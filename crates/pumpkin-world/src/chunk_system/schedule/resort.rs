//! What the scheduler does when the loading levels change: tasks are dropped for the chunks that
//! need less, and created, with the tasks of their neighbors, for the chunks that need more.

use std::collections::VecDeque;

use pumpkin_data::chunk_view_lut::get_chebyshev_ring;
use slotmap::Key;

use super::{GenerationSchedule, TaskHeapNode};
use crate::chunk_system::channel::LevelChange;
use crate::chunk_system::chunk_holder::ChunkHolder;
use crate::chunk_system::chunk_state::{Chunk, StagedChunkEnum};
use crate::chunk_system::dag::{Node, NodeKey};
use crate::chunk_system::{ChunkLevel, ChunkLoading, ChunkPos};

/// Chunks that must reach a stage so that the task of the third field can run.
type Worklist = VecDeque<(ChunkPos, StagedChunkEnum, NodeKey)>;

impl GenerationSchedule {
    pub(super) fn resort_work(
        &mut self,
        new_data: (Option<LevelChange>, Option<Vec<ChunkPos>>),
    ) -> bool {
        if new_data.0.is_none() && new_data.1.is_none() {
            return false;
        }
        if let Some(high_priority) = new_data.1 {
            self.last_high_priority = high_priority;
            self.queue_dirty = true;
        }
        let Some(new_level) = new_data.0 else {
            return true;
        };
        let mut worklist = Worklist::new();

        for (pos, (old_stage, new_stage)) in new_level.0 {
            self.retarget_chunk(pos, old_stage, new_stage, &new_level.1, &mut worklist);
        }
        self.run_worklist(&new_level.1, worklist);

        self.last_level = new_level.1;
        self.queue_dirty = true;
        true
    }

    /// Moves the target of the chunk at `pos` from `old_stage` to `new_stage`.
    fn retarget_chunk(
        &mut self,
        pos: ChunkPos,
        old_stage: StagedChunkEnum,
        new_stage: StagedChunkEnum,
        levels: &ChunkLevel,
        worklist: &mut Worklist,
    ) {
        debug_assert_ne!(old_stage, new_stage);
        debug_assert_eq!(
            new_stage,
            StagedChunkEnum::level_to_stage(*levels.get(&pos).unwrap_or(&ChunkLoading::MAX_LEVEL))
        );
        let mut holder = self.chunk_map.remove(&pos).unwrap_or_default();
        debug_assert_eq!(holder.target_stage, old_stage);
        holder.target_stage = new_stage;

        // Effective target is what we actually need to schedule tasks up to.
        let effective_old = old_stage.max(holder.dependency_stage);
        let effective_new = new_stage.max(holder.dependency_stage);

        if effective_old > effective_new {
            self.drop_tasks_above(&mut holder, effective_new, effective_old);
            if new_stage == StagedChunkEnum::None {
                self.start_unloading(pos, &mut holder);
            }
        } else {
            if old_stage == StagedChunkEnum::None {
                self.stop_unloading(pos, &mut holder);
            }
            for i in (effective_old.max(holder.current_stage) as u8 + 1)..=(effective_new as u8) {
                self.add_task(pos, &mut holder, i, levels, worklist);
            }
        }
        self.chunk_map.insert(pos, holder);
    }

    /// Drops the tasks of the stages above `effective_new` that no worker has taken yet.
    fn drop_tasks_above(
        &mut self,
        holder: &mut ChunkHolder,
        effective_new: StagedChunkEnum,
        effective_old: StagedChunkEnum,
    ) {
        for i in (effective_new.max(holder.current_stage) as usize + 1)..=(effective_old as usize) {
            let task = &mut holder.tasks[i];
            if !task.is_null() {
                let is_in_flight = self.graph.nodes.get(*task).is_some_and(|n| n.in_flight);
                if !is_in_flight {
                    self.waiting_for_chunks.remove(task);
                    self.drop_node(*task);
                    *task = NodeKey::null();
                }
            }
        }
    }

    /// A chunk that needs nothing any more is unloaded, unless a neighbor still needs it.
    fn start_unloading(&mut self, pos: ChunkPos, holder: &mut ChunkHolder) {
        if holder.dependency_stage != StagedChunkEnum::None {
            let has_valid_task = self.graph.prune_edge_chain(&mut holder.occupied_by);
            if !has_valid_task {
                holder.dependency_stage = StagedChunkEnum::None;
            }
        }
        if holder.dependency_stage == StagedChunkEnum::None {
            self.unload_chunks.insert(pos);
        }
    }

    /// A chunk that is wanted again is no longer unloaded, and a finished one is published.
    fn stop_unloading(&mut self, pos: ChunkPos, holder: &mut ChunkHolder) {
        self.unload_chunks.remove(&pos);
        if holder.current_stage == StagedChunkEnum::Full && !holder.public {
            holder.public = true;
            match holder.chunk.as_ref().expect("chunk exists") {
                Chunk::Level(chunk) => {
                    self.apply_lighting_override(chunk);
                    self.publish_chunk(pos, chunk.clone());
                    self.listener.process_new_chunk(pos, chunk);
                }
                Chunk::Proto(_) => panic!(),
            }
        }
    }

    /// Makes the task of stage `i` of the chunk, with the tasks it needs, and queues it if free.
    fn add_task(
        &mut self,
        pos: ChunkPos,
        holder: &mut ChunkHolder,
        i: u8,
        levels: &ChunkLevel,
        worklist: &mut Worklist,
    ) {
        let task = &mut holder.tasks[i as usize];
        if task.is_null() {
            *task = self.graph.nodes.insert(Node::new(pos, i.into()));
            if !holder.occupied.is_null() {
                self.graph.add_edge(holder.occupied, *task);
            }
        }
        let task = *task;
        if i > 1 {
            self.add_dependencies(pos, holder, i, task, worklist);
        }
        let node = self.graph.nodes.get_mut(task).expect("node exists");
        if node.in_degree == 0 && !node.in_queue {
            node.in_queue = true;
            self.queue.push(TaskHeapNode(
                Self::calc_priority(levels, &self.last_high_priority, pos, node.stage),
                task,
            ));
        }
    }

    /// Chains the task of stage `i` to the earlier stages of its chunk, and asks the neighbors
    /// in its radius to reach the stages it needs.
    fn add_dependencies(
        &mut self,
        pos: ChunkPos,
        holder: &mut ChunkHolder,
        i: u8,
        task: NodeKey,
        worklist: &mut Worklist,
    ) {
        let stage = StagedChunkEnum::from(i);
        let dependency = stage.get_direct_dependencies();
        let radius = stage.get_direct_radius();
        self.unload_chunks.remove(&pos);
        let newly_created = Self::ensure_dependency_chain(
            &mut self.graph,
            &mut self.queue,
            &self.last_level,
            &self.last_high_priority,
            task,
            pos,
            holder,
            dependency[0],
        );
        Self::ask_neighbors_of_new_tasks(pos, holder, &newly_created, worklist);

        for r in 1..=(radius as u8) {
            let req_stage = dependency[r as usize];
            for &(dx, dz) in get_chebyshev_ring(r) {
                let new_pos = pos.add_raw(dx as i32, dz as i32);
                self.unload_chunks.remove(&new_pos);
                worklist.push_back((new_pos, req_stage, task));
            }
        }
    }

    /// Every task the chain just made needs its own neighbors to reach some stage.
    fn ask_neighbors_of_new_tasks(
        pos: ChunkPos,
        holder: &ChunkHolder,
        newly_created: &[bool],
        worklist: &mut Worklist,
    ) {
        for (stage_i, &created) in newly_created.iter().enumerate() {
            if created && stage_i > 1 {
                let stage = StagedChunkEnum::from(stage_i as u8);
                let dependency = stage.get_direct_dependencies();
                let radius = stage.get_direct_radius();
                let cur_task = holder.tasks[stage_i];
                for r in 1..=(radius as u8) {
                    let neighbor_req = dependency[r as usize];
                    for &(dx, dz) in get_chebyshev_ring(r) {
                        let neighbor_pos = pos.add_raw(dx as i32, dz as i32);
                        worklist.push_back((neighbor_pos, neighbor_req, cur_task));
                    }
                }
            }
        }
    }

    /// Gives every chunk the worklist names the tasks it needs, and the chunks those need.
    fn run_worklist(&mut self, levels: &ChunkLevel, mut worklist: Worklist) {
        while let Some((pos, req_stage, dep_task)) = worklist.pop_front() {
            self.unload_chunks.remove(&pos);
            let mut holder = self.chunk_map.remove(&pos).unwrap_or_default();
            let newly_created = Self::ensure_dependency_chain(
                &mut self.graph,
                &mut self.queue,
                levels,
                &self.last_high_priority,
                dep_task,
                pos,
                &mut holder,
                req_stage,
            );
            Self::ask_neighbors_of_new_tasks(pos, &holder, &newly_created, &mut worklist);
            self.chunk_map.insert(pos, holder);
        }
    }
}
