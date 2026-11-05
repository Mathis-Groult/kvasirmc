//! One thread or many for the server tick, and how many threads "many" is.
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

/// Cores kept free for the rest of the machine (other programs, the network and disk threads).
pub const DEFAULT_RESERVED_CORES: usize = 2;

/// The number of cores to leave to others: the value of `KVASIR_RESERVED_CORES` when it is a
/// number, else [`DEFAULT_RESERVED_CORES`].
#[must_use]
pub fn reserved_cores(setting: Option<&str>) -> usize {
    setting
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(DEFAULT_RESERVED_CORES)
}

/// How many threads the tick pool gets on a machine with `cpus` cores of which `reserved` are left
/// to others. Never fewer than one.
#[must_use]
pub const fn pool_threads(cpus: usize, reserved: usize) -> usize {
    let free = cpus.saturating_sub(reserved);
    if free == 0 { 1 } else { free }
}

/// The thresholds. All durations are tick lengths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    /// Go parallel when the median of the last `attack_window` ticks, run sequentially, is above
    /// this: most of them do not fit the budget, so a burst (an explosion, a redstone clock
    /// starting) is handled within about a second, while a hiccup of a few ticks is not.
    pub attack_median: Duration,
    /// How many recent ticks the attack looks at, and the fewest ticks in sequential mode before
    /// it may react.
    pub attack_window: usize,
    /// Try sequential again when the median of the last `release_window` ticks, run in parallel,
    /// is below this: one thread has a wide margin.
    pub release_median: Duration,
    /// How many recent ticks the release looks at, and the fewest ticks in parallel mode before it
    /// may change back. Long, so that a burst is not followed by a quick return to one thread.
    pub release_window: usize,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            attack_median: Duration::from_millis(40),
            attack_window: 20,
            release_median: Duration::from_millis(18),
            release_window: 200,
        }
    }
}

/// Decides the mode of the next tick from the lengths of the last ones.
#[derive(Debug, Clone)]
pub struct Controller {
    policy: Policy,
    mode: Mode,
    /// Tick lengths since the mode last changed, newest last, at most the longer window.
    recent: VecDeque<Duration>,
}

impl Controller {
    #[must_use]
    pub fn new(policy: Policy) -> Self {
        Self {
            policy,
            mode: Mode::Sequential,
            recent: VecDeque::with_capacity(policy.attack_window.max(policy.release_window).max(1)),
        }
    }

    /// The mode the next tick should use.
    #[must_use]
    pub const fn mode(&self) -> Mode {
        self.mode
    }

    /// Notes that a tick in the current mode took `tick`, and returns the mode for the next one.
    pub fn observe(&mut self, tick: Duration) -> Mode {
        let keep = self
            .policy
            .attack_window
            .max(self.policy.release_window)
            .max(1);
        if self.recent.len() == keep {
            self.recent.pop_front();
        }
        self.recent.push_back(tick);
        // Judge only a full window of ticks of one mode: after a change the old mode's ticks are
        // forgotten, so a change is never undone by the very ticks that caused it.
        let window = match self.mode {
            Mode::Sequential => self.policy.attack_window,
            Mode::Parallel => self.policy.release_window,
        }
        .max(1);
        if self.recent.len() < window {
            return self.mode;
        }
        let median = median_of_last(&self.recent, window);
        let next = match self.mode {
            Mode::Sequential if median > self.policy.attack_median => Mode::Parallel,
            Mode::Parallel if median < self.policy.release_median => Mode::Sequential,
            stay => stay,
        };
        if next != self.mode {
            self.mode = next;
            self.recent.clear();
        }
        self.mode
    }
}

