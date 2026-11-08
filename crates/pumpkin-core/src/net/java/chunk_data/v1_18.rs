use super::util::write_compound_nbt;
use pumpkin_protocol::codec::bit_set::BitSet;
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::ser::NetworkWriteExt;
use pumpkin_protocol::ser::WritingError;
use pumpkin_util::math::position::get_local_cord;
use pumpkin_util::version::JavaMinecraftVersion;
use pumpkin_world::chunk::ChunkData;
use pumpkin_world::chunk::format::LightContainer;
use pumpkin_world::chunk::palette::{BiomePalette, BlockPalette, NetworkPalette};
use std::io::Write;

/// Serializes chunk data for Minecraft 1.18+ through 26.2+.
///
/// The packet is the position, the heightmaps, the sections (blocks and biomes, length-prefixed),
/// the block entities and the light; each part is written by a function of its own.
pub fn write_chunk_data(
    chunk: &ChunkData,
    mut write: impl Write,
    version: &JavaMinecraftVersion,
) -> Result<(), WritingError> {
    write.write_i32_be(chunk.x)?;
    write.write_i32_be(chunk.z)?;

    write_heightmaps(chunk, &mut write, *version)?;
    write_sections(chunk, &mut write, *version)?;
    write_block_entities(chunk, &mut write, *version)?;
    write_light(chunk, &mut write, *version)
}

/// Heightmaps, as a map of indexes to arrays from 1.21.5 and as an NBT compound before.
fn write_heightmaps(
    chunk: &ChunkData,
    write: &mut impl Write,
    version: JavaMinecraftVersion,
) -> Result<(), WritingError> {
    let heightmaps = chunk
        .heightmap
        .lock()
        .map_err(|_| WritingError::Message("heightmap lock poisoned".into()))?;
    if version >= JavaMinecraftVersion::V_1_21_5 {
        write.write_var_int(&VarInt(3))?; // Map size

        let mut write_heightmap = |index: i32, data: &[i64]| -> Result<(), WritingError> {
            write.write_var_int(&VarInt(index))?;
            write.write_var_int(&VarInt(data.len() as i32))?;
            for val in data {
                write.write_i64_be(*val)?;
            }
            Ok(())
        };

        write_heightmap(1, heightmaps.world_surface.as_deref().unwrap_or(&[0; 37]))?;
        write_heightmap(4, heightmaps.motion_blocking.as_deref().unwrap_or(&[0; 37]))?;
        write_heightmap(
            5,
            heightmaps
                .motion_blocking_no_leaves
                .as_deref()
                .unwrap_or(&[0; 37]),
        )?;
    } else {
        let mut comp = pumpkin_nbt::compound::NbtCompound::new();
        if let Some(ref ws) = heightmaps.world_surface {
            comp.put(
                "WORLD_SURFACE",
                pumpkin_nbt::tag::NbtTag::LongArray(ws.to_vec()),
            );
        }
        if let Some(ref mb) = heightmaps.motion_blocking {
            comp.put(
                "MOTION_BLOCKING",
                pumpkin_nbt::tag::NbtTag::LongArray(mb.to_vec()),
            );
        }
        if let Some(ref mbnl) = heightmaps.motion_blocking_no_leaves {
            comp.put(
                "MOTION_BLOCKING_NO_LEAVES",
                pumpkin_nbt::tag::NbtTag::LongArray(mbnl.to_vec()),
            );
        }
        write_compound_nbt(write, &comp, version)?;
    }
    Ok(())
}

/// A palette: its entries for an indirect palette, the single entry for a single-value one, and
/// nothing for a direct one.
fn write_network_palette<V: Into<VarInt>>(
    buf: &mut Vec<u8>,
    palette: NetworkPalette<V>,
) -> Result<(), WritingError> {
    match palette {
        NetworkPalette::Single(registry_id) => {
            buf.write_var_int(&registry_id.into())?;
        }
        NetworkPalette::Indirect(palette) => {
            buf.write_var_int(&palette.len().try_into().map_err(|_| {
                WritingError::Message(format!(
                    "{} is not representable as a VarInt!",
                    palette.len()
                ))
            })?)?;
            for registry_id in palette {
                buf.write_var_int(&registry_id.into())?;
            }
        }
        NetworkPalette::Direct => {}
    }
    Ok(())
}

