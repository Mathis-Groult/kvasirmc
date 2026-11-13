use super::{
    DataComponentCodec, data_to_proto_sound, deserialize_consume_effect,
    deserialize_item_stack_template, deserialize_status_effects, proto_to_data_sound,
    serialize_consume_effect, serialize_status_effects,
};
use crate::codec::var_int::VarInt;
use crate::ser::{NetworkReadExt, NetworkWriteExt, ReadingError, WritingError};
use pumpkin_data::data_component_impl::{
    ConsumableImpl, ConsumeAnimation, ConsumeEffect, FoodImpl, IDSetContent, PotionContentsImpl,
    PotionDurationScaleImpl, SuspiciousStewEffect, SuspiciousStewEffectsImpl, UseCooldownImpl,
    UseEffectsImpl, UseRemainderImpl, VillagerFoodImpl,
};
use pumpkin_data::effect::StatusEffect;
use std::borrow::Cow;

impl DataComponentCodec<Self> for SuspiciousStewEffectsImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        let effect_count = i32::try_from(self.effects.len())
            .map_err(|_| WritingError::Message("Too many suspicious stew effects".into()))?;
        seq.write_var_int(&VarInt(effect_count))?;
        for effect in self.effects.iter() {
            let id = StatusEffect::from_minecraft_name(&effect.effect)
                .ok_or_else(|| WritingError::Message("Unknown suspicious stew effect".into()))?
                .id;
            seq.write_var_int(&VarInt(i32::from(id)))?;
            seq.write_var_int(&VarInt(effect.duration))?;
        }
        Ok(())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        const MAX_EFFECTS: i32 = 128;

        let count = seq.get_var_int()?.0;
        if !(0..=MAX_EFFECTS).contains(&count) {
            return Err(ReadingError::Message(
                "Invalid suspicious stew effect count".into(),
            ));
        }

        let mut effects =
            Vec::with_capacity(usize::try_from(count).map_err(|_| {
                ReadingError::Message("Invalid suspicious stew effect count".into())
            })?);
        for _ in 0..count {
            let id = u16::try_from(seq.get_var_int()?.0)
                .map_err(|_| ReadingError::Message("Invalid suspicious stew effect id".into()))?;
            let effect = StatusEffect::from_id(id)
                .ok_or_else(|| ReadingError::Message("Unknown suspicious stew effect id".into()))?;
            let duration = seq.get_var_int()?.0;
            effects.push(SuspiciousStewEffect {
                effect: Cow::Borrowed(effect.minecraft_name),
                duration,
            });
        }
        Ok(Self {
            effects: Cow::Owned(effects),
        })
    }
}

impl DataComponentCodec<Self> for ConsumableImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_f32(self.consume_seconds)?;
        seq.write_var_int(&VarInt(self.animation as i32))?;
        crate::IdOr::<crate::SoundEvent>::write(
            &data_to_proto_sound(&self.sound_event),
            seq,
            |w, e| {
                w.write_string(&e.sound_name)?;
                w.write_option(&e.range, |w2, r| w2.write_f32(*r))
            },
        )?;
        seq.write_bool(self.consume_particles)?;
        seq.write_var_int(&VarInt(self.effects.len() as i32))?;

        for effect in self.effects.iter() {
            serialize_consume_effect(effect, seq)?;
        }

        Ok(())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        const MAX_CONSUME_EFFECTS: i32 = 256;

        let consume_seconds = seq.get_f32()?;
        let animation_id = seq.get_var_int()?;

        let animation: ConsumeAnimation = animation_id
            .0
            .try_into()
            .map_err(|()| ReadingError::Message("Invalid ConsumableImpl animation id!".into()))?;
        let proto_sound_event = crate::IdOr::<crate::SoundEvent>::read(seq, |r| {
            let sound_name = r.get_str()?.into();
            let range = r.get_option(NetworkReadExt::get_f32)?;
            Ok(crate::SoundEvent { sound_name, range })
        })?;
        let consume_particles = seq.get_bool()?;

        let sound_event = proto_to_data_sound(&proto_sound_event).ok_or(ReadingError::Message(
            "Invalid sound in ConsumableImpl".into(),
        ))?;
        let effects_len = seq.get_var_int()?.0;
        if !(0..=MAX_CONSUME_EFFECTS).contains(&effects_len) {
            return Err(ReadingError::Message("Invalid consume effect count".into()));
        }

        let mut effects_vec = Vec::with_capacity(effects_len as usize);

        for _ in 0..effects_len {
            effects_vec.push(deserialize_consume_effect(seq)?);
        }

        let effects: Cow<'static, [ConsumeEffect]> = Cow::Owned(effects_vec);

        Ok(Self {
            consume_seconds,
            animation,
            sound_event,
            consume_particles,
            effects,
        })
    }
}

