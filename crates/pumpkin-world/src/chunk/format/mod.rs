use std::{
    path::PathBuf,
    str::FromStr,
    sync::atomic::{AtomicBool, Ordering},
};

use bytes::{BufMut, Bytes};
use pumpkin_data::{BlockStateId, chunk::ChunkStatus};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_util::resource_location::{FromResourceLocation, ResourceLocation};

use crate::{
    chunk::{
        ChunkEntityData, ChunkReadingError, ChunkSerializingError,
        format::anvil::{SingleChunkDataSerializer, WORLD_DATA_VERSION},
        io::{Dirtiable, file_manager::PathFromLevelFolder},
    },
    level::LevelFolder,
    tick::{ScheduledTick, TickPriority},
};
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;

use super::{ChunkData, ChunkHeightmaps, ChunkParsingError};
pub mod anvil;
mod anvil_read;
pub mod linear;
pub mod pump;

impl SingleChunkDataSerializer for ChunkData {
    #[inline]
    fn from_bytes(bytes: &Bytes, pos: Vector2<i32>) -> Result<Self, ChunkReadingError> {
        Self::internal_from_bytes(bytes, pos).map_err(ChunkReadingError::ParsingError)
    }

    #[inline]
    fn to_bytes(&self) -> Result<Bytes, ChunkSerializingError> {
        Ok(self.internal_to_bytes())
    }

    #[inline]
    fn to_pump_bytes(&self) -> Result<Bytes, ChunkSerializingError> {
        Ok(self.internal_to_pump_bytes())
    }

    #[inline]
    fn position(&self) -> (i32, i32) {
        (self.x, self.z)
    }
}

impl PathFromLevelFolder for ChunkData {
    #[inline]
    fn file_path(folder: &LevelFolder, file_name: &str) -> PathBuf {
        folder.region_folder.join(file_name)
    }
}

impl Dirtiable for ChunkData {
    #[inline]
    fn mark_dirty(&self, flag: bool) {
        self.dirty.store(flag, Ordering::Relaxed);
    }

    #[inline]
    fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }
}

/// The section stores `Y` as a byte, short, int or long depending on who wrote
/// the file. The datafixer writes ints. Reading only bytes would map every int
/// section to `Y = 0`, and they would overwrite each other.
fn section_y(section: &NbtCompound) -> i32 {
    use pumpkin_nbt::tag::NbtTag;

    match section.get("Y") {
        Some(NbtTag::Byte(value)) => i32::from(*value),
        Some(NbtTag::Short(value)) => i32::from(*value),
        Some(NbtTag::Int(value)) => *value,
        Some(NbtTag::Long(value)) => i32::try_from(*value).unwrap_or(0),
        _ => 0,
    }
}

/// What vanilla does when `yPos` is missing, for example after it upgrades a
/// world in place: the lowest section that stores biomes, and never above 0.
/// That gives `-4` in the Overworld and `0` in the Nether and End, which are
/// the section minimums of those dimensions.
fn lowest_biome_section_y(root_tag: &NbtCompound) -> Option<i32> {
    let sections = root_tag.get_list("sections")?;
    sections
        .iter()
        .filter_map(|tag| match tag {
            pumpkin_nbt::tag::NbtTag::Compound(compound) if compound.has("biomes") => {
                Some(section_y(compound))
            }
            _ => None,
        })
        .min()
        .map(|y| y.min(0))
}

