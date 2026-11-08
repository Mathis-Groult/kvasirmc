//! A record of what the chunk readers answer for a corpus of chunks and of damaged chunk bytes,
//! made before `ChunkData::internal_from_pump_bytes` and `internal_from_bytes` were split into
//! small functions (`tests/golden/chunk_read.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! The corpus is built in code: chunks with terrain, biomes, light, heightmaps, ticks, block
//! entities and custom data are written in both formats, read back, and written again; the line
//! of each case holds hashes of the bytes written and of the bytes the reader's result writes.
//! Damaged inputs are every prefix of a small chunk, and its header with each byte replaced.
//! Each outcome is recorded, an error by its message and a panic as such.

use std::fmt::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::Ordering;

use pumpkin_data::{Block, BlockStateId, chunk::ChunkStatus, fluid::Fluid};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_nbt::tag::NbtTag;
use pumpkin_util::math::{position::BlockPos, vector2::Vector2};

use crate::chunk::{ChunkData, ChunkHeightmaps, format::LightContainer};
use crate::tick::{ScheduledTick, TickPriority};

/// FNV-1a: a hash that does not change with the Rust version.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn state(block: &Block) -> BlockStateId {
    block.default_state.id
}

/// Terrain: stone with dirt and grass on top, water in the hollows, heights varying by column.
fn fill_terrain(chunk: &ChunkData, sea_level: i32) {
    let mut sections = chunk
        .section
        .block_sections
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for x in 0..16_i32 {
        for z in 0..16_i32 {
            let top = 50 + (x * 5 + z * 3 + chunk.x * 7 + chunk.z).rem_euclid(14);
            for y in chunk.section.min_y..=top.max(sea_level) {
                let block = if y > top {
                    Block::WATER
                } else if y == top {
                    Block::GRASS_BLOCK
                } else if y > top - 4 {
                    Block::DIRT
                } else {
                    Block::STONE
                };
                let section = ((y - chunk.section.min_y) >> 4) as usize;
                sections[section].set(x as usize, (y & 15) as usize, z as usize, state(&block));
            }
        }
    }
    // A few blocks that tick at random, light sources and a log with leaves.
    let extras = [
        (3, 70, 4, Block::OAK_LOG),
        (3, 71, 4, Block::OAK_LEAVES),
        (4, 71, 4, Block::OAK_LEAVES),
        (9, 66, 9, Block::TORCH),
        (10, 66, 9, Block::WHEAT),
    ];
    for (x, y, z, block) in extras {
        let section = ((y - chunk.section.min_y) >> 4) as usize;
        sections[section].set(x, (y & 15) as usize, z, state(&block));
    }
}

fn fill_biomes(chunk: &ChunkData) {
    let mut sections = chunk
        .section
        .biome_sections
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (index, section) in sections.iter_mut().enumerate() {
        for x in 0..4 {
            for z in 0..4 {
                for y in 0..4 {
                    section.set(x, y, z, ((index + x + z * 2 + y) % 5) as u8);
                }
            }
        }
    }
}

fn fill_light(chunk: &ChunkData) {
    let mut light = chunk
        .light_engine
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let count = chunk.section.count;
    light.sky_light = (0..count)
        .map(|index| {
            if index % 3 == 0 {
                LightContainer::Empty(15)
            } else {
                LightContainer::Full(
                    (0..LightContainer::ARRAY_SIZE)
                        .map(|i| ((i + index) % 251) as u8)
                        .collect(),
                )
            }
        })
        .collect();
    light.block_light = (0..count)
        .map(|index| {
            if index % 4 == 1 {
                LightContainer::Full(
                    (0..LightContainer::ARRAY_SIZE)
                        .map(|i| ((i * 3 + index) % 241) as u8)
                        .collect(),
                )
            } else {
                LightContainer::Empty(0)
            }
        })
        .collect();
    chunk.light_populated.store(true, Ordering::Relaxed);
}

fn fill_heightmaps(chunk: &ChunkData) {
    let mut maps = chunk
        .heightmap
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let longs = |seed: i64| -> Box<[i64]> {
        (0..ChunkHeightmaps::LONGS as i64)
            .map(|i| seed.wrapping_mul(i + 1).wrapping_add(i))
            .collect()
    };
    maps.world_surface = Some(longs(0x1234_5678));
    maps.motion_blocking = Some(longs(0x0f0f_0f0f));
    maps.motion_blocking_no_leaves = Some(longs(-77));
}

