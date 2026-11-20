//! A record of what `GenerationSchedule::receive_chunk` does, made before it is split
//! (`tests/golden/receive_chunk.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene builds a schedule by hand (holders, graph nodes, queue, a published chunk), hands
//! it one message and writes down what is left: every holder, every graph node, the queue, the
//! tasks that wait, the published chunks, the light of each chunk, and who was told.

use std::collections::BinaryHeap;
use std::fmt::Write;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crossbeam::queue::SegQueue;
use pumpkin_config::lighting::LightingEngineConfig;
use pumpkin_util::world_seed::Seed;
use slotmap::Key;

use crate::chunk::ChunkData;
use crate::chunk::format::LightContainer;
use crate::chunk_cache::{PublicChunks, Version};
use crate::chunk_system::chunk_holder::ChunkHolder;
use crate::chunk_system::chunk_state::{Chunk, StagedChunkEnum};
use crate::chunk_system::dag::{DAG, Node, NodeKey};
use crate::chunk_system::generation_cache::Cache;
use crate::chunk_system::worker_logic::RecvChunk;
use crate::chunk_system::{
    ChunkLevel, ChunkListener, ChunkPos, HashMapType, HashSetType, LevelChannel,
};
use crate::generation::generator::WorldGenerator;
use crate::generation::get_world_gen;
use crate::generation::proto_chunk::ProtoChunk;
use crate::level::SyncChunk;

use super::{GenerationSchedule, TaskHeapNode};

use StagedChunkEnum as S;

fn stage_of(index: usize) -> StagedChunkEnum {
    StagedChunkEnum::from(index as u8)
}

/// How a holder's `occupied` key is set up.
#[derive(Clone, Copy)]
pub(super) enum Occupied {
    Null,
    Live,
    /// A key of a node that is no longer in the graph.
    Stale,
}

#[derive(Clone, Copy)]
pub(super) enum Held {
    Nothing,
    Level,
    Proto(StagedChunkEnum),
}

#[derive(Clone, Copy)]
pub(super) struct Spec {
    x: i32,
    z: i32,
    current: StagedChunkEnum,
    target: StagedChunkEnum,
    dependency: StagedChunkEnum,
    public: bool,
    held: Held,
    /// The stages that have a task, from the lowest to this one.
    tasks_to: StagedChunkEnum,
    /// The lowest stage that has a task.
    first_task: StagedChunkEnum,
    occupied: Occupied,
}

impl Spec {
    pub(super) const fn at(x: i32, z: i32) -> Self {
        Self {
            x,
            z,
            current: S::None,
            target: S::Full,
            dependency: S::None,
            public: false,
            held: Held::Nothing,
            tasks_to: S::None,
            first_task: S::Empty,
            occupied: Occupied::Null,
        }
    }
    pub(super) const fn current(mut self, stage: StagedChunkEnum) -> Self {
        self.current = stage;
        self
    }
    pub(super) const fn target(mut self, stage: StagedChunkEnum) -> Self {
        self.target = stage;
        self
    }
    pub(super) const fn public(mut self) -> Self {
        self.public = true;
        self
    }
    pub(super) const fn held(mut self, held: Held) -> Self {
        self.held = held;
        self
    }
    pub(super) const fn tasks_to(mut self, stage: StagedChunkEnum) -> Self {
        self.tasks_to = stage;
        self
    }
    pub(super) const fn dependency(mut self, stage: StagedChunkEnum) -> Self {
        self.dependency = stage;
        self
    }
    pub(super) const fn first_task(mut self, stage: StagedChunkEnum) -> Self {
        self.first_task = stage;
        self
    }
    pub(super) const fn occupied(mut self, occupied: Occupied) -> Self {
        self.occupied = occupied;
        self
    }
}

pub(super) struct Fixture {
    pub(super) schedule: GenerationSchedule,
    pub(super) world_gen: Arc<WorldGenerator>,
    pub(super) heard: crossbeam::channel::Receiver<(ChunkPos, std::sync::Weak<ChunkData>)>,
}

