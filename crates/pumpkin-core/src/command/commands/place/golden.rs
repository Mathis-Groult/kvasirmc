//! A record of `/place structure` and `/place feature` run through the real dispatcher
//! (`tests/golden/place_command.txt`; rewrite it on purpose with `KVASIR_BLESS=1`), made before
//! `PlaceStructureExecutor::execute` and `PlaceFeatureExecutor::execute` were split.
//!
//! Every case runs in its own area of loaded empty chunks (17 x 17 chunks around the target,
//! 32 chunks apart), from the console standing at the area's centre. One line per case: the
//! command (relative to the area), its result (count or error text), then every chunk that is no
//! longer empty, as its chunk offset from the target's chunk, an FNV-1a hash of its block states,
//! and its block entities (count and a hash of their sorted positions and ids). A case that does
//! not return within the deadline is recorded as `HANG`.

use std::fmt::Write;
use std::sync::Arc;
use std::time::Duration;

use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::text::TextComponent;

use crate::command::{CommandSender, CommandSource};
use crate::entity::ai::test_support;
use crate::server::Server;
use crate::world::World;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Chunks loaded on each side of the target's chunk.
const RADIUS: i32 = 8;

/// `{p}` is the target block (the area's centre at y 64, one above the synthetic grass the
/// legacy structures and features are built on), `{p30}`, `{p100}`, `{plow}` and `{phigh}` the
/// same column at y 30 (inside the synthetic stone), y 100, the world's bottom and its top.
const CASES: &[&str] = &[
    "place structure minecraft:igloo {p}",
    "place structure minecraft:swamp_hut {p}",
    "place structure minecraft:desert_pyramid {p}",
    "place structure minecraft:jungle_pyramid {p}",
    "place structure minecraft:shipwreck {p}",
    "place structure minecraft:shipwreck_beached {p}",
    "place structure minecraft:ocean_ruin_cold {p}",
    "place structure minecraft:ocean_ruin_warm {p}",
    "place structure minecraft:ruined_portal {p}",
    "place structure minecraft:ruined_portal_nether {p}",
    "place structure minecraft:buried_treasure {p}",
    "place structure minecraft:nether_fossil {p}",
    "place structure minecraft:mineshaft {p}",
    "place structure minecraft:mineshaft_mesa {p}",
    "place structure minecraft:monument {p}",
    "place structure minecraft:fortress {p}",
    "place structure minecraft:pillager_outpost {p}",
    "place structure minecraft:village_plains {p}",
    "place structure minecraft:trail_ruins {p}",
    "place structure minecraft:abandoned_camp_forest {p}",
    "place structure minecraft:desert_pyramid {p100}",
    "place structure minecraft:jungle_pyramid {plow}",
    "place structure minecraft:ruined_portal {phigh}",
    "place structure minecraft:pillager_outpost {p100}",
    "place structure minecraft:desert_pyramid",
    "place structure minecraft:no_such_structure {p}",
    "place feature minecraft:oak {p}",
    "place feature minecraft:ore_diamond {p}",
    "place feature minecraft:monster_room {p}",
    "place feature minecraft:flower_default {p}",
    "place feature minecraft:spring_water {p}",
    "place feature minecraft:lake_lava_surface {p}",
    "place feature minecraft:amethyst_geode {p}",
    "place feature minecraft:desert_well {p}",
    "place feature minecraft:patch_pumpkin {p}",
    "place feature minecraft:ore_diamond {p30}",
    "place feature minecraft:monster_room {p30}",
    "place feature minecraft:amethyst_geode {p30}",
    "place feature minecraft:spring_water {p30}",
    "place feature minecraft:oak {p100}",
    "place feature minecraft:oak {plow}",
    "place feature minecraft:oak {phigh}",
    "place feature minecraft:oak",
    "place feature minecraft:no_such_feature {p}",
];

