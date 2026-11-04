//! Telling each mob how much AI attention it gets this tick.
//!
//! The decision itself is `kvasir-moblod`; this measures what it needs (the nearest player, who
//! sees the mob, how many of its kind share its chunk) and hands the answer to the mob.

use std::sync::{Arc, atomic::Ordering::Relaxed};

use kvasir_moblod::{Config, Sight, Tier};
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use rustc_hash::FxHashMap;

use super::World;
use crate::entity::{EntityBase, mob::MobEntity};

/// Where each key stands among the entries with the same key, in list order: the first of a key
/// is 0, the next 1. Entries without a key (not mobs) get 0.
pub fn ranks_within_keys<K: std::hash::Hash + Eq>(
    keys: impl Iterator<Item = Option<K>>,
) -> Vec<u32> {
    let mut seen: FxHashMap<K, u32> = FxHashMap::default();
    keys.map(|key| {
        key.map_or(0, |key| {
            let count = seen.entry(key).or_insert(0);
            let rank = *count;
            *count += 1;
            rank
        })
    })
    .collect()
}

/// For each ticking entity, its rank among the mobs of its kind in its chunk. All 0 when the
/// crowd rule is off (limit 0), so the pass costs nothing then.
pub fn crowd_ranks(config: &Config, tickable: &[(&Arc<dyn EntityBase>, Vector2<i32>)]) -> Vec<u32> {
    if config.crowd_limit == 0 {
        return vec![0; tickable.len()];
    }
    ranks_within_keys(tickable.iter().map(|(entity, chunk)| {
        entity
            .get_mob()
            .map(|_| (chunk.x, chunk.y, entity.get_entity().entity_type.id))
    }))
}

/// Squared distance from `pos` to the nearest of `players`; infinite with no players.
pub fn nearest_player_sq<'a>(
    pos: &Vector3<f64>,
    players: impl Iterator<Item = &'a Vector3<f64>>,
) -> f64 {
    players
        .map(|player| {
            let (dx, dy, dz) = (player.x - pos.x, player.y - pos.y, player.z - pos.z);
            dx * dx + dy * dy + dz * dz
        })
        .fold(f64::INFINITY, f64::min)
}

/// A mob that is busy in a way slowing it would show: it has a target, was hurt in the last
/// second, is leashed, rides or is ridden, is in water or lava, or is on fire.
///
/// Fluid matters because the goals that keep a mob afloat, or stop it when it climbs out, are
/// started and stopped by the full decision pass; at a slow rate a mob at a shore would hop
/// about for several ticks after leaving the water.
fn is_on_alert(mob: &MobEntity) -> bool {
    let entity = &mob.living_entity.entity;
    entity.touching_water.load(Relaxed)
        || entity.touching_lava.load(Relaxed)
        || entity.fire_ticks.load(Relaxed) > 0
        || mob.get_target().is_some()
        || mob.living_entity.hurt_cooldown.load(Relaxed) > 0
        || entity
            .leashed_to
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
        || entity.get_vehicle().is_some()
        || entity.has_passengers()
}

impl World {
    /// Sets the AI level of `entity` if it is a mob; returns the tier it would have with the
    /// feature on, which is the tier it gets unless the feature is off (then it keeps vanilla's
    /// rate). The tier is always worked out so that a run with the feature off can still be
    /// compared with one that has it on.
    pub(super) fn attend_to_mob(
        &self,
        config: &Config,
        entity: &dyn EntityBase,
        crowd_rank: u32,
        player_positions: &[Vector3<f64>],
    ) -> Option<Tier> {
        let mob = entity.get_mob()?.get_mob_entity();
        let base = entity.get_entity();
        let mut sight = Sight {
            nearest_player_sq: nearest_player_sq(&base.pos.load(), player_positions.iter()),
            observed: false,
            crowd_rank,
            on_alert: false,
        };
        let natural = Config {
            enabled: true,
            ..*config
        };
        let mut tier = kvasir_moblod::tier(&natural, &sight);
        if tier != Tier::Full {
            sight.observed = self.entity_tracker.is_tracked_by_any_player(base.entity_id);
            sight.on_alert = is_on_alert(mob);
            tier = kvasir_moblod::tier(&natural, &sight);
        }
        let applied = if config.enabled { tier } else { Tier::Full };
        mob.set_ai_level(applied, kvasir_moblod::rate(config, applied));
        Some(tier)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_count_up_per_key_in_list_order() {
        let keys = [Some('a'), Some('b'), Some('a'), None, Some('a'), Some('b')];
        assert_eq!(ranks_within_keys(keys.into_iter()), [0, 0, 1, 0, 2, 1]);
    }

    #[test]
    fn no_entries_have_no_ranks() {
        assert!(ranks_within_keys(std::iter::empty::<Option<u8>>()).is_empty());
    }

    #[test]
    fn the_nearest_player_is_found_in_three_dimensions() {
        let players = [Vector3::new(0.0, 0.0, 0.0), Vector3::new(30.0, 40.0, 0.0)];
        let at = |x, y, z| nearest_player_sq(&Vector3::new(x, y, z), players.iter());
        assert!((at(3.0, 4.0, 0.0) - 25.0).abs() < 1e-9);
        assert!((at(30.0, 44.0, 0.0) - 16.0).abs() < 1e-9);
        assert!(
            (at(0.0, 100.0, 0.0) - 4500.0).abs() < 1e-9,
            "30 across and 60 up to the second player"
        );
    }

    #[test]
    fn with_no_player_every_mob_is_infinitely_far() {
        let far = nearest_player_sq(&Vector3::new(1.0, 2.0, 3.0), std::iter::empty());
        assert!(far.is_infinite());
    }
}
