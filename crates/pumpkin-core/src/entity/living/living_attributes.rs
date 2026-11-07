//! Attribute values and their refresh.

use super::super::EntityBase;
use super::LivingEntity;
use crate::entity::attributes::AttributeInstance;
use pumpkin_data::attributes::Attributes;

impl LivingEntity {
    /// Convenience helper to mutate an attribute instance. Automatically inserts
    /// a new instance populated from the registry base if needed.
    pub fn update_attribute<F: FnOnce(&mut AttributeInstance)>(
        &self,
        attribute: &Attributes,
        f: F,
    ) {
        let mut map = self
            .attributes
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let inst = map.entry(attribute.id).or_insert_with(|| {
            let base = self
                .entity
                .entity_type
                .attributes
                .iter()
                .find(|a| a.0.id == attribute.id)
                .map_or_else(
                    || {
                        tracing::debug!(
                            "Entity type {} has no base value for attribute {}; falling back to default {}",
                            self.entity.entity_type.resource_name,
                            attribute.name,
                            attribute.default_value,
                        );
                        attribute.default_value
                    },
                    |a| a.1,
                );
            AttributeInstance::new(base)
        });

        f(inst);
        inst.mark_dirty();
    }

    /// Vanilla `refreshDirtyAttributes`: reports every attribute modified since the last refresh.
    pub(super) fn refresh_dirty_attributes(&self, caller: &dyn EntityBase) {
        let updated: Vec<u8> = self
            .attributes
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter_mut()
            .filter_map(|(id, instance)| std::mem::take(instance.updated.get_mut()).then_some(*id))
            .collect();
        if let Some(mob) = caller.get_mob() {
            for attribute in updated {
                mob.get_mob_entity().on_attribute_updated(attribute);
            }
        }
    }

    /// Returns the computed value for `attribute` using the local instance, falling back
    /// to `attribute.default_value` if no local instance exists.
    pub fn get_attribute_value(&self, attribute: &Attributes) -> f64 {
        let map = self
            .attributes
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.get(&attribute.id)
            .map_or(attribute.default_value, AttributeInstance::value)
    }

    /// Returns the base attribute value for `attribute` for this entity's type.
    pub fn get_attribute_base(&self, attribute: &Attributes) -> f64 {
        // Check the local base value first (could be modified)
        let map = self
            .attributes
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(instance) = map.get(&attribute.id) {
            return instance.base_value;
        }

        // Fall back to registry base value if no local instance exists
        self.entity
            .entity_type
            .attributes
            .iter()
            .find(|a| a.0.id == attribute.id)
            .map_or(attribute.default_value, |a| a.1)
    }

    /// Update or insert the base value for an attribute on this entity.
    /// If the attribute doesn't exist locally yet, it will be inserted.
    pub fn set_attribute_base(&self, attribute: &Attributes, new_base: f64) {
        let mut map = self
            .attributes
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(inst) = map.get_mut(&attribute.id) {
            inst.base_value = new_base;
            inst.mark_dirty();
        } else {
            let ai = AttributeInstance::new(new_base);
            ai.mark_dirty();
            map.insert(attribute.id, ai);
        }
    }

    pub fn reset_effects_and_attributes(&self) {
        // Clear active effects and reset modified attributes
        let effects_to_remove: Vec<_> = {
            let lock = self
                .active_effects
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            lock.keys().copied().collect()
        };

        for effect_type in effects_to_remove {
            self.remove_effect(effect_type);
        }
    }
}