/// The packed entries of a palette: a length-prefixed array up to 1.21.4, the bare longs after.
fn write_packed_data(
    buf: &mut Vec<u8>,
    packed_data: &[i64],
    version: JavaMinecraftVersion,
) -> Result<(), WritingError> {
    if version <= JavaMinecraftVersion::V_1_21_4 {
        buf.write_list(packed_data, |buf, &packed| buf.write_i64_be(packed))?;
    } else {
        for packed in packed_data {
            buf.write_i64_be(*packed)?;
        }
    }
    Ok(())
}

/// One section: the block counts, the block palette with its data, the biome palette with its
/// data. Returns the bytes 1.21.5 expects as padding for it.
fn write_section(
    buf: &mut Vec<u8>,
    block_palette: &BlockPalette,
    biome_palette: &BiomePalette,
    version: JavaMinecraftVersion,
) -> Result<usize, WritingError> {
    let non_empty_block_count = block_palette.non_air_block_count() as i16;
    buf.write_i16_be(non_empty_block_count)?;
    if version >= JavaMinecraftVersion::V_26_1 {
        // New in 26.1, fluid count
        let liquid_count = block_palette.liquid_block_count() as i16;
        buf.write_i16_be(liquid_count)?;
    }

    let block_network = block_palette.convert_network();
    buf.write_u8(block_network.bits_per_entry)?;
    write_network_palette(buf, block_network.palette)?;
    write_packed_data(buf, &block_network.packed_data, version)?;

    let biome_network = biome_palette.convert_network();
    buf.write_u8(biome_network.bits_per_entry)?;
    write_network_palette(buf, biome_network.palette)?;
    write_packed_data(buf, &biome_network.packed_data, version)?;

    if version == JavaMinecraftVersion::V_1_21_5 {
        let block_storage_len = block_network.packed_data.len() as i32;
        let biome_storage_len = biome_network.packed_data.len() as i32;
        return Ok(
            VarInt(block_storage_len).written_size() + VarInt(biome_storage_len).written_size()
        );
    }
    Ok(0)
}

/// All sections, written to a buffer first because the packet prefixes them with their length.
fn write_sections(
    chunk: &ChunkData,
    write: &mut impl Write,
    version: JavaMinecraftVersion,
) -> Result<(), WritingError> {
    let mut blocks_and_biomes_buf = Vec::new();
    let block_sections = chunk
        .section
        .block_sections
        .read()
        .map_err(|_| WritingError::Message("block_sections read lock poisoned".into()))?;
    let biome_sections = chunk
        .section
        .biome_sections
        .read()
        .map_err(|_| WritingError::Message("biome_sections read lock poisoned".into()))?;

    let mut zero_bytes_count = 0;
    for (block_palette, biome_palette) in block_sections.iter().zip(biome_sections.iter()) {
        zero_bytes_count += write_section(
            &mut blocks_and_biomes_buf,
            block_palette,
            biome_palette,
            version,
        )?;
    }

    if version == JavaMinecraftVersion::V_1_21_5 && zero_bytes_count > 0 {
        blocks_and_biomes_buf.resize(blocks_and_biomes_buf.len() + zero_bytes_count, 0);
    }

    write.write_var_int(&blocks_and_biomes_buf.len().try_into().map_err(|_| {
        WritingError::Message(format!(
            "{} is not representable as a VarInt!",
            blocks_and_biomes_buf.len()
        ))
    })?)?;
    write.write_slice(&blocks_and_biomes_buf)
}

/// The id of a block entity's type in the registry; 0 when it is unknown.
fn block_entity_type_id(nbt: &pumpkin_nbt::compound::NbtCompound) -> usize {
    nbt.get_string("id").map_or(0, |id_str| {
        let name = id_str.split(':').next_back().unwrap_or(id_str);
        pumpkin_data::block_properties::BLOCK_ENTITY_TYPES
            .iter()
            .position(|&n| n == name)
            .unwrap_or(0)
    })
}

