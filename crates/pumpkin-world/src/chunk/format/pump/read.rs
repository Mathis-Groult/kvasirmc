//! Reading a chunk from the pump binary format (see `internal_to_pump_bytes` for the layout).
//!
//! The reader walks the bytes front to back: a header, the heightmaps the flags announce, the
//! sections, the two tick lists, the block entities and the custom data. Each part is a function
//! that takes the unread bytes and moves them forward; every length is checked before it is used.

use std::sync::{
    RwLock,
    atomic::{AtomicBool, AtomicU64},
};

use bytes::Buf;
use pumpkin_data::{Block, BlockId, chunk::ChunkStatus, fluid::Fluid};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_util::math::{position::BlockPos, vector2::Vector2};
use rustc_hash::FxHashMap;

use super::PUMP_CHUNK_MAGIC;
use crate::chunk::format::LightContainer;
use crate::chunk::{
    ChunkData, ChunkHeightmaps, ChunkLight, ChunkParsingError, ChunkSections,
    palette::{BiomePalette, BlockPalette},
};
use crate::tick::{ScheduledTick, TickPriority, scheduler::ChunkTickScheduler};

const HEADER_MIN_LEN: usize = 4 + 1 + 4 + 4 + 4 + 4 + 1 + 1 + 8;
const MAX_SECTIONS: usize = 256;
const MAX_TICKS: usize = 65_536;
const MAX_BLOCK_ENTITIES: usize = 65_536;
/// Bytes a tick takes: three coordinates, the delay, the priority and the block or fluid id.
const TICK_LEN: usize = 16;

/// What follows the header's checks: the unread bytes are the caller's.
struct Header {
    min_y: i32,
    status: ChunkStatus,
    flags: u8,
    inhabited_time: u64,
}

impl Header {
    const fn light_correct(&self) -> bool {
        (self.flags & 1) != 0
    }

    const fn has_custom_data(&self) -> bool {
        (self.flags & (1 << 4)) != 0
    }
}

/// The per-section data: blocks, biomes and both kinds of light, one entry per section.
struct Sections {
    blocks: Vec<BlockPalette>,
    biomes: Vec<BiomePalette>,
    block_light: Vec<LightContainer>,
    sky_light: Vec<LightContainer>,
}

fn error(message: impl Into<String>) -> ChunkParsingError {
    ChunkParsingError::ErrorDeserializingChunk(message.into())
}

const fn status_from_byte(byte: u8) -> ChunkStatus {
    match byte {
        1 => ChunkStatus::StructureStarts,
        2 => ChunkStatus::StructureReferences,
        3 => ChunkStatus::Biomes,
        4 => ChunkStatus::Terrain,
        5 => ChunkStatus::Features,
        6 => ChunkStatus::InitializeLight,
        7 => ChunkStatus::Light,
        8 => ChunkStatus::Spawn,
        9 => ChunkStatus::Full,
        _ => ChunkStatus::Empty,
    }
}

/// Checks the magic, the version and the position, and reads the fixed fields; returns the bytes
/// after them.
fn read_header(
    chunk_data: &[u8],
    position: Vector2<i32>,
) -> Result<(Header, &[u8]), ChunkParsingError> {
    if chunk_data.len() < HEADER_MIN_LEN || chunk_data[..4] != PUMP_CHUNK_MAGIC {
        return Err(error("Invalid pump chunk header"));
    }

    let mut buf = &chunk_data[4..];
    let version = buf.get_u8();
    if version != 1 {
        return Err(error(format!("Unsupported pump chunk version {version}")));
    }

    let _data_version = buf.get_i32();
    let x_pos = buf.get_i32();
    let z_pos = buf.get_i32();
    if x_pos != position.x || z_pos != position.y {
        return Err(error(format!(
            "Expected data for chunk {},{} but got it for {},{}!",
            position.x, position.y, x_pos, z_pos,
        )));
    }

    let min_y = buf.get_i32();
    let status = status_from_byte(buf.get_u8());
    let flags = buf.get_u8();
    let inhabited_time = buf.get_u64();
    Ok((
        Header {
            min_y,
            status,
            flags,
            inhabited_time,
        },
        buf,
    ))
}

