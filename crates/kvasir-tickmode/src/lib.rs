//! One thread or many for the server tick.
//!
//! Measured on this engine (10 and 30 bots on a 6-core virtual machine): a tick that runs on one
//! thread has the same median as one that runs on six and a clearly shorter tail, because every
//! hand-off to a worker is a chance to wait for a thread the host did not schedule. Parallel
//! ticking only pays when one thread cannot finish the tick, so this decides, from the lengths of
//! recent ticks alone, which of the two to use next. Pure: durations in, a mode out.

// Tests state expectations with `unwrap`; the engine's lints forbid it everywhere else.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::{collections::VecDeque, time::Duration};

/// How the next tick runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// On one thread. The default.
    Sequential,
    /// On the worker pool.
    Parallel,
}

/// The thresholds. All durations are tick lengths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    /// Go parallel when the median of the recent ticks, run sequentially, is above this: more
    /// than half of the ticks do not fit the budget, so the overload is not a hiccup.
    pub overloaded_median: Duration,
    /// Try sequential again when the median of the recent ticks, run in parallel, is below this:
    /// one thread has a wide margin.
    pub calm_median: Duration,
    /// How many recent ticks the median is taken over, and the fewest ticks in a mode before it
    /// may change again.
    pub window: usize,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            overloaded_median: Duration::from_millis(45),
            calm_median: Duration::from_millis(18),
            window: 200,
        }
    }
}

/// Decides the mode of the next tick from the lengths of the last ones.
#[derive(Debug, Clone)]
pub struct Controller {
    policy: Policy,
    mode: Mode,
    /// Tick lengths since the mode last changed, newest last, at most `policy.window`.
    recent: VecDeque<Duration>,
}

impl Controller {
    #[must_use]
    pub fn new(policy: Policy) -> Self {
        Self {
            policy,
            mode: Mode::Sequential,
            recent: VecDeque::with_capacity(policy.window),
        }
    }

    /// The mode the next tick should use.
    #[must_use]
    pub const fn mode(&self) -> Mode {
        self.mode
    }

    /// Notes that a tick in the current mode took `tick`, and returns the mode for the next one.
    pub fn observe(&mut self, tick: Duration) -> Mode {
        if self.recent.len() == self.policy.window.max(1) {
            self.recent.pop_front();
        }
        self.recent.push_back(tick);
        // Judge only a full window of ticks of one mode: after a change the old mode's ticks are
        // forgotten, so a change is never undone by the very ticks that caused it.
        if self.recent.len() < self.policy.window.max(1) {
            return self.mode;
        }
        let median = median(&self.recent);
        let next = match self.mode {
            Mode::Sequential if median > self.policy.overloaded_median => Mode::Parallel,
            Mode::Parallel if median < self.policy.calm_median => Mode::Sequential,
            keep => keep,
        };
        if next != self.mode {
            self.mode = next;
            self.recent.clear();
        }
        self.mode
    }
}

fn median(ticks: &VecDeque<Duration>) -> Duration {
    let mut sorted: Vec<Duration> = ticks.iter().copied().collect();
    sorted.sort_unstable();
    sorted[sorted.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> Policy {
        Policy {
            overloaded_median: Duration::from_millis(45),
            calm_median: Duration::from_millis(18),
            window: 10,
        }
    }

    fn feed(controller: &mut Controller, ms: u64, count: usize) -> Mode {
        let mut mode = controller.mode();
        for _ in 0..count {
            mode = controller.observe(Duration::from_millis(ms));
        }
        mode
    }

    #[test]
    fn it_starts_sequential_and_stays_there_while_ticks_are_short() {
        let mut c = Controller::new(policy());
        assert_eq!(c.mode(), Mode::Sequential);
        assert_eq!(feed(&mut c, 30, 500), Mode::Sequential);
    }

    #[test]
    fn a_tick_over_budget_most_of_the_time_goes_parallel() {
        let mut c = Controller::new(policy());
        assert_eq!(
            feed(&mut c, 60, 9),
            Mode::Sequential,
            "a window is not full yet"
        );
        assert_eq!(feed(&mut c, 60, 1), Mode::Parallel);
    }

    #[test]
    fn hiccups_do_not_count_as_overload() {
        // one tick in three is a 400 ms host hiccup; the typical tick is 30 ms
        let mut c = Controller::new(policy());
        for i in 0..300 {
            c.observe(Duration::from_millis(if i % 3 == 0 { 400 } else { 30 }));
        }
        assert_eq!(c.mode(), Mode::Sequential);
    }

    #[test]
    fn a_median_between_the_thresholds_changes_nothing_in_either_mode() {
        let mut c = Controller::new(policy());
        assert_eq!(feed(&mut c, 44, 100), Mode::Sequential);
        feed(&mut c, 60, 10);
        assert_eq!(c.mode(), Mode::Parallel);
        assert_eq!(
            feed(&mut c, 30, 100),
            Mode::Parallel,
            "30 ms is not calm enough to leave parallel"
        );
    }

    #[test]
    fn calm_ticks_bring_it_back_to_sequential() {
        let mut c = Controller::new(policy());
        feed(&mut c, 60, 10);
        assert_eq!(c.mode(), Mode::Parallel);
        assert_eq!(feed(&mut c, 10, 9), Mode::Parallel);
        assert_eq!(feed(&mut c, 10, 1), Mode::Sequential);
    }

    #[test]
    fn the_ticks_of_the_old_mode_are_forgotten_at_a_change() {
        let mut c = Controller::new(policy());
        feed(&mut c, 60, 10);
        // calm ticks in parallel: the 60 ms ticks of before must not delay or hasten the change
        assert_eq!(
            feed(&mut c, 10, 5),
            Mode::Parallel,
            "half a window is not enough"
        );
        assert_eq!(feed(&mut c, 10, 5), Mode::Sequential);
    }

    #[test]
    fn a_mode_lasts_at_least_one_window() {
        let mut c = Controller::new(policy());
        let mut changes = 0;
        let mut last = c.mode();
        // worst case: whatever mode it is in, the tick looks like the opposite
        for _ in 0..200 {
            let ms = if c.mode() == Mode::Sequential { 100 } else { 5 };
            let now = c.observe(Duration::from_millis(ms));
            if now != last {
                changes += 1;
                last = now;
            }
        }
        assert!(
            changes <= 200 / 10,
            "{changes} changes in 200 ticks with a window of 10"
        );
        assert!(changes >= 2, "it does react");
    }

    #[test]
    fn a_window_of_zero_behaves_like_one() {
        let mut c = Controller::new(Policy {
            window: 0,
            ..policy()
        });
        assert_eq!(c.observe(Duration::from_millis(100)), Mode::Parallel);
        assert_eq!(c.observe(Duration::from_millis(1)), Mode::Sequential);
    }

    #[test]
    fn the_default_policy_is_in_the_documented_range() {
        let p = Policy::default();
        assert!(p.calm_median < p.overloaded_median);
        assert!(
            p.overloaded_median < Duration::from_millis(50),
            "must act before the budget is gone"
        );
        assert_eq!(p.window, 200);
    }
}
