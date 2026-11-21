//! The cloned default listener table equals the per-advancement registration.
#![cfg_attr(test, allow(clippy::unwrap_used))]

#[cfg(test)]
mod parse_golden;

use std::sync::Arc;

use pumpkin_data::{ADVANCEMENT_TREE, Advancement};
use tempfile::tempdir;
use uuid::Uuid;

use super::PlayerAdvancement;
use crate::data::advancement_data::AdvancementManager;

/// What `active_listeners` holds, by trigger: advancement, criterion and conditions in order.
fn listeners_text(pa: &PlayerAdvancement) -> Vec<String> {
    let mut text: Vec<String> = pa
        .active_listeners
        .iter()
        .map(|(trigger, listeners)| {
            let entries: Vec<String> = listeners
                .iter()
                .map(|l| {
                    format!(
                        "{}/{}@{:p}",
                        l.advancement.id,
                        l.criterion,
                        Arc::as_ptr(&l.conditions)
                    )
                })
                .collect();
            format!("{trigger}: {}", entries.join(" "))
        })
        .collect();
    text.sort();
    text
}

#[test]
fn new_player_listeners_match_the_per_advancement_registration() {
    let temp_dir = tempdir().unwrap();
    let manager = Arc::new(AdvancementManager::new(temp_dir.path(), true));
    let mut pa = PlayerAdvancement::new(manager, Uuid::new_v4());
    let cloned = listeners_text(&pa);
    // Progress with nothing done takes the per-advancement path and must give the same.
    let _ = pa
        .progress
        .get_mut_or_start_progress(Advancement::STORY_ROOT);
    pa.register_all_listeners();
    assert_eq!(cloned, listeners_text(&pa));
    let total: usize = ADVANCEMENT_TREE
        .nodes_vector
        .iter()
        .map(|node| node.value.criteria_definitions.len())
        .sum();
    assert_eq!(
        total,
        pa.active_listeners.values().map(Vec::len).sum::<usize>()
    );
}
