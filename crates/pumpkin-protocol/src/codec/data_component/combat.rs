use super::{
    DataComponentCodec, MAX_INITIAL_COLLECTION_SIZE, data_to_proto_sound, deserialize,
    deserialize_consume_effect, deserialize_idset, proto_to_data_sound, read_count,
    serialize_idset,
};
use crate::codec::var_int::VarInt;
use crate::ser::{NetworkReadExt, NetworkWriteExt, ReadingError, WritingError};
use pumpkin_data::Enchantment;
use pumpkin_data::data_component::DataComponent;
use pumpkin_data::data_component_impl::{
    AdditionalTradeCostImpl, AttackRangeImpl, AttributeModifiersImpl, BlocksAttacksImpl,
    CanBreakImpl, CanPlaceOnImpl, DamageResistantImpl, DamageResistantType, DamageTypeImpl,
    DeathProtectionImpl, EnchantableImpl, EnchantmentsImpl, EquipmentSlot, EquippableImpl,
    GliderImpl, IDSet, IntangibleProjectileImpl, InteractAnimationImpl, KineticWeaponImpl,
    MinimumAttackChargeImpl, MobVisibilityImpl, OminousBottleAmplifierImpl, PiercingWeaponImpl,
    RepairCostImpl, RepairableImpl, StoredEnchantmentsImpl, SwingAnimationImpl, SwingAnimationType,
    ToolImpl, TrimImpl, WeaponImpl,
};
use pumpkin_data::entity::EntityType;
use pumpkin_nbt::tag::NbtTag;
use pumpkin_util::version::JavaMinecraftVersion;
use std::borrow::Cow;

impl DataComponentCodec<Self> for RepairCostImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.cost))
    }
    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let cost = seq.get_var_int()?.0;
        Ok(Self { cost })
    }
}

impl DataComponentCodec<Self> for EnchantmentsImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.enchantment.len() as i32))?;
        for (enc, level) in self.enchantment.iter() {
            seq.write_var_int(&VarInt::from(enc.id))?;
            seq.write_var_int(&VarInt::from(*level))?;
        }
        Ok(())
    }
    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        const MAX_ENCHANTMENTS: usize = 256;

        let len = seq.get_var_int()?.0 as usize;
        if len > MAX_ENCHANTMENTS {
            return Err(ReadingError::Message("Too many enchantments".into()));
        }
        let mut enc = Vec::with_capacity(len);
        for _ in 0..len {
            let id = seq.get_var_int()?.0 as u8;
            let level = seq.get_var_int()?.0;
            enc.push((
                Enchantment::from_id(id).ok_or(ReadingError::Message(
                    "EnchantmentsImpl Enchantment VarInt Incorrect!".into(),
                ))?,
                level,
            ));
        }
        Ok(Self {
            enchantment: Cow::from(enc),
        })
    }
}

