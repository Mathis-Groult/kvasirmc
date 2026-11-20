//! Making the chain of generation tasks of one chunk, up to the stage another task needs.

use std::collections::BinaryHeap;

use slotmap::Key;

use super::{GenerationSchedule, TaskHeapNode};
use crate::chunk_system::chunk_holder::ChunkHolder;
use crate::chunk_system::chunk_state::StagedChunkEnum;
use crate::chunk_system::dag::{DAG, Edge, Node, NodeKey};
use crate::chunk_system::{ChunkLevel, ChunkPos};

type Created = [bool; StagedChunkEnum::COUNT];

impl GenerationSchedule {
    /// Ensure that the dependency chain for `req_stage` exists on `holder` (for chunk at
    /// `chunk_pos`) and wire it to depend on `dependency_task`.
    ///
    /// Bumps `holder.dependency_stage` (NOT `target_stage`) to at least `req_stage` so
    /// that neighbor chunks pulled in as generation dependencies are not discarded before
    /// their dependency is satisfied. `target_stage` is left alone so the level-change
    /// bookkeeping invariant (`old_stage == holder.target_stage`) is never violated.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn ensure_dependency_chain(
        graph: &mut DAG,
        queue: &mut BinaryHeap<TaskHeapNode>,
        last_level: &ChunkLevel,
        last_high_priority: &[ChunkPos],
        dependency_task: NodeKey,
        chunk_pos: ChunkPos,
        holder: &mut ChunkHolder,
        req_stage: StagedChunkEnum,
    ) -> Created {
        // Insert occupied_by edge head
        holder.occupied_by = graph
            .edges
            .insert(Edge::new(dependency_task, holder.occupied_by));

        if !holder.occupied.is_null() {
            graph.add_edge(holder.occupied, dependency_task);
        }

        // Bump dependency_stage so this chunk's IO/generation tasks are scheduled and
        // kept alive even if target_stage is None (outside player view radius).
        // We deliberately do NOT touch target_stage — that field is owned by resort_work
        // and must match the level-change bookkeeping or the debug_assert will fire.
        if holder.dependency_stage < req_stage {
            holder.dependency_stage = req_stage;
        }

        // Effective target is the max of what the player wants and what dependencies need.
        let effective_target = holder.target_stage.max(holder.dependency_stage);
        let mut newly_created = [false; StagedChunkEnum::COUNT];

        // Create any missing tasks from current_stage+1 up to effective_target.
        // We do this even when current_stage >= req_stage, because dependency_stage may
        // require tasks beyond req_stage that haven't been created yet.
        if holder.current_stage < effective_target {
            let start = (holder.current_stage as usize + 1).max(StagedChunkEnum::Empty as usize);
            let end = effective_target as u8 as usize;
            Self::create_missing_tasks(graph, holder, chunk_pos, start..=end, &mut newly_created);
            Self::link_new_tasks(graph, holder, start..=end, &newly_created);
            Self::queue_entry_task(
                graph,
                queue,
                last_level,
                last_high_priority,
                chunk_pos,
                holder.tasks[start],
                start,
            );
        }

        // If req_stage is already satisfied, dependency_task doesn't need to wait —
        // it was only blocked on `occupied` (handled above) and the stage itself is done.
        // Do NOT add an edge here: tasks[req_stage] is null (completed and dropped).
        if holder.current_stage >= req_stage {
            return newly_created;
        }

        // Wire req_stage task → dependency_task so dependency_task can't run until
        // this chunk reaches req_stage. tasks[req_stage] is guaranteed non-null here:
        // effective_target >= req_stage (we just set dependency_stage = req_stage) and
        // current_stage < req_stage, so the task was created in the loop above (or
        // already existed).
        let ano_task = holder.tasks[req_stage as u8 as usize];
        debug_assert!(
            !ano_task.is_null(),
            "holder.tasks[req_stage] must not be null before adding edge"
        );
        graph.add_edge(ano_task, dependency_task);

        newly_created
    }

    /// Makes a task for each stage in `stages` that has none, and marks it in `newly_created`.
    fn create_missing_tasks(
        graph: &mut DAG,
        holder: &mut ChunkHolder,
        chunk_pos: ChunkPos,
        stages: std::ops::RangeInclusive<usize>,
        newly_created: &mut Created,
    ) {
        let start = *stages.start();
        for (i, flag) in newly_created[stages].iter_mut().enumerate() {
            let stage_i = start + i;
            if holder.tasks[stage_i].is_null() {
                let new_node = graph
                    .nodes
                    .insert(Node::new(chunk_pos, StagedChunkEnum::from(stage_i as u8)));
                holder.tasks[stage_i] = new_node;
                *flag = true;
                if !holder.occupied.is_null() {
                    graph.add_edge(holder.occupied, new_node);
                }
            }
        }
    }

    /// Chains each new task to the one before it, and to the one after it if that one is old.
    fn link_new_tasks(
        graph: &mut DAG,
        holder: &ChunkHolder,
        stages: std::ops::RangeInclusive<usize>,
        newly_created: &Created,
    ) {
        let empty = StagedChunkEnum::Empty as usize;
        let end = *stages.end();
        for stage_i in stages {
            if !newly_created[stage_i] {
                continue;
            }
            let cur = holder.tasks[stage_i];

            if stage_i > empty {
                let prev = holder.tasks[stage_i - 1];
                if !prev.is_null() {
                    graph.add_edge(prev, cur);
                }
            }
            if stage_i < end {
                let next = holder.tasks[stage_i + 1];
                if !next.is_null() && !newly_created[stage_i + 1] {
                    graph.add_edge(cur, next);
                }
            }
        }
    }

    /// Queues the entry task (lowest unblocked stage) if nothing holds it back.
    fn queue_entry_task(
        graph: &mut DAG,
        queue: &mut BinaryHeap<TaskHeapNode>,
        last_level: &ChunkLevel,
        last_high_priority: &[ChunkPos],
        chunk_pos: ChunkPos,
        entry_task: NodeKey,
        start: usize,
    ) {
        if !entry_task.is_null()
            && let Some(n) = graph.nodes.get_mut(entry_task)
            && n.in_degree == 0
            && !n.in_queue
        {
            n.in_queue = true;
            queue.push(TaskHeapNode(
                Self::calc_priority(
                    last_level,
                    last_high_priority,
                    chunk_pos,
                    StagedChunkEnum::from(start as u8),
                ),
                entry_task,
            ));
        }
    }
}
