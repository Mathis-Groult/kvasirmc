//! Triggers from events involving other entities.

use crate::entity::player::Player;
use crate::entity::player::advancement::predicate::{self, CriterionConditions};

impl Player {
    /// Handles the arm `AdvancementTrigger::PlayerKilledEntity{entity_type_resource,}`.
    pub(super) fn player_killed_entity(&self, entity_type_resource: &str) {
        self.trigger_criterion_event("minecraft:player_killed_entity", |cond| match cond {
            CriterionConditions::PlayerKilledEntity(pred) => {
                pred.matches(entity_type_resource, None, None)
            }
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::BeeNestDestroyed{block_id,num_bees_inside,}`.
    pub(super) fn bee_nest_destroyed(&self, block_id: &str, num_bees_inside: i32) {
        self.trigger_criterion_event("minecraft:bee_nest_destroyed", |cond| match cond {
            CriterionConditions::BeeNestDestroyed {
                block,
                num_bees_inside: req_bees,
            } => {
                block
                    .as_ref()
                    .is_none_or(|b| predicate::matches_id(b, block_id))
                    && req_bees.is_none_or(|r| num_bees_inside >= r)
            }
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::LightningStrike{bystander_type,blocks_set_on_fire,distance,}`.
    pub(super) fn lightning_strike(
        &self,
        bystander_type: Option<&str>,
        blocks_set_on_fire: i32,
        distance: f64,
    ) {
        self.trigger_criterion_event("minecraft:lightning_strike", |cond| match cond {
            CriterionConditions::LightningStrike {
                bystander_type: req_b,
                blocks_set_on_fire: req_fire,
                distance_max,
            } => {
                if let Some(r) = req_b {
                    let Some(b) = bystander_type else {
                        return false;
                    };
                    if !predicate::matches_id(r, b) {
                        return false;
                    }
                }
                if let Some(f) = req_fire
                    && blocks_set_on_fire != *f
                {
                    return false;
                }
                if let Some(max_d) = distance_max
                    && distance > *max_d
                {
                    return false;
                }
                true
            }
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::PlayerInteractedWithEntity{entity_type,item_id,is_baby,player_wearing_safe_armor,}`.
    pub(super) fn player_interacted_with_entity(
        &self,
        entity_type: &str,
        item_id: &str,
        is_baby: bool,
        player_wearing_safe_armor: bool,
    ) {
        self.trigger_criterion_event(
            "minecraft:player_interacted_with_entity",
            |cond| match cond {
                CriterionConditions::PlayerInteractedWithEntity {
                    entity,
                    item,
                    player,
                } => {
                    if let Some(e) = entity
                        && !e.matches(entity_type, None, Some(is_baby))
                    {
                        return false;
                    }
                    if let Some(i) = item
                        && !i.matches(item_id, 1, &[])
                    {
                        return false;
                    }
                    if let Some(p) = player
                        && p.safe_armor_disallowed == Some(true)
                        && player_wearing_safe_armor
                    {
                        return false;
                    }
                    true
                }
                CriterionConditions::Custom { .. } => true,
                _ => false,
            },
        );
    }

    /// Handles the arm `AdvancementTrigger::ThrownItemPickedUpByEntity{entity_type,item_id,is_baby,player_wearing_safe_armor,}`.
    pub(super) fn thrown_item_picked_up_by_entity(
        &self,
        entity_type: &str,
        item_id: &str,
        is_baby: bool,
        player_wearing_safe_armor: bool,
    ) {
        self.trigger_criterion_event(
            "minecraft:thrown_item_picked_up_by_entity",
            |cond| match cond {
                CriterionConditions::ThrownItemPickedUpByEntity {
                    entity,
                    item,
                    player,
                } => {
                    if let Some(e) = entity
                        && !e.matches(entity_type, None, Some(is_baby))
                    {
                        return false;
                    }
                    if let Some(i) = item
                        && !i.matches(item_id, 1, &[])
                    {
                        return false;
                    }
                    if let Some(p) = player
                        && p.safe_armor_disallowed == Some(true)
                        && player_wearing_safe_armor
                    {
                        return false;
                    }
                    true
                }
                CriterionConditions::Custom { .. } => true,
                _ => false,
            },
        );
    }
}
