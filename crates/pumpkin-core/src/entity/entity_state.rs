//! Removal and liveness.

use super::Entity;
use pumpkin_data::entity::EntityPose;
use std::sync::atomic::Ordering::Relaxed;

impl Entity {
    /// Removes the `Entity` from their current `World`
    pub fn remove(&self) {
        self.world.load().remove_entity(self);
    }

    pub fn is_removed(&self) -> bool {
        self.removal_reason.load().is_some()
    }

    pub fn is_alive(&self) -> bool {
        !self.is_removed()
    }

    pub fn reset_state(&self) {
        self.pose.store(EntityPose::Standing);
        self.fall_flying.store(false, Relaxed);
        self.extinguish();
        self.set_on_fire(false);
    }
}
