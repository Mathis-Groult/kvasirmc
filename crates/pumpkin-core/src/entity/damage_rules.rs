//! Invulnerability and damage immunity.

use super::player::Player;
use super::{Entity, EntityBase};
use pumpkin_data::damage::DamageType;
use pumpkin_data::tag::{self, Taggable};
use std::sync::atomic::Ordering::{self, Relaxed};

impl Entity {
    /// Vanilla `Entity.isInvulnerableToBase`. Driven by the damage type tags, so datapacks
    /// decide what bypasses invulnerability or counts as fire and fall damage.
    /// `cause` is the attacker (vanilla `DamageSource.getEntity`).
    pub fn is_invulnerable_to(
        &self,
        damage_type: &DamageType,
        cause: Option<&dyn EntityBase>,
    ) -> bool {
        let bypasses = damage_type.has_tag(&tag::DamageType::MINECRAFT_BYPASSES_INVULNERABILITY);
        let creative_cause = cause
            .and_then(EntityBase::get_player)
            .is_some_and(Player::is_creative);

        self.removed.load(Ordering::SeqCst)
            || (self.invulnerable.load(Ordering::Relaxed) && !bypasses && !creative_cause)
            || (damage_type.has_tag(&tag::DamageType::MINECRAFT_IS_FIRE) && self.is_fire_immune())
            || (damage_type.has_tag(&tag::DamageType::MINECRAFT_IS_FALL)
                && self
                    .entity_type
                    .has_tag(&tag::EntityType::MINECRAFT_FALL_DAMAGE_IMMUNE))
            // Plugin immunities, which never block damage that bypasses invulnerability.
            || (!bypasses
                && self
                    .damage_immunities
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .contains(damage_type))
    }

    /// Sets if the entity is invulnerable to a specific damage type
    pub fn set_damage_immunity(&self, damage_type: DamageType, immune: bool) {
        let mut immunities = self
            .damage_immunities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if immune {
            if !immunities.contains(&damage_type) {
                immunities.push(damage_type);
            }
        } else {
            // retain is cleaner than finding index and removing
            immunities.retain(|dt| dt != &damage_type);
        }
    }

    /// Sets if the entity is invulnerable to all damage types (except `GENERIC_KILL` and `OUT_OF_WORLD`)
    pub fn set_invulnerable(&self, invulnerable: bool) {
        self.invulnerable.store(invulnerable, Relaxed);
    }
}
