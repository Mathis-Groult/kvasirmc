//! Status effects and consumables.

use super::super::EntityBase;
use super::{
    EffectParticle, EffectParticles, EffectSource, LivingEntity, consume_effect_probability_applies,
};
use crate::entity::attributes::Modifier;
use crate::entity::attributes::ModifierOperation;
use pumpkin_data::attributes::Attributes;
use pumpkin_data::data_component_impl::Operation;
use pumpkin_data::data_component_impl::food::{ConsumableImpl, ConsumeEffect};
use pumpkin_data::data_component_impl::{EquipmentSlot, EquippableImpl};
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::entity::{EntityStatus, EntityType};
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::potion::Effect;
use pumpkin_data::sound::SoundCategory;
use pumpkin_data::tag::{self, Taggable};
use pumpkin_data::tracked_data;
use pumpkin_data::{damage::DamageType, sound::Sound};
use pumpkin_protocol::bedrock::server::actor_event::ActorEventID;
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::codec::var_ulong::VarULong;
use pumpkin_protocol::java::client::play::CUpdateMobEffect;
use std::sync::atomic::Ordering::Relaxed;

impl LivingEntity {
    /// Undead: healing hurts them, harming heals them.
    #[must_use]
    pub fn is_inverted_heal_and_harm(&self) -> bool {
        self.entity
            .entity_type
            .has_tag(&tag::EntityType::MINECRAFT_INVERTED_HEALING_AND_HARM)
    }

    /// Vanilla `HealOrHarmMobEffect`. False for other effects.
    /// `scale` from splash distance or 1 when drunk, `None` for an added effect,
    /// whose heal never goes negative. With a `source`, harm is indirect magic
    /// credited to it and its owner.
    pub fn apply_instant_effect(
        &self,
        effect_type: &StatusEffect,
        amplifier: u8,
        scale: Option<f64>,
        source: Option<EffectSource<'_>>,
    ) -> bool {
        let is_harm = if effect_type == &StatusEffect::INSTANT_DAMAGE {
            true
        } else if effect_type == &StatusEffect::INSTANT_HEALTH {
            false
        } else {
            return false;
        };
        // Java's int shift: wraps the count, so high amplifiers stay in range.
        let scaled = |base: i32| {
            let shifted = base.wrapping_shl(u32::from(amplifier));
            scale.map_or(shifted, |scale| (scale * f64::from(shifted) + 0.5) as i32) as f32
        };
        if is_harm == self.is_inverted_heal_and_harm() {
            let amount = scaled(4);
            self.heal(if scale.is_none() {
                amount.max(0.0)
            } else {
                amount
            });
        } else if let Some(dyn_self) = self
            .entity
            .world
            .load()
            .get_entity_by_id(self.entity.entity_id)
        {
            let (damage_type, direct, owner) = source
                .map_or((DamageType::MAGIC, None, None), |s| {
                    (DamageType::INDIRECT_MAGIC, Some(s.direct), s.owner)
                });
            dyn_self.damage_with_context(&*dyn_self, scaled(6), damage_type, None, direct, owner);
        }
        true
    }

    // canBeAffected, with the mob overrides folded in
    pub(super) fn can_be_affected(&self, effect: &Effect) -> bool {
        let entity_type = self.entity.entity_type;
        if (entity_type == &EntityType::WITHER_SKELETON || entity_type == &EntityType::WITHER)
            && effect.effect_type == &StatusEffect::WITHER
        {
            return false;
        }

        if entity_type.has_tag(&tag::EntityType::MINECRAFT_IMMUNE_TO_INFESTED) {
            effect.effect_type != &StatusEffect::INFESTED
        } else if entity_type.has_tag(&tag::EntityType::MINECRAFT_IMMUNE_TO_OOZING) {
            effect.effect_type != &StatusEffect::OOZING
        } else if entity_type.has_tag(&tag::EntityType::MINECRAFT_IGNORES_POISON_AND_REGEN) {
            effect.effect_type != &StatusEffect::REGENERATION
                && effect.effect_type != &StatusEffect::POISON
        } else {
            true
        }
    }

