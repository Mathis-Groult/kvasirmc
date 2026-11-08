//! Reading a chunk from the Java (anvil) NBT format.
//!
//! The root compound is read once, then each part of the chunk is built from it by a function of
//! its own: the sections (blocks, biomes and light), the heightmaps, the two tick lists, the block
//! entities, the status and the custom data. Anything missing or malformed is left out rather than
//! failing the chunk, except the position and the lowest section, which are required.

use std::sync::{
    RwLock,
    atomic::{AtomicBool, AtomicU64},
};

use pumpkin_data::{Block, BlockStateId, chunk::ChunkStatus, fluid::Fluid};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_nbt::tag::NbtTag;
use pumpkin_util::math::{position::BlockPos, vector2::Vector2};
use pumpkin_util::resource_location::FromResourceLocation;
use rustc_hash::FxHashMap;

use super::{
    ChunkSectionBiomes, ChunkSectionBlockStates, LightContainer, extract_u8_array,
    extract_u16_array, lowest_biome_section_y, parse_scheduled_tick, section_y,
};
use crate::chunk::{
    ChunkData, ChunkHeightmaps, ChunkLight, ChunkParsingError, ChunkSections,
    palette::{BiomePalette, BlockPalette},
};
use crate::generation::section_coords;
use crate::tick::{ScheduledTick, scheduler::ChunkTickScheduler};

/// The per-section data, one entry per section from the lowest one up.
struct Sections {
    block_light: Vec<LightContainer>,
    sky_light: Vec<LightContainer>,
    blocks: Vec<BlockPalette>,
    biomes: Vec<BiomePalette>,
}

fn error(message: impl Into<String>) -> ChunkParsingError {
    ChunkParsingError::ErrorDeserializingChunk(message.into())
}

/// The root compound of the chunk. Bytes that start like a named compound are read as one, any
/// other as an unnamed compound.
fn read_root(chunk_data: &[u8]) -> Result<NbtCompound, ChunkParsingError> {
    let is_named = chunk_data.len() >= 3
        && chunk_data[0] == 0x0a
        && chunk_data[1] == 0x00
        && chunk_data[2] == 0x00;

    let mut cursor = std::io::Cursor::new(chunk_data);
    let mut reader = pumpkin_nbt::deserializer::NbtReadHelperJava::new(&mut cursor);
    let nbt = if is_named {
        pumpkin_nbt::Nbt::read(&mut reader)
    } else {
        pumpkin_nbt::Nbt::read_unnamed(&mut reader)
    }
    .map_err(|e| error(e.to_string()))?;
    Ok(nbt.root_tag)
}

/// Checks that the chunk is the one asked for.
fn check_position(root_tag: &NbtCompound, position: Vector2<i32>) -> Result<(), ChunkParsingError> {
    let x_pos = root_tag
        .get_int("xPos")
        .ok_or_else(|| error("Missing xPos"))?;
    let z_pos = root_tag
        .get_int("zPos")
        .ok_or_else(|| error("Missing zPos"))?;

    if x_pos != position.x || z_pos != position.y {
        return Err(error(format!(
            "Expected data for chunk {},{} but got it for {},{}!",
            position.x, position.y, x_pos, z_pos,
        )));
    }
    Ok(())
}

/// The index of the lowest section.
fn lowest_section(root_tag: &NbtCompound) -> Result<i8, ChunkParsingError> {
    // Vanilla omits yPos when it upgrades a world in place. It uses the
    // dimension minimum for such chunks, which is the lowest section that
    // stores biomes. Do the same instead of rejecting the chunk.
    let min_y_section = match root_tag.get_int("yPos") {
        Some(y_pos) => y_pos,
        None => lowest_biome_section_y(root_tag).ok_or_else(|| error("Missing yPos"))?,
    };

    // Section indices are bytes in this format, and the loops below already
    // treat them as such. Reject a yPos outside that range instead of
    // truncating it, because the untruncated value sizes the vectors below.
    i8::try_from(min_y_section).map_err(|_| {
        error(format!(
            "yPos {min_y_section} is outside the range of a section index"
        ))
    })
}

/// How many sections there are from `min_y_section` up to the highest one listed.
fn section_count(root_tag: &NbtCompound, min_y_section: i8) -> usize {
    let mut max_y_section = min_y_section;
    if let Some(sections_list) = root_tag.get_list("sections") {
        for section_tag in sections_list {
            if let NbtTag::Compound(section_compound) = section_tag {
                let y = section_y(section_compound) as i8;
                if y > max_y_section {
                    max_y_section = y;
                }
            }
        }
    }

    (i32::from(max_y_section) - i32::from(min_y_section) + 1).max(0) as usize
}

