//! Mob AI level of detail.
//!
//! A mob's decisions (which goal to start, whom to target) matter only when something can see or
//! meet the mob. Its physics must keep running every tick or it would visibly stall, so this
//! crate only decides how often the *full decision pass* runs. Vanilla runs it every second tick
//! and ticks only the running goals in between; a mob that is far from every player, unseen, or
//! one of a crowd gets it every fourth or eighth tick instead.
//!
//! Pure: distances and flags in, a rate out. The engine measures the distances and applies the
//! rate.

// Tests state expectations with `unwrap`; the engine's lints forbid it everywhere else.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use serde::{Deserialize, Serialize};

/// Ticks between full decision passes in vanilla Minecraft (`Mob.serverAiStep`).
pub const VANILLA_RATE: u8 = 2;

/// The thresholds. Distances are in blocks, rates in ticks between full decisions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Off: every mob keeps the vanilla rate, whatever its tier.
    pub enabled: bool,
    /// A mob this close to a player keeps the vanilla rate.
    pub full_range: f64,
    /// A mob that no client has been sent is dormant beyond this distance.
    pub dormant_range: f64,
    /// A mob that some client sees is dormant beyond this (larger) distance: farther than
    /// nearly all of the screen it appears on.
    pub seen_dormant_range: f64,
    /// Ticks between full decisions of a relaxed mob.
    pub relaxed_rate: u8,
    /// Ticks between full decisions of a dormant mob.
    pub dormant_rate: u8,
    /// Mobs of one kind in one chunk beyond this many are at least relaxed. 0 turns it off.
    pub crowd_limit: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            full_range: 32.0,
            dormant_range: 64.0,
            seen_dormant_range: 96.0,
            relaxed_rate: 4,
            dormant_rate: 8,
            crowd_limit: 16,
        }
    }
}

impl Config {
    /// The same thresholds made consistent: rates at least vanilla's and not decreasing with
    /// the tier, ranges finite and not decreasing with the tier.
    #[must_use]
    pub fn sanitised(self) -> Self {
        let finite = |value: f64, fallback: f64| {
            if value.is_finite() {
                value.max(0.0)
            } else {
                fallback
            }
        };
        let defaults = Self::default();
        let full_range = finite(self.full_range, defaults.full_range);
        let relaxed_rate = self.relaxed_rate.max(VANILLA_RATE);
        let dormant_range = finite(self.dormant_range, defaults.dormant_range).max(full_range);
        Self {
            full_range,
            dormant_range,
            seen_dormant_range: finite(self.seen_dormant_range, defaults.seen_dormant_range)
                .max(dormant_range),
            relaxed_rate,
            dormant_rate: self.dormant_rate.max(relaxed_rate),
            ..self
        }
    }
}

/// What the engine knows about one mob this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sight {
    /// Squared distance in blocks to the nearest player that can be affected by the mob.
    pub nearest_player_sq: f64,
    /// Some player's client has been sent this mob.
    pub observed: bool,
    /// Position among the mobs of its kind in its chunk, from 0.
    pub crowd_rank: u32,
    /// The mob is busy in a way that must not be slowed: it has a target, was hurt, is leashed
    /// or rides or is ridden.
    pub on_alert: bool,
}

/// How much attention a mob gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// Vanilla rate.
    Full,
    /// Seen from afar, or one of a crowd.
    Relaxed,
    /// Far from every player and seen by none.
    Dormant,
}

impl Tier {
    pub const ALL: [Self; 3] = [Self::Full, Self::Relaxed, Self::Dormant];

    /// Position in [`Tier::ALL`], for counters.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Relaxed => "relaxed",
            Self::Dormant => "dormant",
        }
    }
}