impl DataComponentCodec<Self> for EquippableImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt(self.slot.get_slot_index()))?;
        crate::IdOr::<crate::SoundEvent>::write(
            &data_to_proto_sound(&self.equip_sound),
            seq,
            |w, e| {
                w.write_string(&e.sound_name)?;
                w.write_option(&e.range, |w2, r| w2.write_f32(*r))
            },
        )?;

        seq.write_bool(self.asset_id.is_some())?;
        if let Some(asset) = &self.asset_id {
            seq.write_string(asset)?;
        }

        seq.write_bool(self.camera_overlay.is_some())?;
        if let Some(overlay) = &self.camera_overlay {
            seq.write_string(overlay)?;
        }

        seq.write_bool(self.allowed_entities.is_some())?;
        if let Some(allowed) = &self.allowed_entities {
            serialize_idset(allowed, seq)?;
        }

        seq.write_bool(self.dispensable)?;
        seq.write_bool(self.swappable)?;
        seq.write_bool(self.damage_on_hurt)?;
        seq.write_bool(self.equip_on_interact)?;
        seq.write_bool(self.can_be_sheared)?;
        crate::IdOr::<crate::SoundEvent>::write(
            &data_to_proto_sound(&self.shearing_sound),
            seq,
            |w, e| {
                w.write_string(&e.sound_name)?;
                w.write_option(&e.range, |w2, r| w2.write_f32(*r))
            },
        )
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let slot_index = seq.get_var_int()?.0;
        let slot = EquipmentSlot::from_slot_index(slot_index).ok_or(ReadingError::Message(
            format!("Invalid equipment slot index {slot_index}"),
        ))?;
        let equip_sound = proto_to_data_sound(&crate::IdOr::<crate::SoundEvent>::read(seq, |r| {
            let sound_name = r.get_str()?.into();
            let range = r.get_option(NetworkReadExt::get_f32)?;
            Ok(crate::SoundEvent { sound_name, range })
        })?)
        .ok_or(ReadingError::Message(
            "Invalid sound in EquippableImpl".into(),
        ))?;

        let asset_id = if seq.get_bool()? {
            Some(Cow::Owned(seq.get_str()?.into()))
        } else {
            None
        };

        let camera_overlay = if seq.get_bool()? {
            Some(Cow::Owned(seq.get_str()?.into()))
        } else {
            None
        };

        let has_allowed_entities = seq.get_bool()?;

        let allowed_entities: Option<IDSet<EntityType>> = if has_allowed_entities {
            Some(deserialize_idset(seq)?)
        } else {
            None
        };

        let dispensable = seq.get_bool()?;
        let swappable = seq.get_bool()?;
        let damage_on_hurt = seq.get_bool()?;
        let equip_on_interact = seq.get_bool()?;
        let can_be_sheared = seq.get_bool()?;
        let shearing_sound =
            proto_to_data_sound(&crate::IdOr::<crate::SoundEvent>::read(seq, |r| {
                let sound_name = r.get_str()?.into();
                let range = r.get_option(NetworkReadExt::get_f32)?;
                Ok(crate::SoundEvent { sound_name, range })
            })?)
            .ok_or(ReadingError::Message(
                "Invalid shearing sound in EquippableImpl".into(),
            ))?;

        Ok(Self {
            slot,
            equip_sound,
            asset_id,
            camera_overlay,
            allowed_entities,
            dispensable,
            swappable,
            damage_on_hurt,
            equip_on_interact,
            can_be_sheared,
            shearing_sound,
        })
    }
}

impl DataComponentCodec<Self> for StoredEnchantmentsImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.enchantment.len() as i32))?;
        for (enc, level) in self.enchantment.iter() {
            seq.write_var_int(&VarInt::from(enc.id))?;
            seq.write_var_int(&VarInt::from(*level))?;
        }
        Ok(())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        const MAX_ENCHANTMENTS: usize = 256;

        let len = seq.get_var_int()?.0 as usize;

        if len > MAX_ENCHANTMENTS {
            return Err(ReadingError::Message("Too many enchantments".into()));
        }

        let mut stored_enchantments = Vec::with_capacity(len);
        for _ in 0..len {
            let id = seq.get_var_int()?.0 as u8;
            let level = seq.get_var_int()?.0;
            stored_enchantments.push((
                Enchantment::from_id(id).ok_or(ReadingError::Message(
                    "StoredEnchantmentsImpl Enchantment VarInt Incorrect!".into(),
                ))?,
                level,
            ));
        }
        Ok(Self {
            enchantment: Cow::from(stored_enchantments),
        })
    }
}

impl DataComponentCodec<Self> for RepairableImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        serialize_idset(&self.items, seq)
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        Ok(Self {
            items: deserialize_idset(seq)?,
        })
    }
}

impl DataComponentCodec<Self> for SwingAnimationImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.animation_type.to_id()))?;
        seq.write_var_int(&VarInt::from(self.duration))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let type_id = seq.get_var_int()?.0;
        let animation_type = SwingAnimationType::from_id(type_id).ok_or_else(|| {
            ReadingError::Message(format!("Invalid SwingAnimationType id {type_id}"))
        })?;
        let duration = seq.get_var_int()?.0;
        Ok(Self {
            animation_type,
            duration,
        })
    }
}

