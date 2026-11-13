use super::DataComponentCodec;
use crate::codec::var_int::VarInt;
use crate::ser::{NetworkReadExt, NetworkWriteExt, ReadingError, WritingError};
use pumpkin_data::data_component_impl::{
    AxolotlVariantImpl, CatCollarImpl, CatSoundVariantImpl, CatVariantImpl,
    ChickenSoundVariantImpl, ChickenVariantImpl, CowSoundVariantImpl, CowVariantImpl,
    FoxVariantImpl, FrogVariantImpl, HorseVariantImpl, LlamaVariantImpl, MooshroomVariantImpl,
    PaintingVariantImpl, ParrotVariantImpl, PigSoundVariantImpl, PigVariantImpl, RabbitVariantImpl,
    SalmonSizeImpl, SheepColorImpl, ShulkerColorImpl, TropicalFishBaseColorImpl,
    TropicalFishPatternColorImpl, TropicalFishPatternImpl, VillagerVariantImpl, WolfCollarImpl,
    WolfSoundVariantImpl, WolfVariantImpl, ZombieNautilusVariantImpl,
};
use std::borrow::Cow;

fn unknown_variant(component: &str, value: &str) -> WritingError {
    WritingError::Message(format!("unknown {component} value {value}"))
}

/// Vanilla `ByteBufCodecs.holderRegistry`: the entry's id in the synced registry.
macro_rules! codec_registry_variant {
    ($struct_name:ident, $variant:ty) => {
        impl DataComponentCodec<Self> for $struct_name {
            fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
                let variant = <$variant>::from_name(&self.value)
                    .ok_or_else(|| unknown_variant(stringify!($struct_name), &self.value))?;
                let id = i32::try_from(variant.id())
                    .map_err(|_| unknown_variant(stringify!($struct_name), &self.value))?;
                seq.write_var_int(&VarInt(id))
            }
            fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
                let id = seq.get_var_int()?.0;
                let variant = usize::try_from(id)
                    .ok()
                    .and_then(|id| <$variant>::all().get(id))
                    .ok_or_else(|| {
                        ReadingError::Message(format!(
                            "unknown {} id {id}",
                            stringify!($struct_name)
                        ))
                    })?;
                Ok(Self {
                    value: Cow::Owned(format!("minecraft:{}", variant.to_name())),
                })
            }
        }
    };
}

codec_registry_variant!(WolfVariantImpl, pumpkin_data::wolf_variant::WolfVariant);
codec_registry_variant!(
    WolfSoundVariantImpl,
    pumpkin_data::wolf_sound_variant::WolfSoundVariant
);
codec_registry_variant!(PigVariantImpl, pumpkin_data::pig_variant::PigVariant);
codec_registry_variant!(
    PigSoundVariantImpl,
    pumpkin_data::pig_sound_variant::PigSoundVariant
);
codec_registry_variant!(CowVariantImpl, pumpkin_data::cow_variant::CowVariant);
codec_registry_variant!(
    CowSoundVariantImpl,
    pumpkin_data::cow_sound_variant::CowSoundVariant
);
codec_registry_variant!(
    ChickenVariantImpl,
    pumpkin_data::chicken_variant::ChickenVariant
);
codec_registry_variant!(
    ChickenSoundVariantImpl,
    pumpkin_data::chicken_sound_variant::ChickenSoundVariant
);
codec_registry_variant!(
    ZombieNautilusVariantImpl,
    pumpkin_data::zombie_nautilus_variant::ZombieNautilusVariant
);
codec_registry_variant!(FrogVariantImpl, pumpkin_data::frog_variant::FrogVariant);
codec_registry_variant!(CatVariantImpl, pumpkin_data::cat_variant::CatVariant);
codec_registry_variant!(
    CatSoundVariantImpl,
    pumpkin_data::cat_sound_variant::CatSoundVariant
);

impl DataComponentCodec<Self> for VillagerVariantImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        let variant = pumpkin_data::villager::VillagerType::from_name(&self.value)
            .ok_or_else(|| unknown_variant("VillagerVariantImpl", &self.value))?;
        seq.write_var_int(&VarInt(variant as i32))
    }
    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let id = seq.get_var_int()?.0;
        let variant = pumpkin_data::villager::VillagerType::from_i32(id)
            .ok_or_else(|| ReadingError::Message(format!("unknown VillagerVariantImpl id {id}")))?;
        Ok(Self {
            value: Cow::Owned(format!("minecraft:{}", variant.to_name())),
        })
    }
}