/// What the client gets of a block entity: its data without the keys the server keeps.
fn client_block_entity_nbt(
    nbt: &pumpkin_nbt::compound::NbtCompound,
) -> pumpkin_nbt::compound::NbtCompound {
    let mut client_nbt = nbt.clone();
    for key in [
        "id",
        "x",
        "y",
        "z",
        "LootTable",
        "LootTableSeed",
        "PumpkinCustomData",
        "BukkitValues",
    ] {
        client_nbt.child_tags.remove(key);
    }
    client_nbt
}

fn write_block_entities(
    chunk: &ChunkData,
    write: &mut impl Write,
    version: JavaMinecraftVersion,
) -> Result<(), WritingError> {
    let block_entities = chunk
        .pending_block_entities
        .lock()
        .map_err(|_| WritingError::Message("block_entities lock poisoned".into()))?;
    write.write_var_int(&VarInt(block_entities.len() as i32))?;
    for (pos, nbt) in block_entities.iter() {
        let local_xz = ((get_local_cord(pos.0.x) & 0xF) << 4) | (get_local_cord(pos.0.z) & 0xF);

        write.write_u8(local_xz as u8)?;
        write.write_i16_be(pos.0.y as i16)?;
        write.write_var_int(&VarInt(block_entity_type_id(nbt) as i32))?;
        write_compound_nbt(&mut *write, &client_block_entity_nbt(nbt), version)?;
    }
    Ok(())
}

/// Which sections have light and which are known to have none, as the bit masks the packet uses.
/// Bit 0 is the section below the world and bit `n + 1` the section above it; both are always
/// empty.
struct LightMasks {
    sky: u64,
    block: u64,
    empty_sky: u64,
    empty_block: u64,
}

impl LightMasks {
    fn of(sky_light: &[LightContainer], block_light: &[LightContainer]) -> Self {
        let num_sections = sky_light.len();
        let mut masks = Self {
            sky: 0,
            block: 0,
            // The sections below and above the world.
            empty_sky: 1 | (1 << (num_sections + 1)),
            empty_block: 1 | (1 << (num_sections + 1)),
        };
        // Bits 1..=num_sections represent the actual world sections
        for section_index in 0..num_sections {
            let bit_index = section_index + 1;

            if let LightContainer::Full(_) = &sky_light[section_index] {
                masks.sky |= 1 << bit_index;
            } else {
                masks.empty_sky |= 1 << bit_index;
            }

            if let LightContainer::Full(_) = &block_light[section_index] {
                masks.block |= 1 << bit_index;
            } else {
                masks.empty_block |= 1 << bit_index;
            }
        }
        masks
    }
}

/// The light arrays of the sections whose bit is set: their count, then each array.
fn write_light_arrays(
    write: &mut impl Write,
    light: &[LightContainer],
    mask: u64,
) -> Result<(), WritingError> {
    let light_data_size = VarInt(LightContainer::ARRAY_SIZE as i32);
    write.write_var_int(&VarInt(mask.count_ones() as i32))?;
    for container in light {
        if let LightContainer::Full(data) = container {
            write.write_var_int(&light_data_size)?;
            write.write_slice(data.as_ref())?;
        }
    }
    Ok(())
}

fn write_light(
    chunk: &ChunkData,
    write: &mut impl Write,
    version: JavaMinecraftVersion,
) -> Result<(), WritingError> {
    // Light masks include sections from -1 (below world) to num_sections (above world)
    // This means we need to account for 2 extra sections in the bitset
    let light_engine = chunk
        .light_engine
        .lock()
        .map_err(|_| WritingError::Message("light_engine lock poisoned".into()))?;
    let num_sections = light_engine.sky_light.len();
    let masks = LightMasks::of(
        &light_engine.sky_light,
        &light_engine.block_light[..num_sections],
    );

    // Trust edges (1.18 - 1.19.4; removed in 1.20)
    if version < JavaMinecraftVersion::V_1_20 {
        write.write_bool(true)?;
    }

    // Sky light mask, block light mask, empty sky light mask, empty block light mask
    for mask in [masks.sky, masks.block, masks.empty_sky, masks.empty_block] {
        BitSet(Box::new([mask as i64])).encode_with_version(&mut *write, &version)?;
    }

    write_light_arrays(write, &light_engine.sky_light, masks.sky)?;
    write_light_arrays(
        write,
        &light_engine.block_light[..num_sections],
        masks.block,
    )
}
