//! A record of the bytes `write_chunk_data` produces for a corpus of chunks and client versions,
//! made before the function was split into small ones (`tests/golden/chunk_data_packet.txt`;
//! rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! One line per chunk and version: the length of the packet and an FNV-1a hash of its bytes.

use std::fmt::Write;
use std::sync::atomic::Ordering;

use pumpkin_data::{Block, BlockStateId};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_nbt::tag::NbtTag;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::version::JavaMinecraftVersion;
use pumpkin_world::chunk::format::LightContainer;
use pumpkin_world::chunk::{ChunkData, ChunkHeightmaps};

use super::v1_18::write_chunk_data;

const VERSIONS: [JavaMinecraftVersion; 10] = [
    JavaMinecraftVersion::V_1_18,
    JavaMinecraftVersion::V_1_19_4,
    JavaMinecraftVersion::V_1_20,
    JavaMinecraftVersion::V_1_20_3,
    JavaMinecraftVersion::V_1_20_5,
    JavaMinecraftVersion::V_1_21_4,
    JavaMinecraftVersion::V_1_21_5,
    JavaMinecraftVersion::V_1_21_6,
    JavaMinecraftVersion::V_26_1,
    JavaMinecraftVersion::V_26_3,
];

/// FNV-1a: a hash that does not change with the Rust version.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Stone with dirt and grass on top, water in the hollows; heights vary by column. `varied` adds
/// blocks that make the palette of some sections large.
fn fill_terrain(chunk: &ChunkData, varied: bool) {
    let mut sections = chunk
        .section
        .block_sections
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for x in 0..16i32 {
        for z in 0..16i32 {
            let top = 50 + (x * 5 + z * 3 + chunk.x * 7 + chunk.z).rem_euclid(14);
            for y in chunk.section.min_y..=top.max(62) {
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
                sections[section].set(
                    x as usize,
                    (y & 15) as usize,
                    z as usize,
                    block.default_state.id,
                );
            }
        }
    }
    if varied {
        // Many different blocks in the one section: a palette that needs a direct encoding.
        let section = ((100 - chunk.section.min_y) >> 4) as usize;
        for (index, raw) in (0..300u16).enumerate() {
            if let Some(id) = BlockStateId::new(raw.wrapping_mul(37) % BlockStateId::COUNT) {
                sections[section].set(index % 16, (index / 16) % 16, (index / 7) % 16, id);
            }
        }
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
    let full = |seed: usize| {
        LightContainer::Full(
            (0..LightContainer::ARRAY_SIZE)
                .map(|i| ((i + seed) % 251) as u8)
                .collect(),
        )
    };
    light.sky_light = (0..count)
        .map(|i| {
            if i % 3 == 0 {
                LightContainer::Empty(15)
            } else {
                full(i)
            }
        })
        .collect();
    light.block_light = (0..count)
        .map(|i| {
            if i % 4 == 1 {
                full(i * 3)
            } else {
                LightContainer::Empty(0)
            }
        })
        .collect();
}

fn fill_heightmaps(chunk: &ChunkData, only_surface: bool) {
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
    if !only_surface {
        maps.motion_blocking = Some(longs(0x0f0f_0f0f));
        maps.motion_blocking_no_leaves = Some(longs(-77));
    }
}

fn block_entity(id: &str, x: i32, y: i32, z: i32) -> NbtCompound {
    let mut nbt = NbtCompound::new();
    nbt.put_string("id", id.to_owned());
    nbt.put_int("x", x);
    nbt.put_int("y", y);
    nbt.put_int("z", z);
    nbt.put_string("LootTable", "minecraft:chests/simple_dungeon".to_owned());
    nbt.put_long("LootTableSeed", 5);
    nbt.put_list(
        "Items",
        vec![NbtTag::Compound({
            let mut item = NbtCompound::new();
            item.put_string("id", "minecraft:apple".to_owned());
            item.put_byte("Count", 3);
            item
        })],
    );
    nbt.put_string("CustomName", "{\"text\":\"golden\"}".to_owned());
    nbt
}

fn fill_block_entities(chunk: &ChunkData) {
    let base_x = chunk.x * 16;
    let base_z = chunk.z * 16;
    let mut entities = chunk
        .pending_block_entities
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (id, dx, y, dz) in [
        ("minecraft:chest", 2, 65, 3),
        ("minecraft:furnace", 15, -20, 0),
        ("sign", 0, 70, 15),
        ("minecraft:not_a_block_entity", 7, 64, 7),
    ] {
        entities.insert(
            BlockPos::new(base_x + dx, y, base_z + dz),
            block_entity(id, base_x + dx, y, base_z + dz),
        );
    }
    // One without an id.
    let mut anonymous = NbtCompound::new();
    anonymous.put_int("x", base_x + 9);
    anonymous.put_int("y", 66);
    anonymous.put_int("z", base_z + 9);
    entities.insert(BlockPos::new(base_x + 9, 66, base_z + 9), anonymous);
}

fn corpus() -> Vec<(&'static str, ChunkData)> {
    let mut cases = vec![("empty", ChunkData::empty(0, 0))];

    let terrain = ChunkData::empty(3, -5);
    fill_terrain(&terrain, false);
    fill_biomes(&terrain);
    cases.push(("terrain_biomes", terrain));

    let varied = ChunkData::empty(-1, 2);
    fill_terrain(&varied, true);
    fill_biomes(&varied);
    cases.push(("varied_palettes", varied));

    let lit = ChunkData::empty(4, 4);
    fill_terrain(&lit, false);
    fill_light(&lit);
    cases.push(("light", lit));

    let maps = ChunkData::empty(-7, -9);
    fill_terrain(&maps, false);
    fill_heightmaps(&maps, false);
    cases.push(("heightmaps", maps));

    let surface = ChunkData::empty(8, 8);
    fill_heightmaps(&surface, true);
    cases.push(("surface_heightmap_only", surface));

    let entities = ChunkData::empty(-2, -3);
    fill_terrain(&entities, false);
    fill_block_entities(&entities);
    cases.push(("block_entities", entities));

    let full = ChunkData::empty(30, 31);
    fill_terrain(&full, true);
    fill_biomes(&full);
    fill_light(&full);
    fill_heightmaps(&full, false);
    fill_block_entities(&full);
    full.light_populated.store(true, Ordering::Relaxed);
    cases.push(("everything", full));
    cases
}

#[test]
fn chunk_data_packet_is_written_as_before() -> Result<(), Box<dyn std::error::Error>> {
    let mut now = String::new();
    for (name, chunk) in corpus() {
        for version in VERSIONS {
            let mut bytes = Vec::new();
            match write_chunk_data(&chunk, &mut bytes, &version) {
                Ok(()) => writeln!(
                    now,
                    "{name} {version:?} len={} hash={:016x}",
                    bytes.len(),
                    fnv(&bytes)
                )?,
                Err(error) => writeln!(now, "{name} {version:?} err {error}")?,
            }
        }
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("chunk_data_packet.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, now);
    Ok(())
}
