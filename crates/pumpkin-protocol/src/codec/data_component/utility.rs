use super::{
    DataComponentCodec, deserialize_item_stack_template, read_count, serialize_item_stack_template,
};
use crate::codec::var_int::VarInt;
use crate::ser::{NetworkReadExt, NetworkWriteExt, ReadingError, WritingError};
use pumpkin_data::data_component_impl::{
    BlockTransformerImpl, BrewingFuelImpl, BundleContentsImpl, ChargedProjectilesImpl,
    CompostableImpl, CookingFuelImpl, DyeImpl, DyedColorImpl, FireworkExplosionImpl,
    FireworkExplosionShape, FireworksImpl, JukeboxPlayableImpl, LodestoneTrackerImpl,
    MapDecorationsImpl, MapIdImpl, MapPostProcessingImpl, ProfileImpl, RecipesImpl, WaxedImpl,
};
use pumpkin_data::sound::Sound;
use pumpkin_util::version::JavaMinecraftVersion;
use std::borrow::Cow;

impl DataComponentCodec<Self> for DyedColorImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_i32(self.rgb)
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        Ok(Self {
            rgb: seq.get_i32()?,
        })
    }
}

impl DataComponentCodec<Self> for FireworkExplosionImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        // Shape (VarInt enum)
        seq.write_var_int(&VarInt::from(self.shape.to_id()))?;
        // Colors list
        seq.write_var_int(&VarInt::from(self.colors.len() as i32))?;
        for color in &self.colors {
            seq.write_i32(*color)?;
        }
        // Fade colors list
        seq.write_var_int(&VarInt::from(self.fade_colors.len() as i32))?;
        for color in &self.fade_colors {
            seq.write_i32(*color)?;
        }
        // hasTrail
        seq.write_bool(self.has_trail)?;
        // hasTwinkle
        seq.write_bool(self.has_twinkle)?;
        Ok(())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        // Needs a length cap during deserialization to prevent OOM from malicious packets
        // Vanilla doesn't have any limits (Integer.MAX_VALUE is technically a limit but not enforced in practice)
        const MAX_COLORS: usize = 256;
        const MAX_FADE_COLORS: usize = 256;

        // Shape (VarInt enum)
        let shape_id = seq.get_var_int()?.0;
        let shape = FireworkExplosionShape::from_id(shape_id).ok_or(ReadingError::Message(
            "Invalid FireworkExplosionShape id!".into(),
        ))?;

        // Colors list
        let colors_len = seq.get_var_int()?.0 as usize;
        if colors_len > MAX_COLORS {
            return Err(ReadingError::Message(format!(
                "FireworkExplosionImpl colors_len {colors_len} exceeds maximum of {MAX_COLORS}"
            )));
        }
        let mut colors = Vec::with_capacity(colors_len);
        for _ in 0..colors_len {
            let color = seq.get_i32()?;
            colors.push(color);
        }

        // Fade colors list
        let fade_colors_len = seq.get_var_int()?.0 as usize;
        if fade_colors_len > MAX_FADE_COLORS {
            return Err(ReadingError::Message(format!(
                "FireworkExplosionImpl fade_colors_len {fade_colors_len} exceeds maximum of {MAX_FADE_COLORS}"
            )));
        }
        let mut fade_colors = Vec::with_capacity(fade_colors_len);
        for _ in 0..fade_colors_len {
            let color = seq.get_i32()?;
            fade_colors.push(color);
        }

        // hasTrail
        let has_trail = seq.get_bool()?;

        // hasTwinkle
        let has_twinkle = seq.get_bool()?;

        Ok(Self::new(
            shape,
            colors,
            fade_colors,
            has_trail,
            has_twinkle,
        ))
    }
}

