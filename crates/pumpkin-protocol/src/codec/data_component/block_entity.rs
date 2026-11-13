use super::{
    DataComponentCodec, MAX_INITIAL_COLLECTION_SIZE, deserialize_item_stack_template, read_count,
    serialize_item_stack_template,
};
use crate::codec::var_int::VarInt;
use crate::ser::{NetworkReadExt, NetworkWriteExt, ReadingError, WritingError};
use pumpkin_data::data_component_impl::{
    BeesImpl, BlockEntityDataImpl, BlockStateImpl, BucketEntityDataImpl, ContainerImpl,
    ContainerLootImpl, EntityDataImpl, SignTextBackImpl, SignTextFrontImpl, SulfurCubeContentImpl,
};
use pumpkin_data::entity::EntityType;
use pumpkin_nbt::tag::NbtTag;
use pumpkin_util::version::JavaMinecraftVersion;
use std::borrow::Cow;

impl DataComponentCodec<Self> for EntityDataImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        let mut nbt = self.nbt.clone().unwrap_or_default();
        // Vanilla TypedEntityData always carries its type, there is no fallback.
        let id = nbt
            .get_string("id")
            .ok_or_else(|| WritingError::Message("entity_data has no 'id'".into()))?;
        let type_id = EntityType::from_name(id.strip_prefix("minecraft:").unwrap_or(id))
            .map(|entity_type| i32::from(entity_type.id))
            .ok_or_else(|| WritingError::Message(format!("Unknown entity type {id}")))?;
        nbt.child_tags.remove("id");
        seq.write_var_int(&VarInt(type_id))?;
        seq.write_nbt(NbtTag::Compound(nbt))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let type_id = seq.get_var_int()?.0;
        let tag = seq.get_nbt_with_version(&JavaMinecraftVersion::V_26_2)?;
        // Vanilla's TypedEntityData keeps the type apart from the tag.
        //Pumpkin keeps it as "id" in the NBT.
        let entity_type = u16::try_from(type_id)
            .ok()
            .and_then(EntityType::from_raw)
            .ok_or_else(|| ReadingError::Message(format!("Unknown entity type id {type_id}")))?;
        let mut nbt = match tag {
            Some(NbtTag::Compound(c)) => c,
            _ => pumpkin_nbt::compound::NbtCompound::new(),
        };
        nbt.put_string("id", format!("minecraft:{}", entity_type.resource_name));
        Ok(Self { nbt: Some(nbt) })
    }
}

impl DataComponentCodec<Self> for BucketEntityDataImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_nbt(NbtTag::Compound(self.nbt.clone().unwrap_or_default()))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let tag = seq.get_nbt_with_version(&JavaMinecraftVersion::V_26_2)?;
        let nbt = match tag {
            Some(NbtTag::Compound(c)) => Some(c),
            _ => None,
        };
        Ok(Self { nbt })
    }
}

impl DataComponentCodec<Self> for BlockEntityDataImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt(0))?;
        seq.write_nbt(NbtTag::Compound(self.nbt.clone()))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let _type_id = seq.get_var_int()?;
        let tag = seq.get_nbt_with_version(&JavaMinecraftVersion::V_26_2)?;
        let nbt = if let Some(NbtTag::Compound(c)) = tag {
            c
        } else {
            pumpkin_nbt::compound::NbtCompound::new()
        };
        Ok(Self { nbt })
    }
}

impl DataComponentCodec<Self> for ContainerImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.items.len() as i32))?;
        for (_slot, stack) in &self.items {
            seq.write_bool(true)?;
            serialize_item_stack_template(stack, seq)?;
        }
        Ok(())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        const MAX_SIZE: usize = 256;
        let len = read_count(seq, MAX_SIZE)?;
        let mut items = Vec::with_capacity(len);
        for slot in 0..len {
            if seq.get_bool()? {
                let stack = deserialize_item_stack_template(seq)?;
                items.push((slot as u8, stack));
            }
        }
        Ok(Self { items })
    }
}

impl DataComponentCodec<Self> for BlockStateImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.properties.len() as i32))?;
        for (k, v) in self.properties.iter() {
            seq.write_string(k)?;
            seq.write_string(v)?;
        }
        Ok(())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let len = read_count(seq, usize::MAX)?;
        let mut properties = Vec::with_capacity(len.min(MAX_INITIAL_COLLECTION_SIZE));
        for _ in 0..len {
            let k = seq.get_str()?.to_string();
            let v = seq.get_str()?.to_string();
            properties.push((Cow::Owned(k), Cow::Owned(v)));
        }
        Ok(Self {
            properties: Cow::Owned(properties),
        })
    }
}