fn fill_ticks_entities_and_custom_data(chunk: &ChunkData) {
    let base = BlockPos::new(chunk.x * 16, 64, chunk.z * 16);
    for (order, (dx, delay, priority)) in [
        (1, 3, TickPriority::Normal),
        (2, 0, TickPriority::VeryHigh),
        (5, 40, TickPriority::Low),
    ]
    .into_iter()
    .enumerate()
    {
        chunk.block_ticks.schedule_tick(
            &ScheduledTick {
                delay,
                priority,
                position: BlockPos::new(base.0.x + dx, base.0.y, base.0.z),
                value: &Block::STONE,
            },
            order as u64,
        );
    }
    chunk.fluid_ticks.schedule_tick(
        &ScheduledTick {
            delay: 5,
            priority: TickPriority::Normal,
            position: BlockPos::new(base.0.x + 7, 62, base.0.z + 1),
            value: &Fluid::WATER,
        },
        9,
    );
    let mut chest = NbtCompound::new();
    chest.put_string("id", "minecraft:chest".to_owned());
    chest.put_int("x", base.0.x + 2);
    chest.put_int("y", 65);
    chest.put_int("z", base.0.z + 2);
    chest.put_list(
        "Items",
        vec![NbtTag::Compound({
            let mut item = NbtCompound::new();
            item.put_string("id", "minecraft:apple".to_owned());
            item.put_byte("Count", 3);
            item.put_byte("Slot", 0);
            item
        })],
    );
    chunk
        .pending_block_entities
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(BlockPos::new(base.0.x + 2, 65, base.0.z + 2), chest);
    let mut custom = chunk
        .custom_data
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    custom.put_string("kvasir:note", "golden".to_owned());
    custom.put_int("kvasir:count", 7);
    chunk.inhabited_time.store(123_456, Ordering::Relaxed);
}

/// Every stage of building a chunk is a case, so a failure names what the reader got wrong.
fn corpus() -> Vec<(String, ChunkData)> {
    let mut cases = Vec::new();
    cases.push(("empty".to_owned(), ChunkData::empty(0, 0)));

    let terrain = ChunkData::empty(3, -5);
    fill_terrain(&terrain, 62);
    cases.push(("terrain".to_owned(), terrain));

    let biomes = ChunkData::empty(-1, 2);
    fill_terrain(&biomes, 62);
    fill_biomes(&biomes);
    cases.push(("terrain_biomes".to_owned(), biomes));

    let lit = ChunkData::empty(4, 4);
    fill_terrain(&lit, 62);
    fill_biomes(&lit);
    fill_light(&lit);
    cases.push(("terrain_biomes_light".to_owned(), lit));

    let maps = ChunkData::empty(-7, -9);
    fill_terrain(&maps, 62);
    fill_biomes(&maps);
    fill_heightmaps(&maps);
    cases.push(("terrain_biomes_heightmaps".to_owned(), maps));

    let full = ChunkData::empty(30, 31);
    fill_terrain(&full, 62);
    fill_biomes(&full);
    fill_light(&full);
    fill_heightmaps(&full);
    fill_ticks_entities_and_custom_data(&full);
    cases.push(("everything".to_owned(), full));

    for (name, status) in [
        ("status_empty", ChunkStatus::Empty),
        ("status_structure_starts", ChunkStatus::StructureStarts),
        (
            "status_structure_references",
            ChunkStatus::StructureReferences,
        ),
        ("status_biomes", ChunkStatus::Biomes),
        ("status_terrain", ChunkStatus::Terrain),
        ("status_features", ChunkStatus::Features),
        ("status_initialize_light", ChunkStatus::InitializeLight),
        ("status_light", ChunkStatus::Light),
        ("status_spawn", ChunkStatus::Spawn),
    ] {
        let mut chunk = ChunkData::empty(1, 1);
        chunk.status = status;
        cases.push((name.to_owned(), chunk));
    }

    let small = ChunkData::empty(2, 3);
    fill_ticks_entities_and_custom_data(&small);
    cases.push(("small_ticks_entities_custom".to_owned(), small));
    cases
}

/// What a reader does with `bytes`: the hash of the bytes the chunk it returns writes, or its error.
fn outcome(
    read: impl Fn(&[u8], Vector2<i32>) -> Result<ChunkData, String>,
    bytes: &[u8],
    at: Vector2<i32>,
) -> String {
    match catch_unwind(AssertUnwindSafe(|| read(bytes, at))) {
        Ok(Ok(chunk)) => format!(
            "ok x={} z={} status={:?} sections={} mask={} again={:016x}",
            chunk.x,
            chunk.z,
            chunk.status,
            chunk.section.count,
            chunk.section.randomly_ticking_mask.load(Ordering::Relaxed),
            fnv(&chunk.internal_to_pump_bytes()),
        ),
        Ok(Err(message)) => format!("err {message}"),
        Err(_) => "panic".to_owned(),
    }
}

fn read_pump(bytes: &[u8], at: Vector2<i32>) -> Result<ChunkData, String> {
    ChunkData::internal_from_pump_bytes(bytes, at).map_err(|error| error.to_string())
}

fn read_anvil(bytes: &[u8], at: Vector2<i32>) -> Result<ChunkData, String> {
    ChunkData::internal_from_bytes(bytes, at).map_err(|error| error.to_string())
}

