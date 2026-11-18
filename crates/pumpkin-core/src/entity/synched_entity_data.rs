use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use bytes::BufMut;
use pumpkin_data::meta_data_type::MetaDataType;
use pumpkin_data::tracked_data::{TrackedData, TrackedId};
use pumpkin_protocol::java::client::play::{Metadata, MetadataSerializer};
use pumpkin_protocol::ser::WritingError;
use pumpkin_util::version::JavaMinecraftVersion;

pub trait ErasedSerializer: Send + Sync {
    fn write(
        &self,
        index: TrackedId,
        r#type: MetaDataType,
        writer: &mut dyn std::io::Write,
        version: &JavaMinecraftVersion,
    ) -> Result<(), WritingError>;

    fn write_canonical(&self, index: TrackedId, r#type: MetaDataType) -> Vec<u8>;
}

struct SerializerHolder<T> {
    value: T,
}

impl<T: MetadataSerializer + Clone + Send + Sync + 'static> ErasedSerializer
    for SerializerHolder<T>
{
    fn write(
        &self,
        index: TrackedId,
        r#type: MetaDataType,
        writer: &mut dyn std::io::Write,
        version: &JavaMinecraftVersion,
    ) -> Result<(), WritingError> {
        let meta = Metadata::new_raw(index, r#type, &self.value);
        meta.write(writer, version)
    }

    fn write_canonical(&self, index: TrackedId, r#type: MetaDataType) -> Vec<u8> {
        let mut buf = Vec::new();
        let meta = Metadata::new_raw(index, r#type, &self.value);
        let _ = meta.write(&mut buf, &JavaMinecraftVersion::V_26_3);
        buf
    }
}

pub struct DataItem {
    pub tracked: TrackedData,
    pub serializer: Box<dyn ErasedSerializer>,
    pub canonical_bytes: Vec<u8>,
    pub dirty: bool,
    pub is_default: bool,
}

pub struct SynchedEntityData {
    items: Mutex<HashMap<TrackedData, DataItem>>,
    is_dirty: AtomicBool,
}

impl Default for SynchedEntityData {
    fn default() -> Self {
        Self::new()
    }
}

impl SynchedEntityData {
    /// An empty tracker; values are added by [`Self::define`] and [`Self::set`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            items: Mutex::new(HashMap::new()),
            is_dirty: AtomicBool::new(false),
        }
    }

    /// Registers a tracked value with the default the client already assumes, so it
    /// is only sent once something actually sets it.
    pub fn define<T: MetadataSerializer + Clone + Send + Sync + 'static>(
        &self,
        tracked: TrackedData,
        value: T,
    ) {
        let holder = SerializerHolder { value };
        let canonical_bytes = holder.write_canonical(tracked.id, tracked.r#type);
        let mut items = self
            .items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        items.insert(
            tracked,
            DataItem {
                tracked,
                serializer: Box::new(holder),
                canonical_bytes,
                dirty: false,
                is_default: true,
            },
        );
    }

    /// Stores a tracked value and marks it dirty, returning whether it differs from
    /// the value that was already stored.
    pub fn set<T: MetadataSerializer + Clone + Send + Sync + 'static>(
        &self,
        tracked: TrackedData,
        value: T,
    ) -> bool {
        let holder = SerializerHolder { value };
        let new_canonical = holder.write_canonical(tracked.id, tracked.r#type);

        let mut items = self
            .items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(item) = items.get_mut(&tracked) {
            if item.canonical_bytes == new_canonical {
                return false;
            }
            item.canonical_bytes = new_canonical;
            item.serializer = Box::new(holder);
            item.dirty = true;
            item.is_default = false;
        } else {
            items.insert(
                tracked,
                DataItem {
                    tracked,
                    serializer: Box::new(holder),
                    canonical_bytes: new_canonical,
                    dirty: true,
                    is_default: false,
                },
            );
        }
        self.is_dirty.store(true, Ordering::Release);
        true
    }

    /// Whether any value changed since the last [`Self::clear_dirty`].
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.is_dirty.load(Ordering::Acquire)
    }

