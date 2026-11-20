//! Triggers from items, containers, recipes and crafting.

use crate::entity::player::Player;
use crate::entity::player::advancement::predicate::{self, CriterionConditions};

impl Player {
    /// Handles the arm `AdvancementTrigger::InventoryChanged`.
    pub(super) fn inventory_changed(&self) {
        let mut inventory_items: Vec<predicate::InventoryItemSnapshot<'_>> = Vec::new();
        let mut slots_full = 0;
        let mut slots_empty = 0;
        let mut slots_occupied = 0;

        if let Ok(main_inv) = self.inventory.main_inventory.try_read() {
            for stack in main_inv.iter() {
                if stack.is_empty() {
                    slots_empty += 1;
                } else {
                    slots_occupied += 1;
                    if stack.item_count >= 64 {
                        slots_full += 1;
                    }
                    inventory_items.push((
                        stack.item.registry_key,
                        u32::from(stack.item_count),
                        &[],
                    ));
                }
            }
        }
        if let Ok(equipment) = self.inventory.entity_equipment.try_lock() {
            for stack in equipment.equipment.values() {
                if !stack.is_empty() {
                    inventory_items.push((
                        stack.item.registry_key,
                        u32::from(stack.item_count),
                        &[],
                    ));
                }
            }
        }

        self.trigger_criterion_event("minecraft:inventory_changed", |cond| match cond {
            CriterionConditions::InventoryChanged(pred) => {
                pred.matches(&inventory_items, slots_full, slots_empty, slots_occupied)
            }
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::ConsumeItem{item_id}`.
    pub(super) fn consume_item(&self, item_id: &str) {
        self.trigger_criterion_event("minecraft:consume_item", |cond| match cond {
            CriterionConditions::ConsumeItem(pred) => pred.matches(item_id, 1, &[]),
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::FishedItem{item_id}`.
    pub(super) fn fished_item(&self, item_id: &str) {
        self.trigger_criterion_event("minecraft:fishing_rod_hooked", |cond| match cond {
            CriterionConditions::FishingRodHooked(pred) => pred.matches(item_id, 1, &[]),
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::AllayDropItemOnBlock{block_id,item_id}`.
    pub(super) fn allay_drop_item_on_block(&self, block_id: &str, item_id: &str) {
        self.trigger_criterion_event("minecraft:allay_drop_item_on_block", |cond| match cond {
            CriterionConditions::AllayDropItemOnBlock(pred) => {
                pred.matches(block_id, item_id, &[], None, None)
            }
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::ThrownItemPickedUpByPlayer{thrower_type}`.
    pub(super) fn thrown_item_picked_up_by_player(&self, thrower_type: &str) {
        self.trigger_criterion_event(
            "minecraft:thrown_item_picked_up_by_player",
            |cond| match cond {
                CriterionConditions::ThrownItemPickedUpByPlayer(pred) => {
                    pred.matches(thrower_type, None, None)
                }
                CriterionConditions::Custom { .. } => true,
                _ => false,
            },
        );
    }

    /// Handles the arm `AdvancementTrigger::FilledBucket{item_id}`.
    pub(super) fn filled_bucket(&self, item_id: &str) {
        self.trigger_criterion_event("minecraft:filled_bucket", |cond| match cond {
            CriterionConditions::FilledBucket(pred) => pred.matches(item_id, 1, &[]),
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::ConstructBeacon{level}`.
    pub(super) fn construct_beacon(&self, level: i32) {
        self.trigger_criterion_event("minecraft:construct_beacon", |cond| match cond {
            CriterionConditions::ConstructBeacon(pred) => pred.matches(level),
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::RecipeCrafted{recipe_id}`.
    pub(super) fn recipe_crafted(&self, recipe_id: &str) {
        self.trigger_criterion_event("minecraft:recipe_crafted", |cond| match cond {
            CriterionConditions::RecipeCrafted(pred) => pred.matches(recipe_id),
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::RecipeUnlocked{recipe_id}`.
    pub(super) fn recipe_unlocked(&self, recipe_id: &str) {
        self.trigger_criterion_event("minecraft:recipe_unlocked", |cond| match cond {
            CriterionConditions::RecipeUnlocked(pred) => pred.matches(recipe_id),
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::PlayerGeneratesContainerLoot{loot_table_id}`.
    pub(super) fn player_generates_container_loot(&self, loot_table_id: &str) {
        self.trigger_criterion_event(
            "minecraft:player_generates_container_loot",
            |cond| match cond {
                CriterionConditions::PlayerGeneratesContainerLoot(lt) => {
                    predicate::matches_id(lt, loot_table_id)
                }
                CriterionConditions::Custom { .. } => true,
                _ => false,
            },
        );
    }

    /// Handles the arm `AdvancementTrigger::EnchantedItem{..}`.
    pub(super) fn enchanted_item(&self) {
        self.trigger_criterion_event("minecraft:enchanted_item", |_cond| true);
    }

    /// Handles the arm `AdvancementTrigger::BrewedPotion`.
    pub(super) fn brewed_potion(&self) {
        self.trigger_criterion_event("minecraft:brewed_potion", |_cond| true);
    }

    /// Handles the arm `AdvancementTrigger::ShotCrossbow{item_id}`.
    pub(super) fn shot_crossbow(&self, item_id: &str) {
        self.trigger_criterion_event("minecraft:shot_crossbow", |cond| match cond {
            CriterionConditions::ShotCrossbow(pred) => pred.matches(item_id, 1, &[]),
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::CrafterRecipeCrafted{recipe_id}`.
    pub(super) fn crafter_recipe_crafted(&self, recipe_id: &str) {
        self.trigger_criterion_event("minecraft:crafter_recipe_crafted", |cond| match cond {
            CriterionConditions::CrafterRecipeCrafted(r) => predicate::matches_id(r, recipe_id),
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::UsedTotem{item_id}`.
    pub(super) fn used_totem(&self, item_id: &str) {
        self.trigger_criterion_event("minecraft:used_totem", |cond| match cond {
            CriterionConditions::UsedTotem(pred) => pred.matches(item_id, 1, &[]),
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }
}
