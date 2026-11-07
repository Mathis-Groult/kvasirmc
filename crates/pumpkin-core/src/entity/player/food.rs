//! Hunger, saturation and exhaustion.

use super::Player;
use core::f32;

impl Player {
    // --- Hunger & Saturation Aliases ---
    pub fn get_food_saturation(&self) -> f32 {
        self.get_saturation()
    }

    pub fn set_food_saturation(&self, saturation: f32) {
        self.set_saturation(saturation);
    }

    pub fn get_food_exhaustion(&self) -> f32 {
        self.get_exhaustion()
    }

    pub fn set_food_exhaustion(&self, exhaustion: f32) {
        self.set_exhaustion(exhaustion);
    }

    #[must_use]
    pub fn can_eat(&self, can_always_eat: bool) -> bool {
        self.abilities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .invulnerable
            || can_always_eat
            || self.hunger_manager.level.load() < 20
    }

    pub fn can_food_heal(&self) -> bool {
        let health = self.living_entity.health.load();
        let max_health = self.living_entity.get_max_health();
        health > 0.0 && health < max_health
    }

    pub fn add_exhaustion(&self, exhaustion: f32) {
        if self
            .abilities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .invulnerable
        {
            return;
        }
        let mut exhaustion_event =
            crate::plugin::api::events::entity::entity_exhaustion::EntityExhaustionEvent::new(
                self.entity_id(),
                exhaustion,
            );
        if let Some(server) = self.world().server.upgrade() {
            server
                .plugin_manager
                .fire_blocking(&server, &mut exhaustion_event);
        }
        if exhaustion_event.cancelled {
            return;
        }
        self.hunger_manager
            .add_exhaustion(exhaustion_event.exhaustion);
    }

    pub fn get_food_level(&self) -> u8 {
        self.hunger_manager.level.load()
    }

    pub fn set_food_level(&self, food_level: u8) {
        let mut food_event =
            crate::plugin::api::events::entity::food_level_change::FoodLevelChangeEvent::new(
                self.living_entity.entity.entity_id,
                food_level,
            );
        if let Some(server) = self.world().server.upgrade() {
            server
                .plugin_manager
                .fire_blocking(&server, &mut food_event);
        }
        if food_event.cancelled {
            return;
        }
        self.hunger_manager.set_level(food_event.food_level);
        self.send_health();
    }

    pub fn get_saturation(&self) -> f32 {
        self.hunger_manager.saturation.load()
    }

    pub fn set_saturation(&self, saturation: f32) {
        self.hunger_manager.set_saturation(saturation);
        self.send_health();
    }

    pub fn get_exhaustion(&self) -> f32 {
        self.hunger_manager.get_exhaustion()
    }

    pub fn set_exhaustion(&self, exhaustion: f32) {
        self.hunger_manager.set_exhaustion(exhaustion);
        self.send_health();
    }
}