/// The tier of a mob. With the feature off, or a mob on alert, always [`Tier::Full`].
#[must_use]
pub fn tier(config: &Config, sight: &Sight) -> Tier {
    if !config.enabled || sight.on_alert {
        return Tier::Full;
    }
    let by_distance = if sight.nearest_player_sq <= config.full_range * config.full_range {
        Tier::Full
    } else {
        let limit = if sight.observed {
            config.seen_dormant_range
        } else {
            config.dormant_range
        };
        if sight.nearest_player_sq <= limit * limit {
            Tier::Relaxed
        } else {
            Tier::Dormant
        }
    };
    let crowded = config.crowd_limit > 0 && sight.crowd_rank >= config.crowd_limit;
    if crowded {
        by_distance.max(Tier::Relaxed)
    } else {
        by_distance
    }
}

/// Ticks between two full decisions of a mob of this tier.
#[must_use]
pub const fn rate(config: &Config, tier: Tier) -> u8 {
    match tier {
        Tier::Full => VANILLA_RATE,
        Tier::Relaxed => config.relaxed_rate,
        Tier::Dormant => config.dormant_rate,
    }
}

/// Whether this mob runs its full decision pass on this tick.
///
/// At [`VANILLA_RATE`] this is exactly vanilla's `(tickCount + id) % 2 == 0 || tickCount <= 1`.
/// The id spreads mobs over the ticks so the work does not bunch up.
#[must_use]
pub const fn is_decision_tick(rate: u8, tick_count: i32, entity_id: i32) -> bool {
    let rate = if rate < VANILLA_RATE {
        VANILLA_RATE
    } else {
        rate
    };
    tick_count <= 1 || tick_count.wrapping_add(entity_id).rem_euclid(rate as i32) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sight(distance: f64) -> Sight {
        Sight {
            nearest_player_sq: distance * distance,
            observed: false,
            crowd_rank: 0,
            on_alert: false,
        }
    }

    #[test]
    fn a_mob_inside_the_full_range_is_full() {
        let config = Config::default();
        assert_eq!(tier(&config, &sight(0.0)), Tier::Full);
        assert_eq!(tier(&config, &sight(32.0)), Tier::Full);
    }

    #[test]
    fn a_mob_that_a_client_sees_is_relaxed_up_to_the_seen_dormant_range() {
        let config = Config::default();
        let seen = |distance| Sight {
            observed: true,
            ..sight(distance)
        };
        assert_eq!(tier(&config, &seen(32.1)), Tier::Relaxed);
        assert_eq!(
            tier(&config, &seen(65.0)),
            Tier::Relaxed,
            "unseen it would be dormant"
        );
        assert_eq!(tier(&config, &seen(96.0)), Tier::Relaxed);
        assert_eq!(tier(&config, &seen(96.1)), Tier::Dormant);
    }

    #[test]
    fn an_unseen_mob_is_relaxed_up_to_the_dormant_range_and_dormant_beyond() {
        let config = Config::default();
        assert_eq!(tier(&config, &sight(32.1)), Tier::Relaxed);
        assert_eq!(tier(&config, &sight(64.0)), Tier::Relaxed);
        assert_eq!(tier(&config, &sight(64.1)), Tier::Dormant);
    }

    #[test]
    fn a_mob_on_alert_is_full_at_any_distance() {
        let far = Sight {
            on_alert: true,
            ..sight(200.0)
        };
        assert_eq!(tier(&Config::default(), &far), Tier::Full);
    }

    #[test]
    fn crowd_members_past_the_limit_are_at_least_relaxed_even_beside_a_player() {
        let config = Config::default();
        let ranked = |crowd_rank, distance| Sight {
            crowd_rank,
            ..sight(distance)
        };
        assert_eq!(tier(&config, &ranked(15, 3.0)), Tier::Full);
        assert_eq!(tier(&config, &ranked(16, 3.0)), Tier::Relaxed);
        // A crowd does not lift a dormant mob back up.
        assert_eq!(tier(&config, &ranked(40, 100.0)), Tier::Dormant);
        // A crowd member on alert still reacts.
        let alert = Sight {
            on_alert: true,
            ..ranked(40, 3.0)
        };
        assert_eq!(tier(&config, &alert), Tier::Full);
    }

    #[test]
    fn a_crowd_limit_of_zero_switches_the_crowd_rule_off() {
        let config = Config {
            crowd_limit: 0,
            ..Config::default()
        };
        let packed = Sight {
            crowd_rank: 1000,
            ..sight(1.0)
        };
        assert_eq!(tier(&config, &packed), Tier::Full);
    }

    #[test]
    fn disabled_means_every_mob_is_full() {
        let config = Config {
            enabled: false,
            ..Config::default()
        };
        let worst = Sight {
            crowd_rank: 99,
            ..sight(900.0)
        };
        assert_eq!(tier(&config, &worst), Tier::Full);
    }

    #[test]
    fn farther_never_means_a_faster_rate() {
        let config = Config::default();
        let mut last = 0;
        for tenth in 0..3000 {
            let now = rate(&config, tier(&config, &sight(f64::from(tenth) / 10.0)));
            assert!(now >= last, "at {} blocks", f64::from(tenth) / 10.0);
            last = now;
        }
        assert_eq!(last, config.dormant_rate);
    }

    #[test]
    fn rates_follow_the_tier() {
        let config = Config::default();
        assert_eq!(rate(&config, Tier::Full), 2);
        assert_eq!(rate(&config, Tier::Relaxed), 4);
        assert_eq!(rate(&config, Tier::Dormant), 8);
    }

    #[test]
    fn at_the_vanilla_rate_a_decision_tick_is_exactly_vanillas() {
        // Java: `tickCount <= 1 || (tickCount + id) % 2 == 0`; its `%` is -1 or 0 for the
        // negative sums that negative ids make, so "is zero" is the same test as here.
        for id in [-7i32, 0, 1, 2, 12345] {
            for tick in -3i32..200 {
                let vanilla = tick <= 1 || (tick + id) % 2 == 0;
                assert_eq!(
                    is_decision_tick(VANILLA_RATE, tick, id),
                    vanilla,
                    "tick {tick} id {id}"
                );
            }
        }
    }

    #[test]
    fn a_mob_decides_once_per_rate_ticks_and_ids_spread_the_ticks() {
        for rate in [2u8, 4, 8] {
            let decisions = (10..10 + 800)
                .filter(|tick| is_decision_tick(rate, *tick, 5))
                .count();
            assert_eq!(decisions, 800 / usize::from(rate), "rate {rate}");
        }
        let at_tick = |tick| (0..8).filter(|id| is_decision_tick(8, tick, *id)).count();
        assert_eq!(
            at_tick(100),
            1,
            "of eight consecutive ids exactly one decides per tick"
        );
    }

    #[test]
    fn a_new_mob_decides_on_its_first_ticks_whatever_its_rate() {
        assert!(is_decision_tick(8, 0, 3));
        assert!(is_decision_tick(8, 1, 4));
    }

    #[test]
    fn a_rate_below_vanilla_is_treated_as_vanilla() {
        assert_eq!(is_decision_tick(0, 10, 0), is_decision_tick(2, 10, 0));
        assert_eq!(is_decision_tick(1, 11, 0), is_decision_tick(2, 11, 0));
    }

    #[test]
    fn sanitising_keeps_rates_and_ranges_in_order() {
        let messy = Config {
            full_range: f64::NAN,
            dormant_range: 10.0,
            seen_dormant_range: 5.0,
            relaxed_rate: 1,
            dormant_rate: 0,
            ..Config::default()
        };
        let clean = messy.sanitised();
        assert_eq!(clean.full_range, Config::default().full_range);
        assert!(clean.dormant_range >= clean.full_range);
        assert!(clean.seen_dormant_range >= clean.dormant_range);
        assert_eq!(clean.relaxed_rate, VANILLA_RATE);
        assert!(clean.dormant_rate >= clean.relaxed_rate);
        assert_eq!(Config::default().sanitised(), Config::default());
    }

    #[test]
    fn the_config_reads_from_toml_with_missing_keys_defaulted() {
        let config: Config = serde_json::from_str(r#"{"dormant_rate": 16}"#).unwrap();
        assert_eq!(config.dormant_rate, 16);
        assert_eq!(config.relaxed_rate, Config::default().relaxed_rate);
        assert!(config.enabled);
    }
}
