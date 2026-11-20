//! Writing a chunk as the NBT of an anvil chunk.

use std::sync::atomic::Ordering;

use bytes::Bytes;
use pumpkin_data::Block;
use pumpkin_data::chunk::ChunkStatus;
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_nbt::tag::NbtTag;
use pumpkin_util::resource_location::ToResourceLocation;

use super::{ChunkData, LightContainer};
use crate::chunk::ChunkHeightmaps;
use crate::chunk::format::anvil::WORLD_DATA_VERSION;
use crate::chunk::palette::{BiomePalette, BlockPalette};
use crate::generation::section_coords;
use crate::tick::ScheduledTick;

const fn status_name(status: ChunkStatus) -> &'static str {
    match status {
        ChunkStatus::Empty => "minecraft:empty",
        ChunkStatus::StructureStarts => "minecraft:structure_starts",
        ChunkStatus::StructureReferences => "minecraft:structure_references",
        ChunkStatus::Biomes => "minecraft:biomes",
        ChunkStatus::Terrain => "minecraft:terrain",
        ChunkStatus::Features => "minecraft:features",
        ChunkStatus::InitializeLight => "minecraft:initialize_light",
        ChunkStatus::Light => "minecraft:light",
        ChunkStatus::Spawn => "minecraft:spawn",
        ChunkStatus::Full => "minecraft:full",
    }
}

fn namespaced(name: &str) -> String {
    if name.starts_with("minecraft:") {
        name.to_string()
    } else {
        format!("minecraft:{name}")
    }
}

fn heightmaps_nbt(heightmaps: &ChunkHeightmaps) -> NbtCompound {
    let mut compound = NbtCompound::new();
    for (key, array) in [
        ("WORLD_SURFACE", &heightmaps.world_surface),
        ("MOTION_BLOCKING", &heightmaps.motion_blocking),
        (
            "MOTION_BLOCKING_NO_LEAVES",
            &heightmaps.motion_blocking_no_leaves,
        ),
    ] {
        if let Some(array) = array {
            compound.put(key, NbtTag::LongArray(array.to_vec()));
        }
    }
    compound
}

fn block_palette_entry(id: pumpkin_data::BlockStateId) -> NbtTag {
    let block = Block::from_state_id(id);
    let mut comp = NbtCompound::new();
    comp.put_string("Name", namespaced(block.name));
    if let Some(props) = block.properties(id) {
        let prop_vec = props.to_props();
        if !prop_vec.is_empty() {
            let mut props_comp = NbtCompound::new();
            for p in prop_vec {
                props_comp.put_string(p.key_str(), p.value_str().to_string());
            }
            comp.put_compound("Properties", props_comp);
        }
    }
    NbtTag::Compound(comp)
}

fn block_states_nbt(blocks: &BlockPalette) -> NbtCompound {
    let disk = blocks.to_disk_nbt();
    let mut comp = NbtCompound::new();
    if let Some(ref data) = disk.data {
        comp.put("data", NbtTag::LongArray(data.to_vec()));
    }
    let palette = disk
        .palette
        .iter()
        .map(|&id| block_palette_entry(id))
        .collect();
    comp.put_list("palette", palette);
    comp
}

fn biomes_nbt(biomes: &BiomePalette) -> NbtCompound {
    let disk = biomes.to_disk_nbt();
    let mut comp = NbtCompound::new();
    if let Some(ref data) = disk.data {
        comp.put("data", NbtTag::LongArray(data.to_vec()));
    }
    let palette = disk
        .palette
        .iter()
        .map(|&val| {
            let name = pumpkin_data::biome::Biome::from_id(val).map_or("plains", |b| b.registry_id);
            NbtTag::String(namespaced(name).into())
        })
        .collect();
    comp.put_list("palette", palette);
    comp
}

fn light_bytes(light: Option<&LightContainer>) -> Option<NbtTag> {
    match light {
        Some(LightContainer::Full(data)) => Some(NbtTag::ByteArray(
            data.iter().map(|&x| x as i8).collect::<Box<[i8]>>(),
        )),
        _ => None,
    }
}

fn tick_nbt<T: ToResourceLocation>(tick: &ScheduledTick<T>) -> NbtTag {
    let mut comp = NbtCompound::new();
    comp.put_int("x", tick.position.0.x);
    comp.put_int("y", tick.position.0.y);
    comp.put_int("z", tick.position.0.z);
    comp.put_int("t", tick.delay as i32);
    comp.put_int("p", tick.priority as i32);
    comp.put_string("i", tick.value.to_resource_location());
    NbtTag::Compound(comp)
}

fn sections_nbt(chunk: &ChunkData) -> Vec<NbtTag> {
    let light = chunk
        .light_engine
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let blocks = chunk
        .section
        .block_sections
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let biomes = chunk
        .section
        .biome_sections
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let min_section_y = (chunk.section.min_y >> 4) as i8;

    (0..chunk.section.count)
        .map(|i| {
            let mut comp = NbtCompound::new();
            comp.put_byte("Y", i as i8 + min_section_y);
            comp.put_compound("block_states", block_states_nbt(&blocks[i]));
            comp.put_compound("biomes", biomes_nbt(&biomes[i]));
            if let Some(tag) = light_bytes(light.block_light.get(i)) {
                comp.put("BlockLight", tag);
            }
            if let Some(tag) = light_bytes(light.sky_light.get(i)) {
                comp.put("SkyLight", tag);
            }
            NbtTag::Compound(comp)
        })
        .collect()
}

pub(super) fn chunk_bytes(chunk: &ChunkData) -> Bytes {
    let block_entities: Vec<NbtTag> = chunk
        .pending_block_entities
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .values()
        .cloned()
        .map(NbtTag::Compound)
        .collect();

    let mut root = NbtCompound::new();
    root.put_int("DataVersion", WORLD_DATA_VERSION);
    root.put_int("xPos", chunk.x);
    root.put_int("zPos", chunk.z);
    root.put_int(
        "yPos",
        section_coords::block_to_section(chunk.section.min_y),
    );
    root.put_string("Status", status_name(chunk.status).to_string());

    let heightmaps = chunk
        .heightmap
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    root.put_compound("Heightmaps", heightmaps_nbt(&heightmaps));
    drop(heightmaps);

    root.put_list("sections", sections_nbt(chunk));
    root.put_list(
        "block_ticks",
        chunk.block_ticks.to_vec().iter().map(tick_nbt).collect(),
    );
    root.put_list(
        "fluid_ticks",
        chunk.fluid_ticks.to_vec().iter().map(tick_nbt).collect(),
    );
    root.put_list("block_entities", block_entities);

    root.put_bool("isLightOn", chunk.light_populated.load(Ordering::Relaxed));
    root.put_long(
        "InhabitedTime",
        chunk.inhabited_time.load(Ordering::Relaxed) as i64,
    );

    let custom_data = chunk
        .custom_data
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !custom_data.is_empty() {
        root.put_compound("PumpkinCustomData", custom_data.clone());
    }

    pumpkin_nbt::Nbt::from(root).write()
}