impl DataComponentCodec<Self> for MinimumAttackChargeImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_f32(self.charge)
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let charge = seq.get_f32()?;
        Ok(Self { charge })
    }
}

impl DataComponentCodec<Self> for DamageTypeImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.damage_type.id as i32))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let id = seq.get_var_int()?.0 as u8;
        let damage_type = pumpkin_data::damage::DamageType::from_id(id)
            .ok_or_else(|| ReadingError::Message(format!("Invalid DamageType id {id}")))?;
        Ok(Self { damage_type })
    }
}

fn serialize_adventure_predicate(
    predicate: &NbtTag,
    seq: &mut impl NetworkWriteExt,
) -> Result<(), WritingError> {
    let list: Vec<&pumpkin_nbt::compound::NbtCompound> = match predicate {
        NbtTag::Compound(c) => c.get_list("predicates").map_or_else(
            || vec![c],
            |preds| preds.iter().filter_map(NbtTag::extract_compound).collect(),
        ),
        NbtTag::List(preds) => preds.iter().filter_map(NbtTag::extract_compound).collect(),
        _ => Vec::new(),
    };
    seq.write_var_int(&VarInt::from(list.len() as i32))?;
    for c in list {
        if let Some(blocks_tag) = c.get("blocks")
            && let Some(idset) = IDSet::<pumpkin_data::Block>::read(blocks_tag)
        {
            seq.write_bool(true)?;
            serialize_idset(&idset, seq)?;
        } else {
            seq.write_bool(false)?;
        }
        seq.write_bool(false)?;
        seq.write_bool(false)?;
        seq.write_var_int(&VarInt(0))?;
        seq.write_var_int(&VarInt(0))?;
    }
    Ok(())
}

fn deserialize_adventure_predicate(seq: &mut impl NetworkReadExt) -> Result<NbtTag, ReadingError> {
    let count = seq.get_var_int()?.0.max(0) as usize;
    let mut preds = Vec::with_capacity(count.min(64));
    for _ in 0..count {
        let mut comp = pumpkin_nbt::compound::NbtCompound::new();
        let has_blocks = seq.get_bool()?;
        if has_blocks {
            let idset = deserialize_idset::<pumpkin_data::Block>(seq)?;
            idset.write(&mut comp, "blocks");
        }
        let has_props = seq.get_bool()?;
        if has_props {
            let props_len = seq.get_var_int()?.0;
            for _ in 0..props_len {
                let _ = seq.get_str()?;
                let is_exact = seq.get_bool()?;
                if is_exact {
                    let _ = seq.get_str()?;
                } else {
                    if seq.get_bool()? {
                        let _ = seq.get_str()?;
                    }
                    if seq.get_bool()? {
                        let _ = seq.get_str()?;
                    }
                }
            }
        }
        let has_nbt = seq.get_bool()?;
        if has_nbt {
            let _ = seq.get_nbt_with_version(&JavaMinecraftVersion::V_26_2)?;
        }
        let exact_len = seq.get_var_int()?.0;
        for _ in 0..exact_len {
            let comp_id = seq.get_var_int()?.0 as u8;
            if let Some(comp) = DataComponent::try_from_id(comp_id) {
                let _ = deserialize(comp, seq)?;
            }
        }
        let partial_len = seq.get_var_int()?.0;
        for _ in 0..partial_len {
            let _ = seq.get_var_int()?;
        }
        preds.push(NbtTag::Compound(comp));
    }
    Ok(NbtTag::List(preds))
}

impl DataComponentCodec<Self> for CanPlaceOnImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        serialize_adventure_predicate(&self.predicate, seq)
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        Ok(Self {
            predicate: deserialize_adventure_predicate(seq)?,
        })
    }
}

impl DataComponentCodec<Self> for CanBreakImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        serialize_adventure_predicate(&self.predicate, seq)
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        Ok(Self {
            predicate: deserialize_adventure_predicate(seq)?,
        })
    }
}