impl Fixture {
    pub(super) fn new(world_gen: &Arc<WorldGenerator>, lighting: LightingEngineConfig) -> Self {
        let (send_chunk, recv_chunk) = crossbeam::channel::unbounded();
        let (io_read, _) = tokio::sync::mpsc::channel(4);
        let (io_write, _) = tokio::sync::mpsc::channel(4);
        let listener = Arc::new(ChunkListener::new());
        let heard = listener.add_global_chunk_listener();
        let schedule = GenerationSchedule {
            queue: BinaryHeap::new(),
            graph: DAG::default(),
            last_level: ChunkLevel::default(),
            last_high_priority: Vec::new(),
            send_level: Arc::new(LevelChannel::new()),
            public_chunks: PublicChunks::new(
                Arc::new(crate::level::ChunkMap::default()),
                Arc::new(Version::new()),
                Arc::new(SegQueue::new()),
            ),
            chunk_map: std::collections::HashMap::default(),
            unload_chunks: HashSetType::default(),
            waiting_for_chunks: HashSetType::default(),
            io_lock: Arc::new((
                Mutex::new(HashMapType::default()),
                tokio::sync::Notify::new(),
            )),
            running_task_count: 3,
            max_in_flight: 8,
            queue_dirty: false,
            recv_chunk,
            io_read,
            io_write,
            send_chunk,
            listener,
            lighting_config: lighting,
            last_unload: Instant::now(),
            generation_pool: Arc::new(
                match rayon::ThreadPoolBuilder::new().num_threads(1).build() {
                    Ok(pool) => pool,
                    Err(error) => panic!("no thread pool: {error}"),
                },
            ),
        };
        Self {
            schedule,
            world_gen: world_gen.clone(),
            heard,
        }
    }

    pub(super) fn chunk(&self, x: i32, z: i32, held: Held) -> Option<Chunk> {
        match held {
            Held::Nothing => None,
            Held::Level => Some(Chunk::Level(lit_chunk(x, z))),
            Held::Proto(stage) => Some(Chunk::Proto(self.proto(x, z, stage))),
        }
    }

    /// The holder at `(x, z)`, which the scene has set up.
    pub(super) fn holder_at(&mut self, x: i32, z: i32) -> &mut ChunkHolder {
        self.schedule
            .chunk_map
            .get_mut(&ChunkPos::new(x, z))
            .unwrap_or_else(|| panic!("the scene has no holder at {x},{z}"))
    }

    pub(super) fn proto(&self, x: i32, z: i32, stage: StagedChunkEnum) -> Box<ProtoChunk> {
        let mut proto = Box::new(ProtoChunk::new(x, z, &self.world_gen));
        proto.stage = stage;
        proto
    }

    /// Adds a holder with a chain of tasks, one per stage up to `tasks_to`.
    pub(super) fn holder(&mut self, spec: Spec) -> NodeKey {
        let pos = ChunkPos::new(spec.x, spec.z);
        let mut holder = ChunkHolder {
            target_stage: spec.target,
            dependency_stage: spec.dependency,
            current_stage: spec.current,
            chunk: self.chunk(spec.x, spec.z, spec.held),
            public: spec.public,
            ..ChunkHolder::default()
        };
        let graph = &mut self.schedule.graph;
        let mut previous = NodeKey::null();
        for index in (spec.first_task as usize)..=(spec.tasks_to as usize) {
            let key = graph.nodes.insert(Node::new(pos, stage_of(index)));
            if !previous.is_null() {
                graph.add_edge(previous, key);
            }
            holder.tasks[index] = key;
            previous = key;
        }
        match spec.occupied {
            Occupied::Null => {}
            Occupied::Live => {
                holder.occupied = graph.nodes.insert(Node::new(pos, S::Empty));
            }
            Occupied::Stale => {
                let key = graph.nodes.insert(Node::new(pos, S::Empty));
                graph.nodes.remove(key);
                holder.occupied = key;
            }
        }
        if spec.public
            && let Some(Chunk::Level(chunk)) = &holder.chunk
        {
            self.schedule.public_chunks.publish(pos, chunk.clone());
        }
        self.schedule.chunk_map.insert(pos, holder);
        previous
    }

    /// Puts the task of `stage` of the holder at `(x, z)` among the tasks that wait.
    pub(super) fn wait(&mut self, x: i32, z: i32, stage: StagedChunkEnum) {
        let key = self.schedule.chunk_map[&ChunkPos::new(x, z)].tasks[stage as usize];
        self.schedule.waiting_for_chunks.insert(key);
    }

    /// Marks the task of `stage` of the holder at `(x, z)` as running on a worker.
    pub(super) fn in_flight(&mut self, x: i32, z: i32, stage: StagedChunkEnum) {
        let key = self.schedule.chunk_map[&ChunkPos::new(x, z)].tasks[stage as usize];
        if let Some(node) = self.schedule.graph.nodes.get_mut(key) {
            node.in_flight = true;
        }
    }

