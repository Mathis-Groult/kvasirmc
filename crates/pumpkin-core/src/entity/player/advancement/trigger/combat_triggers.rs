//! Triggers from fighting, damage and falling.

use crate::entity::player::Player;
use crate::entity::player::advancement::predicate::{self, CriterionConditions};
use pumpkin_util::math::position::BlockPos;

impl Player {
    /// Handles the arm `AdvancementTrigger::SniperDuel`.
    pub(super) fn sniper_duel(&self) {
        self.trigger_criterion_event("minecraft:player_killed_entity", |cond| match cond {
            CriterionConditions::PlayerKilledEntity(pred) => {
                pred.matches("minecraft:skeleton", Some(50.0), None)
            }
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::TwoBirdsOneArrow|AdvancementTrigger::Arbalistic`.
    pub(super) fn two_birds_one_arrow_or_arbalistic(&self) {
        self.trigger_criterion_event("minecraft:killed_by_arrow", |_cond| true);
    }

    /// Handles the arm `AdvancementTrigger::DeflectedDamage`.
    pub(super) fn deflected_damage(&self) {
        self.trigger_criterion_event("minecraft:entity_hurt_player", |cond| match cond {
            CriterionConditions::EntityHurtPlayer(pred) => pred.matches(true, true, None),
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::PlayerKilled`.
    pub(super) fn player_killed(&self) {
        self.trigger_criterion_event("minecraft:entity_killed_player", |_cond| true);
    }

    /// Handles the arm `AdvancementTrigger::DealtOverkillDamage`.
    pub(super) fn dealt_overkill_damage(&self) {
        self.trigger_criterion_event("minecraft:player_hurt_entity", |cond| match cond {
            CriterionConditions::PlayerHurtEntity(pred) => pred.matches(false, false, Some(100.0)),
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::Bullseye`.
    pub(super) fn bullseye(&self) {
        self.trigger_criterion_event("minecraft:target_hit", |cond| match cond {
            CriterionConditions::TargetHit(pred) => pred.matches(15, Some(30.0)),
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::Levitation{distance_y}`.
    pub(super) fn levitation(&self, distance_y: f64) {
        self.trigger_criterion_event("minecraft:levitation", |cond| match cond {
            CriterionConditions::Levitation {
                distance_y: Some(bounds),
            } => bounds.matches(distance_y),
            CriterionConditions::Levitation { distance_y: None }
            | CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::FallAfterExplosion{distance_y,cause}`.
    pub(super) fn fall_after_explosion(&self, distance_y: f64, cause: &str) {
        self.trigger_criterion_event("minecraft:fall_after_explosion", |cond| match cond {
            CriterionConditions::FallAfterExplosion {
                distance_y: Some(bounds),
                cause_entity,
            } => {
                bounds.matches(distance_y)
                    && cause_entity
                        .as_ref()
                        .is_none_or(|c| predicate::matches_id(c, cause))
            }
            CriterionConditions::FallAfterExplosion {
                distance_y: None,
                cause_entity,
            } => cause_entity
                .as_ref()
                .is_none_or(|c| predicate::matches_id(c, cause)),
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::ChanneledLightning{victim_type}`.
    pub(super) fn channeled_lightning(&self, victim_type: &str) {
        self.trigger_criterion_event("minecraft:channeled_lightning", |cond| match cond {
            CriterionConditions::ChanneledLightning { victim_type: req } => req
                .as_ref()
                .is_none_or(|r| predicate::matches_id(r, victim_type)),
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::KillMobNearSculkCatalyst`.
    pub(super) fn kill_mob_near_sculk_catalyst(&self) {
        self.trigger_criterion_event("minecraft:kill_mob_near_sculk_catalyst", |_cond| {
            let pos = self.position().to_block_pos();
            let world = self.world();
            for dx in -8..=8 {
                for dy in -8..=8 {
                    for dz in -8..=8 {
                        let check = BlockPos::new(pos.0.x + dx, pos.0.y + dy, pos.0.z + dz);
                        if world.get_block(&check) == &pumpkin_data::Block::SCULK_CATALYST {
                            return true;
                        }
                    }
                }
            }
            false
        });
    }

    /// Handles the arm `AdvancementTrigger::SpearMobs{count}`.
    pub(super) fn spear_mobs(&self, count: i32) {
        self.trigger_criterion_event("minecraft:spear_mobs", |cond| match cond {
            CriterionConditions::SpearMobs { count: req } => req.is_none_or(|r| count >= r),
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }
}
