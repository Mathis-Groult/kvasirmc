//! Triggers from animals, villagers and summoned entities.

use crate::entity::player::Player;
use crate::entity::player::advancement::predicate::CriterionConditions;

impl Player {
    /// Handles the arm `AdvancementTrigger::BredAnimal{parent_type}`.
    pub(super) fn bred_animal(&self, parent_type: &str) {
        self.trigger_criterion_event("minecraft:bred_animals", |cond| match cond {
            CriterionConditions::BredAnimals(pred) => pred.matches(parent_type),
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::CuredZombieVillager`.
    pub(super) fn cured_zombie_villager(&self) {
        self.trigger_criterion_event("minecraft:cured_zombie_villager", |_cond| true);
    }

    /// Handles the arm `AdvancementTrigger::TradedWithVillager`.
    pub(super) fn traded_with_villager(&self) {
        self.trigger_criterion_event("minecraft:villager_trade", |_cond| true);
    }

    /// Handles the arm `AdvancementTrigger::TameAnimal{entity_type}`.
    pub(super) fn tame_animal(&self, entity_type: &str) {
        self.trigger_criterion_event("minecraft:tame_animal", |cond| match cond {
            CriterionConditions::TameAnimal(pred) => pred.matches(entity_type, None, None),
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::HeroOfTheVillage`.
    pub(super) fn hero_of_the_village(&self) {
        self.trigger_criterion_event("minecraft:hero_of_the_village", |_cond| true);
    }

    /// Handles the arm `AdvancementTrigger::SummonedEntity{entity_type}`.
    pub(super) fn summoned_entity(&self, entity_type: &str) {
        self.trigger_criterion_event("minecraft:summoned_entity", |cond| match cond {
            CriterionConditions::SummonedEntity(pred) => pred.matches(entity_type, None, None),
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::EffectsChanged`.
    pub(super) fn effects_changed(&self) {
        self.trigger_criterion_event("minecraft:effects_changed", |_cond| true);
    }
}
