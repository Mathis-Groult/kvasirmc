//! How far mobs move per tick, by attention tier.
//!
//! The level of detail of mob AI (see `kvasir-moblod`) is only acceptable if mobs that get less
//! of it still behave like vanilla ones. This counts, per tier, the mob ticks, the ticks in
//! which the mob moved, and the distance moved, so the same scene can be run with the feature on
//! and off and the two compared. Off until [`enable`] (the server does it with
//! `KVASIR_TICK_KINDS`).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Tiers counted.
pub const TIERS: usize = 3;
/// A tick in which a mob moved less than this many blocks is a tick it stood still.
const MOVED_BLOCKS: f64 = 0.001;
/// Distances are kept as whole millionths of a block.
const MICRO: f64 = 1_000_000.0;

static ENABLED: AtomicBool = AtomicBool::new(false);
static TICKS: [AtomicU64; TIERS] = [const { AtomicU64::new(0) }; TIERS];
static MOVING: [AtomicU64; TIERS] = [const { AtomicU64::new(0) }; TIERS];
static MICRO_BLOCKS: [AtomicU64; TIERS] = [const { AtomicU64::new(0) }; TIERS];
static MICRO_VERTICAL: [AtomicU64; TIERS] = [const { AtomicU64::new(0) }; TIERS];
/// Upper edges, in blocks per tick, of the buckets ticks are sorted into by distance moved.
const BUCKET_EDGES: [f64; BUCKETS - 1] = [0.001, 0.02, 0.06, 0.12, 0.3];
/// Buckets: still, creeping, walking, quick, fast, falling or thrown.
pub const BUCKETS: usize = 6;
static BUCKET_TICKS: [[AtomicU64; BUCKETS]; TIERS] =
    [const { [const { AtomicU64::new(0) }; BUCKETS] }; TIERS];

/// What the mobs of one tier did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TierMotion {
    pub tier: usize,
    pub ticks: u64,
    /// Mean blocks moved per mob tick.
    pub blocks_per_tick: f64,
    /// Share of mob ticks in which the mob moved, 0 to 1.
    pub moving: f64,
    /// Mean blocks moved up or down per mob tick (falling, jumping).
    pub vertical_per_tick: f64,
    /// Share of mob ticks in each distance bucket (see [`BUCKETS`]).
    pub buckets: [f64; BUCKETS],
}

/// Starts counting. Not undone.
pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Adds one mob tick of `tier` in which the mob moved `blocks`, `vertical` of them up or down.
pub fn record(tier: usize, blocks: f64, vertical: f64) {
    let slot = tier.min(TIERS - 1);
    TICKS[slot].fetch_add(1, Ordering::Relaxed);
    if blocks > MOVED_BLOCKS {
        MOVING[slot].fetch_add(1, Ordering::Relaxed);
    }
    MICRO_BLOCKS[slot].fetch_add((blocks * MICRO) as u64, Ordering::Relaxed);
    MICRO_VERTICAL[slot].fetch_add((vertical.abs() * MICRO) as u64, Ordering::Relaxed);
    let bucket = BUCKET_EDGES
        .iter()
        .position(|edge| blocks <= *edge)
        .unwrap_or(BUCKETS - 1);
    BUCKET_TICKS[slot][bucket].fetch_add(1, Ordering::Relaxed);
}

/// Every tier that has ticks.
#[must_use]
pub fn report() -> Vec<TierMotion> {
    (0..TIERS)
        .filter_map(|tier| {
            let ticks = TICKS[tier].load(Ordering::Relaxed);
            (ticks > 0).then(|| TierMotion {
                tier,
                ticks,
                blocks_per_tick: MICRO_BLOCKS[tier].load(Ordering::Relaxed) as f64
                    / MICRO
                    / ticks as f64,
                moving: MOVING[tier].load(Ordering::Relaxed) as f64 / ticks as f64,
                vertical_per_tick: MICRO_VERTICAL[tier].load(Ordering::Relaxed) as f64
                    / MICRO
                    / ticks as f64,
                buckets: std::array::from_fn(|bucket| {
                    BUCKET_TICKS[tier][bucket].load(Ordering::Relaxed) as f64 / ticks as f64
                }),
            })
        })
        .collect()
}

/// Starts again from zero.
pub fn reset() {
    for counters in [&TICKS, &MOVING, &MICRO_BLOCKS, &MICRO_VERTICAL] {
        for counter in counters {
            counter.store(0, Ordering::Relaxed);
        }
    }
    for counter in BUCKET_TICKS.iter().flatten() {
        counter.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test: the counters are process-wide.
    #[test]
    fn it_counts_ticks_moving_ticks_and_distance_per_tier() {
        reset();
        record(0, 0.2, 0.0);
        record(0, 0.0, 0.0);
        record(0, 0.0004, 0.0);
        record(2, 1.0, 0.6);
        record(9, 0.5, -0.1); // a tier past the end is counted in the last one
        let report = report();
        assert_eq!(report.len(), 2);
        let full = report[0];
        assert_eq!((full.tier, full.ticks), (0, 3));
        assert!((full.blocks_per_tick - 0.2004 / 3.0).abs() < 1e-6);
        assert!(
            (full.moving - 1.0 / 3.0).abs() < 1e-9,
            "0.0004 blocks is standing still"
        );
        let last = report[1];
        assert_eq!((last.tier, last.ticks), (2, 2));
        assert!((last.blocks_per_tick - 0.75).abs() < 1e-6);
        assert!(
            (last.vertical_per_tick - 0.35).abs() < 1e-6,
            "up and down both count"
        );
        assert!(
            (full.buckets[0] - 2.0 / 3.0).abs() < 1e-9,
            "two still ticks of three"
        );
        assert!(
            (full.buckets[4] - 1.0 / 3.0).abs() < 1e-9,
            "0.2 blocks is a fast tick"
        );
        assert!(
            (last.buckets[5] - 1.0).abs() < 1e-9,
            "1 block and half a block in a tick are both past the last edge"
        );
        assert!((full.buckets.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        reset();
        assert!(super::report().is_empty());
    }
}
