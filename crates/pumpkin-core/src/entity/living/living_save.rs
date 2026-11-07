//! Reading and writing living-entity NBT.

use super::super::NBTStorageInit;
use super::{LastHurtByMob, LivingEntity};
use crate::entity::NBTStorage;
use crate::entity::attributes::{apply_attributes, pack_attributes};
use pumpkin_data::data_component_impl::EquipmentSlot;
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::potion::Effect;
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_nbt::tag::NbtTag;
use std::sync::atomic::Ordering::Relaxed;
use tracing::warn;

impl LivingEntity {
    pub fn write_living_nbt(&self, nbt: &mut NbtCompound) {
        // Avoid persisting a lethal fall distance when the entity is dead to prevent death loops
        let fall_distance = if self.dead.load(Relaxed) {
            0.0
        } else {
            self.fall_distance.load()
        };
        nbt.put_double("fall_distance", f64::from(fall_distance));
        let invulnerable_time = self.hurt_cooldown.load(Relaxed);
        if invulnerable_time > 0 {
            nbt.put_int("invulnerable_time", invulnerable_time);
        }
        nbt.put("Health", NbtTag::Float(self.health.load()));
        nbt.put_short("DeathTime", i16::from(self.death_time.load(Relaxed)));
        nbt.put("AbsorptionAmount", NbtTag::Float(self.absorption.load()));
        nbt.put_list("attributes", pack_attributes(self));
        {
            let effects_vec: Vec<pumpkin_data::potion::Effect> = {
                let effects = self
                    .active_effects
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                effects.values().cloned().collect()
            };
            if !effects_vec.is_empty() {
                // Iterate effects and create Box<[NbtTag]>
                let mut effects_list = Vec::with_capacity(effects_vec.len());
                for effect in effects_vec {
                    let mut effect_nbt = pumpkin_nbt::compound::NbtCompound::new();
                    effect.write_nbt(&mut effect_nbt);
                    effects_list.push(NbtTag::Compound(effect_nbt));
                }
                nbt.put("active_effects", NbtTag::List(effects_list));
            }
        }
        nbt.put_bool("FallFlying", self.entity.is_fall_flying());
        if let Some(player) = self.last_hurt_by_player.load() {
            nbt.put_uuid("last_hurt_by_player", player);
            nbt.put_int(
                "last_hurt_by_player_memory_time",
                self.last_hurt_by_player_memory_time.load(Relaxed),
            );
        }
        if let Some(reference) = self.last_hurt_by_mob.load() {
            nbt.put_uuid("last_hurt_by_mob", reference.uuid);
            nbt.put_int(
                "ticks_since_last_hurt_by_mob",
                self.entity.tick_count.load(Relaxed) - self.get_last_hurt_by_mob_timestamp(),
            );
        }
        let equipment = {
            let guard = self
                .entity_equipment
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut compound = NbtCompound::new();
            for (slot, stack) in &guard.equipment {
                if !stack.is_empty() {
                    let mut item_nbt = NbtCompound::new();
                    stack.write_item_stack(&mut item_nbt);
                    compound.put(slot.to_name(), NbtTag::Compound(item_nbt));
                }
            }
            compound
        };
        if !equipment.child_tags.is_empty() {
            nbt.put("equipment", NbtTag::Compound(equipment));
        }
    }

    pub fn read_living_nbt_non_mut(&self, nbt: &NbtCompound) {
        // Load fall distance, but if this entity is currently marked dead ensure we don't restore
        // a lethal fall distance that would immediately re-kill on spawn.
        let fall_distance = nbt
            .get_numeric_float("fall_distance")
            .or_else(|| nbt.get_numeric_float("FallDistance"))
            .unwrap_or(0.0);
        if self.dead.load(Relaxed) {
            self.fall_distance.store(0.0);
        } else {
            self.fall_distance.store(fall_distance);
        }
        self.hurt_cooldown
            .store(nbt.get_int("invulnerable_time").unwrap_or(0), Relaxed);

        self.absorption
            .store(nbt.get_numeric_float("AbsorptionAmount").unwrap_or(0.0));

        if let Some(packed) = nbt.get_list("attributes") {
            apply_attributes(self, packed);
        }

        let mut read_effects = Vec::new();
        for effect in nbt.get_list("active_effects").unwrap_or_default() {
            if let NbtTag::Compound(effect_nbt) = effect {
                if let Some(mut effect) = Effect::create_from_nbt(&mut effect_nbt.clone()) {
                    effect.blend = true; // TODO: change, is taken from effect give command
                    read_effects.push(effect);
                } else {
                    warn!("Unable to read effect from nbt");
                }
            }
        }
        let mut active_effects = self
            .active_effects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        active_effects.clear();
        for effect in read_effects {
            active_effects.insert(effect.effect_type, effect);
        }
        self.entity
            .set_glowing_from_effect(active_effects.contains_key(&StatusEffect::GLOWING));

        // Vanilla LivingEntity.readAdditionalSaveData defaults to the mob's own max health,
        // not a flat 20; a hoglin (40 max) or iron golem (100 max) with no saved Health would
        // otherwise be silently reset to 20 here.
        self.health.store(
            nbt.get_numeric_float("Health")
                .unwrap_or_else(|| self.get_max_health()),
        );
        if let Some(death_time) = nbt.get_short("DeathTime") {
            self.death_time.store(death_time as u8, Relaxed);
        }
        self.entity
            .fall_flying
            .store(nbt.get_bool("FallFlying").unwrap_or(false), Relaxed);
        self.last_hurt_by_player
            .store(nbt.get_uuid("last_hurt_by_player"));
        self.last_hurt_by_player_memory_time.store(
            nbt.get_int("last_hurt_by_player_memory_time").unwrap_or(0),
            Relaxed,
        );
        self.last_hurt_by_mob.store(
            nbt.get_uuid("last_hurt_by_mob")
                .map(|uuid| LastHurtByMob { uuid, entity_id: 0 }),
        );
        self.last_hurt_by_mob_timestamp.store(
            self.entity.tick_count.load(Relaxed)
                - nbt.get_int("ticks_since_last_hurt_by_mob").unwrap_or(0),
            Relaxed,
        );

        let mut equipment = self
            .entity_equipment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        equipment.clear();
        for (name, tag) in nbt
            .get_compound("equipment")
            .map_or_else(Default::default, |equipment| equipment.child_tags.iter())
        {
            if let Some(slot) = EquipmentSlot::get_from_name(name)
                && let Some(compound) = tag.extract_compound()
                && let Some(stack) = ItemStack::read_item_stack(compound)
            {
                equipment.put(slot, stack);
            }
        }
    }
}
