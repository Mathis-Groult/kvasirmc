//! A record of `/fillbiome` run through the shared script harness
//! (`tests/golden/fillbiome_command.txt`; rewrite it on purpose with `KVASIR_BLESS=1`), made before
//! `FillBiomeExecutor::execute` was split.
//!
//! Chunks -3..=9 on both axes are loaded empty (every cell holds biome id 0, badlands, the first
//! in the registry, so filling with badlands changes nothing). Per step the command, its
//! result, every packet each player was sent, and every loaded chunk whose biomes changed in that
//! step: its position, the number of cells per biome id and an FNV hash of all its biome cells in
//! order.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::sync::Arc;

use pumpkin_util::math::vector2::Vector2;

use crate::command::commands::script_golden::{self, TestResult, fnv};
use crate::entity::ai::test_support;
use crate::world::World;

const CHUNKS: std::ops::RangeInclusive<i32> = -3..=9;

const STEPS: &[&str] = &[
    // One cell, one chunk.
    "fillbiome 0 64 0 0 64 0 minecraft:desert",
    "fillbiome 0 64 0 3 67 3 minecraft:desert",
    "fillbiome 1 65 1 2 66 2 minecraft:jungle",
    // Order of the corners does not matter.
    "fillbiome 15 70 15 4 64 4 minecraft:badlands",
    // Across chunk borders, into negative coordinates.
    "fillbiome -20 60 -20 20 60 20 minecraft:snowy_plains",
    "fillbiome -1 -64 -1 0 -61 0 minecraft:deep_dark",
    "fillbiome 0 316 0 3 319 3 minecraft:the_void",
    // Replace filters.
    "fillbiome -20 60 -20 20 60 20 minecraft:ocean replace minecraft:snowy_plains",
    "fillbiome -20 60 -20 20 60 20 minecraft:river replace minecraft:plains",
    "fillbiome 0 60 0 40 80 40 minecraft:mushroom_fields replace #minecraft:is_ocean",
    "fillbiome 0 60 0 40 80 40 minecraft:forest replace minecraft:desert",
    "fillbiome 0 60 0 40 80 40 minecraft:forest replace minecraft:cherry_grove",
    // The size limit: 32768 cells.
    "fillbiome 0 0 0 127 127 127 minecraft:savanna",
    "fillbiome 0 0 0 128 127 127 minecraft:savanna",
    "fillbiome 0 0 0 131 131 131 minecraft:savanna",
    // Partly outside the loaded chunks.
    "fillbiome 140 64 140 170 64 170 minecraft:taiga",
    // Errors.
    "fillbiome 0 -65 0 0 -65 0 minecraft:desert",
    "fillbiome 0 320 0 0 320 0 minecraft:desert",
    "fillbiome 0 64 0 0 64 0 minecraft:no_such_biome",
    "fillbiome 0 64 0 0 64 0 minecraft:desert replace minecraft:no_such_biome",
    "fillbiome 0 64 0 0 64 0 minecraft:desert replace #minecraft:no_such_tag",
    "fillbiome 0 64 0 minecraft:desert",
    // Appended after the mutation run: one corner out of bounds, and the size limit away from
    // the origin (cells 1..=32 on every axis).
    "fillbiome 0 -65 0 0 64 0 minecraft:desert",
    "fillbiome 0 64 0 0 400 0 minecraft:desert",
    "fillbiome 4 4 4 131 131 131 minecraft:savanna",
    "fillbiome 4 4 4 135 131 131 minecraft:savanna",
];

/// Every loaded chunk's biome record (`ids` and hash), by position.
fn biomes(world: &Arc<World>) -> BTreeMap<(i32, i32), String> {
    let mut all = BTreeMap::new();
    for cx in CHUNKS {
        for cz in CHUNKS {
            let record = world.level.read_chunk_sync(&Vector2::new(cx, cz), |chunk| {
                let mut cells = Vec::new();
                let mut counts: BTreeMap<u8, usize> = BTreeMap::new();
                let mut section = 0;
                while chunk.section.get_noise_biome(section, 0, 0, 0).is_some() {
                    for y in 0..4 {
                        for z in 0..4 {
                            for x in 0..4 {
                                if let Some(id) = chunk.section.get_noise_biome(section, x, y, z) {
                                    cells.push(id);
                                    *counts.entry(id).or_default() += 1;
                                }
                            }
                        }
                    }
                    section += 1;
                }
                let counts: Vec<String> =
                    counts.iter().map(|(id, n)| format!("{id}x{n}")).collect();
                format!("{} {:016x}", counts.join("+"), fnv(&cells))
            });
            if let Some(record) = record {
                all.insert((cx, cz), record);
            }
        }
    }
    all
}

#[tokio::test(flavor = "multi_thread")]
async fn fillbiome_command_changes_the_same_cells() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    for cx in CHUNKS {
        for cz in CHUNKS {
            world.level.loaded_chunks.insert(
                Vector2::new(cx, cz),
                pumpkin_world::chunk::ChunkData::empty_sync(cx, cz),
            );
        }
    }
    let mut online = script_golden::join(&world).await?;
    let source_player = online[0].player.clone();

    let mut before = biomes(&world);
    let mut now = String::new();
    writeln!(
        now,
        "start; {} chunks, first {}",
        before.len(),
        before.values().next().map_or("none", String::as_str)
    )?;
    for step in STEPS {
        let result = script_golden::run(&server, &source_player, step);
        let sent = script_golden::packets(&mut online);
        let after = biomes(&world);
        let changed: Vec<String> = after
            .iter()
            .filter(|(pos, record)| before.get(pos) != Some(record))
            .map(|((cx, cz), record)| format!("{cx},{cz}={record}"))
            .collect();
        writeln!(
            now,
            "{step} -> {result};{sent}; changed [{}]",
            changed.join(" ")
        )?;
        before = after;
    }

    script_golden::bless_or_compare("fillbiome_command.txt", &now)?;
    script_golden::finish(&server, &world, &online).await;
    Ok(())
}
