use super::{
    DataComponentCodec, MAX_INITIAL_COLLECTION_SIZE, data_to_proto_sound, proto_to_data_sound,
    read_count,
};
use crate::codec::var_int::VarInt;
use crate::ser::{NetworkReadExt, NetworkWriteExt, ReadingError, WritingError};
use pumpkin_data::data_component_impl::{
    BannerPatternsImpl, BaseColorImpl, BreakSoundImpl, CreativeSlotLockImpl, CushionColorImpl,
    CustomDataImpl, CustomModelDataImpl, CustomNameImpl, DamageImpl, EnchantmentGlintOverrideImpl,
    InstrumentImpl, ItemModelImpl, ItemNameImpl, LockImpl, LoreImpl, MaxDamageImpl,
    MaxStackSizeImpl, NoteBlockSoundImpl, PotDecorationsImpl, ProvidesBannerPatternsImpl,
    ProvidesPotteryPatternImpl, ProvidesTrimMaterialImpl, Rarity, RarityImpl, TooltipDisplayImpl,
    TooltipStyleImpl, UnbreakableImpl,
};
use pumpkin_nbt::{serializer::NbtWriteHelperJava, tag::NbtTag};
use pumpkin_util::version::JavaMinecraftVersion;
use std::borrow::Cow;

impl DataComponentCodec<Self> for MaxStackSizeImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.size))
    }
    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let size = u8::try_from(seq.get_var_int()?.0)
            .map_err(|_| ReadingError::Message("No MaxStackSize VarInt!".into()))?;
        Ok(Self { size })
    }
}

impl DataComponentCodec<Self> for DamageImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.damage))
    }
    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let damage = seq.get_var_int()?.0;
        Ok(Self { damage })
    }
}

impl DataComponentCodec<Self> for UnbreakableImpl {
    fn serialize(&self, _seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        Ok(())
    }
    fn deserialize(_seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        Ok(Self)
    }
}

impl DataComponentCodec<Self> for ItemModelImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_string(&self.id)
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let id = seq.get_str()?;
        Ok(Self {
            id: Cow::Owned(id.into()),
        })
    }
}

impl DataComponentCodec<Self> for CustomNameImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        let mut bytes = Vec::new();
        NbtTag::String(self.name.clone().get_text().into_boxed_str())
            .serialize(&mut NbtWriteHelperJava::new(&mut bytes))
            .map_err(|e| WritingError::Message(e.to_string()))?;
        seq.write_slice(&bytes)?;
        Ok(())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let tag = seq.get_nbt_with_version(&pumpkin_util::version::JavaMinecraftVersion::V_26_2)?;
        let name = tag.as_ref().map_or_else(
            pumpkin_util::text::TextComponent::empty,
            pumpkin_util::text::TextComponent::from_nbt,
        );
        Ok(Self { name })
    }
}

impl DataComponentCodec<Self> for LoreImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt(self.lines.len() as i32))?;
        for line in &self.lines {
            seq.write_slice(
                &line.encode_for_version(&pumpkin_util::version::JavaMinecraftVersion::V_26_2),
            )?;
        }
        Ok(())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        // TODO: Could probably be extracted?
        const MAX_LORE_LINES: i32 = 256;

        let count = seq.get_var_int()?.0;
        if !(0..=MAX_LORE_LINES).contains(&count) {
            return Err(ReadingError::Message(format!(
                "LoreImpl line count {count} is out of bounds (0-{MAX_LORE_LINES})"
            )));
        }

        let mut lines = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let tag =
                seq.get_nbt_with_version(&pumpkin_util::version::JavaMinecraftVersion::V_26_2)?;
            let text = tag.as_ref().map_or_else(
                pumpkin_util::text::TextComponent::empty,
                pumpkin_util::text::TextComponent::from_nbt,
            );
            lines.push(text);
        }
        Ok(Self { lines })
    }
}