    pub(super) fn publish_other(&self, x: i32, z: i32) {
        self.schedule
            .public_chunks
            .publish(ChunkPos::new(x, z), lit_chunk_with(x, z, 5));
    }

    /// A message with a `size` by `size` square of chunks from `(x, z)`.
    pub(super) fn generation(x: i32, z: i32, size: i32, chunks: Vec<Chunk>) -> RecvChunk {
        let mut cache = Cache::new(x, z, size);
        cache.chunks = chunks;
        RecvChunk::Generation(cache)
    }
}

/// An empty chunk whose light sections are not empty, so that a lighting override shows.
pub(super) fn lit_chunk(x: i32, z: i32) -> SyncChunk {
    lit_chunk_with(x, z, 3)
}

pub(super) fn lit_chunk_with(x: i32, z: i32, block: u8) -> SyncChunk {
    let chunk = ChunkData::empty(x, z);
    let mut engine = chunk
        .light_engine
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    engine.block_light = vec![LightContainer::Empty(block); 3].into_boxed_slice();
    engine.sky_light = vec![LightContainer::Empty(7); 3].into_boxed_slice();
    drop(engine);
    Arc::new(chunk)
}

fn light_text(chunk: &ChunkData) -> String {
    let engine = chunk
        .light_engine
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let part = |sections: &[LightContainer]| {
        let mut text = String::new();
        for section in sections {
            match section {
                LightContainer::Empty(value) => {
                    let _ = write!(text, "{value:x}");
                }
                LightContainer::Full(_) => text.push('f'),
            }
        }
        text
    };
    format!(
        "block {} sky {} dirty {}",
        part(&engine.block_light),
        part(&engine.sky_light),
        chunk.dirty.load(std::sync::atomic::Ordering::Relaxed)
    )
}

fn node_text(graph: &DAG, key: NodeKey) -> String {
    graph.nodes.get(key).map_or_else(
        || "gone".to_owned(),
        |node| {
            let mut to = Vec::new();
            let mut edge = node.edge;
            while let Some(found) = graph.edges.get(edge) {
                to.push(graph.nodes.get(found.to).map_or_else(
                    || "gone".to_owned(),
                    |target| format!("{:?}@{},{}", target.stage, target.pos.x, target.pos.y),
                ));
                edge = found.next;
            }
            format!(
                "{:?}@{},{} deg{} q{} f{} ->[{}]",
                node.stage,
                node.pos.x,
                node.pos.y,
                node.in_degree,
                u8::from(node.in_queue),
                u8::from(node.in_flight),
                to.join(",")
            )
        },
    )
}

pub(super) fn state_text(fixture: &Fixture) -> String {
    let schedule = &fixture.schedule;
    let mut out = String::new();
    let mut positions: Vec<ChunkPos> = schedule.chunk_map.keys().copied().collect();
    positions.sort_by_key(|pos| (pos.x, pos.y));
    for pos in positions {
        let holder = &schedule.chunk_map[&pos];
        let held = match &holder.chunk {
            None => "none".to_owned(),
            Some(Chunk::Level(chunk)) => format!("level[{}]", light_text(chunk)),
            Some(Chunk::Proto(proto)) => format!("proto {:?}", proto.stage),
        };
        let occupied = if holder.occupied.is_null() {
            "null".to_owned()
        } else {
            node_text(&schedule.graph, holder.occupied)
        };
        let tasks: Vec<String> = holder
            .tasks
            .iter()
            .filter(|key| !key.is_null())
            .map(|&key| node_text(&schedule.graph, key))
            .collect();
        let _ = writeln!(
            out,
            "holder {},{} cur {:?} target {:?} dep {:?} public {} chunk {held} occupied {occupied} tasks [{}]",
            pos.x,
            pos.y,
            holder.current_stage,
            holder.target_stage,
            holder.dependency_stage,
            holder.public,
            tasks.join("; ")
        );
    }
    let mut nodes: Vec<String> = schedule
        .graph
        .nodes
        .keys()
        .map(|key| node_text(&schedule.graph, key))
        .collect();
    nodes.sort();
    let _ = writeln!(
        out,
        "graph {} nodes {} edges: {}",
        nodes.len(),
        schedule.graph.edges.len(),
        nodes.join(" | ")
    );
    let mut queue: Vec<String> = schedule
        .queue
        .iter()
        .map(|TaskHeapNode(priority, key)| {
            format!("{priority:?} {}", node_text(&schedule.graph, *key))
        })
        .collect();
    queue.sort();
    let _ = writeln!(out, "queue {}: {}", queue.len(), queue.join(" | "));
    let mut waiting: Vec<String> = schedule
        .waiting_for_chunks
        .iter()
        .map(|&key| node_text(&schedule.graph, key))
        .collect();
    waiting.sort();
    let _ = writeln!(out, "waiting {}: {}", waiting.len(), waiting.join(" | "));
    let mut public = Vec::new();
    for x in -3..=3 {
        for z in -3..=3 {
            if let Some(chunk) = schedule.public_chunks.get(&ChunkPos::new(x, z)) {
                public.push(format!("{x},{z} [{}]", light_text(&chunk)));
            }
        }
    }
    let _ = writeln!(out, "public {}: {}", public.len(), public.join(" | "));
    let mut heard = Vec::new();
    while let Ok((pos, _)) = fixture.heard.try_recv() {
        heard.push(format!("{},{}", pos.x, pos.y));
    }
    let _ = writeln!(
        out,
        "running {} told {}: {}",
        schedule.running_task_count,
        heard.len(),
        heard.join(" ")
    );
    out
}