impl DataComponentCodec<Self> for AttributeModifiersImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.attribute_modifiers.len() as i32))?;
        for modifier in self.attribute_modifiers.iter() {
            seq.write_var_int(&VarInt::from(modifier.r#type.id as i32))?;
            seq.write_string(modifier.id)?;
            seq.write_f64(modifier.amount)?;
            seq.write_var_int(&VarInt::from(modifier.operation as i32))?;
            let slot_id = match modifier.slot {
                pumpkin_data::enchantment::AttributeModifierSlot::Any => 0,
                pumpkin_data::enchantment::AttributeModifierSlot::MainHand => 1,
                pumpkin_data::enchantment::AttributeModifierSlot::OffHand => 2,
                pumpkin_data::enchantment::AttributeModifierSlot::Hand => 3,
                pumpkin_data::enchantment::AttributeModifierSlot::Feet => 4,
                pumpkin_data::enchantment::AttributeModifierSlot::Legs => 5,
                pumpkin_data::enchantment::AttributeModifierSlot::Chest => 6,
                pumpkin_data::enchantment::AttributeModifierSlot::Head => 7,
                pumpkin_data::enchantment::AttributeModifierSlot::Armor => 8,
                pumpkin_data::enchantment::AttributeModifierSlot::Body => 9,
                pumpkin_data::enchantment::AttributeModifierSlot::Saddle => 10,
            };
            seq.write_var_int(&VarInt(slot_id))?;
            seq.write_var_int(&VarInt(0))?;
        }
        Ok(())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let len = seq.get_var_int()?.0 as usize;
        for _ in 0..len {
            let _attr_id = seq.get_var_int()?;
            let _id = seq.get_str()?;
            let _amount = seq.get_f64()?;
            let _operation = seq.get_var_int()?;
            let _slot = seq.get_var_int()?;
            let display_type = seq.get_var_int()?.0;
            if display_type == 2 {
                let _ = seq.get_nbt_with_version(&JavaMinecraftVersion::V_26_2)?;
            }
        }
        Ok(Self {
            attribute_modifiers: Cow::Borrowed(&[]),
        })
    }
}

impl DataComponentCodec<Self> for IntangibleProjectileImpl {
    fn serialize(&self, _seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        Ok(())
    }

    fn deserialize(_seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        Ok(Self)
    }
}

impl DataComponentCodec<Self> for DamageResistantImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_string(self.res_type.as_str())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let tag = seq.get_str()?;
        Ok(Self {
            res_type: DamageResistantType::from_tag(&tag),
        })
    }
}

impl DataComponentCodec<Self> for ToolImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.rules.len() as i32))?;
        for rule in self.rules.iter() {
            serialize_idset(&rule.blocks, seq)?;
            seq.write_bool(rule.speed.is_some())?;
            if let Some(speed) = rule.speed {
                seq.write_f32(speed)?;
            }
            seq.write_bool(rule.correct_for_drops.is_some())?;
            if let Some(correct) = rule.correct_for_drops {
                seq.write_bool(correct)?;
            }
        }
        seq.write_f32(self.default_mining_speed)?;
        seq.write_var_int(&VarInt::from(self.damage_per_block as i32))?;
        seq.write_bool(self.can_destroy_blocks_in_creative)
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let rules_len = read_count(seq, usize::MAX)?;
        let mut rules = Vec::with_capacity(rules_len.min(MAX_INITIAL_COLLECTION_SIZE));
        for _ in 0..rules_len {
            let blocks = deserialize_idset(seq)?;
            let speed = if seq.get_bool()? {
                Some(seq.get_f32()?)
            } else {
                None
            };
            let correct_for_drops = if seq.get_bool()? {
                Some(seq.get_bool()?)
            } else {
                None
            };
            rules.push(pumpkin_data::data_component_impl::ToolRule {
                blocks,
                speed,
                correct_for_drops,
            });
        }
        let default_mining_speed = seq.get_f32()?;
        let damage_per_block = seq.get_var_int()?.0 as u32;
        let can_destroy_blocks_in_creative = seq.get_bool()?;
        Ok(Self {
            rules: Cow::Owned(rules),
            default_mining_speed,
            damage_per_block,
            can_destroy_blocks_in_creative,
        })
    }
}