    #[expect(clippy::too_many_lines)]
    pub fn add_effect(&self, effect: Effect) {
        if !self.can_be_affected(&effect) {
            return;
        }

        let mut effect_event =
            crate::plugin::api::events::entity::entity_potion_effect::EntityPotionEffectEvent::new(
                self.entity.entity_id,
                effect.effect_type.translation_key.to_string(),
                effect.duration,
                effect.amplifier,
            );
        if let Some(server) = self.entity.world.load().server.upgrade() {
            server
                .plugin_manager
                .fire_blocking(&server, &mut effect_event);
        }
        if effect_event.cancelled {
            return;
        }

        // Like vanilla, instant effects are never sent or stored as active effects.
        if self.apply_instant_effect(effect.effect_type, effect.amplifier, None, None) {
            return;
        }

        // Apply non-instant effects

        // Effects that modify attributes (ex. speed) should also update the
        // entity's attribute instances (server-side) and then notify clients.
        if !effect.effect_type.attribute_modifiers.is_empty() {
            // Apply each attribute modifier into the local AttributeInstance
            for m in effect.effect_type.attribute_modifiers {
                let id = m.id.to_string();
                let op = match m.operation {
                    Operation::AddValue => ModifierOperation::Add,
                    Operation::AddMultipliedBase => ModifierOperation::MultiplyBase,
                    Operation::AddMultipliedTotal => ModifierOperation::MultiplyTotal,
                };
                let scaled_amount = m.base_value * (f64::from(effect.amplifier) + 1.);
                let mod_inst = Modifier {
                    id,
                    amount: scaled_amount,
                    operation: op,
                };

                self.update_attribute(m.attribute, |inst| {
                    inst.add_or_replace_permanent_modifier(mod_inst.clone());
                });
            }

            // Recompute packet modifiers from active effects for each affected attribute
            let mut touched_attrs: Vec<pumpkin_data::attributes::Attributes> = Vec::new();
            for m in effect.effect_type.attribute_modifiers {
                if !touched_attrs.iter().any(|a| a.id == m.attribute.id) {
                    touched_attrs.push(m.attribute.clone());
                }
            }

            if !touched_attrs.is_empty() {
                crate::entity::attributes::send_attribute_updates_for_living(self, touched_attrs);
            }
        }

        // Apply absorption effect (+4 absorption per level)
        if effect.effect_type == &StatusEffect::ABSORPTION {
            let added = 4.0 * (effect.amplifier as f32 + 1.0);
            let max_abs = self.get_attribute_value(&Attributes::MAX_ABSORPTION) as f32;
            let new_abs = (self.absorption.load() + added).min(max_abs);
            self.set_absorption(new_abs);
        }

        // Apply invisible effect
        if effect.effect_type == &StatusEffect::INVISIBILITY {
            self.entity.set_invisible(true);
        }

        // Apply glowing effect
        if effect.effect_type == &StatusEffect::GLOWING {
            self.entity.set_glowing_from_effect(true);
        }

        // Broadcast effect to nearby players
        let mut flag: i8 = 0;
        if effect.ambient {
            flag |= 1;
        }
        if effect.show_particles {
            flag |= 2;
        }
        if effect.show_icon {
            flag |= 4;
        }
        if effect.blend {
            flag |= 8;
        }

        let je_packet = CUpdateMobEffect::new(
            self.entity.entity_id.into(),
            VarInt(i32::from(effect.effect_type.id)),
            effect.amplifier.into(),
            effect.duration.into(),
            flag,
        );

        let be_packet = pumpkin_protocol::bedrock::client::CMobEffect {
            target_runtime_id: VarULong(self.entity.entity_id as u64),
            event_id: pumpkin_protocol::bedrock::client::CMobEffect::EVENT_ADD,
            effect_id: VarInt(effect.effect_type.to_bedrock_id()),
            effect_amplifier: VarInt(i32::from(effect.amplifier)),
            show_particles: effect.show_particles,
            effect_duration_ticks: VarInt(effect.duration),
            tick: VarULong(0),
            ambient: effect.ambient,
        };

        let chunk_pos = self.entity.chunk_pos.load();
        self.entity
            .world
            .load()
            .broadcast_to_chunk_editioned(chunk_pos, &je_packet, &be_packet);
        if effect.effect_type != &StatusEffect::INSTANT_HEALTH
            && effect.effect_type != &StatusEffect::INSTANT_DAMAGE
        {
            self.active_effects
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(effect.effect_type, effect);
        }
        self.sync_effect_particles();
    }

    pub(super) fn sync_effect_particles(&self) {
        let effects = self
            .active_effects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let has_effects = !effects.is_empty();
        let particles = EffectParticles(
            effects
                .values()
                .filter(|effect| effect.show_particles)
                .map(EffectParticle::from_effect)
                .collect(),
        );
        let ambient = effects
            .values()
            .filter(|effect| effect.show_particles)
            .all(|effect| effect.ambient);
        drop(effects);

        self.entity
            .set_synced_data(tracked_data::living_entity::EFFECT_PARTICLES, particles);
        if has_effects {
            self.entity
                .set_synced_data(tracked_data::living_entity::EFFECT_AMBIENCE_ID, ambient);
        }
    }

