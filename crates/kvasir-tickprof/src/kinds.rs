//! How long the entities of each kind take to tick.
//!
//! Off unless asked for: it reads the clock twice per entity per tick.

use std::{
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Duration,
};

/// Kinds are numbered below this (the vanilla registry has about 160 entity types).
pub const KINDS: usize = 512;

/// What the entities of one kind took since the last reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindRow {
    pub kind: usize,
    pub ticks: u64,
    pub total_ns: u64,
}

impl KindRow {
    /// Mean time of one entity tick, in microseconds.
    #[must_use]
    pub fn mean_us(&self) -> f64 {
        self.total_ns as f64 / self.ticks.max(1) as f64 / 1000.0
    }
}

/// Counters per kind, safe to bump from any thread.
#[derive(Debug)]
pub struct KindTimes {
    enabled: AtomicBool,
    ticks: [AtomicU64; KINDS],
    nanos: [AtomicU64; KINDS],
}

impl Default for KindTimes {
    fn default() -> Self {
        Self::new()
    }
}

impl KindTimes {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            ticks: [const { AtomicU64::new(0) }; KINDS],
            nanos: [const { AtomicU64::new(0) }; KINDS],
        }
    }

    /// Starts counting.
    pub fn enable(&self) {
        self.enabled.store(true, Ordering::Relaxed);
    }

    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Adds one entity tick of `kind`. A kind past [`KINDS`] is counted in the last slot.
    pub fn record(&self, kind: usize, took: Duration) {
        let slot = kind.min(KINDS - 1);
        self.ticks[slot].fetch_add(1, Ordering::Relaxed);
        self.nanos[slot].fetch_add(
            u64::try_from(took.as_nanos()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
    }

    /// The `count` kinds that took the most time, the most first.
    #[must_use]
    pub fn top(&self, count: usize) -> Vec<KindRow> {
        let mut rows: Vec<KindRow> = (0..KINDS)
            .map(|kind| KindRow {
                kind,
                ticks: self.ticks[kind].load(Ordering::Relaxed),
                total_ns: self.nanos[kind].load(Ordering::Relaxed),
            })
            .filter(|row| row.ticks > 0)
            .collect();
        rows.sort_by_key(|row| std::cmp::Reverse(row.total_ns));
        rows.truncate(count);
        rows
    }

    /// Starts again from zero.
    pub fn reset(&self) {
        for counter in self.ticks.iter().chain(&self.nanos) {
            counter.store(0, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_is_off_until_enabled() {
        let kinds = KindTimes::new();
        assert!(!kinds.is_enabled());
        kinds.enable();
        assert!(kinds.is_enabled());
    }

    #[test]
    fn kinds_are_listed_longest_total_first_with_their_mean() {
        let kinds = KindTimes::new();
        kinds.record(7, Duration::from_micros(10));
        kinds.record(7, Duration::from_micros(30));
        kinds.record(3, Duration::from_micros(500));
        kinds.record(9, Duration::from_micros(1));
        let top = kinds.top(2);
        assert_eq!(top.len(), 2);
        assert_eq!((top[0].kind, top[0].ticks), (3, 1));
        assert_eq!((top[1].kind, top[1].ticks), (7, 2));
        assert!((top[1].mean_us() - 20.0).abs() < 1e-9);
    }

    #[test]
    fn a_kind_out_of_range_is_counted_in_the_last_slot() {
        let kinds = KindTimes::new();
        kinds.record(KINDS + 40, Duration::from_micros(5));
        assert_eq!(kinds.top(1)[0].kind, KINDS - 1);
    }

    #[test]
    fn reset_forgets_everything() {
        let kinds = KindTimes::new();
        kinds.record(1, Duration::from_micros(5));
        kinds.reset();
        assert!(kinds.top(5).is_empty());
    }
}