impl DataComponentCodec<Self> for FireworksImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        // Flight duration (VarInt)
        seq.write_var_int(&VarInt::from(self.flight_duration))?;
        // Explosions list
        seq.write_var_int(&VarInt::from(self.explosions.len() as i32))?;
        for explosion in &self.explosions {
            explosion.serialize(seq)?;
        }
        Ok(())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        // Needs a length cap during deserialization to prevent OOM from malicious packets
        // Vanilla doesn't have any limits
        const MAX_EXPLOSIONS: usize = 256;
        // Vanilla restricts to 0-255 (UNSIGNED_BYTE in data component codec) (do not trust client NBT to limit it)
        const MAX_FLIGHT_DURATION: i32 = 255;

        // Flight duration
        let flight_duration = seq.get_var_int()?.0;
        if !(0..=MAX_FLIGHT_DURATION).contains(&flight_duration) {
            return Err(ReadingError::Message(format!(
                "FireworksImpl flight_duration {flight_duration} is out of bounds (0-{MAX_FLIGHT_DURATION})"
            )));
        }

        // Explosions list
        let explosions_len = seq.get_var_int()?.0 as usize;
        if explosions_len > MAX_EXPLOSIONS {
            return Err(ReadingError::Message(format!(
                "FireworksImpl explosions_len {explosions_len} exceeds maximum of {MAX_EXPLOSIONS}"
            )));
        }
        let mut explosions = Vec::with_capacity(explosions_len);
        for _ in 0..explosions_len {
            // Recursively deserialize each explosion
            let explosion = FireworkExplosionImpl::deserialize(seq)?;
            explosions.push(explosion);
        }

        Ok(Self::new(flight_duration, explosions))
    }
}

impl DataComponentCodec<Self> for MapIdImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.id))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let id = seq.get_var_int()?.0;
        Ok(Self { id })
    }
}

impl DataComponentCodec<Self> for BundleContentsImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.items.len() as i32))?;
        for item in &self.items {
            serialize_item_stack_template(item, seq)?;
        }
        Ok(())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        const MAX_BUNDLE_ITEMS: usize = 64;

        let len = seq.get_var_int()?.0 as usize;

        if len > MAX_BUNDLE_ITEMS {
            return Err(ReadingError::Message(
                "Too many items in BundleContents".into(),
            ));
        }

        let mut items = Vec::with_capacity(len);
        for _ in 0..len {
            items.push(deserialize_item_stack_template(seq)?);
        }
        Ok(Self { items })
    }
}

impl DataComponentCodec<Self> for DyeImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.color.id() as i32))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let id = seq.get_var_int()?.0;
        let color = u8::try_from(id)
            .ok()
            .and_then(pumpkin_data::dye_color::DyeColor::by_id)
            .ok_or_else(|| ReadingError::Message(format!("Invalid DyeColor id {id}")))?;
        Ok(Self { color })
    }
}

impl DataComponentCodec<Self> for MapDecorationsImpl {
    fn serialize(&self, _seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        Ok(())
    }

    fn deserialize(_seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        Ok(Self)
    }
}

impl DataComponentCodec<Self> for MapPostProcessingImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.processing.map_or(0, |p| p as i32)))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let val = seq.get_var_int()?.0;
        let processing = match val {
            0 => Some(pumpkin_data::data_component_impl::MapPostProcessing::Lock),
            1 => Some(pumpkin_data::data_component_impl::MapPostProcessing::Scale),
            _ => None,
        };
        Ok(Self { processing })
    }
}

impl DataComponentCodec<Self> for ChargedProjectilesImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.projectiles.len() as i32))?;
        for _ in &self.projectiles {
            seq.write_var_int(&VarInt(0))?;
            seq.write_var_int(&VarInt(0))?;
            seq.write_var_int(&VarInt(0))?;
            seq.write_var_int(&VarInt(0))?;
        }
        Ok(())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        const MAX_PROJECTILES: usize = 1024;
        let len = read_count(seq, MAX_PROJECTILES)?;
        let mut projectiles = Vec::with_capacity(len);
        for _ in 0..len {
            let _ = deserialize_item_stack_template(seq)?;
            projectiles.push(pumpkin_nbt::compound::NbtCompound::new());
        }
        Ok(Self { projectiles })
    }
}

impl DataComponentCodec<Self> for JukeboxPlayableImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        let song_id = Sound::from_name(self.song).map_or(0, |s| s as i32);
        seq.write_var_int(&VarInt::from(song_id))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let _ = seq.get_var_int()?;
        Ok(Self { song: "" })
    }
}

impl DataComponentCodec<Self> for RecipesImpl {
    fn serialize(&self, _seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        Ok(())
    }

    fn deserialize(_seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        Ok(Self {
            recipes: Cow::Borrowed(&[]),
        })
    }
}