    /// Serializes the values that changed since the last [`Self::clear_dirty`], or
    /// `None` when nothing changed.
    pub fn pack_dirty_for_version(&self, version: &JavaMinecraftVersion) -> Option<Box<[u8]>> {
        if !self.is_dirty.load(Ordering::Acquire) {
            return None;
        }

        let items = self
            .items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut buf = Vec::new();
        let mut has_any = false;

        for item in in_id_order(&items) {
            if item.dirty {
                let before_len = buf.len();
                if item
                    .serializer
                    .write(item.tracked.id, item.tracked.r#type, &mut buf, version)
                    .is_ok()
                    && buf.len() > before_len
                {
                    has_any = true;
                }
            }
        }

        if !has_any {
            return None;
        }

        buf.put_u8(255);
        Some(buf.into_boxed_slice())
    }

    /// Marks every value as sent.
    pub fn clear_dirty(&self) {
        let mut items = self
            .items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for item in items.values_mut() {
            item.dirty = false;
        }
        self.is_dirty.store(false, Ordering::Release);
    }

    /// Serializes every value that differs from the default the client assumes, or
    /// `None` when they are all still at their default.
    pub fn get_non_default_values_for_version(
        &self,
        version: &JavaMinecraftVersion,
    ) -> Option<Box<[u8]>> {
        let items = self
            .items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut buf = Vec::new();
        let mut has_any = false;

        for item in in_id_order(&items) {
            if !item.is_default {
                let before_len = buf.len();
                if item
                    .serializer
                    .write(item.tracked.id, item.tracked.r#type, &mut buf, version)
                    .is_ok()
                    && buf.len() > before_len
                {
                    has_any = true;
                }
            }
        }

        if !has_any {
            return None;
        }

        buf.put_u8(255);
        Some(buf.into_boxed_slice())
    }
}

/// The items in tracked-id order, the order vanilla writes them in.
fn in_id_order(items: &HashMap<TrackedData, DataItem>) -> Vec<&DataItem> {
    let mut sorted: Vec<&DataItem> = items.values().collect();
    sorted.sort_unstable_by_key(|item| item.tracked.id.0);
    sorted
}

#[cfg(test)]
mod test {
    use pumpkin_data::tracked_data::player::PLAYER_MODE_CUSTOMISATION;

    use super::*;

    /// Metadata ids no longer vary per version: core serializes the current format for
    /// every client and `pumpkin-java-multiversion` translates. So the senders need no
    /// version check of their own to decide whether a value reaches a client.
    #[test]
    fn a_set_value_is_serialized_for_every_version() {
        let data = SynchedEntityData::new();
        data.define(PLAYER_MODE_CUSTOMISATION, 0u8);
        assert!(data.set(PLAYER_MODE_CUSTOMISATION, 0x7Fu8));

        let current = data.get_non_default_values_for_version(&JavaMinecraftVersion::V_26_3);
        let legacy = data.get_non_default_values_for_version(&JavaMinecraftVersion::V_1_20_5);
        assert!(current.is_some());
        assert_eq!(current, legacy);
        assert_eq!(
            data.pack_dirty_for_version(&JavaMinecraftVersion::V_1_20_5),
            current
        );
    }

    /// A value still at the default the client assumes is not sent.
    #[test]
    fn default_values_are_not_serialized() {
        let data = SynchedEntityData::new();
        data.define(PLAYER_MODE_CUSTOMISATION, 0u8);

        assert!(
            data.get_non_default_values_for_version(&JavaMinecraftVersion::V_26_3)
                .is_none()
        );
    }

    /// Entries go out in id order, as vanilla packs them (its items sit in an array indexed by
    /// id). They used to follow `HashMap` order, so the same entity data gave different bytes
    /// from run to run.
    #[test]
    fn values_are_written_in_id_order() {
        let ids = [7u8, 2, 9, 0, 5, 3, 8, 1];
        let data = SynchedEntityData::new();
        for id in ids {
            let tracked = TrackedData::new(TrackedId(id), MetaDataType::BYTE);
            data.define(tracked, 0u8);
            assert!(data.set(tracked, id + 1));
        }
        let mut expected = Vec::new();
        let mut sorted = ids;
        sorted.sort_unstable();
        for id in sorted {
            let holder = SerializerHolder { value: id + 1 };
            expected.extend(holder.write_canonical(TrackedId(id), MetaDataType::BYTE));
        }
        expected.push(255);
        let version = JavaMinecraftVersion::V_26_3;
        assert_eq!(
            data.get_non_default_values_for_version(&version).as_deref(),
            Some(expected.as_slice())
        );
        assert_eq!(
            data.pack_dirty_for_version(&version).as_deref(),
            Some(expected.as_slice())
        );
    }
}
