//! What the scheduler does with a message from a worker: a chunk read from disk, the chunks a
//! generation task hands back, or the news that a generation failed.

use slotmap::Key;
use tracing::{debug, error, info, trace, warn};

use super::{GenerationSchedule, TaskHeapNode};
use crate::chunk_system::ChunkPos;
use crate::chunk_system::chunk_state::{Chunk, StagedChunkEnum};
use crate::chunk_system::dag::{Node, NodeKey};
use crate::chunk_system::generation_cache::Cache;
use crate::chunk_system::worker_logic::RecvChunk;
use crate::generation::proto_chunk::ProtoChunk;
use crate::level::SyncChunk;

impl GenerationSchedule {
    pub(super) fn receive_chunk(&mut self, pos: ChunkPos, data: RecvChunk) {
        match data {
            RecvChunk::IO(chunk) => self.receive_io(pos, chunk),
            RecvChunk::Generation(data) => self.receive_generation(pos, data),
            RecvChunk::GenerationFailure {
                pos: fail_pos,
                stage,
                error,
            } => self.receive_failure(pos, fail_pos, stage, &error),
        }
        self.running_task_count -= 1;
    }

    fn receive_io(&mut self, pos: ChunkPos, chunk: Chunk) {
        let mut holder = self.chunk_map.remove(&pos).expect("holder exists");
        if holder.chunk.is_some() {
            warn!(
                "receive_chunk(IO): holder already has chunk at {:?}; replacing",
                pos
            );
        }
        debug_assert_eq!(holder.current_stage, StagedChunkEnum::None);

        let stage = StagedChunkEnum::from(chunk.get_stage_id());
        self.drop_satisfied_tasks(&mut holder, stage);
        holder.current_stage = stage;
        debug_assert!(self.graph.nodes.contains_key(holder.occupied));
        self.drop_node(holder.occupied);
        holder.occupied = NodeKey::null();

        match &chunk {
            Chunk::Level(data) => {
                self.apply_lighting_override(data);
                let result = self.publish_chunk(pos, data.clone());
                if result.is_some() {
                    warn!(
                        "receive_chunk(IO): replacing existing public chunk at {:?}",
                        pos
                    );
                }
                holder.public = true;
                trace!(
                    "Notifying players: chunk {:?} loaded from disk (Full status)",
                    pos
                );
                self.listener.process_new_chunk(pos, data);
            }
            Chunk::Proto(_) => {
                if holder.public {
                    debug!(
                        "Chunk {:?} downgraded to Proto for relighting, marking as non-public",
                        pos
                    );
                    self.unpublish_chunk(pos);
                    holder.public = false;
                }
            }
        }
        holder.chunk = Some(chunk);
        self.chunk_map.insert(pos, holder);

        // A new chunk arrived — unblock any waiting generation tasks
        self.check_waiting_tasks();
    }

    fn receive_generation(&mut self, pos: ChunkPos, data: Cache) {
        let mut dx = 0;
        let mut dy = 0;
        for chunk in data.chunks {
            let new_pos = ChunkPos::new(data.x + dx, data.z + dy);
            match chunk {
                Chunk::Level(chunk) => self.receive_generated_level(pos, new_pos, chunk),
                Chunk::Proto(chunk) => self.receive_generated_proto(pos, new_pos, chunk),
            }
            dy += 1;
            if dy == data.size {
                dy = 0;
                dx += 1;
            }
        }

        // Neighbor chunks returned to holders — unblock waiting tasks
        self.check_waiting_tasks();
    }