fn corpus_lines(out: &mut String) -> std::fmt::Result {
    for (name, chunk) in corpus() {
        let at = Vector2::new(chunk.x, chunk.z);
        let pump = chunk.internal_to_pump_bytes();
        let anvil = chunk.internal_to_bytes();
        writeln!(
            out,
            "corpus {name} pump len={} hash={:016x} read: {}",
            pump.len(),
            fnv(&pump),
            outcome(read_pump, &pump, at)
        )?;
        writeln!(
            out,
            "corpus {name} anvil len={} hash={:016x} read: {}",
            anvil.len(),
            fnv(&anvil),
            outcome(read_anvil, &anvil, at)
        )?;
        writeln!(
            out,
            "corpus {name} pump read as anvil: {}",
            outcome(read_anvil, &pump, at)
        )?;
        writeln!(
            out,
            "corpus {name} wrong position: {} / {}",
            outcome(read_pump, &pump, Vector2::new(at.x + 1, at.y)),
            outcome(read_anvil, &anvil, Vector2::new(at.x, at.y - 1)),
        )?;
    }
    Ok(())
}

/// Every prefix of a small chunk's bytes, and the header with each of its bytes replaced.
fn damaged_pump_lines(out: &mut String) -> std::fmt::Result {
    let chunk = {
        let chunk = ChunkData::empty(2, 3);
        fill_ticks_entities_and_custom_data(&chunk);
        chunk
    };
    let at = Vector2::new(2, 3);
    let bytes = chunk.internal_to_pump_bytes();
    for len in 0..bytes.len() {
        writeln!(
            out,
            "pump prefix {len}: {}",
            outcome(read_pump, &bytes[..len], at)
        )?;
    }
    for index in 0..48.min(bytes.len()) {
        for value in [0x00_u8, 0x01, 0x02, 0x7f, 0x80, 0xff] {
            let mut damaged = bytes.to_vec();
            damaged[index] = value;
            writeln!(
                out,
                "pump byte {index}={value:#04x}: {}",
                outcome(read_pump, &damaged, at)
            )?;
        }
    }
    // The count fields that size what follows.
    let mut tail = bytes.to_vec();
    let sections_at = 4 + 1 + 4 + 4 + 4 + 4 + 1 + 1 + 8;
    for value in [0_u16, 1, 24, 255, 256, 257, 65_535] {
        tail[sections_at..sections_at + 2].copy_from_slice(&value.to_be_bytes());
        writeln!(
            out,
            "pump section count {value}: {}",
            outcome(read_pump, &tail, at)
        )?;
    }
    Ok(())
}

/// The anvil bytes of a chunk with each top-level key removed or replaced by a byte, and with
/// each key of its first section removed or replaced.
fn damaged_anvil_lines(out: &mut String) -> std::fmt::Result {
    let chunk = ChunkData::empty(30, 31);
    fill_terrain(&chunk, 62);
    fill_biomes(&chunk);
    fill_light(&chunk);
    fill_heightmaps(&chunk);
    fill_ticks_entities_and_custom_data(&chunk);
    let at = Vector2::new(30, 31);
    let bytes = chunk.internal_to_bytes();
    let mut cursor = std::io::Cursor::new(&bytes[..]);
    let mut reader = pumpkin_nbt::deserializer::NbtReadHelperJava::new(&mut cursor);
    let nbt = pumpkin_nbt::Nbt::read(&mut reader).map_err(|_| std::fmt::Error)?;
    let root = nbt.root_tag;

    let mut keys: Vec<String> = root.child_tags.keys().map(ToString::to_string).collect();
    keys.sort();
    for key in &keys {
        for how in ["remove", "byte"] {
            let mut edited = root.clone();
            if how == "remove" {
                edited.child_tags.remove(key.as_str());
            } else {
                edited.put_byte(key, 1);
            }
            let damaged = pumpkin_nbt::Nbt::new(String::new(), edited).write();
            writeln!(
                out,
                "anvil {how} {key}: {}",
                outcome(read_anvil, &damaged, at)
            )?;
        }
    }
    let Some(NbtTag::List(sections)) = root.get("sections") else {
        return Err(std::fmt::Error);
    };
    let Some(NbtTag::Compound(first)) = sections.get(2) else {
        return Err(std::fmt::Error);
    };
    let mut section_keys: Vec<String> = first.child_tags.keys().map(ToString::to_string).collect();
    section_keys.sort();
    for key in &section_keys {
        for how in ["remove", "byte"] {
            let mut edited_section = first.clone();
            if how == "remove" {
                edited_section.child_tags.remove(key.as_str());
            } else {
                edited_section.put_byte(key, 1);
            }
            let mut edited = root.clone();
            let mut list = sections.to_vec();
            list[2] = NbtTag::Compound(edited_section);
            edited.put_list("sections", list);
            let damaged = pumpkin_nbt::Nbt::new(String::new(), edited).write();
            writeln!(
                out,
                "anvil section {how} {key}: {}",
                outcome(read_anvil, &damaged, at)
            )?;
        }
    }
    for len in (0..bytes.len()).step_by(61) {
        writeln!(
            out,
            "anvil prefix {len}: {}",
            outcome(read_anvil, &bytes[..len], at)
        )?;
    }
    Ok(())
}

#[test]
fn chunk_readers_answer_as_before() -> Result<(), Box<dyn std::error::Error>> {
    let mut now = String::new();
    corpus_lines(&mut now)?;
    damaged_pump_lines(&mut now)?;
    damaged_anvil_lines(&mut now)?;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("chunk_read.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, now);
    Ok(())
}