fn median_of_last(ticks: &VecDeque<Duration>, count: usize) -> Duration {
    let mut sorted: Vec<Duration> = ticks.iter().rev().take(count).copied().collect();
    sorted.sort_unstable();
    sorted[sorted.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> Policy {
        Policy {
            attack_median: Duration::from_millis(40),
            attack_window: 10,
            release_median: Duration::from_millis(18),
            release_window: 20,
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
    fn a_burst_of_long_ticks_goes_parallel_when_half_the_attack_window_is_long() {
        let mut c = Controller::new(policy());
        feed(&mut c, 10, 100);
        assert_eq!(
            feed(&mut c, 120, 4),
            Mode::Sequential,
            "4 of 10 is not the median"
        );
        assert_eq!(feed(&mut c, 120, 1), Mode::Parallel, "5 of 10 is");
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
    fn two_bad_ticks_in_a_row_are_not_a_burst() {
        let mut c = Controller::new(policy());
        feed(&mut c, 10, 50);
        feed(&mut c, 500, 2);
        assert_eq!(feed(&mut c, 10, 50), Mode::Sequential);
    }

    #[test]
    fn a_median_at_the_attack_line_changes_nothing() {
        let mut c = Controller::new(policy());
        assert_eq!(
            feed(&mut c, 40, 100),
            Mode::Sequential,
            "40 ms is within the line"
        );
    }

    #[test]
    fn parallel_is_kept_for_a_whole_release_window_after_the_burst() {
        let mut c = Controller::new(policy());
        feed(&mut c, 10, 10);
        feed(&mut c, 120, 5);
        assert_eq!(c.mode(), Mode::Parallel);
        assert_eq!(feed(&mut c, 10, 19), Mode::Parallel, "19 of 20 calm ticks");
        assert_eq!(feed(&mut c, 10, 1), Mode::Sequential);
    }

    #[test]
    fn a_median_between_the_lines_keeps_parallel() {
        let mut c = Controller::new(policy());
        feed(&mut c, 10, 10);
        feed(&mut c, 120, 5);
        assert_eq!(
            feed(&mut c, 30, 100),
            Mode::Parallel,
            "30 ms is not calm enough to leave parallel"
        );
    }

    #[test]
    fn the_ticks_of_the_old_mode_are_forgotten_at_a_change() {
        let mut c = Controller::new(policy());
        feed(&mut c, 10, 10);
        feed(&mut c, 120, 5);
        assert_eq!(c.mode(), Mode::Parallel);
        assert_eq!(
            feed(&mut c, 10, 10),
            Mode::Parallel,
            "half a release window"
        );
        assert_eq!(feed(&mut c, 10, 10), Mode::Sequential);
        // back in sequential, the calm ticks of parallel must not hasten another change
        assert_eq!(
            feed(&mut c, 120, 9),
            Mode::Sequential,
            "a window is not full yet"
        );
        assert_eq!(feed(&mut c, 120, 1), Mode::Parallel);
    }

    #[test]
    fn a_mode_lasts_at_least_its_window() {
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
        assert!(changes <= 200 / 10, "{changes} changes in 200 ticks");
        assert!(changes >= 2, "it does react");
    }

    #[test]
    fn a_window_of_zero_behaves_like_one() {
        let mut c = Controller::new(Policy {
            attack_window: 0,
            release_window: 0,
            ..policy()
        });
        assert_eq!(c.observe(Duration::from_millis(100)), Mode::Parallel);
        assert_eq!(c.observe(Duration::from_millis(1)), Mode::Sequential);
    }

    #[test]
    fn the_default_policy_acts_before_the_budget_is_gone_and_releases_slowly() {
        let p = Policy::default();
        assert!(p.release_median < p.attack_median);
        assert!(p.attack_median < Duration::from_millis(50));
        assert!(p.attack_window < p.release_window);
    }

    #[test]
    fn the_pool_leaves_two_cores_to_others_and_scales_with_the_machine() {
        assert_eq!(pool_threads(6, DEFAULT_RESERVED_CORES), 4);
        assert_eq!(pool_threads(16, DEFAULT_RESERVED_CORES), 14);
        assert_eq!(pool_threads(8, 0), 8);
    }

    #[test]
    fn the_setting_overrides_the_default_only_when_it_is_a_number() {
        assert_eq!(reserved_cores(None), 2);
        assert_eq!(reserved_cores(Some("0")), 0);
        assert_eq!(reserved_cores(Some(" 3 ")), 3);
        assert_eq!(reserved_cores(Some("many")), 2);
        assert_eq!(reserved_cores(Some("-1")), 2);
        assert_eq!(reserved_cores(Some("")), 2);
    }

    #[test]
    fn a_small_machine_still_gets_one_thread() {
        assert_eq!(pool_threads(2, 2), 1);
        assert_eq!(pool_threads(1, 2), 1);
        assert_eq!(pool_threads(0, 2), 1);
        assert_eq!(pool_threads(3, 2), 1);
    }
}