/// One light array of a section; a missing array, or one of the wrong length, is empty.
fn light_from(section_compound: &NbtCompound, key: &str) -> LightContainer {
    section_compound
        .get(key)
        .and_then(|tag| tag.extract_byte_array())
        .map(|arr| -> Box<[u8]> {
            // SAFETY: `arr` is an `i8` slice (`&[i8]`). `u8` and `i8` have identical memory layout, alignment (1 byte), and lifetime.
            unsafe {
                Box::from(std::slice::from_raw_parts(
                    arr.as_ptr().cast::<u8>(),
                    arr.len(),
                ))
            }
        })
        // `Full` skips the length check `LightContainer::new` makes,
        // and every reader indexes it up to `ARRAY_SIZE`, so a
        // short array from disk is dropped instead of stored.
        .filter(|data| data.len() == LightContainer::ARRAY_SIZE)
        .map_or(LightContainer::Empty(0), LightContainer::Full)
}

fn block_palette_from(section_compound: &NbtCompound) -> BlockPalette {
    let Some(bs_compound) = section_compound.get_compound("block_states") else {
        return BlockPalette::default();
    };
    let data = bs_compound
        .get_long_array("data")
        .map(|arr| arr.to_vec().into_boxed_slice());
    let palette = bs_compound
        .get("palette")
        .and_then(extract_u16_array)
        .unwrap_or_else(|| vec![BlockStateId::AIR].into_boxed_slice());

    BlockPalette::from_disk_nbt(ChunkSectionBlockStates { data, palette })
}

fn biome_palette_from(section_compound: &NbtCompound) -> BiomePalette {
    let Some(b_compound) = section_compound.get_compound("biomes") else {
        return BiomePalette::default();
    };
    let data = b_compound
        .get_long_array("data")
        .map(|arr| arr.to_vec().into_boxed_slice());
    let palette = b_compound
        .get("palette")
        .and_then(extract_u8_array)
        .unwrap_or_else(|| vec![0].into_boxed_slice());

    BiomePalette::from_disk_nbt(ChunkSectionBiomes { data, palette })
}

/// The sections of the chunk; a section outside the range from `min_y_section` up is skipped.
fn read_sections(root_tag: &NbtCompound, min_y_section: i8, section_count: usize) -> Sections {
    let mut sections = Sections {
        block_light: vec![LightContainer::Empty(0); section_count],
        sky_light: vec![LightContainer::Empty(0); section_count],
        blocks: vec![BlockPalette::default(); section_count],
        biomes: vec![BiomePalette::default(); section_count],
    };

    if let Some(sections_list) = root_tag.get_list("sections") {
        for section_tag in sections_list {
            if let NbtTag::Compound(section_compound) = section_tag {
                let y = section_y(section_compound);
                let Ok(index) = usize::try_from(y.saturating_sub(i32::from(min_y_section))) else {
                    continue;
                };
                if index >= section_count {
                    continue;
                }

                sections.block_light[index] = light_from(section_compound, "BlockLight");
                sections.sky_light[index] = light_from(section_compound, "SkyLight");
                sections.blocks[index] = block_palette_from(section_compound);
                sections.biomes[index] = biome_palette_from(section_compound);
            }
        }
    }
    sections
}

fn heightmaps_from(root_tag: &NbtCompound) -> ChunkHeightmaps {
    // A heightmap of the wrong length is dropped rather than stored:
    // every reader indexes it by column and would run off the end.
    let heightmap = |h_compound: &NbtCompound, key: &str| {
        h_compound
            .get_long_array(key)
            .filter(|a| a.len() == ChunkHeightmaps::LONGS)
            .map(|a| a.to_vec().into_boxed_slice())
    };
    root_tag.get_compound("Heightmaps").map_or(
        ChunkHeightmaps {
            world_surface: None,
            motion_blocking: None,
            motion_blocking_no_leaves: None,
        },
        |h_compound| ChunkHeightmaps {
            world_surface: heightmap(h_compound, "WORLD_SURFACE"),
            motion_blocking: heightmap(h_compound, "MOTION_BLOCKING"),
            motion_blocking_no_leaves: heightmap(h_compound, "MOTION_BLOCKING_NO_LEAVES"),
        },
    )
}

