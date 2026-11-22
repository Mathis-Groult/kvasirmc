use pumpkin_data::packet::clientbound::play::RESPAWN;
use pumpkin_macros::java_packet;
use pumpkin_util::version::JavaMinecraftVersion;

use crate::{
    ClientPacket, ServerPacket,
    java::client::play::player_spawn_data::PlayerSpawnData,
    ser::{NetworkReadExt, NetworkWriteExt, ReadingError, WritingError},
};

#[derive(Clone, Debug, PartialEq, Eq)]
#[java_packet(RESPAWN)]
pub struct CRespawn {
    pub player_spawn_info: PlayerSpawnData,
    pub data_kept: u8,
}

impl CRespawn {
    pub const KEEP_NOTHING: u8 = 0;
    pub const KEEP_ATTRIBUTES: u8 = 0b01;
    pub const KEEP_ATTRIBUTE_MODIFIERS: u8 = Self::KEEP_ATTRIBUTES;
    pub const KEEP_ENTITY_DATA: u8 = 0b10;
    pub const KEEP_ALL_DATA: u8 = Self::KEEP_ATTRIBUTES | Self::KEEP_ENTITY_DATA;

    #[must_use]
    pub const fn new(player_spawn_info: PlayerSpawnData, data_kept: u8) -> Self {
        Self {
            player_spawn_info,
            data_kept,
        }
    }
}

impl ClientPacket for CRespawn {
    fn write_packet_data(
        &self,
        mut write: impl std::io::Write,
        version: &JavaMinecraftVersion,
    ) -> Result<(), WritingError> {
        let v1_14 = *version >= JavaMinecraftVersion::V_1_14;
        let v1_15 = *version >= JavaMinecraftVersion::V_1_15;
        let v1_16 = *version >= JavaMinecraftVersion::V_1_16;
        let v1_16_2 = *version >= JavaMinecraftVersion::V_1_16_2;
        let v1_19 = *version >= JavaMinecraftVersion::V_1_19;
        let v1_19_3 = *version >= JavaMinecraftVersion::V_1_19_3;
        let v1_20 = *version >= JavaMinecraftVersion::V_1_20;
        let v1_20_2 = *version >= JavaMinecraftVersion::V_1_20_2;

        if !v1_16 {
            let legacy_dim_id: i32 = match self.player_spawn_info.dimension.minecraft_name {
                "minecraft:the_nether" => -1,
                "minecraft:the_end" => 1,
                _ => 0,
            };
            write.write_i32_be(legacy_dim_id)?;
            if v1_15 {
                write.write_i64_be(self.player_spawn_info.hashed_seed)?;
            } else if !v1_14 {
                // Difficulty: 0: peaceful, 1: easy, 2: normal, 3: hard (default: 2 Normal)
                write.write_u8(2)?;
            }
            write.write_u8(self.player_spawn_info.game_mode)?;
            let level_type = if self.player_spawn_info.is_flat {
                "flat"
            } else if self.player_spawn_info.debug {
                "debug_all_block_states"
            } else {
                "default"
            };
            write.write_string(level_type)?;
            return Ok(());
        }

        if !v1_20_2 {
            if v1_16_2 && *version < JavaMinecraftVersion::V_1_19 {
                let dim_type_compound = crate::java::client::play::login::get_dimension_type_nbt(
                    *version,
                    self.player_spawn_info.dimension.minecraft_name,
                );
                let dim_bytes = pumpkin_nbt::Nbt::new(String::new(), dim_type_compound).write();
                write.write_all(&dim_bytes)?;
            } else {
                write.write_string(self.player_spawn_info.dimension.minecraft_name)?;
            }
            write.write_string(self.player_spawn_info.dimension.minecraft_name)?;
            write.write_i64_be(self.player_spawn_info.hashed_seed)?;
            write.write_u8(self.player_spawn_info.game_mode)?;
            write.write_i8(self.player_spawn_info.previous_gamemode)?;
            write.write_bool(self.player_spawn_info.debug)?;
            write.write_bool(self.player_spawn_info.is_flat)?;
            if v1_19_3 {
                write.write_u8(self.data_kept)?;
            } else {
                write.write_bool((self.data_kept & Self::KEEP_ATTRIBUTES) != 0)?;
            }
            if v1_19 {
                write.write_option(
                    &self.player_spawn_info.death_dimension_name,
                    |write, (dim, pos)| {
                        write.write_string(dim)?;
                        write.write_block_pos(pos, version)?;
                        Ok(())
                    },
                )?;
            }
            if v1_20 {
                write.write_var_int(&self.player_spawn_info.portal_cooldown)?;
            }
            return Ok(());
        }

        self.player_spawn_info
            .write_packet_data(&mut write, version)?;
        write.write_u8(self.data_kept)?;
        Ok(())
    }
}

impl<'a> ServerPacket<'a> for CRespawn {
    fn read(read: &mut &'a [u8], version: &JavaMinecraftVersion) -> Result<Self, ReadingError> {
        if *version < JavaMinecraftVersion::V_1_16 {
            return fields::read_before_1_16(read, *version);
        }
        if *version < JavaMinecraftVersion::V_1_20_2 {
            return fields::read_before_1_20_2(read, *version);
        }

        let player_spawn_info = PlayerSpawnData::read(read, version)?;
        let data_kept = read.get_u8()?;
        Ok(Self::new(player_spawn_info, data_kept))
    }
}

mod fields;

#[cfg(test)]
mod golden;
