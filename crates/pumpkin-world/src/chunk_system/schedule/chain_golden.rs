//! A record of what `GenerationSchedule::ensure_dependency_chain` does, made before it is split
//! (`tests/golden/dependency_chain.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene sets up one chunk in a schedule, adds a task of another chunk that needs the chunk
//! to reach a stage, and writes down the answer and everything the call left behind.

use std::fmt::Write;

use pumpkin_config::lighting::LightingEngineConfig;
use slotmap::Key;

use super::receive_golden::{Fixture, Held, Occupied, Spec, state_text, test_world_gen};
use crate::chunk_system::ChunkPos;
use crate::chunk_system::chunk_state::StagedChunkEnum as S;
use crate::chunk_system::dag::Node;
use crate::chunk_system::schedule::GenerationSchedule;

struct Scene {
    name: &'static str,
    spec: Spec,
    req: S,
}

const fn scene(name: &'static str, spec: Spec, req: S) -> Scene {
    Scene { name, spec, req }
}

fn base() -> Spec {
    Spec::at(0, 0).target(S::None)
}

fn scenes() -> Vec<Scene> {
    let held = |stage| Held::Proto(stage);
    vec![
        scene("fresh_chunk_needs_biomes", base(), S::Biomes),
        scene("fresh_chunk_needs_noise", base(), S::Noise),
        scene("fresh_chunk_needs_empty", base(), S::Empty),
        scene(
            "fresh_chunk_wanted_higher",
            base().target(S::Surface),
            S::Biomes,
        ),
        scene(
            "fresh_chunk_occupied",
            base().occupied(Occupied::Live),
            S::Noise,
        ),
        scene(
            "partly_built",
            base().current(S::Biomes).held(held(S::Biomes)),
            S::Surface,
        ),
        scene(
            "already_there",
            base().current(S::Surface).held(held(S::Surface)),
            S::Biomes,
        ),
        scene(
            "exactly_there",
            base().current(S::Noise).held(held(S::Noise)),
            S::Noise,
        ),
        scene(
            "dependency_already_higher",
            base().dependency(S::Carvers),
            S::Biomes,
        ),
        scene(
            "target_above_current_no_tasks",
            base()
                .target(S::Features)
                .current(S::Noise)
                .held(held(S::Noise)),
            S::Surface,
        ),
        scene(
            "existing_lower_task",
            base().first_task(S::Empty).tasks_to(S::Biomes),
            S::Noise,
        ),
        scene(
            "existing_upper_task",
            base().first_task(S::Noise).tasks_to(S::Noise),
            S::Noise,
        ),
        scene(
            "existing_middle_task",
            base()
                .first_task(S::StructureReferences)
                .tasks_to(S::StructureReferences),
            S::Surface,
        ),
        scene(
            "existing_task_queued_before",
            base()
                .current(S::Biomes)
                .held(held(S::Biomes))
                .first_task(S::StructureStart)
                .tasks_to(S::StructureStart),
            S::StructureReferences,
        ),
        scene(
            "full_chunk",
            base().current(S::Full).held(Held::Level).public(),
            S::Spawn,
        ),
    ]
}

#[test]
fn dependency_chain() -> Result<(), Box<dyn std::error::Error>> {
    let world_gen = test_world_gen();
    let mut report = String::new();
    for scene in scenes() {
        let mut fixture = Fixture::new(&world_gen, LightingEngineConfig::Default);
        fixture.holder(scene.spec);
        let waiter = fixture
            .schedule
            .graph
            .nodes
            .insert(Node::new(ChunkPos::new(9, 9), S::Full));
        let Fixture { schedule, .. } = &mut fixture;
        let mut holder = schedule
            .chunk_map
            .remove(&ChunkPos::new(0, 0))
            .unwrap_or_default();
        let before = holder.occupied_by;
        let created = GenerationSchedule::ensure_dependency_chain(
            &mut schedule.graph,
            &mut schedule.queue,
            &schedule.last_level,
            &schedule.last_high_priority,
            waiter,
            ChunkPos::new(0, 0),
            &mut holder,
            scene.req,
        );
        let mut heads = 0;
        let mut edge = holder.occupied_by;
        while let Some(found) = schedule.graph.edges.get(edge) {
            heads += 1;
            edge = found.next;
        }
        let flags: Vec<String> = created
            .iter()
            .enumerate()
            .filter(|(_, flag)| **flag)
            .map(|(index, _)| format!("{:?}", S::from(index as u8)))
            .collect();
        let _ = writeln!(
            report,
            "== {} req {:?} -> created [{}] occupied_by {} edges (was null {}) waiter in_degree {}",
            scene.name,
            scene.req,
            flags.join(","),
            heads,
            before.is_null(),
            schedule
                .graph
                .nodes
                .get(waiter)
                .map_or(99, |node| node.in_degree)
        );
        schedule.chunk_map.insert(ChunkPos::new(0, 0), holder);
        report.push_str(&state_text(&fixture));
    }

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("dependency_chain.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