impl DataComponentCodec<Self> for ItemNameImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        let mut name = pumpkin_nbt::compound::NbtCompound::new();
        name.put_string("translate", self.name.to_string());
        let mut bytes = Vec::new();
        NbtTag::Compound(name)
            .serialize(&mut NbtWriteHelperJava::new(&mut bytes))
            .map_err(|error| WritingError::Message(error.to_string()))?;
        seq.write_slice(&bytes)
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let tag = seq.get_nbt_with_version(&pumpkin_util::version::JavaMinecraftVersion::V_26_2)?;
        let name = match tag {
            Some(NbtTag::Compound(c)) => c
                .get_string("translate")
                .or_else(|| c.get_string("text"))
                .unwrap_or("")
                .to_string(),
            Some(NbtTag::String(s)) => s.to_string(),
            Some(t) => pumpkin_util::text::TextComponent::from_nbt(&t).get_text(),
            None => String::new(),
        };
        Ok(Self {
            name: Cow::Owned(name),
        })
    }
}

impl DataComponentCodec<Self> for CustomDataImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        let mut bytes = Vec::new();
        NbtTag::Compound(self.data.clone())
            .serialize(&mut NbtWriteHelperJava::new(&mut bytes))
            .map_err(|e| WritingError::Message(e.to_string()))?;
        seq.write_slice(&bytes)?;
        Ok(())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let data = seq
            .get_compound_nbt_with_version(&pumpkin_util::version::JavaMinecraftVersion::V_26_2)?
            .unwrap_or_else(pumpkin_nbt::compound::NbtCompound::new);
        Ok(Self { data })
    }
}

impl DataComponentCodec<Self> for RarityImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.rarity.to_id()))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let id = seq.get_var_int()?.0;
        let rarity = Rarity::from_id(id)
            .ok_or_else(|| ReadingError::Message(format!("Invalid Rarity id {id}")))?;
        Ok(Self { rarity })
    }
}

impl DataComponentCodec<Self> for MaxDamageImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.max_damage))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let max_damage = seq.get_var_int()?.0;
        Ok(Self { max_damage })
    }
}

impl DataComponentCodec<Self> for CustomModelDataImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.floats.len() as i32))?;
        for f in &self.floats {
            seq.write_f32(*f)?;
        }
        seq.write_var_int(&VarInt::from(self.flags.len() as i32))?;
        for b in &self.flags {
            seq.write_bool(*b)?;
        }
        seq.write_var_int(&VarInt::from(self.strings.len() as i32))?;
        for s in &self.strings {
            seq.write_string(s)?;
        }
        seq.write_var_int(&VarInt::from(self.colors.len() as i32))?;
        for c in &self.colors {
            seq.write_i32(*c)?;
        }
        Ok(())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let floats_len = read_count(seq, usize::MAX)?;
        let mut floats = Vec::with_capacity(floats_len.min(MAX_INITIAL_COLLECTION_SIZE));
        for _ in 0..floats_len {
            floats.push(seq.get_f32()?);
        }
        let flags_len = read_count(seq, usize::MAX)?;
        let mut flags = Vec::with_capacity(flags_len.min(MAX_INITIAL_COLLECTION_SIZE));
        for _ in 0..flags_len {
            flags.push(seq.get_bool()?);
        }
        let strings_len = read_count(seq, usize::MAX)?;
        let mut strings = Vec::with_capacity(strings_len.min(MAX_INITIAL_COLLECTION_SIZE));
        for _ in 0..strings_len {
            strings.push(seq.get_str()?.to_string());
        }
        let colors_len = read_count(seq, usize::MAX)?;
        let mut colors = Vec::with_capacity(colors_len.min(MAX_INITIAL_COLLECTION_SIZE));
        for _ in 0..colors_len {
            colors.push(seq.get_i32()?);
        }
        Ok(Self {
            floats,
            flags,
            strings,
            colors,
        })
    }
}

impl DataComponentCodec<Self> for TooltipDisplayImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_bool(false)?;
        seq.write_var_int(&VarInt(0))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let _hide_tooltip = seq.get_bool()?;
        let len = seq.get_var_int()?.0 as usize;
        for _ in 0..len {
            let _comp_id = seq.get_var_int()?;
        }
        Ok(Self)
    }
}