fn read_heightmap(buf: &mut &[u8], present: bool) -> Result<Option<Box<[i64]>>, ChunkParsingError> {
    if !present {
        return Ok(None);
    }
    if buf.remaining() < ChunkHeightmaps::LONGS * 8 {
        return Err(error("Unexpected EOF reading heightmap"));
    }
    let mut hm = Vec::with_capacity(ChunkHeightmaps::LONGS);
    for _ in 0..ChunkHeightmaps::LONGS {
        hm.push(buf.get_i64());
    }
    Ok(Some(hm.into_boxed_slice()))
}

/// The three heightmaps the flags announce, in the order world surface, motion blocking, motion
/// blocking without leaves.
fn read_heightmaps(buf: &mut &[u8], flags: u8) -> Result<ChunkHeightmaps, ChunkParsingError> {
    Ok(ChunkHeightmaps {
        world_surface: read_heightmap(buf, (flags & (1 << 1)) != 0)?,
        motion_blocking: read_heightmap(buf, (flags & (1 << 2)) != 0)?,
        motion_blocking_no_leaves: read_heightmap(buf, (flags & (1 << 3)) != 0)?,
    })
}

/// One light array when `present`, else an empty container; `what` names it in the error.
fn read_light(
    buf: &mut &[u8],
    present: bool,
    what: &str,
) -> Result<LightContainer, ChunkParsingError> {
    if !present {
        return Ok(LightContainer::Empty(0));
    }
    if buf.remaining() < LightContainer::ARRAY_SIZE {
        return Err(error(format!("Unexpected EOF reading {what} light")));
    }
    let data: Box<[u8]> = buf[..LightContainer::ARRAY_SIZE].into();
    buf.advance(LightContainer::ARRAY_SIZE);
    Ok(LightContainer::Full(data))
}

fn read_sections(buf: &mut &[u8]) -> Result<Sections, ChunkParsingError> {
    if buf.remaining() < 2 {
        return Err(error("Unexpected EOF reading section count"));
    }
    let section_count = buf.get_u16() as usize;
    if section_count > MAX_SECTIONS {
        return Err(error(format!(
            "Section count {section_count} exceeds maximum {MAX_SECTIONS}"
        )));
    }

    let mut sections = Sections {
        blocks: Vec::with_capacity(section_count),
        biomes: Vec::with_capacity(section_count),
        block_light: Vec::with_capacity(section_count),
        sky_light: Vec::with_capacity(section_count),
    };
    for _ in 0..section_count {
        sections.blocks.push(BlockPalette::read_pump_binary(buf)?);
        sections.biomes.push(BiomePalette::read_pump_binary(buf)?);

        if !buf.has_remaining() {
            return Err(error("Unexpected EOF reading section light flags"));
        }
        let light_flags = buf.get_u8();
        let block_light = read_light(buf, (light_flags & 1) != 0, "block")?;
        let sky_light = read_light(buf, (light_flags & 2) != 0, "sky")?;
        sections.block_light.push(block_light);
        sections.sky_light.push(sky_light);
    }
    Ok(sections)
}

/// A tick list: a count, then that many 16-byte ticks. A tick whose priority or id is unknown is
/// dropped. `kind` ("block" or "fluid") names the list in errors and `resolve` maps the id to its
/// value.
fn read_ticks<V>(
    buf: &mut &[u8],
    kind: &str,
    resolve: impl Fn(u16) -> Option<V>,
) -> Result<Vec<ScheduledTick<V>>, ChunkParsingError> {
    if buf.remaining() < 4 {
        return Err(error(format!("Unexpected EOF reading {kind} tick count")));
    }
    let count = buf.get_u32() as usize;
    if count > MAX_TICKS || buf.remaining() < count * TICK_LEN {
        return Err(error(format!("Invalid {kind} tick count")));
    }
    let mut ticks = Vec::with_capacity(count);
    for _ in 0..count {
        let x = buf.get_i32();
        let y = buf.get_i32();
        let z = buf.get_i32();
        let delay = buf.get_u8();
        let priority = i32::from(buf.get_i8());
        let raw_id = buf.get_u16();
        if let Ok(priority) = TickPriority::try_from(priority)
            && let Some(value) = resolve(raw_id)
        {
            ticks.push(ScheduledTick {
                delay,
                priority,
                position: BlockPos::new(x, y, z),
                value,
            });
        }
    }
    Ok(ticks)
}

