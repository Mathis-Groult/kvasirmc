use super::chunk_holder::ChunkHolder;
use super::chunk_state::{Chunk, StagedChunkEnum};
use super::dag::{DAG, EdgeKey, Node, NodeKey};
use super::worker_logic::{RecvChunk, io_read_work, io_write_work};
use super::{
    ChunkLevel, ChunkListener, ChunkLoading, ChunkPos, HashMapType, HashSetType, IOLock,
    LevelChannel,
};
use crate::chunk::io::Dirtiable;
use crate::level::{Level, SyncChunk};
use pumpkin_config::lighting::LightingEngineConfig;
use slotmap::Key;
use std::cmp::{Ordering, max};
use std::collections::{BinaryHeap, HashMap};
use std::mem::swap;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};
use std::thread;
use tracing::{debug, error, info};

mod event_loop;
mod receive;
mod resort;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TaskPriority {
    stage: u8,
    x: i32,
    z: i32,
    eff_level: i8,
}

pub(crate) struct TaskHeapNode(TaskPriority, NodeKey);
impl PartialEq for TaskHeapNode {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl TaskHeapNode {
    #[cfg(test)]
    pub(crate) const fn node_key(&self) -> NodeKey {
        self.1
    }
}
impl Eq for TaskHeapNode {}
impl PartialOrd for TaskHeapNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for TaskHeapNode {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0).reverse()
    }
}

pub struct GenerationSchedule {
    queue: BinaryHeap<TaskHeapNode>,
    graph: DAG,

    last_level: ChunkLevel,
    last_high_priority: Vec<ChunkPos>,
    send_level: Arc<LevelChannel>,

    public_chunks: crate::chunk_cache::PublicChunks,
    chunk_map: HashMap<ChunkPos, ChunkHolder>,
    unload_chunks: HashSetType<ChunkPos>,

    /// Tasks that are graph-ready (`in_degree` == 0) but cannot yet run because
    /// one or more of their required neighbor chunks haven't been delivered yet.
    /// Parked here and re-queued by `check_waiting_tasks()` as chunk data arrives.
    waiting_for_chunks: HashSetType<NodeKey>,

    io_lock: IOLock,
    running_task_count: u16,
    max_in_flight: u16,
    queue_dirty: bool,
    recv_chunk: crossbeam::channel::Receiver<(ChunkPos, RecvChunk)>,
    io_read: tokio::sync::mpsc::Sender<Vec<ChunkPos>>,
    io_write: tokio::sync::mpsc::Sender<Vec<(ChunkPos, Chunk)>>,
    send_chunk: crossbeam::channel::Sender<(ChunkPos, RecvChunk)>,
    listener: Arc<ChunkListener>,
    lighting_config: LightingEngineConfig,
    last_unload: std::time::Instant,
    generation_pool: Arc<rayon::ThreadPool>,
}

impl GenerationSchedule {
    fn publish_chunk(&self, pos: ChunkPos, chunk: SyncChunk) -> Option<SyncChunk> {
        self.public_chunks.publish(pos, chunk)
    }

    fn unpublish_chunk(&self, pos: ChunkPos) -> Option<SyncChunk> {
        self.public_chunks.unpublish(pos)
    }