    /// Vanilla `LivingEntity.removeAllEffects`: true when there was any effect to remove.
    pub fn remove_all_effects(&self) -> bool {
        let effects: Vec<_> = self
            .active_effects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .copied()
            .collect();
        let any = !effects.is_empty();
        for effect in effects {
            self.remove_effect(effect);
        }
        any
    }

    pub fn remove_effect(&self, effect_type: &'static StatusEffect) -> bool {
        // Remove the effect
        let succeeded = self
            .active_effects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&effect_type)
            .is_some();

        // Broadcast effect removal
        self.entity
            .world
            .load()
            .send_remove_mob_effect(&self.entity, effect_type);

        // Remove attribute modifiers, if any
        if !effect_type.attribute_modifiers.is_empty() {
            let mut touched_attrs = Vec::new();

            for m in effect_type.attribute_modifiers {
                let id = m.id.to_string();

                // Clean local server state
                self.update_attribute(m.attribute, |inst| {
                    inst.remove_modifier(&id);
                });

                // Track unique attributes for the packet update
                if !touched_attrs
                    .iter()
                    .any(|a: &Attributes| a.id == m.attribute.id)
                {
                    touched_attrs.push(m.attribute.clone());
                }
            }

            // Sync the clean state to the client
            if !touched_attrs.is_empty() {
                crate::entity::attributes::send_attribute_updates_for_living(self, touched_attrs);
            }
        }

        // If absorption effect removed, clear current absorption amount and notify clients
        if effect_type == &StatusEffect::ABSORPTION {
            self.set_absorption(0.0);
        }

        // If health boost effect removed, clamp current health to new max and notify clients
        if effect_type == &StatusEffect::HEALTH_BOOST {
            let new_max = self.get_max_health();
            if self.health.load() > new_max {
                // Update local health and send both health and absorption metadata together
                self.set_health(new_max.max(0.0));
            }
        }

        // If invisible effect removed, disable invisibility
        if effect_type == &StatusEffect::INVISIBILITY {
            self.entity.set_invisible(false);
        }

        // If glowing effect removed, disable glowing
        if effect_type == &StatusEffect::GLOWING {
            self.entity.set_glowing_from_effect(false);
        }

        if succeeded {
            self.sync_effect_particles();
        }