impl DataComponentCodec<Self> for WeaponImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.item_damage_per_attack as i32))?;
        seq.write_f32(self.disable_blocking_for_seconds)
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let item_damage_per_attack = seq.get_var_int()?.0 as u32;
        let disable_blocking_for_seconds = seq.get_f32()?;
        Ok(Self {
            item_damage_per_attack,
            disable_blocking_for_seconds,
        })
    }
}

impl DataComponentCodec<Self> for AttackRangeImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_f32(self.min_reach)?;
        seq.write_f32(self.max_reach)?;
        seq.write_f32(self.min_creative_reach)?;
        seq.write_f32(self.max_creative_reach)?;
        seq.write_f32(self.hitbox_margin)?;
        seq.write_f32(self.mob_factor)
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let min_reach = seq.get_f32()?;
        let max_reach = seq.get_f32()?;
        let min_creative_reach = seq.get_f32()?;
        let max_creative_reach = seq.get_f32()?;
        let hitbox_margin = seq.get_f32()?;
        let mob_factor = seq.get_f32()?;
        Ok(Self {
            min_reach,
            max_reach,
            min_creative_reach,
            max_creative_reach,
            hitbox_margin,
            mob_factor,
        })
    }
}

impl DataComponentCodec<Self> for EnchantableImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.value))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let value = seq.get_var_int()?.0;
        Ok(Self { value })
    }
}

impl DataComponentCodec<Self> for GliderImpl {
    fn serialize(&self, _seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        Ok(())
    }

    fn deserialize(_seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        Ok(Self)
    }
}

impl DataComponentCodec<Self> for DeathProtectionImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt(0))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let len = seq.get_var_int()?.0 as usize;
        for _ in 0..len {
            let _ = deserialize_consume_effect(seq)?;
        }
        Ok(Self)
    }
}

impl DataComponentCodec<Self> for BlocksAttacksImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_f32(0.0)?;
        seq.write_f32(1.0)?;
        seq.write_var_int(&VarInt(0))?;
        seq.write_var_int(&VarInt(0))?;
        seq.write_bool(false)?;
        seq.write_bool(false)?;
        seq.write_bool(false)
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let _block_delay = seq.get_f32()?;
        let _disable_scale = seq.get_f32()?;
        let red_len = seq.get_var_int()?.0 as usize;
        for _ in 0..red_len {
            let _ = seq.get_f32()?;
            if seq.get_bool()? {
                let id_type = seq.get_var_int()?.0;
                if id_type == 0 {
                    let _ = seq.get_str()?;
                } else if id_type > 0 {
                    for _ in 0..(id_type - 1) {
                        let _ = seq.get_var_int()?;
                    }
                }
            }
            let _ = seq.get_f32()?;
            let _ = seq.get_f32()?;
        }
        let item_damage_type = seq.get_var_int()?.0;
        if item_damage_type == 1 {
            let _ = seq.get_f32()?;
            let _ = seq.get_f32()?;
        }
        if seq.get_bool()? {
            let id_type = seq.get_var_int()?.0;
            if id_type == 0 {
                let _ = seq.get_str()?;
            } else if id_type > 0 {
                for _ in 0..(id_type - 1) {
                    let _ = seq.get_var_int()?;
                }
            }
        }
        if seq.get_bool()? {
            let _ = seq.get_var_int()?;
        }
        if seq.get_bool()? {
            let _ = seq.get_var_int()?;
        }
        Ok(Self)
    }
}

