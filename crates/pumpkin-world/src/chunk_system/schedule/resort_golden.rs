//! A record of what `GenerationSchedule::resort_work` does, made before it is split
//! (`tests/golden/resort_work.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene builds a schedule by hand, hands it a level change (and maybe a high-priority
//! list) and writes down the answer and what is left of the schedule.

use std::fmt::Write;

use pumpkin_config::lighting::LightingEngineConfig;

use super::receive_golden::{Fixture, Held, Occupied, Spec, state_text, test_world_gen};
use crate::chunk_system::channel::LevelChange;
use crate::chunk_system::chunk_state::StagedChunkEnum as S;
use crate::chunk_system::{ChunkLevel, ChunkPos, HashMapType};

type Message = (Option<LevelChange>, Option<Vec<ChunkPos>>);

struct Scene {
    name: &'static str,
    build: fn(&mut Fixture) -> Message,
}

/// The level of the loading tickets that gives each stage.
const fn level_of(stage: S) -> i8 {
    match stage {
        S::Full => 43,
        S::Spawn => 44,
        S::Lighting => 45,
        S::Features => 46,
        S::Carvers => 47,
        S::Surface => 48,
        _ => 49,
    }
}

/// A change for the chunks `(x, z, from, to)`; every other chunk in `levels` keeps its level.
fn change(moves: &[(i32, i32, S, S)], levels: &[(i32, i32, i8)]) -> Message {
    let mut stages: HashMapType<ChunkPos, (S, S)> = HashMapType::default();
    let mut level: ChunkLevel = ChunkLevel::default();
    for &(x, z, from, to) in moves {
        stages.insert(ChunkPos::new(x, z), (from, to));
        level.insert(ChunkPos::new(x, z), level_of(to));
    }
    for &(x, z, value) in levels {
        level.insert(ChunkPos::new(x, z), value);
    }
    (Some((stages, level)), None)
}

fn target_holder(f: &mut Fixture, x: i32, z: i32, target: S) {
    f.holder(Spec::at(x, z).target(target));
}

fn raising_scenes() -> Vec<Scene> {
    vec![
        Scene {
            name: "nothing",
            build: |_| (None, None),
        },
        Scene {
            name: "high_priority_only",
            build: |_| (None, Some(vec![ChunkPos::new(2, -3), ChunkPos::new(0, 0)])),
        },
        Scene {
            name: "load_surface_from_nothing",
            build: |_| change(&[(0, 0, S::None, S::Surface)], &[]),
        },
        Scene {
            name: "load_with_high_priority",
            build: |_| {
                let mut message = change(&[(0, 0, S::None, S::Carvers)], &[]);
                message.1 = Some(vec![ChunkPos::new(0, 0)]);
                message
            },
        },
        Scene {
            name: "load_two_chunks",
            build: |_| {
                change(
                    &[(0, 0, S::None, S::Surface), (4, 0, S::None, S::Carvers)],
                    &[],
                )
            },
        },
        Scene {
            name: "load_features",
            build: |_| change(&[(0, 0, S::None, S::Features)], &[]),
        },
        Scene {
            name: "raise_existing_holder",
            build: |f| {
                f.holder(
                    Spec::at(0, 0)
                        .current(S::Noise)
                        .target(S::Carvers)
                        .held(Held::Proto(S::Noise))
                        .tasks_to(S::Carvers)
                        .first_task(S::Surface)
                        .occupied(Occupied::Live),
                );
                change(&[(0, 0, S::Carvers, S::Features)], &[])
            },
        },
        Scene {
            name: "raise_with_dependency",
            build: |f| {
                f.holder(
                    Spec::at(0, 0)
                        .current(S::Empty)
                        .target(S::Biomes)
                        .held(Held::Proto(S::Empty))
                        .dependency(S::Noise),
                );
                change(&[(0, 0, S::Biomes, S::Surface)], &[])
            },
        },
    ]
}

fn lowering_scenes() -> Vec<Scene> {
    vec![
        Scene {
            name: "lower_drops_tasks",
            build: |f| {
                f.holder(
                    Spec::at(0, 0)
                        .current(S::Empty)
                        .target(S::Features)
                        .held(Held::Proto(S::Empty))
                        .first_task(S::Biomes)
                        .tasks_to(S::Features),
                );
                change(&[(0, 0, S::Features, S::Surface)], &[])
            },
        },
        Scene {
            name: "lower_keeps_in_flight_task",
            build: |f| {
                f.holder(
                    Spec::at(0, 0)
                        .current(S::Empty)
                        .target(S::Features)
                        .held(Held::Proto(S::Empty))
                        .first_task(S::Biomes)
                        .tasks_to(S::Features),
                );
                f.in_flight(0, 0, S::Noise);
                f.wait(0, 0, S::Surface);
                change(&[(0, 0, S::Features, S::Surface)], &[])
            },
        },
        Scene {
            name: "lower_below_current_stage",
            build: |f| {
                f.holder(
                    Spec::at(0, 0)
                        .current(S::Features)
                        .target(S::Features)
                        .held(Held::Proto(S::Features))
                        .first_task(S::Carvers)
                        .tasks_to(S::Features),
                );
                change(&[(0, 0, S::Features, S::Surface)], &[])
            },
        },
        Scene {
            name: "unload",
            build: |f| {
                f.holder(
                    Spec::at(0, 0)
                        .current(S::Empty)
                        .target(S::Biomes)
                        .held(Held::Proto(S::Empty))
                        .first_task(S::Biomes)
                        .tasks_to(S::Biomes),
                );
                change(&[(0, 0, S::Biomes, S::None)], &[])
            },
        },
        Scene {
            name: "unload_with_live_dependency",
            build: |f| {
                f.holder(
                    Spec::at(0, 0)
                        .current(S::Empty)
                        .target(S::Biomes)
                        .held(Held::Proto(S::Empty))
                        .dependency(S::Noise)
                        .occupied(Occupied::Live),
                );
                f.holder(
                    Spec::at(1, 0)
                        .current(S::Empty)
                        .target(S::None)
                        .first_task(S::Biomes)
                        .tasks_to(S::Biomes),
                );
                change(&[(0, 0, S::Biomes, S::None)], &[])
            },
        },
        Scene {
            name: "unload_without_edges",
            build: |f| {
                f.holder(
                    Spec::at(0, 0)
                        .current(S::Empty)
                        .target(S::Biomes)
                        .held(Held::Proto(S::Empty))
                        .dependency(S::Noise),
                );
                change(&[(0, 0, S::Biomes, S::None)], &[])
            },
        },
    ]
}

