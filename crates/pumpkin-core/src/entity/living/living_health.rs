//! Health, maximum health and absorption.

use super::LivingEntity;
use pumpkin_data::attributes::Attributes;
use pumpkin_data::entity::EntityType;
use pumpkin_data::tracked_data;
use std::sync::atomic::Ordering::Relaxed;

impl LivingEntity {
    pub fn is_dead_or_dying(&self) -> bool {
        self.health.load() <= 0.0 || self.dead.load(Relaxed)
    }

    /// Like vanilla, the dead are not healed and a negative amount takes health.
    ///
    /// A negative amount can only come from a splash or lingering instant health
    /// potion with a custom amplifier of 29 mod 32 (`4 << 29` overflows). In vanilla:
    /// 1. Health is clamped to 0, `die` is never called.
    /// 2. Mobs then play the death animation and vanish after 20 ticks, without drops.
    /// 3. A player at 0 health gets the death screen and respawns, without drops.
    ///
    /// Kept for parity, the tick and respawn paths already treat 0 health like vanilla.
    // TODO: config option to prevent this (clamp the amount or run the real death flow).
    pub fn heal(&self, additional_health: f32) {
        if self.health.load() <= 0.0 || additional_health == 0.0 {
            return;
        }
        let mut event =
            crate::plugin::api::events::entity::entity_regain_health::EntityRegainHealthEvent::new(
                self.entity.entity_id,
                additional_health,
            );
        if let Some(server) = self.entity.world.load().server.upgrade() {
            server.plugin_manager.fire_blocking(&server, &mut event);
            if event.cancelled {
                return;
            }
        }
        // A handler may have changed health, or killed the entity.
        let health = self.health.load();
        if health > 0.0 {
            self.set_health(health + additional_health);
        }
    }

    pub fn set_health(&self, health: f32) {
        // Clamp to [0, max_health]
        let max_health = self.get_max_health();
        let clamped = health.max(0.0).min(max_health);
        self.health.store(clamped);
        // tell everyone entities health changed
        self.entity
            .set_synced_data(tracked_data::living_entity::DATA_HEALTH_ID, clamped);
    }

    /// Returns the current maximum health for this entity
    pub fn get_max_health(&self) -> f32 {
        self.get_attribute_value(&Attributes::MAX_HEALTH) as f32
    }

    /// Sets the maximum health for this entity
    pub fn set_max_health(&self, max_health: f32) {
        // Update base attribute
        self.set_attribute_base(&Attributes::MAX_HEALTH, max_health as f64);

        // Broadcast the attribute change
        crate::entity::attributes::send_attribute_updates_for_living(
            self,
            vec![Attributes::MAX_HEALTH],
        );

        // Clamp current health to new max if needed and send metadata update
        let current_health = self.health.load();
        if current_health > max_health {
            self.set_health(max_health);
        }
    }

    /// Returns the current absorption amount for this entity (yellow hearts)
    pub fn get_absorption(&self) -> f32 {
        self.absorption.load()
    }

    /// Sets the current absorption amount for this entity (yellow hearts)
    pub fn set_absorption(&self, new_abs: f32) {
        // Must be at least 0
        let new_abs = new_abs.max(0.0);

        // Set local state
        self.absorption.store(new_abs);

        // Broadcast attribute update for max_absorption so clients receive
        // the updated absorption value via the attribute packet.
        crate::entity::attributes::send_attribute_updates_for_living(
            self,
            vec![Attributes::MAX_ABSORPTION],
        );

        // Send absorption metadata for players (visual yellow hearts)
        if self.entity.entity_type == &EntityType::PLAYER {
            self.entity
                .set_synced_data(tracked_data::player::DATA_PLAYER_ABSORPTION_ID, new_abs);
        }
    }

    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.entity.is_alive() && self.health.load() > 0.0
    }
}