/// A length-prefixed unnamed NBT payload; `what` names it in the errors.
fn read_nbt_payload(buf: &mut &[u8], what: &str) -> Result<NbtCompound, ChunkParsingError> {
    if buf.remaining() < 4 {
        return Err(error(format!("Unexpected EOF reading {what} length")));
    }
    let len = buf.get_u32() as usize;
    if buf.remaining() < len {
        return Err(error(format!("Unexpected EOF reading {what} payload")));
    }
    let mut cursor = std::io::Cursor::new(&buf[..len]);
    let mut reader = pumpkin_nbt::deserializer::NbtReadHelperJava::new(&mut cursor);
    let nbt = pumpkin_nbt::Nbt::read_unnamed(&mut reader)
        .map_err(|e| error(e.to_string()))?
        .root_tag;
    buf.advance(len);
    Ok(nbt)
}

/// The block entities, keyed by their `x`, `y` and `z`; one without all three is dropped.
fn read_block_entities(
    buf: &mut &[u8],
) -> Result<FxHashMap<BlockPos, NbtCompound>, ChunkParsingError> {
    if buf.remaining() < 4 {
        return Err(error("Unexpected EOF reading block entity count"));
    }
    let count = buf.get_u32() as usize;
    if count > MAX_BLOCK_ENTITIES {
        return Err(error("Invalid block entity count"));
    }
    let mut block_entities = FxHashMap::default();
    for _ in 0..count {
        let nbt = read_nbt_payload(buf, "block entity")?;
        if let Some(x) = nbt.get_int("x")
            && let Some(y) = nbt.get_int("y")
            && let Some(z) = nbt.get_int("z")
        {
            block_entities.insert(BlockPos::new(x, y, z), nbt);
        }
    }
    Ok(block_entities)
}

impl ChunkData {
    pub fn internal_from_pump_bytes(
        chunk_data: &[u8],
        position: Vector2<i32>,
    ) -> Result<Self, ChunkParsingError> {
        let (header, mut buf) = read_header(chunk_data, position)?;
        let heightmaps = read_heightmaps(&mut buf, header.flags)?;
        let sections = read_sections(&mut buf)?;

        let (random_tick_sections, randomly_ticking_mask) =
            ChunkSections::build_random_tick_sections_cache(&sections.blocks);
        let section = ChunkSections {
            count: sections.blocks.len(),
            block_sections: RwLock::new(sections.blocks.into_boxed_slice()),
            random_tick_sections: RwLock::new(random_tick_sections),
            randomly_ticking_mask: std::sync::atomic::AtomicU32::new(randomly_ticking_mask),
            biome_sections: RwLock::new(sections.biomes.into_boxed_slice()),
            min_y: header.min_y,
        };
        let light_engine = ChunkLight {
            block_light: sections.block_light.into_boxed_slice(),
            sky_light: sections.sky_light.into_boxed_slice(),
        };

        let block_ticks = read_ticks(&mut buf, "block", |raw_id| {
            BlockId::new(raw_id).map(Block::from_id)
        })?;
        let fluid_ticks = read_ticks(&mut buf, "fluid", Fluid::from_id)?;
        let block_entities = read_block_entities(&mut buf)?;
        let custom_data = if header.has_custom_data() {
            read_nbt_payload(&mut buf, "custom data")?
        } else {
            NbtCompound::default()
        };

        Ok(Self {
            section,
            heightmap: std::sync::Mutex::new(heightmaps),
            x: position.x,
            z: position.y,
            dirty: AtomicBool::new(false),
            block_ticks: ChunkTickScheduler::from_iter(block_ticks),
            fluid_ticks: ChunkTickScheduler::from_iter(fluid_ticks),
            pending_block_entities: std::sync::Mutex::new(block_entities),
            light_engine: std::sync::Mutex::new(light_engine),
            light_populated: AtomicBool::new(header.light_correct()),
            status: header.status,
            blending_data: None,
            inhabited_time: AtomicU64::new(header.inhabited_time),
            custom_data: std::sync::Mutex::new(custom_data),
        })
    }
}
