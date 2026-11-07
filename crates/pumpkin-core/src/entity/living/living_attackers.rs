//! Who hurt this entity last and who gets the credit.

use super::super::EntityBase;
use super::{LastHurtByMob, LivingEntity};
use crate::entity::mob::neutral::find_by_uuid;
use pumpkin_data::damage::DamageType;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::atomic::Ordering::Relaxed;

impl LivingEntity {
    pub fn get_last_hurt_by_mob(&self) -> Option<Arc<dyn EntityBase>> {
        let reference = self.last_hurt_by_mob.load()?;
        let world = self.entity.world.load();
        if reference.entity_id != 0 {
            return world.get_entity_by_id(reference.entity_id);
        }
        let attacker = find_by_uuid(&world, reference.uuid)?;
        self.last_hurt_by_mob.store(Some(LastHurtByMob {
            entity_id: attacker.get_entity().entity_id,
            ..reference
        }));
        Some(attacker)
    }

    pub fn get_last_hurt_by_mob_id(&self) -> Option<i32> {
        match self.last_hurt_by_mob.load()?.entity_id {
            0 => self
                .get_last_hurt_by_mob()
                .map(|attacker| attacker.get_entity().entity_id),
            entity_id => Some(entity_id),
        }
    }

    pub fn get_last_hurt_by_mob_timestamp(&self) -> i32 {
        self.last_hurt_by_mob_timestamp.load(Relaxed)
    }

    pub fn set_last_hurt_by_mob(&self, attacker: Option<&dyn EntityBase>) {
        self.last_hurt_by_mob.store(attacker.map(|attacker| {
            let entity = attacker.get_entity();
            LastHurtByMob {
                uuid: entity.entity_uuid,
                entity_id: entity.entity_id,
            }
        }));
        self.last_hurt_by_mob_timestamp
            .store(self.entity.tick_count.load(Relaxed), Relaxed);
    }

    pub(super) fn tick_last_hurt_by_mob(&self) {
        if self.last_hurt_by_mob.load().is_none() {
            return;
        }
        if self.entity.tick_count.load(Relaxed) - self.get_last_hurt_by_mob_timestamp() > 100
            || self
                .get_last_hurt_by_mob()
                .is_some_and(|attacker| !attacker.get_living_entity().is_some_and(Self::is_alive))
        {
            self.set_last_hurt_by_mob(None);
        }
    }

    pub fn get_last_hurt_by_player(&self) -> Option<Arc<crate::entity::player::Player>> {
        let uuid = self.last_hurt_by_player.load()?;
        self.entity.world.load().get_player_by_uuid(uuid)
    }

    /// Returns the entity that should receive kill credit for this entity's death.
    /// Following vanilla Java logic (`LivingEntity.getKillCredit`):
    /// 1. Prioritize `last_hurt_by_player`, which is forgotten 100 ticks after the hit.
    /// 2. Then `last_hurt_by_mob`, which is cleared 100 ticks after the hit.
    pub fn get_kill_credit(&self) -> Option<Arc<dyn EntityBase>> {
        if self.last_hurt_by_player.load().is_some() {
            return self
                .get_last_hurt_by_player()
                .map(|player| player as Arc<dyn EntityBase>);
        }

        if let Some(mob) = self.get_last_hurt_by_mob() {
            return Some(mob);
        }

        None
    }

    pub fn was_recently_stabbed(&self, target_id: i32, now: i32, allowed_ticks: i32) -> bool {
        self.recent_kinetic_enemies
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&target_id)
            .is_some_and(|stabbed_at| now - stabbed_at < allowed_ticks)
    }

    pub fn remember_stabbed_entity(&self, target_id: i32, now: i32) {
        self.recent_kinetic_enemies
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(target_id, now);
    }

    /// Forgotten after 40 ticks.
    pub fn get_last_damage_type(&self) -> Option<DamageType> {
        self.get_last_damage_source()
            .map(|(damage_type, _)| damage_type)
    }

    /// Vanilla `getLastDamageSource`: the last damage type and the entity id of its causing
    /// entity (0 when none), forgotten after 40 ticks.
    pub fn get_last_damage_source(&self) -> Option<(DamageType, i32)> {
        let stamp = self.last_damage_stamp.load(Ordering::Relaxed);
        let mut last = self
            .last_damage_type
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.entity.world.load().get_world_age() - stamp > 40 {
            *last = None;
        }
        last.map(|damage_type| (damage_type, self.last_damage_attacker.load(Relaxed)))
    }
}