/// Vanilla `ByteBufCodecs.holder`: 0 for an inline value, otherwise the registry id plus one.
impl DataComponentCodec<Self> for PaintingVariantImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        let variant = pumpkin_data::painting_variant::PaintingVariant::from_name(&self.value)
            .ok_or_else(|| unknown_variant("PaintingVariantImpl", &self.value))?;
        let id = i32::try_from(variant.id())
            .map_err(|_| unknown_variant("PaintingVariantImpl", &self.value))?;
        seq.write_var_int(&VarInt(id + 1))
    }
    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let id = seq.get_var_int()?.0;
        let variant = id
            .checked_sub(1)
            .and_then(|id| usize::try_from(id).ok())
            .and_then(|id| pumpkin_data::painting_variant::PaintingVariant::all().get(id))
            .ok_or_else(|| {
                ReadingError::Message(format!("unsupported PaintingVariantImpl holder {id}"))
            })?;
        Ok(Self {
            value: Cow::Owned(format!("minecraft:{}", variant.to_name())),
        })
    }
}

/// Vanilla `ByteBufCodecs.idMapper` over a Java enum; unknown ids follow its `ByIdMap` strategy.
macro_rules! codec_id_mapped_variant {
    ($struct_name:ident, $variant:ty) => {
        impl DataComponentCodec<Self> for $struct_name {
            fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
                let variant = <$variant>::from_name(&self.value)
                    .ok_or_else(|| unknown_variant(stringify!($struct_name), &self.value))?;
                seq.write_var_int(&VarInt(variant.id()))
            }
            fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
                let id = seq.get_var_int()?.0;
                Ok(Self {
                    value: Cow::Borrowed(<$variant>::by_id(id).name()),
                })
            }
        }
    };
}

codec_id_mapped_variant!(FoxVariantImpl, pumpkin_data::entity_variant::FoxVariant);
codec_id_mapped_variant!(SalmonSizeImpl, pumpkin_data::entity_variant::SalmonSize);
codec_id_mapped_variant!(
    ParrotVariantImpl,
    pumpkin_data::entity_variant::ParrotVariant
);
codec_id_mapped_variant!(
    TropicalFishPatternImpl,
    pumpkin_data::entity_variant::TropicalFishPattern
);
codec_id_mapped_variant!(
    MooshroomVariantImpl,
    pumpkin_data::entity_variant::MooshroomVariant
);
codec_id_mapped_variant!(
    RabbitVariantImpl,
    pumpkin_data::entity_variant::RabbitVariant
);
codec_id_mapped_variant!(HorseVariantImpl, pumpkin_data::entity_variant::HorseVariant);
codec_id_mapped_variant!(LlamaVariantImpl, pumpkin_data::entity_variant::LlamaVariant);
codec_id_mapped_variant!(
    AxolotlVariantImpl,
    pumpkin_data::entity_variant::AxolotlVariant
);

/// Vanilla `DyeColor.STREAM_CODEC`: the color id, unknown ids read as white.
macro_rules! codec_dye_color {
    ($struct_name:ident) => {
        impl DataComponentCodec<Self> for $struct_name {
            fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
                let color = pumpkin_data::dye_color::DyeColor::by_name(&self.value)
                    .ok_or_else(|| unknown_variant(stringify!($struct_name), &self.value))?;
                seq.write_var_int(&VarInt(i32::from(color.id())))
            }
            fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
                let color = pumpkin_data::dye_color::DyeColor::by_id_or_white(seq.get_var_int()?.0);
                Ok(Self {
                    value: Cow::Borrowed(color.name()),
                })
            }
        }
    };
}

codec_dye_color!(WolfCollarImpl);
codec_dye_color!(TropicalFishBaseColorImpl);
codec_dye_color!(TropicalFishPatternColorImpl);
codec_dye_color!(CatCollarImpl);
codec_dye_color!(SheepColorImpl);
codec_dye_color!(ShulkerColorImpl);
