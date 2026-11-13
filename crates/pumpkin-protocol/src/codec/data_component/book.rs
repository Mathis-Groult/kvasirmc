use super::{DataComponentCodec, MAX_INITIAL_COLLECTION_SIZE, read_count};
use crate::codec::var_int::VarInt;
use crate::ser::{NetworkReadExt, NetworkWriteExt, ReadingError, WritingError};
use pumpkin_data::data_component_impl::{
    DebugStickStateImpl, WritableBookContentImpl, WrittenBookContentImpl,
};
use pumpkin_util::version::JavaMinecraftVersion;

impl DataComponentCodec<Self> for WritableBookContentImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.pages.len() as i32))?;
        for page in &self.pages {
            seq.write_string(page)?;
            seq.write_bool(false)?;
        }
        Ok(())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        const MAX_PAGES: usize = 100;
        let len = read_count(seq, MAX_PAGES)?;
        let mut pages = Vec::with_capacity(len);
        for _ in 0..len {
            let raw = seq.get_str()?.to_string();
            let has_filtered = seq.get_bool()?;
            if has_filtered {
                let _ = seq.get_str()?;
            }
            pages.push(raw);
        }
        Ok(Self { pages })
    }
}

impl DataComponentCodec<Self> for WrittenBookContentImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_string(&self.title)?;
        seq.write_bool(false)?;
        seq.write_string(&self.author)?;
        seq.write_var_int(&VarInt(0))?;
        seq.write_var_int(&VarInt::from(self.pages.len() as i32))?;
        for page in &self.pages {
            seq.write_slice(&page.encode_for_version(&JavaMinecraftVersion::V_26_2))?;
            seq.write_bool(false)?;
        }
        seq.write_bool(true)
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let title = seq.get_str()?.to_string();
        if seq.get_bool()? {
            let _ = seq.get_str()?;
        }
        let author = seq.get_str()?.to_string();
        let _generation = seq.get_var_int()?.0;
        let pages_len = read_count(seq, usize::MAX)?;
        let mut pages = Vec::with_capacity(pages_len.min(MAX_INITIAL_COLLECTION_SIZE));
        for _ in 0..pages_len {
            let tag = seq.get_nbt_with_version(&JavaMinecraftVersion::V_26_2)?;
            let comp = tag.as_ref().map_or_else(
                pumpkin_util::text::TextComponent::empty,
                pumpkin_util::text::TextComponent::from_nbt,
            );
            if seq.get_bool()? {
                let _ = seq.get_nbt_with_version(&JavaMinecraftVersion::V_26_2)?;
            }
            pages.push(comp);
        }
        let _resolved = seq.get_bool()?;
        Ok(Self {
            title,
            author,
            pages,
        })
    }
}

impl DataComponentCodec<Self> for DebugStickStateImpl {
    fn serialize(&self, _seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        Ok(())
    }

    fn deserialize(_seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        Ok(Self {
            properties: Vec::new(),
        })
    }
}
