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
}