impl DataComponentCodec<Self> for CreativeSlotLockImpl {
    fn serialize(&self, _seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        Ok(())
    }

    fn deserialize(_seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        Ok(Self)
    }
}

impl DataComponentCodec<Self> for EnchantmentGlintOverrideImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_bool(self.glint)
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let glint = seq.get_bool()?;
        Ok(Self { glint })
    }
}

impl DataComponentCodec<Self> for TooltipStyleImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_string(&self.id)
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let id = seq.get_str()?.to_string();
        Ok(Self { id })
    }
}

impl DataComponentCodec<Self> for InstrumentImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        let id = pumpkin_data::instrument::Instrument::from_name(&self.instrument)
            .map_or(1, |inst| inst.id() as i32 + 1);
        seq.write_var_int(&VarInt(id))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let raw_id = seq.get_var_int()?.0;
        let instrument = if raw_id <= 0 {
            let _sound = crate::IdOr::<crate::SoundEvent>::read(seq, |r| {
                let sound_name = r.get_str()?.to_string();
                let range = r.get_option(NetworkReadExt::get_f32)?;
                Ok(crate::SoundEvent { sound_name, range })
            })
            .map_err(|e| ReadingError::Message(format!("No sound: {e}")))?;
            let _use_duration = seq.get_f32()?;
            let _range = seq.get_f32()?;
            let _description = seq.get_nbt_with_version(&JavaMinecraftVersion::V_26_2)?;
            Cow::Borrowed("minecraft:ponder_goat_horn")
        } else {
            let idx = (raw_id - 1) as usize;
            pumpkin_data::instrument::Instrument::all()
                .get(idx)
                .map_or(Cow::Borrowed("minecraft:ponder_goat_horn"), |inst| {
                    Cow::Borrowed(inst.asset_id())
                })
        };
        Ok(Self { instrument })
    }
}

impl DataComponentCodec<Self> for ProvidesTrimMaterialImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        let id = pumpkin_data::trim_material::TrimMaterial::from_name(&self.material)
            .map_or(1, |mat| mat.id() as i32 + 1);
        seq.write_var_int(&VarInt(id))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let raw_id = seq.get_var_int()?.0;
        let material = if raw_id <= 0 {
            let _base_asset = seq.get_str()?;
            let overrides_len = seq.get_var_int()?.0 as usize;
            for _ in 0..overrides_len.min(256) {
                let _key = seq.get_str()?;
                let _val = seq.get_str()?;
            }
            let _description = seq.get_nbt_with_version(&JavaMinecraftVersion::V_26_2)?;
            Cow::Borrowed("minecraft:quartz")
        } else {
            let idx = (raw_id - 1) as usize;
            pumpkin_data::trim_material::TrimMaterial::all()
                .get(idx)
                .map_or(Cow::Borrowed("minecraft:quartz"), |mat| {
                    Cow::Borrowed(mat.asset_id())
                })
        };
        Ok(Self { material })
    }
}

impl DataComponentCodec<Self> for ProvidesBannerPatternsImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_string(self.key.strip_prefix('#').unwrap_or(&self.key))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let tag = seq.get_str()?;
        let key = if tag.starts_with('#') {
            Cow::Owned(tag.to_string())
        } else {
            Cow::Owned(format!("#{tag}"))
        };
        Ok(Self { key })
    }
}

impl DataComponentCodec<Self> for NoteBlockSoundImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_string(&self.sound)
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let sound = seq.get_str()?.to_string();
        Ok(Self { sound })
    }
}

impl DataComponentCodec<Self> for BannerPatternsImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.layers.len() as i32))?;
        for layer in &self.layers {
            seq.write_var_int(&VarInt(0))?;
            seq.write_var_int(&VarInt::from(layer.color.id() as i32))?;
        }
        Ok(())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let len = read_count(seq, usize::MAX)?;
        let mut layers = Vec::with_capacity(len.min(MAX_INITIAL_COLLECTION_SIZE));
        for _ in 0..len {
            let _pattern = seq.get_var_int()?.0;
            let color_id = seq.get_var_int()?.0 as u8;
            let color = pumpkin_data::dye_color::DyeColor::by_id(color_id).unwrap_or_default();
            layers.push(pumpkin_data::data_component_impl::BannerPatternLayer {
                pattern: String::new(),
                color,
            });
        }
        Ok(Self { layers })
    }
}

