//! Triggers from using, damaging and shearing with items.

use crate::entity::player::Player;
use crate::entity::player::advancement::predicate::CriterionConditions;

impl Player {
    /// Handles the arm `AdvancementTrigger::ItemUsedOnBlock{block_id,item_id,state,smokey,biome,}`.
    pub(super) fn item_used_on_block(
        &self,
        block_id: &str,
        item_id: &str,
        state: Option<&(String, String)>,
        smokey: Option<bool>,
        biome: Option<&str>,
    ) {
        self.trigger_criterion_event("minecraft:item_used_on_block", |cond| match cond {
            CriterionConditions::ItemUsedOnBlock(pred) => pred.matches(
                block_id,
                item_id,
                state.as_ref().map(|(k, v)| (k.as_str(), v.as_str())),
                smokey,
                biome,
            ),
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::UsingItem{item_id,looking_at,}`.
    pub(super) fn using_item(&self, item_id: &str, looking_at: Option<&str>) {
        self.trigger_criterion_event("minecraft:using_item", |cond| match cond {
            CriterionConditions::UsingItem {
                item,
                looking_at: exp_look,
            } => {
                if !item.matches(item_id, 1, &[]) {
                    return false;
                }
                if let Some(expected) = exp_look {
                    let Some(actual) = looking_at else {
                        return false;
                    };
                    if !expected.matches(pumpkin_data::tag::RegistryKey::EntityType, actual) {
                        return false;
                    }
                }
                true
            }
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::ItemDurabilityChanged{item_id,vehicle_type,}`.
    pub(super) fn item_durability_changed(&self, item_id: &str, vehicle_type: Option<&str>) {
        self.trigger_criterion_event("minecraft:item_durability_changed", |cond| match cond {
            CriterionConditions::ItemDurabilityChanged {
                item,
                vehicle_type: exp_vt,
            } => {
                if !item.matches(item_id, 1, &[]) {
                    return false;
                }
                if let Some(expected) = exp_vt {
                    let Some(actual) = vehicle_type else {
                        return false;
                    };
                    if !expected.matches(pumpkin_data::tag::RegistryKey::EntityType, actual) {
                        return false;
                    }
                }
                true
            }
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::PlayerShearedEquipment{entity_type,item_id,}`.
    pub(super) fn player_sheared_equipment(&self, entity_type: &str, item_id: &str) {
        self.trigger_criterion_event("minecraft:player_sheared_equipment", |cond| match cond {
            CriterionConditions::PlayerShearedEquipment { entity, item } => {
                entity
                    .as_ref()
                    .is_none_or(|e| e.matches(entity_type, None, None))
                    && item.as_ref().is_none_or(|i| i.matches(item_id, 1, &[]))
            }
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }
}