fn extract_u16_array(tag: &pumpkin_nbt::tag::NbtTag) -> Option<Box<[BlockStateId]>> {
    match tag {
        pumpkin_nbt::tag::NbtTag::IntArray(arr) => Some(
            arr.iter()
                .map(|&x| BlockStateId::new_or_air(x as u16))
                .collect(),
        ),
        pumpkin_nbt::tag::NbtTag::ByteArray(arr) => Some(
            arr.iter()
                .map(|&x| BlockStateId::new_or_air(x as u16))
                .collect(),
        ),
        pumpkin_nbt::tag::NbtTag::LongArray(arr) => Some(
            arr.iter()
                .map(|&x| BlockStateId::new_or_air(x as u16))
                .collect(),
        ),
        pumpkin_nbt::tag::NbtTag::List(list) => {
            let ids: Box<[BlockStateId]> = list
                .iter()
                .map(|t| match t {
                    pumpkin_nbt::tag::NbtTag::Int(x) => BlockStateId::new_or_air(*x as u16),
                    pumpkin_nbt::tag::NbtTag::Short(x) => BlockStateId::new_or_air(*x as u16),
                    pumpkin_nbt::tag::NbtTag::Byte(x) => BlockStateId::new_or_air(*x as u16),
                    pumpkin_nbt::tag::NbtTag::Long(x) => BlockStateId::new_or_air(*x as u16),
                    pumpkin_nbt::tag::NbtTag::String(s) => {
                        let entry =
                            crate::generation::structure::template::PaletteEntry::new(s.to_string());
                        if let Some(state) =
                            crate::generation::structure::template::BlockStateResolver::resolve_simple(
                                &entry,
                            )
                        {
                            return state.id;
                        }
                        BlockStateId::AIR
                    }
                    pumpkin_nbt::tag::NbtTag::Compound(compound) => {
                        if let Ok(entry) =
                            crate::generation::structure::template::PaletteEntry::from_nbt_compound(
                                compound,
                            )
                            && let Some(state) =
                                crate::generation::structure::template::BlockStateResolver::resolve_simple(
                                    &entry,
                                )
                        {
                            return state.id;
                        }
                        BlockStateId::AIR
                    }
                    _ => BlockStateId::AIR,
                })
                .collect();
            Some(ids)
        }
        _ => None,
    }
}

fn extract_u8_array(tag: &pumpkin_nbt::tag::NbtTag) -> Option<Box<[u8]>> {
    match tag {
        pumpkin_nbt::tag::NbtTag::ByteArray(arr) => Some(arr.iter().map(|&x| x as u8).collect()),
        pumpkin_nbt::tag::NbtTag::IntArray(arr) => Some(arr.iter().map(|&x| x as u8).collect()),
        pumpkin_nbt::tag::NbtTag::List(list) => {
            let bytes: Box<[u8]> = list
                .iter()
                .map(|t| match t {
                    pumpkin_nbt::tag::NbtTag::Byte(x) => *x as u8,
                    pumpkin_nbt::tag::NbtTag::Int(x) => *x as u8,
                    pumpkin_nbt::tag::NbtTag::Short(x) => *x as u8,
                    pumpkin_nbt::tag::NbtTag::String(s) => {
                        let name = s.strip_prefix("minecraft:").unwrap_or(s);
                        pumpkin_data::biome::Biome::from_name(name).map_or(0, |b| b.id)
                    }
                    _ => 0,
                })
                .collect();
            Some(bytes)
        }
        _ => None,
    }
}

fn parse_scheduled_tick<T>(nbt: &pumpkin_nbt::compound::NbtCompound) -> Option<ScheduledTick<T>>
where
    T: FromResourceLocation,
{
    let x = nbt.get_int("x")?;
    let y = nbt.get_int("y")?;
    let z = nbt.get_int("z")?;
    let delay = nbt.get_int("t")? as u8;
    let priority = TickPriority::try_from(nbt.get_int("p")?).ok()?;
    let res_loc_str = nbt.get_string("i")?;
    let res_loc = ResourceLocation::from_str(res_loc_str).ok()?;
    let value = T::from_resource_location(&res_loc)?;
    Some(ScheduledTick {
        delay,
        priority,
        position: BlockPos::new(x, y, z),
        value,
    })
}

mod anvil_write;

impl ChunkData {
    pub fn internal_to_bytes(&self) -> Bytes {
        anvil_write::chunk_bytes(self)
    }