impl DataComponentCodec<Self> for BaseColorImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        let color_id =
            pumpkin_data::dye_color::DyeColor::by_name(&self.color).map_or(0, |c| c.id() as i32);
        seq.write_var_int(&VarInt::from(color_id))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let id = seq.get_var_int()?.0 as u8;
        let color = pumpkin_data::dye_color::DyeColor::by_id(id)
            .map_or("white", |c| c.name())
            .to_string();
        Ok(Self { color })
    }
}

impl DataComponentCodec<Self> for PotDecorationsImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.sherds.len() as i32))?;
        for sherd in &self.sherds {
            let name = sherd.strip_prefix("minecraft:").unwrap_or(sherd);
            let id = pumpkin_data::item::Item::from_registry_key(name)
                .map_or(pumpkin_data::item::Item::BRICK.id as i32, |item| {
                    item.id as i32
                });
            seq.write_var_int(&VarInt::from(id))?;
        }
        Ok(())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let len = seq.get_var_int()?.0 as usize;
        let mut sherds = Vec::with_capacity(len.min(16));
        for _ in 0..len {
            let id = seq.get_var_int()?.0 as u16;
            let name = pumpkin_data::item::Item::from_id(id)
                .map_or("minecraft:brick", |item| item.registry_key);
            let full_name = if name.starts_with("minecraft:") {
                Cow::Borrowed(name)
            } else {
                Cow::Owned(format!("minecraft:{name}"))
            };
            sherds.push(full_name);
        }
        Ok(Self { sherds })
    }
}

impl DataComponentCodec<Self> for LockImpl {
    fn serialize(&self, _seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        Ok(())
    }

    fn deserialize(_seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        Ok(Self {
            predicate: pumpkin_nbt::compound::NbtCompound::new(),
        })
    }
}

impl DataComponentCodec<Self> for BreakSoundImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        crate::IdOr::<crate::SoundEvent>::write(&data_to_proto_sound(&self.sound), seq, |w, e| {
            w.write_string(&e.sound_name)?;
            w.write_option(&e.range, |w2, r| w2.write_f32(*r))
        })
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let proto = crate::IdOr::<crate::SoundEvent>::read(seq, |r| {
            let sound_name = r.get_str()?.into();
            let range = r.get_option(NetworkReadExt::get_f32)?;
            Ok(crate::SoundEvent { sound_name, range })
        })?;
        let sound = proto_to_data_sound(&proto)
            .ok_or_else(|| ReadingError::Message("Invalid sound in BreakSoundImpl".into()))?;
        Ok(Self { sound })
    }
}

impl DataComponentCodec<Self> for ProvidesPotteryPatternImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        let id = pumpkin_data::decorated_pot_pattern::DecoratedPotPattern::from_name(&self.pattern)
            .map_or(0, |p| p.id() as i32);
        seq.write_var_int(&VarInt(id))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let id = seq.get_var_int()?.0;
        let pattern = usize::try_from(id)
            .ok()
            .and_then(|idx| {
                pumpkin_data::decorated_pot_pattern::DecoratedPotPattern::all().get(idx)
            })
            .map_or(Cow::Borrowed("minecraft:angler"), |p| {
                Cow::Owned(format!("minecraft:{}", p.to_name()))
            });
        Ok(Self { pattern })
    }
}

impl DataComponentCodec<Self> for CushionColorImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.color.id() as i32))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let id = seq.get_var_int()?.0;
        let color = u8::try_from(id)
            .ok()
            .and_then(pumpkin_data::dye_color::DyeColor::by_id)
            .ok_or_else(|| ReadingError::Message(format!("Invalid CushionColor id {id}")))?;
        Ok(Self { color })
    }
}
