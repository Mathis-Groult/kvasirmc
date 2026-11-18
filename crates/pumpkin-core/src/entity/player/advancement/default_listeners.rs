//! The listener table a player with no advancement progress starts with, built once (card #76).

use std::collections::HashMap;
use std::sync::Arc;

use pumpkin_data::{ADVANCEMENT_TREE, Advancement};

use super::{ActiveCriterionListener, VANILLA_CONDITIONS_CACHE, predicate};

/// The listeners of every criterion of every advancement, grouped by trigger in tree order:
/// what a player with no progress gets. Built once, so a new player clones it instead of
/// hashing each (advancement, criterion) pair again (84 % of creating a test player).
pub(super) static DEFAULT_LISTENERS: std::sync::LazyLock<
    HashMap<&'static str, Vec<ActiveCriterionListener>>,
> = std::sync::LazyLock::new(|| {
    let mut listeners = HashMap::new();
    for node in &ADVANCEMENT_TREE.nodes_vector {
        push_every_criterion(&mut listeners, node.value);
    }
    listeners
});

/// Appends a listener for each criterion of `advancement` under its trigger.
pub(super) fn push_every_criterion(
    listeners: &mut HashMap<&'static str, Vec<ActiveCriterionListener>>,
    advancement: &'static Advancement,
) {
    for criterion_def in advancement.criteria_definitions {
        let conditions = VANILLA_CONDITIONS_CACHE
            .get(&(advancement, criterion_def.name))
            .cloned()
            .unwrap_or_else(|| {
                Arc::new(predicate::CriterionConditions::from(
                    criterion_def.conditions,
                ))
            });
        listeners
            .entry(criterion_def.trigger)
            .or_default()
            .push(ActiveCriterionListener {
                advancement,
                criterion: criterion_def.name,
                conditions,
            });
    }
}