    #[allow(clippy::too_many_lines)]
    pub fn internal_to_pump_bytes(&self) -> Bytes {
        const PUMP_CHUNK_VERSION: u8 = 1;

        let is_light_correct = self.light_populated.load(Ordering::Relaxed);
        let block_entities_nbt = {
            let entities_guard = self
                .pending_block_entities
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            entities_guard.values().cloned().collect::<Vec<_>>()
        };

        let light_lock = self
            .light_engine
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let heightmap_lock = self
            .heightmap
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let block_lock = self
            .section
            .block_sections
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let biome_lock = self
            .section
            .biome_sections
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let custom_data = self
            .custom_data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let ws = heightmap_lock
            .world_surface
            .as_deref()
            .filter(|a| a.len() == ChunkHeightmaps::LONGS);
        let mb = heightmap_lock
            .motion_blocking
            .as_deref()
            .filter(|a| a.len() == ChunkHeightmaps::LONGS);
        let mbnl = heightmap_lock
            .motion_blocking_no_leaves
            .as_deref()
            .filter(|a| a.len() == ChunkHeightmaps::LONGS);
        let has_custom = !custom_data.is_empty();

        let mut flags = u8::from(is_light_correct);
        if ws.is_some() {
            flags |= 1 << 1;
        }
        if mb.is_some() {
            flags |= 1 << 2;
        }
        if mbnl.is_some() {
            flags |= 1 << 3;
        }
        if has_custom {
            flags |= 1 << 4;
        }

        let status_u8 = match self.status {
            ChunkStatus::Empty => 0,
            ChunkStatus::StructureStarts => 1,
            ChunkStatus::StructureReferences => 2,
            ChunkStatus::Biomes => 3,
            ChunkStatus::Terrain => 4,
            ChunkStatus::Features => 5,
            ChunkStatus::InitializeLight => 6,
            ChunkStatus::Light => 7,
            ChunkStatus::Spawn => 8,
            ChunkStatus::Full => 9,
        };

        let mut out = Vec::with_capacity(16 * 1024);
        out.extend_from_slice(&pump::PUMP_CHUNK_MAGIC);
        out.put_u8(PUMP_CHUNK_VERSION);
        out.put_i32(WORLD_DATA_VERSION);
        out.put_i32(self.x);
        out.put_i32(self.z);
        out.put_i32(self.section.min_y);
        out.put_u8(status_u8);
        out.put_u8(flags);
        out.put_u64(self.inhabited_time.load(Ordering::Relaxed));

        for hm in [ws, mb, mbnl].into_iter().flatten() {
            for &val in hm {
                out.put_i64(val);
            }
        }

        out.put_u16(self.section.count as u16);
        for i in 0..self.section.count {
            block_lock[i].write_pump_binary(&mut out);
            biome_lock[i].write_pump_binary(&mut out);

            let bl = match light_lock.block_light.get(i) {
                Some(LightContainer::Full(data)) if data.len() == LightContainer::ARRAY_SIZE => {
                    Some(data.as_ref())
                }
                _ => None,
            };
            let sl = match light_lock.sky_light.get(i) {
                Some(LightContainer::Full(data)) if data.len() == LightContainer::ARRAY_SIZE => {
                    Some(data.as_ref())
                }
                _ => None,
            };
            let mut light_flags = 0u8;
            if bl.is_some() {
                light_flags |= 1;
            }
            if sl.is_some() {
                light_flags |= 2;
            }
            out.put_u8(light_flags);
            if let Some(data) = bl {
                out.extend_from_slice(data);
            }
            if let Some(data) = sl {
                out.extend_from_slice(data);
            }
        }

        let block_ticks = self.block_ticks.to_vec();
        out.put_u32(block_ticks.len() as u32);
        for tick in block_ticks {
            out.put_i32(tick.position.0.x);
            out.put_i32(tick.position.0.y);
            out.put_i32(tick.position.0.z);
            out.put_u8(tick.delay);
            out.put_i8(tick.priority as i32 as i8);
            out.put_u16(tick.value.id.as_u16());
        }

        let fluid_ticks = self.fluid_ticks.to_vec();
        out.put_u32(fluid_ticks.len() as u32);
        for tick in fluid_ticks {
            out.put_i32(tick.position.0.x);
            out.put_i32(tick.position.0.y);
            out.put_i32(tick.position.0.z);
            out.put_u8(tick.delay);
            out.put_i8(tick.priority as i32 as i8);
            out.put_u16(tick.value.id);
        }

        out.put_u32(block_entities_nbt.len() as u32);
        for entity_comp in block_entities_nbt {
            let bytes = pumpkin_nbt::Nbt::new(String::new(), entity_comp).write_unnamed();
            out.put_u32(bytes.len() as u32);
            out.extend_from_slice(&bytes);
        }

        if has_custom {
            let bytes = pumpkin_nbt::Nbt::new(String::new(), custom_data.clone()).write_unnamed();
            out.put_u32(bytes.len() as u32);
            out.extend_from_slice(&bytes);
        }

        out.into()
    }