impl DataComponentCodec<Self> for LodestoneTrackerImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        if let Some(target) = &self.target {
            seq.write_bool(true)?;
            seq.write_string(&target.dimension)?;
            let pos = pumpkin_util::math::position::BlockPos::new(target.x, target.y, target.z);
            seq.write_block_pos(&pos, &JavaMinecraftVersion::V_26_2)?;
        } else {
            seq.write_bool(false)?;
        }
        seq.write_bool(self.tracked)
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let target = if seq.get_bool()? {
            let dimension = seq.get_str()?.to_string();
            let pos = seq.get_block_pos(&JavaMinecraftVersion::V_26_2)?;
            Some(pumpkin_data::data_component_impl::LodestoneTarget {
                dimension,
                x: pos.0.x,
                y: pos.0.y,
                z: pos.0.z,
            })
        } else {
            None
        };
        let tracked = seq.get_bool()?;
        Ok(Self { target, tracked })
    }
}

impl DataComponentCodec<Self> for ProfileImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_bool(false)?;
        if let Some(name) = &self.name {
            seq.write_bool(true)?;
            seq.write_string(name)?;
        } else {
            seq.write_bool(false)?;
        }
        if let Some(id) = &self.id {
            seq.write_bool(true)?;
            let uuid = uuid::Uuid::from_u128(
                ((id[0] as u128) << 96)
                    | ((id[1] as u128 & 0xFFFFFFFF) << 64)
                    | ((id[2] as u128 & 0xFFFFFFFF) << 32)
                    | (id[3] as u128 & 0xFFFFFFFF),
            );
            seq.write_uuid(&uuid)?;
        } else {
            seq.write_bool(false)?;
        }
        seq.write_var_int(&VarInt::from(self.properties.len() as i32))?;
        for prop in &self.properties {
            seq.write_string(&prop.name)?;
            seq.write_string(&prop.value)?;
            if let Some(sig) = &prop.signature {
                seq.write_bool(true)?;
                seq.write_string(sig)?;
            } else {
                seq.write_bool(false)?;
            }
        }
        if let Some(texture) = &self.texture {
            seq.write_bool(true)?;
            seq.write_string(texture)?;
        } else {
            seq.write_bool(false)?;
        }
        if let Some(cape) = &self.cape {
            seq.write_bool(true)?;
            seq.write_string(cape)?;
        } else {
            seq.write_bool(false)?;
        }
        if let Some(elytra) = &self.elytra {
            seq.write_bool(true)?;
            seq.write_string(elytra)?;
        } else {
            seq.write_bool(false)?;
        }
        if self.model.is_some() {
            seq.write_bool(true)?;
            seq.write_var_int(&VarInt(0))?;
        } else {
            seq.write_bool(false)?;
        }
        Ok(())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let full_profile = seq.get_bool()?;
        let mut name = None;
        let mut id = None;
        let mut properties = Vec::new();
        if full_profile {
            let uuid = seq.get_uuid()?;
            let u = uuid.as_u128();
            id = Some([
                (u >> 96) as i32,
                (u >> 64) as i32,
                (u >> 32) as i32,
                u as i32,
            ]);
            name = Some(seq.get_str()?.to_string());
        } else {
            if seq.get_bool()? {
                name = Some(seq.get_str()?.to_string());
            }
            if seq.get_bool()? {
                let uuid = seq.get_uuid()?;
                let u = uuid.as_u128();
                id = Some([
                    (u >> 96) as i32,
                    (u >> 64) as i32,
                    (u >> 32) as i32,
                    u as i32,
                ]);
            }
        }
        let props_len = seq.get_var_int()?.0 as usize;
        for _ in 0..props_len {
            let prop_name = seq.get_str()?.to_string();
            let prop_value = seq.get_str()?.to_string();
            let sig = if seq.get_bool()? {
                Some(seq.get_str()?.to_string())
            } else {
                None
            };
            properties.push(pumpkin_data::data_component_impl::ProfileProperty {
                name: prop_name,
                value: prop_value,
                signature: sig,
            });
        }
        let texture = if seq.get_bool()? {
            Some(seq.get_str()?.to_string())
        } else {
            None
        };
        let cape = if seq.get_bool()? {
            Some(seq.get_str()?.to_string())
        } else {
            None
        };
        let elytra = if seq.get_bool()? {
            Some(seq.get_str()?.to_string())
        } else {
            None
        };
        let model = if seq.get_bool()? {
            let _ = seq.get_var_int()?;
            Some("wide".to_string())
        } else {
            None
        };
        Ok(Self {
            name,
            id,
            properties,
            texture,
            cape,
            elytra,
            model,
        })
    }
}

