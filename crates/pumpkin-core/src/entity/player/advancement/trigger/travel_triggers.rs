//! Triggers from falling, travelling and riding.

use crate::entity::player::Player;
use crate::entity::player::advancement::predicate::{self, CriterionConditions};

impl Player {
    /// Handles the arm `AdvancementTrigger::FallFromHeight{distance_y,from_y,to_y,}`.
    pub(super) fn fall_from_height(&self, distance_y: f64, from_y: f64, to_y: f64) {
        self.trigger_criterion_event("minecraft:fall_from_height", |cond| match cond {
            CriterionConditions::FallFromHeight {
                distance_y: Some(bounds),
            } => bounds.matches(distance_y) && from_y >= 319.0 && to_y <= -59.0,
            CriterionConditions::FallFromHeight { distance_y: None } => {
                from_y >= 319.0 && to_y <= -59.0
            }
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::NetherTravel{distance_horizontal,}`.
    pub(super) fn nether_travel(&self, distance_horizontal: f64) {
        self.trigger_criterion_event("minecraft:nether_travel", |cond| match cond {
            CriterionConditions::NetherTravel {
                distance_horizontal: Some(bounds),
            } => bounds.matches(distance_horizontal),
            CriterionConditions::NetherTravel {
                distance_horizontal: None,
            }
            | CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::RideEntityInLava{distance_horizontal,dimension,vehicle_type,}`.
    pub(super) fn ride_entity_in_lava(
        &self,
        distance_horizontal: f64,
        dimension: &str,
        vehicle_type: Option<&str>,
    ) {
        self.trigger_criterion_event("minecraft:ride_entity_in_lava", |cond| match cond {
            CriterionConditions::RideEntityInLava {
                distance_horizontal: exp_d,
                dimension: exp_dim,
                vehicle_type: exp_vt,
            } => {
                if let Some(d) = exp_d
                    && !d.matches(distance_horizontal)
                {
                    return false;
                }
                if let Some(dim) = exp_dim
                    && !predicate::matches_id(dim, dimension)
                {
                    return false;
                }
                if let Some(vt) = exp_vt {
                    let Some(actual) = vehicle_type else {
                        return false;
                    };
                    if !vt.matches(pumpkin_data::tag::RegistryKey::EntityType, actual) {
                        return false;
                    }
                }
                true
            }
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::StartedRiding{vehicle_type,passenger_type,}`.
    pub(super) fn started_riding(&self, vehicle_type: Option<&str>, passenger_type: Option<&str>) {
        self.trigger_criterion_event("minecraft:started_riding", |cond| match cond {
            CriterionConditions::StartedRiding(Some(pred)) => {
                if let Some(vt) = &pred.vehicle_type {
                    let Some(actual_v) = vehicle_type else {
                        return false;
                    };
                    if !vt.matches(pumpkin_data::tag::RegistryKey::EntityType, actual_v) {
                        return false;
                    }
                }
                if let Some(pt) = &pred.passenger_type {
                    let Some(actual_p) = passenger_type else {
                        return false;
                    };
                    if !pt.matches(pumpkin_data::tag::RegistryKey::EntityType, actual_p) {
                        return false;
                    }
                }
                true
            }
            CriterionConditions::StartedRiding(None) | CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }
}