    pub fn create(
        io_read_thread_count: usize,
        level: Arc<Level>,
        level_channel: Arc<LevelChannel>,
        listener: Arc<ChunkListener>,
        thread_tracker: &mut Vec<thread::JoinHandle<()>>,
    ) {
        let (send_chunk, recv_chunk) = crossbeam::channel::unbounded();

        let (send_read_io, recv_read_io) = tokio::sync::mpsc::channel(io_read_thread_count + 5);
        let recv_read_io = Arc::new(tokio::sync::Mutex::new(recv_read_io));

        let (send_write_io, recv_write_io) = tokio::sync::mpsc::channel(500);

        let io_lock = Arc::new((
            Mutex::new(HashMapType::default()),
            tokio::sync::Notify::new(),
        ));

        for _ in 0..io_read_thread_count {
            level.chunk_system_tasks.spawn(io_read_work(
                recv_read_io.clone(),
                send_chunk.clone(),
                level.clone(),
                io_lock.clone(),
            ));
        }

        level.chunk_system_tasks.spawn(io_write_work(
            recv_write_io,
            level.clone(),
            io_lock.clone(),
        ));

        let cpus = thread::available_parallelism().map_or(1, std::num::NonZero::get);
        let gen_threads = (cpus / 2).clamp(2, 16);
        let generation_pool = Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(gen_threads)
                .thread_name(|i| format!("ChunkGen-{i}"))
                .build()
                .expect("Failed to build Chunk Generation ThreadPool"),
        );
        let max_in_flight = (gen_threads * 2) as u16;

        let level_sched = level;
        let lighting_config = level_sched.lighting_config;
        let handle = thread::Builder::new()
            .name("Schedule".to_string())
            .spawn(move || {
                let scheduler = Self {
                    queue: BinaryHeap::new(),
                    graph: DAG::default(),
                    last_level: ChunkLevel::default(),
                    last_high_priority: Vec::new(),
                    send_level: level_channel,
                    public_chunks: level_sched.public_chunks(),
                    unload_chunks: HashSetType::default(),
                    waiting_for_chunks: HashSetType::default(),
                    io_lock,
                    running_task_count: 0,
                    max_in_flight,
                    queue_dirty: false,
                    recv_chunk,
                    io_read: send_read_io,
                    io_write: send_write_io,
                    send_chunk,
                    listener,
                    chunk_map: HashMap::default(),
                    lighting_config,
                    last_unload: std::time::Instant::now(),
                    generation_pool,
                };
                scheduler.work(&level_sched);
            })
            .expect("Failed to spawn Scheduler Thread");