impl DataComponentCodec<Self> for PiercingWeaponImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_bool(self.deals_knockback)?;
        seq.write_bool(self.dismounts)?;
        if let Some(sound) = &self.sound {
            seq.write_bool(true)?;
            let proto_sound = data_to_proto_sound(sound);
            crate::IdOr::<crate::SoundEvent>::write(&proto_sound, seq, |w, e| {
                w.write_string(&e.sound_name)?;
                w.write_option(&e.range, |w2, r| w2.write_f32(*r))
            })?;
        } else {
            seq.write_bool(false)?;
        }
        if let Some(sound) = &self.hit_sound {
            seq.write_bool(true)?;
            let proto_sound = data_to_proto_sound(sound);
            crate::IdOr::<crate::SoundEvent>::write(&proto_sound, seq, |w, e| {
                w.write_string(&e.sound_name)?;
                w.write_option(&e.range, |w2, r| w2.write_f32(*r))
            })?;
        } else {
            seq.write_bool(false)?;
        }
        Ok(())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let deals_knockback = seq.get_bool()?;
        let dismounts = seq.get_bool()?;
        let sound = if seq.get_bool()? {
            let proto = crate::IdOr::<crate::SoundEvent>::read(seq, |r| {
                let sound_name = r.get_str()?.to_string();
                let range = r.get_option(NetworkReadExt::get_f32)?;
                Ok(crate::SoundEvent { sound_name, range })
            })
            .map_err(|e| ReadingError::Message(format!("No sound: {e}")))?;
            proto_to_data_sound(&proto)
        } else {
            None
        };
        let hit_sound = if seq.get_bool()? {
            let proto = crate::IdOr::<crate::SoundEvent>::read(seq, |r| {
                let sound_name = r.get_str()?.to_string();
                let range = r.get_option(NetworkReadExt::get_f32)?;
                Ok(crate::SoundEvent { sound_name, range })
            })
            .map_err(|e| ReadingError::Message(format!("No sound: {e}")))?;
            proto_to_data_sound(&proto)
        } else {
            None
        };
        Ok(Self {
            deals_knockback,
            dismounts,
            sound,
            hit_sound,
        })
    }
}

fn serialize_kinetic_condition(
    cond: &pumpkin_data::data_component_impl::KineticConditionImpl,
    seq: &mut impl NetworkWriteExt,
) -> Result<(), WritingError> {
    seq.write_var_int(&VarInt::from(cond.max_duration_ticks))?;
    seq.write_f32(cond.min_speed)?;
    seq.write_f32(cond.min_relative_speed)
}

fn deserialize_kinetic_condition(
    seq: &mut impl NetworkReadExt,
) -> Result<pumpkin_data::data_component_impl::KineticConditionImpl, ReadingError> {
    let max_duration_ticks = seq.get_var_int()?.0;
    let min_speed = seq.get_f32()?;
    let min_relative_speed = seq.get_f32()?;
    Ok(pumpkin_data::data_component_impl::KineticConditionImpl {
        max_duration_ticks,
        min_speed,
        min_relative_speed,
    })
}