    pub fn set_custom_data(&self, namespace: &str, key: &str, value: pumpkin_nbt::tag::NbtTag) {
        let mut custom_data = self
            .custom_data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let mut namespace_data = custom_data
            .child_tags
            .remove(namespace)
            .and_then(|tag| match tag {
                pumpkin_nbt::tag::NbtTag::Compound(compound) => Some(compound),
                _ => None,
            })
            .unwrap_or_default();

        namespace_data.child_tags.insert(key.into(), value);
        custom_data.child_tags.insert(
            namespace.into(),
            pumpkin_nbt::tag::NbtTag::Compound(namespace_data),
        );
        self.dirty.store(true, Ordering::Relaxed);
    }

    pub fn get_custom_data(&self, namespace: &str, key: &str) -> Option<pumpkin_nbt::tag::NbtTag> {
        let custom_data = self
            .custom_data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        custom_data
            .get(namespace)?
            .extract_compound()?
            .get(key)
            .cloned()
    }

    pub fn remove_custom_data(&self, namespace: &str, key: &str) {
        let mut custom_data = self
            .custom_data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let Some(pumpkin_nbt::tag::NbtTag::Compound(mut namespace_data)) =
            custom_data.child_tags.remove(namespace)
        else {
            return;
        };

        namespace_data.child_tags.remove(key);
        if !namespace_data.is_empty() {
            custom_data.child_tags.insert(
                namespace.into(),
                pumpkin_nbt::tag::NbtTag::Compound(namespace_data),
            );
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    pub fn has_custom_data(&self, namespace: &str, key: &str) -> bool {
        self.get_custom_data(namespace, key).is_some()
    }
}

impl PathFromLevelFolder for ChunkEntityData {
    #[inline]
    fn file_path(folder: &LevelFolder, file_name: &str) -> PathBuf {
        folder.entities_folder.join(file_name)
    }
}

impl Dirtiable for ChunkEntityData {
    #[inline]
    fn mark_dirty(&self, flag: bool) {
        self.dirty.store(flag, Ordering::Relaxed);
    }

    #[inline]
    fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }
}

impl SingleChunkDataSerializer for ChunkEntityData {
    #[inline]
    fn from_bytes(bytes: &Bytes, pos: Vector2<i32>) -> Result<Self, ChunkReadingError> {
        Self::internal_from_bytes(bytes, pos).map_err(ChunkReadingError::ParsingError)
    }

    #[inline]
    fn to_bytes(&self) -> Result<Bytes, ChunkSerializingError> {
        Ok(self.internal_to_bytes())
    }

