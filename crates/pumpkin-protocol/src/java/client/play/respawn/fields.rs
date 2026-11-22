//! The field groups `CRespawn::read` decodes: one reader per wire layout and one per field group
//! that is gated by a protocol version.

use pumpkin_data::dimension::Dimension;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::resource_location::ResourceLocation;
use pumpkin_util::version::JavaMinecraftVersion;

use super::CRespawn;
use crate::VarInt;
use crate::java::client::play::player_spawn_data::PlayerSpawnData;
use crate::ser::{NetworkReadExt, ReadingError};

/// The layout before 1.16: a legacy dimension id, the level type as text and no previous game
/// mode, death location or cooldown.
pub(super) fn read_before_1_16(
    read: &mut &[u8],
    version: JavaMinecraftVersion,
) -> Result<CRespawn, ReadingError> {
    let legacy_dim_id = read.get_i32_be()?;
    let dimension = match legacy_dim_id {
        -1 => Dimension::THE_NETHER,
        1 => Dimension::THE_END,
        _ => Dimension::OVERWORLD,
    };
    let hashed_seed = if version >= JavaMinecraftVersion::V_1_15 {
        read.get_i64_be()?
    } else {
        0
    };
    if version < JavaMinecraftVersion::V_1_14 {
        let _difficulty = read.get_u8()?;
    }
    let game_mode = read.get_u8()?;
    let level_type = read.get_str()?;
    let is_flat = &*level_type == "flat";
    let debug = &*level_type == "debug_all_block_states";

    let player_spawn_info = PlayerSpawnData::new(
        dimension,
        hashed_seed,
        game_mode,
        -1,
        debug,
        is_flat,
        None,
        VarInt(0),
        VarInt(63),
    );
    Ok(CRespawn::new(player_spawn_info, CRespawn::KEEP_ALL_DATA))
}

/// The layout from 1.16 up to 1.20: the dimension (name, or NBT in 1.16.2 to 1.18.2) comes
/// first and the keep flag sits before the death location.
pub(super) fn read_before_1_20_2(
    read: &mut &[u8],
    version: JavaMinecraftVersion,
) -> Result<CRespawn, ReadingError> {
    let dimension = read_dimension(read, version)?;
    let _world_name = read.get_str()?;
    let hashed_seed = read.get_i64_be()?;
    let game_mode = read.get_u8()?;
    let previous_gamemode = read.get_i8()?;
    let debug = read.get_bool()?;
    let is_flat = read.get_bool()?;
    let data_kept = read_data_kept(read, version)?;
    let death_dimension_name = read_death_location(read, version)?;
    let portal_cooldown = if version >= JavaMinecraftVersion::V_1_20 {
        read.get_var_int()?
    } else {
        VarInt(0)
    };

    let player_spawn_info = PlayerSpawnData::new(
        dimension,
        hashed_seed,
        game_mode,
        previous_gamemode,
        debug,
        is_flat,
        death_dimension_name,
        portal_cooldown,
        VarInt(63),
    );
    Ok(CRespawn::new(player_spawn_info, data_kept))
}

/// The dimension of the 1.16 to 1.20 layout: an NBT dimension type in 1.16.2 to 1.18.2 (skipped,
/// the dimension is then the overworld), a name otherwise.
fn read_dimension(
    read: &mut &[u8],
    version: JavaMinecraftVersion,
) -> Result<Dimension, ReadingError> {
    if version >= JavaMinecraftVersion::V_1_16_2 && version < JavaMinecraftVersion::V_1_19 {
        let mut cursor = std::io::Cursor::new(*read);
        let mut reader = pumpkin_nbt::deserializer::NbtReadHelperJava::new(&mut cursor);
        let _nbt = pumpkin_nbt::Nbt::read(&mut reader)
            .map_err(|e| ReadingError::Message(format!("Invalid dimension type NBT: {e}")))?;
        let bytes_read = cursor.position() as usize;
        *read = &read[bytes_read..];
        Ok(Dimension::OVERWORLD)
    } else {
        let dim_name = read.get_str()?;
        Ok(Dimension::from_name(&dim_name)
            .cloned()
            .unwrap_or(Dimension::OVERWORLD))
    }
}

/// The keep flag: a bit set from 1.19.3, one bool before (true keeps everything).
fn read_data_kept(read: &mut &[u8], version: JavaMinecraftVersion) -> Result<u8, ReadingError> {
    Ok(if version >= JavaMinecraftVersion::V_1_19_3 {
        read.get_u8()?
    } else if read.get_bool()? {
        CRespawn::KEEP_ALL_DATA
    } else {
        CRespawn::KEEP_ENTITY_DATA
    })
}

/// The last death location, present from 1.19.
fn read_death_location(
    read: &mut &[u8],
    version: JavaMinecraftVersion,
) -> Result<Option<(ResourceLocation, BlockPos)>, ReadingError> {
    if version < JavaMinecraftVersion::V_1_19 || !read.get_bool()? {
        return Ok(None);
    }
    let dim = read.get_str()?.into();
    let pos = read.get_block_pos(&version)?;
    Ok(Some((dim, pos)))
}