        thread_tracker.push(handle);
    }

    fn apply_lighting_override(&self, chunk: &SyncChunk) {
        match self.lighting_config {
            LightingEngineConfig::Full => {
                let mut engine = chunk
                    .light_engine
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                for section in &mut engine.block_light {
                    section.fill(15);
                }
                for section in &mut engine.sky_light {
                    section.fill(15);
                }
                chunk.dirty.store(true, Relaxed);
            }
            LightingEngineConfig::Dark => {
                let mut engine = chunk
                    .light_engine
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                for section in &mut engine.block_light {
                    section.fill(0);
                }
                for section in &mut engine.sky_light {
                    section.fill(0);
                }
                chunk.dirty.store(true, Relaxed);
            }
            LightingEngineConfig::Default => {}
        }
    }

    fn calc_priority(
        last_level: &ChunkLevel,
        last_high_priority: &[ChunkPos],
        pos: ChunkPos,
        stage: StagedChunkEnum,
    ) -> TaskPriority {
        let max_r = match stage {
            StagedChunkEnum::Full | StagedChunkEnum::Spawn => 0,
            StagedChunkEnum::Lighting | StagedChunkEnum::Features => 1,
            StagedChunkEnum::Carvers | StagedChunkEnum::Surface | StagedChunkEnum::Noise => 2,
            StagedChunkEnum::Biomes
            | StagedChunkEnum::StructureReferences
            | StagedChunkEnum::StructureStart
            | StagedChunkEnum::Empty
            | StagedChunkEnum::None => 3,
        };
        let mut eff_level = ChunkLoading::MAX_LEVEL;
        for dx in -max_r..=max_r {
            for dz in -max_r..=max_r {
                if let Some(&lvl) = last_level.get(&pos.add_raw(dx, dz))
                    && lvl < eff_level
                {
                    eff_level = lvl;
                }
            }
        }
        if eff_level == ChunkLoading::MAX_LEVEL {
            return TaskPriority {
                eff_level: 127,
                stage: stage as u8,
                x: pos.x,
                z: pos.y,
            };
        }
        if !last_high_priority.is_empty() {
            let mut min_dst = i32::MAX;
            for i in last_high_priority {
                let dst = max((i.x - pos.x).abs(), (i.y - pos.y).abs());
                min_dst = min_dst.min(dst);
                if dst <= StagedChunkEnum::FULL_RADIUS
                    && stage <= StagedChunkEnum::FULL_DEPENDENCIES[dst as usize]
                {
                    return TaskPriority {
                        eff_level: eff_level.saturating_sub(100).saturating_add(dst as i8),
                        stage: stage as u8,
                        x: pos.x,
                        z: pos.y,
                    };
                }
            }
            eff_level = eff_level.saturating_add(min_dst.min(60) as i8);
        }
        TaskPriority {
            eff_level,
            stage: stage as u8,
            x: pos.x,
            z: pos.y,
        }
    }

    fn sort_queue(&mut self) {
        if self.queue.is_empty() {
            return;
        }
        let mut tasks: Vec<_> = self.queue.drain().collect();
        for i in &mut tasks {
            if let Some(node) = self.graph.nodes.get(i.1) {
                i.0 = Self::calc_priority(
                    &self.last_level,
                    &self.last_high_priority,
                    node.pos,
                    node.stage,
                );
            }
        }
        self.queue = BinaryHeap::from(tasks);
    }

    /// TODO: will remove at some point
    pub(crate) fn restore_ready_tasks(
        graph: &mut DAG,
        queue: &mut BinaryHeap<TaskHeapNode>,
        chunk_map: &HashMap<ChunkPos, ChunkHolder>,
        last_level: &ChunkLevel,
        last_high_priority: &[ChunkPos],
        waiting_for_chunks: &HashSetType<NodeKey>,
    ) -> usize {
        debug_assert!(queue.is_empty());

        let mut to_drop = Vec::new();
        let mut ready = Vec::new();

        for (key, node) in &graph.nodes {
            if node.stage == StagedChunkEnum::None
                || node.in_flight
                || waiting_for_chunks.contains(&key)
            {
                continue;
            }

            let Some(holder) = chunk_map.get(&node.pos) else {
                to_drop.push(key);
                continue;
            };

            let effective_target = holder.target_stage.max(holder.dependency_stage);
            if holder.current_stage >= node.stage
                || holder.tasks[node.stage as usize] != key
                || node.stage > effective_target
            {
                to_drop.push(key);
                continue;
            }

            if node.in_degree == 0 && !node.in_queue {
                ready.push((key, node.pos, node.stage));
            }
        }

        for key in to_drop {
            let Some(old) = graph.nodes.remove(key) else {
                continue;
            };
            let mut edge = old.edge;
            while !edge.is_null() {
                let Some(cur) = graph.edges.remove(edge) else {
                    break;
                };
                if let Some(target_node) = graph.nodes.get_mut(cur.to)
                    && target_node.in_degree > 0
                {
                    target_node.in_degree -= 1;
                    if target_node.in_degree == 0 && !target_node.in_queue && !target_node.in_flight
                    {
                        target_node.in_queue = true;
                        queue.push(TaskHeapNode(
                            Self::calc_priority(
                                last_level,
                                last_high_priority,
                                target_node.pos,
                                target_node.stage,
                            ),
                            cur.to,
                        ));
                    }
                }
                edge = cur.next;
            }
        }

        for (key, pos, stage) in ready {
            if let Some(node) = graph.nodes.get_mut(key)
                && node.in_degree == 0
                && !node.in_queue
            {
                node.in_queue = true;
                queue.push(TaskHeapNode(
                    Self::calc_priority(last_level, last_high_priority, pos, stage),
                    key,
                ));
            }
        }

        queue.len()
    }

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
    ) -> [bool; StagedChunkEnum::COUNT] {
        // Insert occupied_by edge head
        holder.occupied_by = graph.edges.insert(crate::chunk_system::dag::Edge::new(
            dependency_task,
            holder.occupied_by,
        ));

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
            let empty = StagedChunkEnum::Empty as usize;
            let start = (holder.current_stage as usize + 1).max(empty);
            let end = effective_target as u8 as usize;

            for (i, flag) in newly_created[start..=end].iter_mut().enumerate() {
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

            for stage_i in start..=end {
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

            // Queue the entry task (lowest unblocked stage)
            let entry_task = holder.tasks[start];
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
        let req_end = req_stage as u8 as usize;
        let ano_task = holder.tasks[req_end];
        debug_assert!(
            !ano_task.is_null(),
            "holder.tasks[req_stage] must not be null before adding edge"
        );
        graph.add_edge(ano_task, dependency_task);

        newly_created
    }

    pub(crate) fn is_features_order_ready(
        chunk_map: &HashMap<ChunkPos, ChunkHolder>,
        pos: ChunkPos,
    ) -> bool {
        for dx in -2..=2 {
            for dz in -2..=2 {
                if dx > 0 || (dx == 0 && dz >= 0) {
                    continue;
                }
                let neighbor_pos = pos.add_raw(dx, dz);
                if let Some(holder) = chunk_map.get(&neighbor_pos) {
                    let effective_target = holder.target_stage.max(holder.dependency_stage);
                    if effective_target >= StagedChunkEnum::Features
                        && holder.current_stage < StagedChunkEnum::Features
                    {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn is_task_ready(&self, pos: ChunkPos, stage: StagedChunkEnum) -> bool {
        let read_radius = stage.get_read_radius();
        let chunks_present = (-read_radius..=read_radius).all(|dx| {
            (-read_radius..=read_radius).all(|dy| {
                self.chunk_map
                    .get(&pos.add_raw(dx, dy))
                    .is_some_and(|h| h.chunk.is_some())
            })
        });
        if !chunks_present {
            return false;
        }
        if stage == StagedChunkEnum::Features
            && !Self::is_features_order_ready(&self.chunk_map, pos)
        {
            return false;
        }
        true
    }

    /// Check if any tasks parked in `waiting_for_chunks` now have all their neighbor
    /// chunk data available, and re-queue them if so.
    /// Must be called after every `receive_chunk` call.
    fn check_waiting_tasks(&mut self) {
        if self.waiting_for_chunks.is_empty() {
            return;
        }

        let mut now_ready: Vec<NodeKey> = Vec::new();
        let mut stale: Vec<NodeKey> = Vec::new();

        let waiting_keys: Vec<NodeKey> = self.waiting_for_chunks.iter().copied().collect();
        for node_key in waiting_keys {
            let Some(node) = self.graph.nodes.get(node_key) else {
                self.waiting_for_chunks.remove(&node_key);
                continue;
            };

            let Some(holder) = self.chunk_map.get(&node.pos) else {
                self.waiting_for_chunks.remove(&node_key);
                stale.push(node_key);
                continue;
            };

            let effective_target = holder.target_stage.max(holder.dependency_stage);
            if holder.current_stage >= node.stage
                || holder.tasks[node.stage as usize] != node_key
                || node.stage > effective_target
            {
                self.waiting_for_chunks.remove(&node_key);
                stale.push(node_key);
                continue;
            }

            if self.is_task_ready(node.pos, node.stage) && node.in_degree == 0 {
                self.waiting_for_chunks.remove(&node_key);
                now_ready.push(node_key);
            }
        }

        for node_key in stale {
            self.drop_node(node_key);
        }

        for node_key in now_ready {
            if let Some(n) = self.graph.nodes.get_mut(node_key)
                && n.in_degree == 0
                && !n.in_queue
            {
                n.in_queue = true;
                let priority =
                    Self::calc_priority(&self.last_level, &self.last_high_priority, n.pos, n.stage);
                self.queue.push(TaskHeapNode(priority, node_key));
            }
        }
    }

    fn recompute_dependency_stages(&mut self) {
        let mut required: HashMapType<ChunkPos, StagedChunkEnum> = HashMapType::default();
        let mut worklist: Vec<(ChunkPos, StagedChunkEnum)> = self
            .chunk_map
            .iter()
            .filter(|(_, holder)| holder.target_stage != StagedChunkEnum::None)
            .map(|(pos, holder)| (*pos, holder.target_stage))
            .collect();

        while let Some((pos, req)) = worklist.pop() {
            let entry = required.entry(pos).or_insert(StagedChunkEnum::None);
            if *entry >= req {
                continue;
            }
            let start = *entry as u8 + 1;
            *entry = req;

            // Only expand the stages that were not already accounted for.
            for i in start..=(req as u8) {
                let stage = StagedChunkEnum::from(i);
                let radius = stage.get_direct_radius();
                if radius == 0 {
                    continue;
                }
                let dependencies = stage.get_direct_dependencies();
                for r in 1..=(radius as u8) {
                    let req = dependencies[r as usize];
                    for &(dx, dz) in pumpkin_data::chunk_view_lut::get_chebyshev_ring(r) {
                        let neighbor = pos.add_raw(dx as i32, dz as i32);
                        worklist.push((neighbor, req));
                    }
                }
            }
        }

        let mut nodes_to_drop = Vec::new();
        let mut newly_unused = Vec::new();
        for (pos, holder) in &mut self.chunk_map {
            let new_dependency = required.get(pos).copied().unwrap_or(StagedChunkEnum::None);
            if new_dependency >= holder.dependency_stage {
                continue;
            }
            holder.dependency_stage = new_dependency;

            let effective_target = holder.target_stage.max(new_dependency);
            for i in (effective_target as usize + 1)..StagedChunkEnum::COUNT {
                let task = holder.tasks[i];
                if !task.is_null() {
                    nodes_to_drop.push((*pos, i, task));
                }
            }
            if effective_target == StagedChunkEnum::None {
                newly_unused.push(*pos);
            }
        }
        self.unload_chunks.extend(newly_unused);

        for (pos, index, task) in nodes_to_drop {
            if self
                .graph
                .nodes
                .get(task)
                .is_some_and(|node| node.in_flight)
            {
                continue;
            }
            self.waiting_for_chunks.remove(&task);
            self.drop_node(task);
            if let Some(holder) = self.chunk_map.get_mut(&pos) {
                holder.tasks[index] = NodeKey::null();
            }
        }

        self.purge_dropped_queue_entries();
    }

    /// Drop heap entries whose node has been cancelled. They are skipped when popped,
    /// but a saturated queue is never drained, so without this the heap keeps every
    /// cancelled task of every chunk the player has flown past and `sort_queue` gets
    /// slower on each level change.
    fn purge_dropped_queue_entries(&mut self) {
        if self.queue.is_empty() {
            return;
        }
        let graph = &self.graph;
        let tasks: Vec<_> = self
            .queue
            .drain()
            .filter(|task| graph.nodes.contains_key(task.1))
            .collect();
        self.queue = BinaryHeap::from(tasks);
    }

    fn garbage_collect_dependencies(&mut self) {
        self.recompute_dependency_stages();

        // Garbage collect stranded dependencies and empty holders
        let mut stranded = Vec::new();
        let mut empty_holders = Vec::new();

        for (pos, holder) in &self.chunk_map {
            if holder.target_stage == StagedChunkEnum::None {
                if holder.dependency_stage != StagedChunkEnum::None {
                    stranded.push(*pos);
                } else if holder.current_stage == StagedChunkEnum::None
                    && holder.chunk.is_none()
                    && holder.occupied.is_null()
                    && holder.tasks.iter().all(Key::is_null)
                    && !holder.public
                {
                    empty_holders.push(*pos);
                }
            }
        }

        for pos in stranded {
            let holder = self.chunk_map.get_mut(&pos).expect("holder exists");
            if !holder.occupied.is_null() && self.graph.nodes.contains_key(holder.occupied) {
                continue;
            }

            let has_valid_task = self.graph.prune_edge_chain(&mut holder.occupied_by);
            if !has_valid_task {
                holder.dependency_stage = StagedChunkEnum::None;
                self.unload_chunks.insert(pos);
            }
        }

        for pos in empty_holders {
            if let Some(mut holder) = self.chunk_map.remove(&pos) {
                self.graph.drop_edge_chain(holder.occupied_by);
                holder.occupied_by = EdgeKey::null();
            }
        }
    }

    fn process_unload_queue(&mut self) {
        if self.unload_chunks.is_empty() {
            return;
        }

        let mut unload_chunks = HashSetType::default();
        swap(&mut unload_chunks, &mut self.unload_chunks);
        let mut chunks = Vec::with_capacity(unload_chunks.len());
        for pos in unload_chunks {
            let Some(mut holder) = self.chunk_map.remove(&pos) else {
                continue;
            };
            if holder.target_stage != StagedChunkEnum::None
                || holder.dependency_stage != StagedChunkEnum::None
            {
                self.chunk_map.insert(pos, holder);
                continue;
            }
            if !holder.occupied.is_null() {
                self.chunk_map.insert(pos, holder);
                self.unload_chunks.insert(pos);
                continue;
            }

            for task in holder.tasks {
                if !task.is_null() {
                    let is_in_flight = self.graph.nodes.get(task).is_some_and(|n| n.in_flight);
                    if !is_in_flight {
                        self.waiting_for_chunks.remove(&task);
                        self.drop_node(task);
                    }
                }
            }

            self.graph.drop_edge_chain(holder.occupied_by);
            holder.occupied_by = EdgeKey::null();

            if holder.public {
                self.unpublish_chunk(pos);
                holder.public = false;
            }

            if let Some(tmp) = holder.chunk {
                match tmp {
                    Chunk::Level(chunk) => {
                        // Save chunk to disk if dirty
                        if chunk.is_dirty() {
                            chunks.push((pos, Chunk::Level(chunk)));
                        }
                    }
                    Chunk::Proto(proto) => {
                        if !matches!(proto.stage, StagedChunkEnum::Empty | StagedChunkEnum::None) {
                            chunks.push((pos, Chunk::Proto(proto)));
                        }
                    }
                }
            }
        }
        if chunks.is_empty() {
            return;
        }
        let mut data = self
            .io_lock
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (pos, _chunk) in &chunks {
            *data.entry(*pos).or_insert(0) += 1;
        }
        drop(data);
        if let Err(e) = self.io_write.blocking_send(chunks) {
            error!(
                "Failed to send chunks to io write thread during save (may have shut down): {:?}",
                e
            );
        }
    }

    fn save_all_chunk(&mut self, save_proto_chunk: bool) {
        let mut chunks = Vec::with_capacity(self.chunk_map.len());

        for (pos, holder) in &mut self.chunk_map {
            if let Some(chunk) = &holder.chunk {
                let should_save = match chunk {
                    Chunk::Level(sync_chunk) => sync_chunk.is_dirty(),
                    Chunk::Proto(proto) => {
                        save_proto_chunk
                            && !matches!(
                                proto.stage,
                                crate::chunk_system::chunk_state::StagedChunkEnum::Empty
                                    | crate::chunk_system::chunk_state::StagedChunkEnum::None
                            )
                    }
                };

                if should_save {
                    let chunk_to_save = match chunk {
                        Chunk::Level(sync_chunk) => Chunk::Level(sync_chunk.clone()),
                        Chunk::Proto(_) => holder.chunk.take().expect("proto chunk exists"),
                    };
                    chunks.push((*pos, chunk_to_save));
                }
            }
        }

        if chunks.is_empty() {
            return;
        }

        info!(
            "Saving {} chunks (collected from {} holders)...",
            chunks.len(),
            self.chunk_map.len()
        );

        let mut data = self
            .io_lock
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (pos, _) in &chunks {
            *data.entry(*pos).or_insert(0) += 1;
        }
        drop(data);

        if let Err(e) = self.io_write.blocking_send(chunks) {
            error!("Failed to send chunks to io write thread: {:?}", e);
        }
    }

    fn drop_node(&mut self, node: NodeKey) {
        let Some(old) = self.graph.nodes.remove(node) else {
            return;
        };
        let mut edge = old.edge;
        while !edge.is_null() {
            let cur = self.graph.edges.remove(edge).expect("edge exists");
            if let Some(node) = self.graph.nodes.get_mut(cur.to) {
                debug_assert!(node.in_degree >= 1);
                node.in_degree -= 1;
                if node.in_degree == 0 && !node.in_queue {
                    self.queue.push(TaskHeapNode(
                        Self::calc_priority(
                            &self.last_level,
                            &self.last_high_priority,
                            node.pos,
                            node.stage,
                        ),
                        cur.to,
                    ));
                    node.in_queue = true;
                }
            }
            edge = cur.next;
        }
    }

    fn drop_satisfied_tasks(&mut self, holder: &mut ChunkHolder, stage: StagedChunkEnum) {
        // A neighboring generation cache may already have advanced this holder past the
        // returning task's stage. Drop every scheduled task satisfied by the returned data,
        // including that task's stale in-flight node.
        for task_idx in (StagedChunkEnum::None as usize + 1)..=(stage as usize) {
            if !holder.tasks[task_idx].is_null() {
                self.waiting_for_chunks.remove(&holder.tasks[task_idx]);
                self.drop_node(holder.tasks[task_idx]);
                holder.tasks[task_idx] = NodeKey::null();
            }
        }
    }

    fn work(mut self, level: &Arc<Level>) {
        debug!(
            "schedule thread start id: {:?} name: {}",
            thread::current().id(),
            thread::current().name().unwrap_or("unknown")
        );
        loop {
            if self.handle_level_flags(level) {
                break;
            }

            self.resort_and_unload_periodically();

            // 2. Process all pending chunk results from workers
            while let Ok((pos, data)) = self.recv_chunk.try_recv() {
                self.receive_chunk(pos, data);
            }

            // 3. Re-sort if world state changed or new tasks added
            if self.queue_dirty {
                self.sort_queue();
                self.queue_dirty = false;
            }

            self.dispatch_ready_tasks(level);

            if self.wait_for_work(level) {
                break;
            }
        }
        self.drain_and_close();
    }

    fn debug_check(&self) -> bool {
        if !self.graph.nodes.is_empty() {
            for (key, value) in &self.graph.nodes {
                error!("unrelease node {key:?}: {value:?}");
            }
            panic!("nodes count error");
        }
        for (pos, holder) in &self.chunk_map {
            for i in &holder.tasks {
                debug_assert!(i.is_null());
            }
            debug_assert_eq!(
                holder.target_stage,
                StagedChunkEnum::level_to_stage(
                    *self.last_level.get(pos).unwrap_or(&ChunkLoading::MAX_LEVEL)
                )
            );
            let effective = holder.target_stage.max(holder.dependency_stage);
            debug_assert!(holder.current_stage >= effective);
            debug_assert!(holder.occupied.is_null());
            if holder.current_stage != StagedChunkEnum::None {
                debug_assert_eq!(
                    holder.chunk.as_ref().expect("chunk exists").get_stage_id(),
                    holder.current_stage as u8
                );
            }
        }
        true
    }
}