/// One scene: what the schedule looks like, and the message it gets for the chunk at `at`.
struct Scene {
    name: &'static str,
    lighting: LightingEngineConfig,
    at: (i32, i32),
    build: Box<dyn Fn(&mut Fixture) -> RecvChunk>,
}

/// The nine holders around the origin, each waiting for its next stages.
fn ring(fixture: &mut Fixture, current: StagedChunkEnum, occupied: Occupied) {
    for x in -1..=1 {
        for z in -1..=1 {
            fixture.holder(
                Spec::at(x, z)
                    .current(current)
                    .tasks_to(S::Features)
                    .occupied(occupied),
            );
        }
    }
}

fn level(_fixture: &Fixture, x: i32, z: i32) -> Chunk {
    Chunk::Level(lit_chunk(x, z))
}

fn proto(fixture: &Fixture, x: i32, z: i32, stage: StagedChunkEnum) -> Chunk {
    Chunk::Proto(fixture.proto(x, z, stage))
}

/// A square of chunks, each made by `make` from its position.
fn square(
    fixture: &Fixture,
    from: (i32, i32),
    size: i32,
    make: impl Fn(&Fixture, i32, i32) -> Chunk,
) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    for dx in 0..size {
        for dz in 0..size {
            chunks.push(make(fixture, from.0 + dx, from.1 + dz));
        }
    }
    chunks
}

fn io_level(fixture: &mut Fixture) -> RecvChunk {
    fixture.holder(Spec::at(0, 0).tasks_to(S::Full).occupied(Occupied::Live));
    RecvChunk::IO(level(fixture, 0, 0))
}

fn io_scenes() -> Vec<Scene> {
    let io = |name, lighting, build: fn(&mut Fixture) -> RecvChunk| Scene {
        name,
        lighting,
        at: (0, 0),
        build: Box::new(build),
    };
    let default = LightingEngineConfig::Default;
    vec![
        io("io_level_default", default, io_level),
        io("io_level_full_light", LightingEngineConfig::Full, io_level),
        io("io_level_dark", LightingEngineConfig::Dark, io_level),
        io("io_level_replaces_public", default, |f| {
            f.publish_other(0, 0);
            io_level(f)
        }),
        io("io_level_holder_has_chunk", default, |f| {
            f.holder(
                Spec::at(0, 0)
                    .held(Held::Proto(S::Empty))
                    .tasks_to(S::Full)
                    .occupied(Occupied::Live),
            );
            RecvChunk::IO(level(f, 0, 0))
        }),
        io("io_proto_noise", default, |f| {
            f.holder(Spec::at(0, 0).tasks_to(S::Full).occupied(Occupied::Live));
            RecvChunk::IO(proto(f, 0, 0, S::Noise))
        }),
        io("io_proto_empty_keeps_tasks", default, |f| {
            f.holder(
                Spec::at(0, 0)
                    .tasks_to(S::Features)
                    .occupied(Occupied::Live),
            );
            RecvChunk::IO(proto(f, 0, 0, S::Empty))
        }),
        io("io_proto_downgrades_public", default, |f| {
            f.holder(
                Spec::at(0, 0)
                    .held(Held::Level)
                    .public()
                    .current(S::None)
                    .tasks_to(S::Full)
                    .occupied(Occupied::Live),
            );
            RecvChunk::IO(proto(f, 0, 0, S::Features))
        }),
        io("io_proto_wakes_waiting_task", default, |f| {
            f.holder(
                Spec::at(0, 0)
                    .tasks_to(S::Features)
                    .occupied(Occupied::Live),
            );
            f.holder(
                Spec::at(1, 0)
                    .current(S::Empty)
                    .target(S::Biomes)
                    .first_task(S::Biomes)
                    .tasks_to(S::Biomes)
                    .held(Held::Proto(S::Empty)),
            );
            f.wait(1, 0, S::Biomes);
            RecvChunk::IO(proto(f, 0, 0, S::Empty))
        }),
        io("io_level_drops_stale_waiting_task", default, |f| {
            f.holder(
                Spec::at(0, 0)
                    .tasks_to(S::Features)
                    .occupied(Occupied::Live),
            );
            f.holder(Spec::at(5, 5).current(S::Empty).tasks_to(S::Biomes));
            f.wait(5, 5, S::Biomes);
            RecvChunk::IO(level(f, 0, 0))
        }),
    ]
}