fn fnv(hash: u64, bytes: &[u8]) -> u64 {
    let mut hash = hash;
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

const FNV_START: u64 = 0xcbf2_9ce4_8422_2325;

/// The target chunk of area `index`.
const fn area_chunk(index: usize) -> (i32, i32) {
    (index as i32 * 32, 0)
}

fn load_area(world: &Arc<World>, (cx, cz): (i32, i32)) {
    for x in cx - RADIUS..=cx + RADIUS {
        for z in cz - RADIUS..=cz + RADIUS {
            world.level.loaded_chunks.insert(
                Vector2::new(x, z),
                pumpkin_world::chunk::ChunkData::empty_sync(x, z),
            );
        }
    }
}

fn unload_area(world: &Arc<World>, (cx, cz): (i32, i32)) {
    for x in cx - RADIUS..=cx + RADIUS {
        for z in cz - RADIUS..=cz + RADIUS {
            world.level.loaded_chunks.remove(&Vector2::new(x, z));
            world.block_entities.remove(&Vector2::new(x, z));
        }
    }
}

/// Hash of one chunk's block states, or `None` while it is still empty.
fn chunk_hash(world: &Arc<World>, position: Vector2<i32>, empty: u64) -> Option<u64> {
    let chunk = world.level.loaded_chunks.get(&position)?.value().clone();
    let sections = chunk
        .section
        .block_sections
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut hash = FNV_START;
    for section in sections.iter() {
        for state in section {
            hash = fnv(hash, &state.as_u16().to_le_bytes());
        }
    }
    (hash != empty).then_some(hash)
}

/// Count and hash of the block entities of one chunk (sorted by position).
fn block_entities_text(world: &Arc<World>, position: Vector2<i32>) -> Option<String> {
    let entities = world.block_entities.get(&position)?;
    if entities.is_empty() {
        return None;
    }
    let mut lines: Vec<String> = entities
        .iter()
        .map(|(pos, entity)| {
            format!(
                "{},{},{}={}",
                pos.0.x,
                pos.0.y,
                pos.0.z,
                entity.resource_location()
            )
        })
        .collect();
    lines.sort();
    let hash = fnv(FNV_START, lines.join(";").as_bytes());
    Some(format!("{}:{hash:016x}", lines.len()))
}

fn record(world: &Arc<World>, (cx, cz): (i32, i32), empty: u64) -> String {
    let mut chunks = Vec::new();
    for x in cx - RADIUS..=cx + RADIUS {
        for z in cz - RADIUS..=cz + RADIUS {
            let position = Vector2::new(x, z);
            let blocks = chunk_hash(world, position, empty);
            let entities = block_entities_text(world, position);
            if blocks.is_none() && entities.is_none() {
                continue;
            }
            chunks.push(format!(
                "{},{}:{:016x}{}",
                x - cx,
                z - cz,
                blocks.unwrap_or(empty),
                entities.map(|e| format!(" be={e}")).unwrap_or_default()
            ));
        }
    }
    format!("chunks {} [{}]", chunks.len(), chunks.join(" "))
}

fn run(server: &Arc<Server>, world: &Arc<World>, centre: Vector3<f64>, command: &str) -> String {
    let source = CommandSource::new(
        CommandSender::Console,
        world.clone(),
        None,
        centre,
        Vector2::new(0.0, 0.0),
        "Golden".to_string(),
        TextComponent::text("Golden"),
        server.clone(),
    )
    .with_silent();
    let server = server.clone();
    let command = command.to_owned();
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let dispatcher = server.command_dispatcher.load();
        let result = dispatcher.execute_input(&command, &source);
        let _ = send.send(match result {
            Ok(count) => format!("ok {count}"),
            Err(error) => format!("err {}", error.message.get_text()),
        });
    });
    receive
        .recv_timeout(Duration::from_secs(300))
        .unwrap_or_else(|_| "HANG".to_owned())
}

/// The command with every placeholder replaced by absolute coordinates in the area.
fn expand(case: &str, (cx, cz): (i32, i32)) -> String {
    let (x, z) = (cx * 16 + 8, cz * 16 + 8);
    case.replace("{p100}", &format!("{x} 100 {z}"))
        .replace("{p30}", &format!("{x} 30 {z}"))
        .replace("{plow}", &format!("{x} -64 {z}"))
        .replace("{phigh}", &format!("{x} 319 {z}"))
        .replace("{p}", &format!("{x} 64 {z}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn place_command_places_the_same_blocks() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();

    // The hash of a chunk nothing was written to.
    let empty = {
        let probe = (-100, -100);
        world.level.loaded_chunks.insert(
            Vector2::new(probe.0, probe.1),
            pumpkin_world::chunk::ChunkData::empty_sync(probe.0, probe.1),
        );
        let hash = chunk_hash(&world, Vector2::new(probe.0, probe.1), 0).unwrap_or(0);
        world
            .level
            .loaded_chunks
            .remove(&Vector2::new(probe.0, probe.1));
        hash
    };

    let mut now = String::new();
    for (index, case) in CASES.iter().enumerate() {
        let area = area_chunk(index + 1);
        load_area(&world, area);
        let centre = Vector3::new(
            f64::from(area.0 * 16 + 8) + 0.5,
            64.0,
            f64::from(area.1 * 16 + 8) + 0.5,
        );
        let result = run(&server, &world, centre, &expand(case, area));
        let after = record(&world, area, empty);
        writeln!(now, "{case} -> {result}; {after}")?;
        unload_area(&world, area);
    }

    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/place_command.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &now)?;
    } else {
        assert_eq!(std::fs::read_to_string(&path)?, now);
    }
    server.stop();
    Ok(())
}