fn reloading_scenes() -> Vec<Scene> {
    vec![
        Scene {
            name: "reload_cancels_unload",
            build: |f| {
                f.holder(
                    Spec::at(0, 0)
                        .current(S::Empty)
                        .target(S::None)
                        .held(Held::Proto(S::Empty)),
                );
                f.holder(Spec::at(1, 1).current(S::Empty).target(S::None));
                f.schedule.unload_chunks.insert(ChunkPos::new(0, 0));
                f.schedule.unload_chunks.insert(ChunkPos::new(1, 1));
                change(&[(0, 0, S::None, S::Surface)], &[])
            },
        },
        Scene {
            name: "reload_publishes_full_chunk",
            build: |f| {
                f.holder(
                    Spec::at(0, 0)
                        .current(S::Full)
                        .target(S::None)
                        .held(Held::Level),
                );
                change(&[(0, 0, S::None, S::Full)], &[])
            },
        },
        Scene {
            name: "reload_full_chunk_dark_lighting",
            build: |f| {
                f.schedule.lighting_config = LightingEngineConfig::Dark;
                f.holder(
                    Spec::at(0, 0)
                        .current(S::Full)
                        .target(S::None)
                        .held(Held::Level),
                );
                change(&[(0, 0, S::None, S::Full)], &[])
            },
        },
        Scene {
            name: "reload_published_full_chunk",
            build: |f| {
                f.holder(
                    Spec::at(0, 0)
                        .current(S::Full)
                        .target(S::None)
                        .held(Held::Level)
                        .public(),
                );
                change(&[(0, 0, S::None, S::Full)], &[])
            },
        },
        Scene {
            name: "neighbor_already_built",
            build: |f| {
                for x in -3..=3 {
                    for z in -3..=3 {
                        f.holder(
                            Spec::at(x, z)
                                .current(S::Noise)
                                .target(S::None)
                                .held(Held::Proto(S::Noise))
                                .first_task(S::Surface)
                                .tasks_to(S::Surface),
                        );
                    }
                }
                change(&[(0, 0, S::None, S::Carvers)], &[(5, 5, 20)])
            },
        },
        Scene {
            name: "old_levels_matter_for_priority",
            build: |f| {
                let mut levels = ChunkLevel::default();
                levels.insert(ChunkPos::new(1, 0), 10);
                f.schedule.last_level = levels;
                target_holder(f, 3, 3, S::None);
                change(&[(0, 0, S::None, S::Features)], &[(2, 0, 30)])
            },
        },
    ]
}

fn scenes() -> Vec<Scene> {
    let mut all = raising_scenes();
    all.extend(lowering_scenes());
    all.extend(reloading_scenes());
    all
}

#[test]
fn resort_work() -> Result<(), Box<dyn std::error::Error>> {
    let world_gen = test_world_gen();
    let mut report = String::new();
    for scene in scenes() {
        let mut fixture = Fixture::new(&world_gen, LightingEngineConfig::Default);
        let message = (scene.build)(&mut fixture);
        let answer = fixture.schedule.resort_work(message);
        let _ = writeln!(report, "== {} -> {answer}", scene.name);
        let schedule = &fixture.schedule;
        let mut unload: Vec<String> = schedule
            .unload_chunks
            .iter()
            .map(|pos| format!("{},{}", pos.x, pos.y))
            .collect();
        unload.sort();
        let mut levels: Vec<String> = schedule
            .last_level
            .iter()
            .map(|(pos, level)| format!("{},{}={level}", pos.x, pos.y))
            .collect();
        levels.sort();
        let high: Vec<String> = schedule
            .last_high_priority
            .iter()
            .map(|pos| format!("{},{}", pos.x, pos.y))
            .collect();
        let _ = writeln!(
            report,
            "dirty {} unload {}: {} levels {}: {} high: {}",
            schedule.queue_dirty,
            unload.len(),
            unload.join(" "),
            levels.len(),
            levels.join(" "),
            high.join(" ")
        );
        report.push_str(&state_text(&fixture));
    }

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("resort_work.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