fn generation_scenes() -> Vec<Scene> {
    let default = LightingEngineConfig::Default;
    vec![
        generation_scene("gen_level_center", default, |f| {
            ring(f, S::Empty, Occupied::Live);
            f.holder_at(0, 0).current_stage = S::Spawn;
            let chunks = square(f, (-1, -1), 3, |f, x, z| {
                if (x, z) == (0, 0) {
                    level(f, x, z)
                } else {
                    proto(f, x, z, S::Features)
                }
            });
            Fixture::generation(-1, -1, 3, chunks)
        }),
        generation_scene("gen_level_center_was_public", default, |f| {
            ring(f, S::Empty, Occupied::Live);
            let center = f.holder_at(0, 0);
            center.current_stage = S::Spawn;
            center.public = true;
            f.publish_other(0, 0);
            let chunks = square(f, (-1, -1), 3, |f, x, z| {
                if (x, z) == (0, 0) {
                    level(f, x, z)
                } else {
                    proto(f, x, z, S::Lighting)
                }
            });
            Fixture::generation(-1, -1, 3, chunks)
        }),
        generation_scene(
            "gen_level_center_wrong_stage_full_light",
            LightingEngineConfig::Full,
            |f| {
                ring(f, S::Empty, Occupied::Stale);
                f.holder_at(0, 0).current_stage = S::Lighting;
                let chunks = square(f, (-1, -1), 3, level);
                Fixture::generation(-1, -1, 3, chunks)
            },
        ),
        generation_scene("gen_level_everywhere_occupied", default, |f| {
            ring(f, S::Empty, Occupied::Live);
            f.holder_at(0, 0).current_stage = S::Spawn;
            let chunks = square(f, (-1, -1), 3, level);
            Fixture::generation(-1, -1, 3, chunks)
        }),
        generation_scene(
            "gen_level_center_dark_unoccupied",
            LightingEngineConfig::Dark,
            |f| {
                ring(f, S::Empty, Occupied::Null);
                f.holder_at(0, 0).current_stage = S::Spawn;
                let chunks = square(f, (-1, -1), 3, level);
                Fixture::generation(-1, -1, 3, chunks)
            },
        ),
    ]
}

