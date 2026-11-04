//! Where an entity's tick goes, part by part, across all entities.
//!
//! The parts are steps of the entity tick code that are timed with [`time`]. Everything is off
//! until [`enable`] is called (the server does it when `KVASIR_TICK_KINDS` is set), and then
//! each timed step reads the clock twice. Parts nest: [`Part::Movement`] holds
//! [`Part::AiStep`], so the shares do not add up.

use std::time::Instant;

use crate::kinds::{KindRow, KindTimes};

/// A timed step of an entity's tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// `Entity::tick`: portal, fluid state, fire, out of world.
    BaseTick,
    /// Inside `BaseTick`: working out whether the entity is in water or lava.
    FluidState,
    /// A mob's ambient sound and its own base tick (despawn check, leash).
    MobBaseTick,
    /// `tick_movement`: the AI step, jumping and travel.
    Movement,
    /// Inside `Movement`: the mob's AI step (sensing, targets, goals, navigation).
    AiStep,
    /// Inside `AiStep`: applying path requests and stepping the navigator.
    AiNavigation,
    /// Inside `AiStep`: the sensing cache (what the mob can see).
    AiSensing,
    /// Inside `AiStep`: the target selector (who to attack).
    AiTargets,
    /// Inside `AiStep`: the goal selector (what to do).
    AiGoals,
    /// Inside `AiStep`: the mob type's own AI step.
    AiCustom,
    /// Inside `AiStep`: move, look and jump controls.
    AiControls,
    /// Inside `Movement`: moving the entity (walking, swimming, falling).
    Travel,
    /// Inside `Movement`: the blocks the entity touches (cactus, fire, suffocation).
    BlockCollisions,
    /// Inside `Movement`: pushing and being pushed by the entities around.
    PushEntities,
    /// Inside `Movement`: the mob's step after travel (growing up, look direction, body turn).
    MobAiStepEnd,
    /// Freezing in powder snow.
    Frozen,
    /// The block under the entity reacting to a step.
    StepBlocks,
    /// Status effects.
    Effects,
}

impl Part {
    pub const ALL: [Self; 18] = [
        Self::BaseTick,
        Self::FluidState,
        Self::MobBaseTick,
        Self::Movement,
        Self::AiStep,
        Self::AiNavigation,
        Self::AiSensing,
        Self::AiTargets,
        Self::AiGoals,
        Self::AiCustom,
        Self::AiControls,
        Self::Travel,
        Self::BlockCollisions,
        Self::PushEntities,
        Self::MobAiStepEnd,
        Self::Frozen,
        Self::StepBlocks,
        Self::Effects,
    ];

    /// The name used in reports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::BaseTick => "base_tick",
            Self::FluidState => "fluid_state",
            Self::MobBaseTick => "mob_base_tick",
            Self::Movement => "movement",
            Self::AiStep => "ai_step",
            Self::AiNavigation => "ai_navigation",
            Self::AiSensing => "ai_sensing",
            Self::AiTargets => "ai_targets",
            Self::AiGoals => "ai_goals",
            Self::AiCustom => "ai_custom",
            Self::AiControls => "ai_controls",
            Self::Travel => "travel",
            Self::BlockCollisions => "block_collisions",
            Self::PushEntities => "push_entities",
            Self::MobAiStepEnd => "mob_ai_step_end",
            Self::Frozen => "frozen",
            Self::StepBlocks => "step_blocks",
            Self::Effects => "effects",
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

impl Part {
    /// Runs `step` and, when timing is on, adds its time to this part.
    #[inline]
    pub fn time<R>(self, step: impl FnOnce() -> R) -> R {
        time(self, step)
    }
}

static PARTS: KindTimes = KindTimes::new();

/// Starts timing. Not undone.
pub fn enable() {
    PARTS.enable();
}

/// Runs `step`, and when timing is on, adds its time to `part`.
#[inline]
pub fn time<R>(part: Part, step: impl FnOnce() -> R) -> R {
    if !PARTS.is_enabled() {
        return step();
    }
    let began = Instant::now();
    let result = step();
    PARTS.record(part.index(), began.elapsed());
    result
}

/// Every timed part with its totals, the most time first.
#[must_use]
pub fn report() -> Vec<(Part, KindRow)> {
    PARTS
        .top(Part::ALL.len())
        .into_iter()
        .filter_map(|row| Part::ALL.get(row.kind).map(|part| (*part, row)))
        .collect()
}

/// Starts again from zero.
pub fn reset() {
    PARTS.reset();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_part_has_its_own_index_and_name() {
        let mut names: Vec<_> = Part::ALL.iter().map(|part| part.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Part::ALL.len());
        for (index, part) in Part::ALL.iter().enumerate() {
            assert_eq!(part.index(), index, "ALL is in declaration order");
        }
    }

    #[test]
    fn a_step_returns_its_result_whether_or_not_timing_is_on() {
        assert_eq!(time(Part::Effects, || 7), 7);
        enable();
        assert_eq!(time(Part::Effects, || 8), 8);
        let rows = report();
        let (_, row) = rows
            .iter()
            .find(|(part, _)| *part == Part::Effects)
            .expect("the timed step is reported");
        assert!(row.ticks >= 1);
        reset();
        assert!(report().is_empty());
    }
}