impl DataComponentCodec<Self> for BlockTransformerImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        let id = match self.transformer.as_ref() {
            "minecraft:hoe" | "hoe" => 1,
            "minecraft:shovel" | "shovel" => 2,
            _ => 0,
        };
        seq.write_var_int(&VarInt(id))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let id = seq.get_var_int()?.0;
        let transformer = match id {
            0 => Cow::Borrowed("minecraft:axe"),
            1 => Cow::Borrowed("minecraft:hoe"),
            2 => Cow::Borrowed("minecraft:shovel"),
            _ => {
                return Err(ReadingError::Message(format!(
                    "Invalid BlockTransformer id {id}"
                )));
            }
        };
        Ok(Self { transformer })
    }
}

impl DataComponentCodec<Self> for CompostableImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        let layers = match self.chance {
            0.3 => "minecraft:compostable/low",
            0.5 => "minecraft:compostable/low_medium",
            0.65 => "minecraft:compostable/medium",
            0.85 => "minecraft:compostable/medium_high",
            _ => "minecraft:compostable/always_add_one",
        };
        seq.write_bool(false)?;
        seq.write_string(layers)
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let is_direct = seq.get_bool()?;
        let chance = if is_direct {
            let val = seq.get_i32()?;
            if val > 0 { 1.0 } else { 0.3 }
        } else {
            let key = seq.get_str()?;
            match key.strip_prefix("minecraft:").unwrap_or(&key) {
                "compostable/low" => 0.3,
                "compostable/low_medium" => 0.5,
                "compostable/medium" => 0.65,
                "compostable/medium_high" => 0.85,
                _ => 1.0,
            }
        };
        Ok(Self { chance })
    }
}

impl DataComponentCodec<Self> for CookingFuelImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        match &self.burn_time {
            pumpkin_data::data_component_impl::IntProvider::Inline(val) => {
                seq.write_bool(true)?;
                seq.write_i32(*val)?;
            }
            pumpkin_data::data_component_impl::IntProvider::Id(id) => {
                seq.write_bool(false)?;
                seq.write_string(id)?;
            }
        }
        match &self.speed_multiplier {
            pumpkin_data::data_component_impl::FloatProvider::Inline(val) => {
                seq.write_bool(true)?;
                seq.write_f32(*val)
            }
            pumpkin_data::data_component_impl::FloatProvider::Id(id) => {
                seq.write_bool(false)?;
                seq.write_string(id)
            }
        }
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let burn_time = if seq.get_bool()? {
            pumpkin_data::data_component_impl::IntProvider::Inline(seq.get_i32()?)
        } else {
            pumpkin_data::data_component_impl::IntProvider::Id(Cow::Owned(
                seq.get_str()?.to_string(),
            ))
        };
        let speed_multiplier = if seq.get_bool()? {
            pumpkin_data::data_component_impl::FloatProvider::Inline(seq.get_f32()?)
        } else {
            pumpkin_data::data_component_impl::FloatProvider::Id(Cow::Owned(
                seq.get_str()?.to_string(),
            ))
        };
        Ok(Self {
            burn_time,
            speed_multiplier,
        })
    }
}

impl DataComponentCodec<Self> for BrewingFuelImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_bool(false)?;
        seq.write_string("minecraft:brewing/uses_default")?;
        seq.write_bool(false)?;
        seq.write_string("minecraft:brewing/speed_default")
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        if seq.get_bool()? {
            let _uses = seq.get_i32()?;
        } else {
            let _uses_key = seq.get_str()?;
        }
        if seq.get_bool()? {
            let _speed = seq.get_f32()?;
        } else {
            let _speed_key = seq.get_str()?;
        }
        Ok(Self)
    }
}

impl DataComponentCodec<Self> for WaxedImpl {
    fn serialize(&self, _seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        Ok(())
    }

    fn deserialize(_seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        Ok(Self)
    }
}