fn proto_generation_scenes() -> Vec<Scene> {
    let default = LightingEngineConfig::Default;
    vec![
        generation_scene("gen_proto_center", default, |f| {
            ring(f, S::Empty, Occupied::Live);
            let chunks = square(f, (-1, -1), 3, |f, x, z| {
                proto(
                    f,
                    x,
                    z,
                    if (x, z) == (0, 0) {
                        S::Biomes
                    } else {
                        S::Noise
                    },
                )
            });
            Fixture::generation(-1, -1, 3, chunks)
        }),
        generation_scene("gen_proto_neighbors_keep_higher_stage", default, |f| {
            ring(f, S::Spawn, Occupied::Stale);
            let chunks = square(f, (-1, -1), 3, |f, x, z| proto(f, x, z, S::Surface));
            Fixture::generation(-1, -1, 3, chunks)
        }),
        generation_scene("gen_proto_neighbors_unoccupied", default, |f| {
            ring(f, S::StructureStart, Occupied::Null);
            let chunks = square(f, (-1, -1), 3, |f, x, z| {
                proto(f, x, z, if x == 1 { S::Features } else { S::Carvers })
            });
            Fixture::generation(-1, -1, 3, chunks)
        }),
        generation_scene("gen_one_chunk", default, |f| {
            f.holder(
                Spec::at(0, 0)
                    .current(S::Spawn)
                    .tasks_to(S::Full)
                    .occupied(Occupied::Live),
            );
            let chunks = vec![level(f, 0, 0)];
            Fixture::generation(0, 0, 1, chunks)
        }),
        generation_scene("gen_wakes_unrelated_waiting_task", default, |f| {
            f.holder(
                Spec::at(0, 0)
                    .current(S::Spawn)
                    .tasks_to(S::Full)
                    .occupied(Occupied::Live),
            );
            f.holder(
                Spec::at(5, 5)
                    .current(S::Empty)
                    .target(S::Biomes)
                    .first_task(S::Biomes)
                    .tasks_to(S::Biomes)
                    .held(Held::Proto(S::Empty)),
            );
            f.wait(5, 5, S::Biomes);
            let chunks = vec![level(f, 0, 0)];
            Fixture::generation(0, 0, 1, chunks)
        }),
        generation_scene("gen_two_by_two_wakes_waiting", default, |f| {
            for (x, z) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                f.holder(
                    Spec::at(x, z)
                        .current(S::Empty)
                        .target(S::Biomes)
                        .first_task(S::Biomes)
                        .tasks_to(S::Biomes)
                        .held(Held::Proto(S::Empty))
                        .occupied(Occupied::Live),
                );
            }
            f.wait(1, 1, S::Biomes);
            let chunks = square(f, (0, 0), 2, |f, x, z| proto(f, x, z, S::Biomes));
            Fixture::generation(0, 0, 2, chunks)
        }),
    ]
}

fn failure_scenes() -> Vec<Scene> {
    vec![
        failure("fail_resets_holder", |f| {
            f.holder(
                Spec::at(0, 0)
                    .current(S::Noise)
                    .target(S::Features)
                    .held(Held::Proto(S::Noise))
                    .tasks_to(S::Features)
                    .occupied(Occupied::Live),
            );
            f.schedule.last_level.insert(ChunkPos::new(0, 0), 31);
        }),
        failure("fail_with_dependency_and_waiting", |f| {
            f.holder(
                Spec::at(0, 0)
                    .current(S::Biomes)
                    .target(S::Surface)
                    .held(Held::Proto(S::Biomes))
                    .tasks_to(S::Surface)
                    .occupied(Occupied::Stale),
            );
            f.wait(0, 0, S::Noise);
        }),
        failure("fail_target_none", |f| {
            f.holder(
                Spec::at(0, 0)
                    .current(S::Biomes)
                    .target(S::None)
                    .held(Held::Proto(S::Biomes))
                    .tasks_to(S::Noise)
                    .occupied(Occupied::Null),
            );
        }),
        failure("fail_no_holder", |_| {}),
    ]
}

fn scenes() -> Vec<Scene> {
    let mut all = io_scenes();
    all.extend(generation_scenes());
    all.extend(proto_generation_scenes());
    all.extend(failure_scenes());
    all
}

fn generation_scene(
    name: &'static str,
    lighting: LightingEngineConfig,
    build: fn(&mut Fixture) -> RecvChunk,
) -> Scene {
    Scene {
        name,
        lighting,
        at: (0, 0),
        build: Box::new(build),
    }
}

/// A failure scene: the setup is `setup`, the message says that the chunk at the origin failed.
fn failure(name: &'static str, setup: fn(&mut Fixture)) -> Scene {
    Scene {
        name,
        lighting: LightingEngineConfig::Default,
        at: (0, 0),
        build: Box::new(move |fixture| {
            setup(fixture);
            RecvChunk::GenerationFailure {
                pos: ChunkPos::new(0, 0),
                stage: S::Noise,
                error: "boom".to_owned(),
            }
        }),
    }
}

#[test]
fn receive_chunk() -> Result<(), Box<dyn std::error::Error>> {
    let world_gen = test_world_gen();
    let mut report = String::new();
    for scene in scenes() {
        let mut fixture = Fixture::new(&world_gen, scene.lighting);
        let message = (scene.build)(&mut fixture);
        fixture
            .schedule
            .receive_chunk(ChunkPos::new(scene.at.0, scene.at.1), message);
        let _ = writeln!(report, "== {}", scene.name);
        report.push_str(&state_text(&fixture));
    }

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("receive_chunk.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}

pub(super) fn test_world_gen() -> Arc<WorldGenerator> {
    Arc::from(get_world_gen(
        Seed(1),
        pumpkin_data::dimension::Dimension::OVERWORLD,
        true,
        Vec::new(),
        "minecraft:plains".to_owned(),
    ))
}