impl DataComponentCodec<Self> for PotionContentsImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        // Potion ID (optional)
        if let Some(potion_id) = self.potion_id {
            seq.write_bool(true)?;
            seq.write_var_int(&VarInt::from(potion_id))?;
        } else {
            seq.write_bool(false)?;
        }

        // Custom color (optional)
        if let Some(color) = self.custom_color {
            seq.write_bool(true)?;
            seq.write_i32(color)?;
        } else {
            seq.write_bool(false)?;
        }

        // Custom effects list
        serialize_status_effects(&self.custom_effects, seq)?;

        // Custom name (optional)
        if let Some(name) = &self.custom_name {
            seq.write_bool(true)?;
            seq.write_string(name.as_str())?;
        } else {
            seq.write_bool(false)?;
        }

        Ok(())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        // Potion ID (optional)
        let has_potion = seq.get_bool()?;
        let potion_id = has_potion
            .then(|| seq.get_var_int().map(|value| value.0))
            .transpose()?;

        // Custom color (optional)
        let has_color = seq.get_bool()?;
        let custom_color = has_color.then(|| seq.get_i32()).transpose()?;

        // Custom effects list
        let custom_effects = deserialize_status_effects(seq)?;

        // Custom name (optional)
        let has_name = seq.get_bool()?;
        let custom_name = has_name
            .then(|| seq.get_str().map(String::from))
            .transpose()?;

        Ok(Self {
            potion_id,
            custom_color,
            custom_effects,
            custom_name,
        })
    }
}

impl DataComponentCodec<Self> for UseCooldownImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_f32(self.seconds)?;
        seq.write_bool(self.cooldown_group.is_some())?;
        if let Some(group) = &self.cooldown_group {
            seq.write_string(group)?;
        }
        Ok(())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let seconds = seq.get_f32()?;
        let cooldown_group = if seq.get_bool()? {
            Some(seq.get_str()?.into())
        } else {
            None
        };
        Ok(Self {
            seconds,
            cooldown_group,
        })
    }
}

impl DataComponentCodec<Self> for UseEffectsImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_bool(self.can_sprint)?;
        seq.write_bool(self.interact_vibrations)?;
        seq.write_f32(self.speed_multiplier)
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let can_sprint = seq.get_bool()?;
        let interact_vibrations = seq.get_bool()?;
        let speed_multiplier = seq.get_f32()?;
        Ok(Self {
            can_sprint,
            interact_vibrations,
            speed_multiplier,
        })
    }
}

impl DataComponentCodec<Self> for FoodImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.nutrition))?;
        seq.write_f32(self.saturation)?;
        seq.write_bool(self.can_always_eat)
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let nutrition = seq.get_var_int()?.0;
        let saturation = seq.get_f32()?;
        let can_always_eat = seq.get_bool()?;
        Ok(Self {
            nutrition,
            saturation,
            can_always_eat,
        })
    }
}

impl DataComponentCodec<Self> for UseRemainderImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        let item_id = self
            .remainder
            .as_ref()
            .and_then(|key| {
                pumpkin_data::item::Item::from_registry_key(
                    key.strip_prefix("minecraft:").unwrap_or(key),
                )
            })
            .map_or(0, |item| item.id as i32);
        seq.write_var_int(&VarInt(item_id))?;
        seq.write_var_int(&VarInt(1))?;
        seq.write_var_int(&VarInt(0))?;
        seq.write_var_int(&VarInt(0))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let stack = deserialize_item_stack_template(seq)?;
        let remainder = (stack.item.id != 0)
            .then(|| Cow::Owned(format!("minecraft:{}", stack.item.registry_key)));
        Ok(Self { remainder })
    }
}

impl DataComponentCodec<Self> for PotionDurationScaleImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_f32(self.scale)
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let scale = seq.get_f32()?;
        Ok(Self { scale })
    }
}

impl DataComponentCodec<Self> for VillagerFoodImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.nutrition))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let nutrition = seq.get_var_int()?.0;
        Ok(Self { nutrition })
    }
}