/// The ticks listed under `key`; one that does not parse is dropped.
fn ticks_from<T: FromResourceLocation>(root_tag: &NbtCompound, key: &str) -> Vec<ScheduledTick<T>> {
    let mut ticks = Vec::new();
    if let Some(list) = root_tag.get_list(key) {
        for tag in list {
            if let NbtTag::Compound(compound) = tag
                && let Some(tick) = parse_scheduled_tick::<T>(compound)
            {
                ticks.push(tick);
            }
        }
    }
    ticks
}

/// The block entities, keyed by their `x`, `y` and `z`; one without all three is dropped.
fn block_entities_from(root_tag: &NbtCompound) -> FxHashMap<BlockPos, NbtCompound> {
    let mut block_entities = FxHashMap::default();
    if let Some(list) = root_tag.get_list("block_entities") {
        for tag in list {
            if let NbtTag::Compound(nbt) = tag
                && let Some(x) = nbt.get_int("x")
                && let Some(y) = nbt.get_int("y")
                && let Some(z) = nbt.get_int("z")
            {
                block_entities.insert(BlockPos::new(x, y, z), nbt.clone());
            }
        }
    }
    block_entities
}

fn status_from_name(name: &str) -> ChunkStatus {
    match name {
        "minecraft:structure_starts" => ChunkStatus::StructureStarts,
        "minecraft:structure_references" => ChunkStatus::StructureReferences,
        "minecraft:biomes" => ChunkStatus::Biomes,
        "minecraft:terrain" | "minecraft:noise" | "minecraft:surface" | "minecraft:carvers" => {
            ChunkStatus::Terrain
        }
        "minecraft:features" => ChunkStatus::Features,
        "minecraft:initialize_light" => ChunkStatus::InitializeLight,
        "minecraft:light" => ChunkStatus::Light,
        "minecraft:spawn" => ChunkStatus::Spawn,
        "minecraft:full" => ChunkStatus::Full,
        _ => ChunkStatus::Empty,
    }
}

impl ChunkData {
    pub fn internal_from_bytes(
        chunk_data: &[u8],
        position: Vector2<i32>,
    ) -> Result<Self, ChunkParsingError> {
        if chunk_data.starts_with(&super::pump::PUMP_CHUNK_MAGIC) {
            return Self::internal_from_pump_bytes(chunk_data, position);
        }

        let root_tag = read_root(chunk_data)?;
        check_position(&root_tag, position)?;
        let min_y_section = lowest_section(&root_tag)?;
        let count = section_count(&root_tag, min_y_section);
        let sections = read_sections(&root_tag, min_y_section, count);

        let light_engine = ChunkLight {
            block_light: sections.block_light.into_boxed_slice(),
            sky_light: sections.sky_light.into_boxed_slice(),
        };
        let min_y = section_coords::section_to_block(i32::from(min_y_section));
        let (random_tick_sections, randomly_ticking_mask) =
            ChunkSections::build_random_tick_sections_cache(&sections.blocks);
        let section = ChunkSections {
            count: sections.blocks.len(),
            block_sections: RwLock::new(sections.blocks.into_boxed_slice()),
            random_tick_sections: RwLock::new(random_tick_sections),
            randomly_ticking_mask: std::sync::atomic::AtomicU32::new(randomly_ticking_mask),
            biome_sections: RwLock::new(sections.biomes.into_boxed_slice()),
            min_y,
        };

        let custom_data = root_tag
            .get_compound("PumpkinCustomData")
            .or_else(|| root_tag.get_compound("BukkitValues"))
            .cloned()
            .unwrap_or_default();

        Ok(Self {
            section,
            heightmap: std::sync::Mutex::new(heightmaps_from(&root_tag)),
            x: position.x,
            z: position.y,
            // This chunk is read from disk, so it has not been modified
            dirty: AtomicBool::new(false),
            block_ticks: ChunkTickScheduler::from_iter(ticks_from::<&'static Block>(
                &root_tag,
                "block_ticks",
            )),
            fluid_ticks: ChunkTickScheduler::from_iter(ticks_from::<&'static Fluid>(
                &root_tag,
                "fluid_ticks",
            )),
            pending_block_entities: std::sync::Mutex::new(block_entities_from(&root_tag)),
            light_engine: std::sync::Mutex::new(light_engine),
            light_populated: AtomicBool::new(root_tag.get_bool("isLightOn").unwrap_or(false)),
            status: status_from_name(root_tag.get_string("Status").unwrap_or("minecraft:empty")),
            blending_data: None,
            inhabited_time: AtomicU64::new(root_tag.get_long("InhabitedTime").unwrap_or(0) as u64),
            custom_data: std::sync::Mutex::new(custom_data),
        })
    }
}