    fn receive_generated_level(&mut self, pos: ChunkPos, new_pos: ChunkPos, chunk: SyncChunk) {
        let mut holder = self.chunk_map.remove(&new_pos).expect("holder exists");
        let stage = StagedChunkEnum::Full;
        if new_pos == pos {
            if holder.current_stage != StagedChunkEnum::Spawn {
                warn!(
                    "receive_chunk(Level): holder at {:?} for pos {:?} expected {:?}; aligning",
                    holder.current_stage,
                    new_pos,
                    StagedChunkEnum::Spawn
                );
                holder.current_stage = StagedChunkEnum::Spawn;
            }
            self.drop_satisfied_tasks(&mut holder, stage);
            if self.graph.nodes.contains_key(holder.occupied) {
                self.drop_node(holder.occupied);
            }
            holder.current_stage = stage;

            let was_public = holder.public;
            self.apply_lighting_override(&chunk);
            let public_chunk = chunk.clone();
            if was_public {
                self.publish_chunk(new_pos, public_chunk);
                info!(
                    "Notifying players: regenerated chunk at {:?} (was already public)",
                    new_pos
                );
                self.listener.process_new_chunk(new_pos, &chunk);
                holder.chunk = Some(Chunk::Level(chunk));
            } else {
                holder.chunk = Some(Chunk::Level(chunk));
                let result = self.publish_chunk(new_pos, public_chunk);
                holder.public = true;
                if result.is_some() {
                    warn!("public_chunk_map.insert returned existing chunk for {new_pos:?}");
                }
                if let Some(pc) = self.public_chunks.get(&new_pos) {
                    trace!(
                        "Notifying players: new chunk at {:?} (generation complete)",
                        new_pos
                    );
                    self.listener.process_new_chunk(new_pos, &pc);
                } else {
                    error!(
                        "CRITICAL: Failed to retrieve chunk {:?} from public_chunk_map immediately after insert!",
                        new_pos
                    );
                }
            }
        } else {
            self.drop_satisfied_tasks(&mut holder, stage);
            holder.current_stage = stage;
            holder.chunk = Some(Chunk::Level(chunk));
        }

        if !holder.occupied.is_null() && self.graph.nodes.contains_key(holder.occupied) {
            self.drop_node(holder.occupied);
        }
        holder.occupied = NodeKey::null();

        self.chunk_map.insert(new_pos, holder);
    }

    fn receive_generated_proto(
        &mut self,
        pos: ChunkPos,
        new_pos: ChunkPos,
        chunk: Box<ProtoChunk>,
    ) {
        let mut holder = self.chunk_map.remove(&new_pos).expect("holder exists");

        let stage = StagedChunkEnum::from(chunk.stage_id());
        self.drop_satisfied_tasks(&mut holder, stage);

        if new_pos == pos {
            debug_assert_ne!(holder.current_stage, StagedChunkEnum::None);
            if self.graph.nodes.contains_key(holder.occupied) {
                self.drop_node(holder.occupied);
            }
            holder.current_stage = stage;
        } else {
            if holder.current_stage < stage {
                holder.current_stage = stage;
            }
            if !holder.occupied.is_null() && self.graph.nodes.contains_key(holder.occupied) {
                self.drop_node(holder.occupied);
            }
        }

        holder.occupied = NodeKey::null();
        holder.chunk = Some(Chunk::Proto(chunk));
        self.chunk_map.insert(new_pos, holder);
    }

    fn receive_failure(
        &mut self,
        pos: ChunkPos,
        fail_pos: ChunkPos,
        stage: StagedChunkEnum,
        error: &str,
    ) {
        error!(
            "Received generation failure notification for chunk {:?} at stage {:?}: {}",
            fail_pos, stage, error
        );

        if let Some(mut holder) = self.chunk_map.remove(&pos) {
            let target_stage = holder.target_stage;

            if !holder.occupied.is_null() {
                if self.graph.nodes.contains_key(holder.occupied) {
                    self.drop_node(holder.occupied);
                }
                holder.occupied = NodeKey::null();
            }

            for i in 0..holder.tasks.len() {
                if !holder.tasks[i].is_null() {
                    self.waiting_for_chunks.remove(&holder.tasks[i]);
                    self.drop_node(holder.tasks[i]);
                    holder.tasks[i] = NodeKey::null();
                }
            }

            holder.current_stage = StagedChunkEnum::None;
            holder.dependency_stage = StagedChunkEnum::None;
            holder.chunk = None;

            for i in (StagedChunkEnum::None as usize + 1)..=(target_stage as usize) {
                let stage_enum = StagedChunkEnum::from(i as u8);
                let task_node = Node::new(pos, stage_enum);
                holder.tasks[i] = self.graph.nodes.insert(task_node);

                if i > (StagedChunkEnum::None as usize + 1) {
                    self.graph.add_edge(holder.tasks[i - 1], holder.tasks[i]);
                }
            }

            if target_stage > StagedChunkEnum::None {
                let first_task = holder.tasks[StagedChunkEnum::None as usize + 1];
                if let Some(node) = self.graph.nodes.get_mut(first_task) {
                    node.in_queue = true;
                }
                let mut priority = Self::calc_priority(
                    &self.last_level,
                    &self.last_high_priority,
                    pos,
                    StagedChunkEnum::from(1),
                );
                priority.eff_level = priority.eff_level.saturating_sub(50);
                self.queue.push(TaskHeapNode(priority, first_task));
            }

            self.chunk_map.insert(pos, holder);

            warn!(
                "Chunk {:?} reset to None and re-queued for regeneration (target: {:?})",
                pos, target_stage
            );
        } else {
            error!("Failed to find holder for failed chunk {:?}", pos);
        }
    }
}