    #[inline]
    fn position(&self) -> (i32, i32) {
        (self.x, self.z)
    }
}

impl ChunkEntityData {
    fn internal_from_bytes(
        chunk_data: &[u8],
        position: Vector2<i32>,
    ) -> Result<Self, ChunkParsingError> {
        let is_named = chunk_data.len() >= 3
            && chunk_data[0] == 0x0a
            && chunk_data[1] == 0x00
            && chunk_data[2] == 0x00;
        let mut cursor = std::io::Cursor::new(chunk_data);
        let mut reader = pumpkin_nbt::deserializer::NbtReadHelperJava::new(
            pumpkin_nbt::deserializer::NbtStreamReader(&mut cursor),
        );
        let nbt = if is_named {
            pumpkin_nbt::Nbt::read(&mut reader)
        } else {
            pumpkin_nbt::Nbt::read_unnamed(&mut reader)
        }
        .map_err(|e| ChunkParsingError::ErrorDeserializingChunk(e.to_string()))?;

        let pos_array = match (nbt.get_int("Position-X"), nbt.get_int("Position-Z")) {
            (Some(x), Some(z)) => [x, z],
            _ => {
                if let Some(pumpkin_nbt::tag::NbtTag::IntArray(pos)) = nbt.get("Position") {
                    if pos.len() >= 2 {
                        [pos[0], pos[1]]
                    } else {
                        [0, 0]
                    }
                } else {
                    [0, 0]
                }
            }
        };

        if pos_array[0] != position.x || pos_array[1] != position.y {
            return Err(ChunkParsingError::ErrorDeserializingChunk(format!(
                "Expected data for entity chunk {},{} but got it for {},{}!",
                position.x, position.y, pos_array[0], pos_array[1],
            )));
        }

        let entities = match nbt.get("Entities") {
            Some(pumpkin_nbt::tag::NbtTag::List(list)) => list
                .iter()
                .filter_map(|t| match t {
                    pumpkin_nbt::tag::NbtTag::Compound(c) => Some(c.clone()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };

        Ok(Self {
            x: position.x,
            z: position.y,
            data: std::sync::Mutex::new(entities),
            live: AtomicBool::new(false),
            dirty: AtomicBool::new(false),
        })
    }

    fn internal_to_bytes(&self) -> Bytes {
        let mut root = NbtCompound::new();
        root.put_int("DataVersion", WORLD_DATA_VERSION);
        root.put(
            "Position",
            pumpkin_nbt::tag::NbtTag::IntArray(vec![self.x, self.z]),
        );
        let entities_tag: Vec<pumpkin_nbt::tag::NbtTag> = self
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|c| pumpkin_nbt::tag::NbtTag::Compound(c.clone()))
            .collect();
        root.put_list("Entities", entities_tag);

        let nbt = pumpkin_nbt::Nbt::from(root);
        nbt.write()
    }
}

#[derive(Clone)]
pub struct ChunkSectionBiomes {
    pub(crate) data: Option<Box<[i64]>>,
    pub(crate) palette: Box<[u8]>,
}

#[derive(Clone)]
pub struct ChunkSectionBlockStates {
    pub(crate) data: Option<Box<[i64]>>,
    pub(crate) palette: Box<[BlockStateId]>,
}

#[derive(Debug, Clone)]
pub enum LightContainer {
    Empty(u8),
    Full(Box<[u8]>),
}

impl LightContainer {
    pub const DIM: usize = 16;
    pub const ARRAY_SIZE: usize = Self::DIM * Self::DIM * Self::DIM / 2;

    #[must_use]
    pub fn new_empty(default: u8) -> Self {
        assert!(default <= 15, "Default value must be between 0 and 15");
        Self::Empty(default)
    }

    #[must_use]
    pub fn new(data: Box<[u8]>) -> Self {
        assert!(
            data.len() == Self::ARRAY_SIZE,
            "Data length must be {}",
            Self::ARRAY_SIZE
        );
        Self::Full(data)
    }

    #[must_use]
    pub fn new_filled(default: u8) -> Self {
        assert!(default <= 15, "Default value must be between 0 and 15");
        let value = default << 4 | default;
        Self::Full([value; Self::ARRAY_SIZE].into())
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        matches!(self, Self::Empty(_))
    }

    #[inline]
    const fn index(x: usize, y: usize, z: usize) -> usize {
        y * 16 * 16 + z * 16 + x
    }

    #[inline]
    #[must_use]
    pub fn get(&self, x: usize, y: usize, z: usize) -> u8 {
        match self {
            Self::Full(data) => {
                let index = Self::index(x, y, z);
                (data[index >> 1] >> (4 * (index & 1))) & 0x0F
            }
            Self::Empty(default) => *default,
        }
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, z: usize, value: u8) {
        match self {
            Self::Full(data) => {
                let index = Self::index(x, y, z);
                let shift = 4 * (index & 1);
                let mask = 0x0F << shift;
                data[index >> 1] = (data[index >> 1] & !mask) | (value << shift);
            }
            Self::Empty(default) => {
                if value != *default {
                    *self = Self::new_filled(*default);
                    self.set(x, y, z, value);
                }
            }
        }
    }

    #[inline]
    pub fn set_column_y_range(
        &mut self,
        x: usize,
        z: usize,
        y_start: usize,
        y_end: usize,
        value: u8,
    ) {
        if y_start >= y_end {
            return;
        }
        match self {
            Self::Full(data) => {
                let shift = 4 * (x & 1);
                let mask = 0x0F << shift;
                let val = (value & 0x0F) << shift;
                let mut byte_idx = (y_start * 256 + z * 16 + x) >> 1;
                for _ in y_start..y_end {
                    data[byte_idx] = (data[byte_idx] & !mask) | val;
                    byte_idx += 128;
                }
            }
            Self::Empty(default) => {
                if value != *default {
                    *self = Self::new_filled(*default);
                    self.set_column_y_range(x, z, y_start, y_end, value);
                }
            }
        }
    }

    #[inline]
    pub fn fill(&mut self, value: u8) {
        *self = Self::new_filled(value);
    }
}

impl Default for LightContainer {
    fn default() -> Self {
        Self::new_empty(15)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumpkin_data::Block;
    use pumpkin_nbt::compound::NbtCompound;
    use pumpkin_nbt::tag::NbtTag;

    fn test_section(y: i32, block: &str, with_biomes: bool) -> NbtCompound {
        let mut block_states = NbtCompound::new();
        let mut air = NbtCompound::new();
        air.put_string("Name", "minecraft:air".to_string());
        let mut solid = NbtCompound::new();
        solid.put_string("Name", block.to_string());
        block_states.put(
            "palette",
            NbtTag::List(vec![NbtTag::Compound(air), NbtTag::Compound(solid)]),
        );
        block_states.put("data", NbtTag::LongArray(vec![1; 256]));

        let mut section = NbtCompound::new();
        section.put_int("Y", y);
        section.put("block_states", NbtTag::Compound(block_states));
        if with_biomes {
            let mut biomes = NbtCompound::new();
            biomes.put(
                "palette",
                NbtTag::List(vec![NbtTag::String("minecraft:plains".into())]),
            );
            section.put("biomes", NbtTag::Compound(biomes));
        }
        section
    }

    fn test_chunk(sections: Vec<NbtCompound>) -> pumpkin_nbt::Nbt {
        let mut root = NbtCompound::new();
        root.put_int("DataVersion", 5023);
        root.put_int("xPos", 0);
        root.put_int("zPos", 0);
        root.put_string("Status", "minecraft:full".to_string());
        root.put(
            "sections",
            NbtTag::List(sections.into_iter().map(NbtTag::Compound).collect()),
        );
        pumpkin_nbt::Nbt::new(String::new(), root)
    }

    #[test]
    fn chunk_without_y_pos_uses_the_lowest_biome_section() {
        use crate::chunk::ChunkData;
        use pumpkin_util::math::vector2::Vector2;

        // The datafixer writes Y as an int, so the test must too.
        let bytes = test_chunk(vec![test_section(-4, "minecraft:stone", true)]).write();
        let chunk =
            ChunkData::from_bytes(&bytes, Vector2::new(0, 0)).expect("chunk without yPos parses");
        assert_eq!(
            chunk.section.get_block_absolute_y(0, -64, 0),
            Some(Block::STONE.default_state.id)
        );
    }

    #[test]
    fn chunk_with_int_y_sections_keeps_every_section() {
        use crate::chunk::ChunkData;
        use pumpkin_util::math::vector2::Vector2;

        let bytes = test_chunk(vec![
            test_section(-4, "minecraft:stone", true),
            test_section(0, "minecraft:dirt", true),
        ])
        .write();
        let chunk = ChunkData::from_bytes(&bytes, Vector2::new(0, 0)).expect("int Y chunk parses");
        assert_eq!(
            chunk.section.get_block_absolute_y(0, -64, 0),
            Some(Block::STONE.default_state.id)
        );
        assert_eq!(
            chunk.section.get_block_absolute_y(0, 0, 0),
            Some(Block::DIRT.default_state.id)
        );
    }

    #[test]
    fn chunk_with_mixed_numeric_y_tags_keeps_sections() {
        use crate::chunk::ChunkData;
        use pumpkin_util::math::vector2::Vector2;

        // What a real upgraded chunk looks like: the map's own sections store Y
        // as a byte, the datafixer writes ints. The int section must still land
        // at its own height instead of collapsing to Y = 0.
        let mut byte_section = test_section(-4, "minecraft:air", true);
        byte_section.put_byte("Y", -4);

        let bytes = test_chunk(vec![
            byte_section,
            test_section(0, "minecraft:stone", true),
            test_section(4, "minecraft:dirt", true),
        ])
        .write();
        let chunk =
            ChunkData::from_bytes(&bytes, Vector2::new(0, 0)).expect("mixed Y chunk parses");
        assert_eq!(
            chunk.section.get_block_absolute_y(0, 0, 0),
            Some(Block::STONE.default_state.id)
        );
        assert_eq!(
            chunk.section.get_block_absolute_y(0, 64, 0),
            Some(Block::DIRT.default_state.id)
        );
    }

    #[test]
    fn fallback_ignores_light_only_sections_below_zero() {
        use crate::chunk::ChunkData;
        use pumpkin_util::math::vector2::Vector2;

        // Old worlds keep a light grid at Y = -1 after the upgrade. It has no
        // biomes and must not decide where the chunk starts.
        let mut light_section = NbtCompound::new();
        light_section.put_byte("Y", -1);
        light_section.put(
            "BlockLight",
            NbtTag::ByteArray(vec![0x0Fi8; 2048].into_boxed_slice()),
        );

        let bytes = test_chunk(vec![
            light_section,
            test_section(0, "minecraft:stone", true),
        ])
        .write();
        let chunk =
            ChunkData::from_bytes(&bytes, Vector2::new(0, 0)).expect("light section ignored");
        assert_eq!(
            chunk.section.get_block_absolute_y(0, 0, 0),
            Some(Block::STONE.default_state.id)
        );
        assert_eq!(chunk.section.get_block_absolute_y(0, -1, 0), None);
    }

    #[test]
    fn fallback_never_goes_above_zero() {
        use crate::chunk::ChunkData;
        use pumpkin_util::math::vector2::Vector2;

        let bytes = test_chunk(vec![test_section(1, "minecraft:stone", true)]).write();
        let chunk = ChunkData::from_bytes(&bytes, Vector2::new(0, 0)).expect("caps at zero");
        // The only section sits at Y=1, so the cap has to bring the chunk down
        // to Y=0. Without the cap the chunk would start at Y=1 and a block at
        // Y=0 would be out of range.
        assert_eq!(
            chunk.section.get_block_absolute_y(0, 0, 0),
            Some(Block::AIR.default_state.id)
        );
        assert_eq!(
            chunk.section.get_block_absolute_y(0, 16, 0),
            Some(Block::STONE.default_state.id)
        );
    }

    #[test]
    fn chunk_without_y_pos_or_biomes_still_fails() {
        use crate::chunk::ChunkData;
        use pumpkin_util::math::vector2::Vector2;

        let mut light_section = NbtCompound::new();
        light_section.put_byte("Y", -1);
        light_section.put(
            "BlockLight",
            NbtTag::ByteArray(vec![0x0Fi8; 2048].into_boxed_slice()),
        );

        let bytes = test_chunk(vec![light_section]).write();
        let Err(error) = ChunkData::from_bytes(&bytes, Vector2::new(0, 0)) else {
            panic!("chunk without yPos and without biomes must fail");
        };
        assert!(format!("{error:?}").contains("Missing yPos"));
    }

    #[test]
    fn extract_u16_array_from_vanilla_compound_palette() {
        let mut entry1 = NbtCompound::new();
        entry1.put_string("Name", "minecraft:stone".to_string());

        let mut entry2 = NbtCompound::new();
        entry2.put_string("Name", "minecraft:repeater".to_string());
        let mut props = NbtCompound::new();
        props.put_string("facing", "north".to_string());
        props.put_string("delay", "2".to_string());
        props.put_string("locked", "false".to_string());
        props.put_string("powered", "false".to_string());
        entry2.put_compound("Properties", props);

        let list_tag = NbtTag::List(vec![NbtTag::Compound(entry1), NbtTag::Compound(entry2)]);
        let result = extract_u16_array(&list_tag).expect("should extract palette");

        assert_eq!(result.len(), 2);
        assert_eq!(result[0], Block::STONE.default_state.id);

        let repeater_state = Block::REPEATER
            .from_properties(&[
                pumpkin_data::Property::Facing(pumpkin_data::block_properties::Facing::North),
                pumpkin_data::Property::Delay(2),
                pumpkin_data::Property::Locked(false),
                pumpkin_data::Property::Powered(false),
            ])
            .to_state_id(&Block::REPEATER);
        assert_eq!(result[1], repeater_state);
    }

    #[test]
    fn extract_u8_array_from_vanilla_string_palette() {
        let list_tag = NbtTag::List(vec![
            NbtTag::String("minecraft:plains".to_string().into()),
            NbtTag::String("minecraft:the_void".to_string().into()),
        ]);
        let result = extract_u8_array(&list_tag).expect("should extract biome palette");

        assert_eq!(result.len(), 2);
        assert_eq!(
            result[0],
            pumpkin_data::biome::Biome::from_name("plains").unwrap().id
        );
        assert_eq!(
            result[1],
            pumpkin_data::biome::Biome::from_name("the_void")
                .unwrap()
                .id
        );
    }
}