        succeeded
    }

    pub fn has_effect(&self, effect: &'static StatusEffect) -> bool {
        self.active_effects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(&effect)
    }

    pub fn get_effect(&self, effect: &'static StatusEffect) -> Option<Effect> {
        self.active_effects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&effect)
            .cloned()
    }

    pub(super) fn tick_effects(&self) {
        let mut effects_to_remove = Vec::new();
        let mut effects_to_apply = Vec::new();

        {
            let Ok(mut effects) = self.active_effects.try_lock() else {
                return;
            };
            let entity_age = self.entity.tick_count.load(Relaxed);
            for effect in effects.values_mut() {
                if effect.duration == 0 {
                    effects_to_remove.push(effect.effect_type);
                    continue;
                }

                let tick_duration = if effect.duration == -1 {
                    entity_age
                } else {
                    effect.duration
                };

                if let Some(mob_effect) = crate::entity::effect::get_mob_effect(effect.effect_type)
                    && mob_effect.should_apply_effect_tick(tick_duration, effect.amplifier)
                {
                    effects_to_apply.push((mob_effect, effect.amplifier));
                }

                if effect.duration != -1 {
                    effect.duration -= 1;
                }
            }
        }

        // Call the central removal function for each expired effect
        for effect_type in effects_to_remove {
            self.remove_effect(effect_type);
        }

        for (mob_effect, amplifier) in effects_to_apply {
            mob_effect.apply_effect_tick(self, amplifier);
        }
    }

    /// Applies data-driven `apply_effects` consume effects after an item completes use.
    /// Vanilla: `Consumable.onConsume` invokes every configured effect server-side.
    pub(super) fn apply_consumable_effects(&self, caller: &dyn EntityBase, item: &ItemStack) {
        let Some(consumable) = item.get_data_component::<ConsumableImpl>() else {
            return;
        };

        for consume_effect in consumable.effects.iter() {
            match consume_effect {
                ConsumeEffect::ApplyEffects((effects, probability)) => {
                    if !consume_effect_probability_applies(*probability, rand::random()) {
                        continue;
                    }

                    for effect in effects.iter() {
                        let Some(effect_type) =
                            StatusEffect::from_minecraft_name(&effect.effect_id)
                        else {
                            continue;
                        };
                        let Ok(amplifier) = u8::try_from(effect.amplifier) else {
                            continue;
                        };

                        self.add_effect(Effect {
                            effect_type,
                            duration: effect.duration,
                            amplifier,
                            ambient: effect.ambient,
                            show_particles: effect.show_particles,
                            show_icon: effect.show_icon,
                            blend: false,
                        });
                    }
                }
                ConsumeEffect::ClearAllEffects => {
                    self.reset_effects_and_attributes();
                }
                ConsumeEffect::RemoveEffects(idset) => {
                    if let pumpkin_data::data_component_impl::IDSet::IDs(ids) = idset {
                        for effect_type in ids.iter() {
                            self.remove_effect(effect_type);
                        }
                    }
                }
                ConsumeEffect::TeleportRandomly(diameter) => {
                    // Java Edition dismounts the consumer before random teleport attempts.
                    let vehicle = caller
                        .get_entity()
                        .vehicle
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clone();
                    if let Some(vehicle) = vehicle {
                        vehicle
                            .get_entity()
                            .remove_passenger_sync(caller.get_entity().entity_id);
                        if caller.get_entity().has_vehicle() {
                            continue;
                        }
                    }

                    let center = self.entity.pos.load();
                    let Some(pos) = self.find_random_teleport_target(*diameter) else {
                        continue;
                    };
                    let (yaw, pitch) = (self.entity.yaw.load(), self.entity.pitch.load());
                    let world = self.entity.world.load_full();
                    caller.teleport(pos, Some(yaw), Some(pitch), world.clone());

                    let destination = self.entity.pos.load();
                    if destination != center {
                        self.fall_distance.store(0.0);
                        // Vanilla broadcasts entity event 46 (teleport particles) on success.
                        world.send_entity_status(
                            &self.entity,
                            EntityStatus::Teleport,
                            ActorEventID::None,
                        );
                        world.emit_game_event("teleport", center);
                        world.play_sound(
                            Sound::ItemChorusFruitTeleport,
                            SoundCategory::Players,
                            &destination,
                        );
                    }
                }
                ConsumeEffect::PlaySound(sound) => {
                    if let pumpkin_data::data_component_impl::IdOr::Id(sound) = sound {
                        self.entity.world.load().play_sound(
                            *sound,
                            SoundCategory::Neutral,
                            &self.entity.pos.load(),
                        );
                    }
                }
            }
        }
    }

    /// Mirrors vanilla `LivingEntity.getVisibilityPercent`.
    #[must_use]
    pub fn visibility_percent(&self, targeting_entity_type: Option<&'static EntityType>) -> f64 {
        use pumpkin_data::data_component_impl::{IDSet, MobVisibilityImpl};

        let mut percent = 1.0;
        if self.entity.is_sneaking() {
            percent *= 0.8;
        }
        let equipment = self
            .entity_equipment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self
            .entity
            .invisible
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            let worn = [
                EquipmentSlot::HEAD,
                EquipmentSlot::CHEST,
                EquipmentSlot::LEGS,
                EquipmentSlot::FEET,
            ]
            .iter()
            .filter(|slot| !equipment.get(slot).is_empty())
            .count();
            // Clamped in f32 like vanilla's `0.1F`.
            let cover = (worn as f32 / 4.0).max(0.1);
            percent *= 0.7 * f64::from(cover);
        }
        if let Some(target_type) = targeting_entity_type {
            for slot in [
                EquipmentSlot::MAIN_HAND,
                EquipmentSlot::OFF_HAND,
                EquipmentSlot::FEET,
                EquipmentSlot::LEGS,
                EquipmentSlot::CHEST,
                EquipmentSlot::HEAD,
                EquipmentSlot::BODY,
                EquipmentSlot::SADDLE,
            ] {
                let stack = equipment.get(&slot);
                if let Some(mob_visibility) = stack.get_data_component::<MobVisibilityImpl>()
                    && let Some(equippable) = stack.get_data_component::<EquippableImpl>()
                    && *equippable.slot == slot
                {
                    let matches_target = match &mob_visibility.targeting_entity_types {
                        IDSet::Tag(tag) => target_type.is_tagged_with(tag).unwrap_or(false),
                        IDSet::IDs(ids) => ids.iter().any(|id| id.id == target_type.id),
                    };
                    if matches_target {
                        percent *= f64::from(mob_visibility.visibility);
                    }
                }
            }
        }
        percent.clamp(0.0, 10.0)
    }
}