impl DataComponentCodec<Self> for KineticWeaponImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.contact_cooldown_ticks))?;
        seq.write_var_int(&VarInt::from(self.delay_ticks))?;
        if let Some(cond) = &self.dismount_conditions {
            seq.write_bool(true)?;
            serialize_kinetic_condition(cond, seq)?;
        } else {
            seq.write_bool(false)?;
        }
        if let Some(cond) = &self.knockback_conditions {
            seq.write_bool(true)?;
            serialize_kinetic_condition(cond, seq)?;
        } else {
            seq.write_bool(false)?;
        }
        if let Some(cond) = &self.damage_conditions {
            seq.write_bool(true)?;
            serialize_kinetic_condition(cond, seq)?;
        } else {
            seq.write_bool(false)?;
        }
        seq.write_f32(self.forward_movement)?;
        seq.write_f32(self.damage_multiplier)?;
        if let Some(sound) = &self.sound {
            seq.write_bool(true)?;
            let proto = data_to_proto_sound(sound);
            crate::IdOr::<crate::SoundEvent>::write(&proto, seq, |w, e| {
                w.write_string(&e.sound_name)?;
                w.write_option(&e.range, |w2, r| w2.write_f32(*r))
            })?;
        } else {
            seq.write_bool(false)?;
        }
        if let Some(sound) = &self.hit_sound {
            seq.write_bool(true)?;
            let proto = data_to_proto_sound(sound);
            crate::IdOr::<crate::SoundEvent>::write(&proto, seq, |w, e| {
                w.write_string(&e.sound_name)?;
                w.write_option(&e.range, |w2, r| w2.write_f32(*r))
            })?;
        } else {
            seq.write_bool(false)?;
        }
        Ok(())
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let contact_cooldown_ticks = seq.get_var_int()?.0;
        let delay_ticks = seq.get_var_int()?.0;
        let dismount_conditions = if seq.get_bool()? {
            Some(deserialize_kinetic_condition(seq)?)
        } else {
            None
        };
        let knockback_conditions = if seq.get_bool()? {
            Some(deserialize_kinetic_condition(seq)?)
        } else {
            None
        };
        let damage_conditions = if seq.get_bool()? {
            Some(deserialize_kinetic_condition(seq)?)
        } else {
            None
        };
        let forward_movement = seq.get_f32()?;
        let damage_multiplier = seq.get_f32()?;
        let sound = if seq.get_bool()? {
            let proto = crate::IdOr::<crate::SoundEvent>::read(seq, |r| {
                let sound_name = r.get_str()?.to_string();
                let range = r.get_option(NetworkReadExt::get_f32)?;
                Ok(crate::SoundEvent { sound_name, range })
            })
            .map_err(|e| ReadingError::Message(format!("No sound: {e}")))?;
            proto_to_data_sound(&proto)
        } else {
            None
        };
        let hit_sound = if seq.get_bool()? {
            let proto = crate::IdOr::<crate::SoundEvent>::read(seq, |r| {
                let sound_name = r.get_str()?.to_string();
                let range = r.get_option(NetworkReadExt::get_f32)?;
                Ok(crate::SoundEvent { sound_name, range })
            })
            .map_err(|e| ReadingError::Message(format!("No sound: {e}")))?;
            proto_to_data_sound(&proto)
        } else {
            None
        };
        Ok(Self {
            contact_cooldown_ticks,
            delay_ticks,
            dismount_conditions,
            knockback_conditions,
            damage_conditions,
            forward_movement,
            damage_multiplier,
            sound,
            hit_sound,
        })
    }
}

impl DataComponentCodec<Self> for AdditionalTradeCostImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.cost))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let cost = seq.get_var_int()?.0;
        Ok(Self { cost })
    }
}

impl DataComponentCodec<Self> for TrimImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt(0))?;
        seq.write_var_int(&VarInt(0))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let _material = seq.get_var_int()?;
        let _pattern = seq.get_var_int()?;
        Ok(Self {
            material: NbtTag::String("minecraft:quartz".into()),
            pattern: NbtTag::String("minecraft:coast".into()),
        })
    }
}

impl DataComponentCodec<Self> for OminousBottleAmplifierImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.amplifier))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let amplifier = seq.get_var_int()?.0;
        Ok(Self { amplifier })
    }
}

impl DataComponentCodec<Self> for InteractAnimationImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        seq.write_var_int(&VarInt::from(self.animation_type.to_id()))?;
        seq.write_var_int(&VarInt::from(self.duration))
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let anim_id = seq.get_var_int()?.0;
        let animation_type = pumpkin_data::data_component_impl::SwingAnimationType::from_id(
            anim_id,
        )
        .ok_or_else(|| ReadingError::Message(format!("Invalid SwingAnimationType id {anim_id}")))?;
        let duration = seq.get_var_int()?.0;
        Ok(Self {
            animation_type,
            duration,
        })
    }
}

impl DataComponentCodec<Self> for MobVisibilityImpl {
    fn serialize(&self, seq: &mut impl NetworkWriteExt) -> Result<(), WritingError> {
        serialize_idset(&self.targeting_entity_types, seq)?;
        seq.write_f32(self.visibility)
    }

    fn deserialize(seq: &mut impl NetworkReadExt) -> Result<Self, ReadingError> {
        let targeting_entity_types = deserialize_idset(seq)?;
        let visibility = seq.get_f32()?;
        // Like vanilla's validated item stacks, the persistent range applies here too.
        if !Self::is_valid_visibility(visibility) {
            return Err(ReadingError::Message(format!(
                "Mob visibility {visibility} outside 0..={}",
                Self::MAX_VISIBILITY
            )));
        }
        Ok(Self {
            targeting_entity_types,
            visibility,
        })
    }
}