impl DataComponentCodec<Self> for BeesImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.bees.len() as i32))?;
        for bee in &self.bees {
            let comp = bee.extract_compound();
            let entity_data = comp
                .and_then(|c| c.get_compound("entity_data"))
                .cloned()
                .unwrap_or_default();
            let ticks = comp.and_then(|c| c.get_int("ticks_in_hive")).unwrap_or(0);
            let min_ticks = comp
                .and_then(|c| c.get_int("min_ticks_in_hive"))
                .unwrap_or(0);
            seq.write_var_int(&VarInt(i32::from(EntityType::BEE.id)))?;
            seq.write_nbt(NbtTag::Compound(entity_data))?;
            seq.write_var_int(&VarInt(ticks))?;
            seq.write_var_int(&VarInt(min_ticks))?;
        }
        Ok(())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let len = seq.get_var_int()?.0 as usize;
        let mut bees = Vec::with_capacity(len.min(64));
        for _ in 0..len {
            let _entity_type = seq.get_var_int()?;
            let nbt = seq.get_nbt_with_version(&JavaMinecraftVersion::V_26_2)?;
            let ticks = seq.get_var_int()?.0;
            let min_ticks = seq.get_var_int()?.0;
            let mut bee_comp = pumpkin_nbt::compound::NbtCompound::new();
            if let Some(NbtTag::Compound(c)) = nbt {
                bee_comp.put_compound("entity_data", c);
            }
            bee_comp.put_int("ticks_in_hive", ticks);
            bee_comp.put_int("min_ticks_in_hive", min_ticks);
            bees.push(NbtTag::Compound(bee_comp));
        }
        Ok(Self { bees })
    }
}

impl DataComponentCodec<Self> for SulfurCubeContentImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        serialize_item_stack_template(&self.item, seq)
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let item = deserialize_item_stack_template(seq)?;
        Ok(Self { item })
    }
}

impl DataComponentCodec<Self> for ContainerLootImpl {
    fn serialize(&self, _seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        Ok(())
    }

    fn deserialize(_seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        Ok(Self {
            loot_table: String::new(),
            seed: 0,
        })
    }
}

fn serialize_sign_messages(
    messages: &[Cow<'static, str>; 4],
    filtered_messages: Option<&[Cow<'static, str>; 4]>,
    color: pumpkin_data::dye_color::DyeColor,
    has_glowing_text: bool,
    seq: &mut impl NetworkWriteExt,
) -> Result<(), WritingError> {
    for msg in messages {
        seq.write_nbt(NbtTag::String(msg.to_string().into()))?;
    }
    if let Some(filtered) = filtered_messages {
        seq.write_bool(true)?;
        for msg in filtered {
            seq.write_nbt(NbtTag::String(msg.to_string().into()))?;
        }
    } else {
        seq.write_bool(false)?;
    }
    seq.write_var_int(&VarInt::from(color.id() as i32))?;
    seq.write_bool(has_glowing_text)
}

type DeserializedSignData = (
    [Cow<'static, str>; 4],
    Option<[Cow<'static, str>; 4]>,
    pumpkin_data::dye_color::DyeColor,
    bool,
);

fn deserialize_sign_messages(
    seq: &mut impl NetworkReadExt,
) -> Result<DeserializedSignData, ReadingError> {
    let mut messages = [
        Cow::Borrowed(""),
        Cow::Borrowed(""),
        Cow::Borrowed(""),
        Cow::Borrowed(""),
    ];
    for slot in &mut messages {
        let tag = seq.get_nbt_with_version(&JavaMinecraftVersion::V_26_2)?;
        if let Some(t) = tag {
            *slot = Cow::Owned(pumpkin_util::text::TextComponent::from_nbt(&t).get_text());
        }
    }
    let filtered_messages = if seq.get_bool()? {
        let mut filtered = [
            Cow::Borrowed(""),
            Cow::Borrowed(""),
            Cow::Borrowed(""),
            Cow::Borrowed(""),
        ];
        for slot in &mut filtered {
            let tag = seq.get_nbt_with_version(&JavaMinecraftVersion::V_26_2)?;
            if let Some(t) = tag {
                *slot = Cow::Owned(pumpkin_util::text::TextComponent::from_nbt(&t).get_text());
            }
        }
        Some(filtered)
    } else {
        None
    };
    let color_id = seq.get_var_int()?.0;
    let color = u8::try_from(color_id)
        .ok()
        .and_then(pumpkin_data::dye_color::DyeColor::by_id)
        .ok_or_else(|| ReadingError::Message(format!("Invalid SignText DyeColor {color_id}")))?;
    let has_glowing_text = seq.get_bool()?;
    Ok((messages, filtered_messages, color, has_glowing_text))
}

impl DataComponentCodec<Self> for SignTextFrontImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        serialize_sign_messages(
            &self.messages,
            self.filtered_messages.as_ref(),
            self.color,
            self.has_glowing_text,
            seq,
        )
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let (messages, filtered_messages, color, has_glowing_text) =
            deserialize_sign_messages(seq)?;
        Ok(Self {
            messages,
            filtered_messages,
            color,
            has_glowing_text,
        })
    }
}

impl DataComponentCodec<Self> for SignTextBackImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        serialize_sign_messages(
            &self.messages,
            self.filtered_messages.as_ref(),
            self.color,
            self.has_glowing_text,
            seq,
        )
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let (messages, filtered_messages, color, has_glowing_text) =
            deserialize_sign_messages(seq)?;
        Ok(Self {
            messages,
            filtered_messages,
            color,
            has_glowing_text,
        })
    }
}
