//! How many chunks to have in flight: more while the server has room, fewer when ticks run long.
//!
//! Additive increase, multiplicative decrease, the rule that makes TCP share a link: the job finds
//! the largest window the server tolerates without ever being told what that is.

/// Back-off policy with a window of chunks in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Throttle {
    window: u32,
    min: u32,
    max: u32,
}

/// Below this share of the budget the server is idle enough to take more work.
const ROOM_SHARE: f64 = 0.75;

impl Throttle {
    /// Starts at `min`. `min` is at least 1 and not above `max`.
    #[must_use]
    pub fn new(min: u32, max: u32) -> Self {
        let min = min.max(1);
        Self {
            window: min,
            min,
            max: max.max(min),
        }
    }

    /// Chunks that may be in flight now.
    #[must_use]
    pub const fn window(&self) -> u32 {
        self.window
    }

    /// Reacts to a measurement of how long ticks take (milliseconds, a high percentile) against
    /// the budget for a tick. Returns the new window.
    pub fn observe(&mut self, tick_ms: f64, budget_ms: f64) -> u32 {
        if tick_ms > budget_ms {
            self.window = (self.window / 2).max(self.min);
        } else if tick_ms < budget_ms * ROOM_SHARE {
            self.window = (self.window + 1).min(self.max);
        }
        self.window
    }
}

/// A percentile of recent tick durations, in milliseconds.
///
/// `ring` holds the engine's last 100 tick durations in nanoseconds; only the first `filled` slots
/// hold real measurements (the ring fills from the start). `percent` is 0 to 100. With no
/// measurements the answer is 0: a server that has not ticked yet is not slow.
#[must_use]
pub fn tick_percentile_ms(ring: &[i64], filled: usize, percent: usize) -> f64 {
    let mut samples: Vec<i64> = ring.iter().take(filled).copied().collect();
    if samples.is_empty() {
        return 0.0;
    }
    samples.sort_unstable();
    let index = (samples.len().saturating_mul(percent) / 100).min(samples.len() - 1);
    samples[index] as f64 / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUDGET: f64 = 40.0;

    #[test]
    fn grows_by_one_while_the_server_has_room() {
        let mut throttle = Throttle::new(2, 16);
        assert_eq!(throttle.window(), 2);
        assert_eq!(throttle.observe(10.0, BUDGET), 3);
        assert_eq!(throttle.observe(29.0, BUDGET), 4);
    }

    #[test]
    fn halves_when_ticks_run_over_budget() {
        let mut throttle = Throttle::new(2, 64);
        for _ in 0..30 {
            throttle.observe(5.0, BUDGET);
        }
        assert_eq!(throttle.window(), 32);
        assert_eq!(throttle.observe(55.0, BUDGET), 16);
        assert_eq!(throttle.observe(55.0, BUDGET), 8);
    }

    #[test]
    fn holds_between_the_two_thresholds() {
        let mut throttle = Throttle::new(2, 16);
        throttle.observe(1.0, BUDGET);
        let before = throttle.window();
        assert_eq!(throttle.observe(35.0, BUDGET), before);
        assert_eq!(
            throttle.observe(40.0, BUDGET),
            before,
            "exactly on budget is fine"
        );
    }

    #[test]
    fn never_leaves_its_bounds() {
        let mut throttle = Throttle::new(3, 5);
        for _ in 0..20 {
            throttle.observe(1.0, BUDGET);
        }
        assert_eq!(throttle.window(), 5);
        for _ in 0..20 {
            throttle.observe(500.0, BUDGET);
        }
        assert_eq!(throttle.window(), 3);
    }

    #[test]
    fn nonsense_bounds_are_repaired() {
        let throttle = Throttle::new(0, 0);
        assert_eq!(throttle.window(), 1);
        let mut throttle = Throttle::new(8, 2);
        assert_eq!(throttle.observe(1.0, BUDGET), 8, "max is raised to min");
    }

    #[test]
    fn the_percentile_is_taken_over_the_measured_slots_only() {
        // 10 measured ticks of 1..=10 ms; the rest of the ring is unwritten zeros
        let mut ring = [0i64; 100];
        for (slot, ms) in ring.iter_mut().zip(1..=10) {
            *slot = ms * 1_000_000;
        }
        assert!((tick_percentile_ms(&ring, 10, 95) - 10.0).abs() < 1e-9);
        assert!((tick_percentile_ms(&ring, 10, 50) - 6.0).abs() < 1e-9);
        assert!((tick_percentile_ms(&ring, 10, 0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_server_that_has_not_ticked_is_not_slow() {
        assert!(tick_percentile_ms(&[0; 100], 0, 95).abs() < f64::EPSILON);
        assert!(tick_percentile_ms(&[], 5, 95).abs() < f64::EPSILON);
    }

    #[test]
    fn one_slow_tick_among_many_shows_at_the_top_percentile_only() {
        let mut ring = [2_000_000i64; 100];
        ring[17] = 400_000_000;
        assert!((tick_percentile_ms(&ring, 100, 95) - 2.0).abs() < 1e-9);
        assert!((tick_percentile_ms(&ring, 100, 100) - 400.0).abs() < 1e-9);
    }
}
