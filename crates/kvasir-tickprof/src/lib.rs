//! Always-on timing of the phases of a server tick.
//!
//! Every phase has three atomic counters (calls, total time, longest time), so recording costs two
//! atomic additions and one maximum, nothing is allocated, and any thread may record. The
//! engine times its phases anyway; this keeps the numbers instead of throwing them away, so that
//! "where does a tick go" has an answer from a running server without a profiler.

// Tests state expectations with `unwrap`; the engine's lints forbid it everywhere else.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

/// A part of a tick. Phases nest: `World` contains `Chunks`, `Players`, `Entities` and so on, so
/// their shares are of the tick and do not add up to 100%.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    /// The whole server tick, everything below included.
    Server,
    /// One world's tick (called once per dimension per server tick).
    World,
    /// A world's bookkeeping before its chunks tick: pending block updates, active chunks,
    /// weather and time, raids.
    Prelude,
    /// Random ticks, mob spawning, the chunk system's per-tick work.
    Chunks,
    /// Every player's own tick: network, chunk watching, inventory.
    Players,
    /// Planning and ticking entities.
    Entities,
    /// Telling players about entities that moved.
    EntityTracking,
    /// Block entities (furnaces, hoppers, chests).
    BlockEntities,
    /// Handing the chunk system the tickets that changed.
    ChunkTickets,
    /// Inside `Chunks`: collecting which blocks, fluids and random positions tick this tick.
    ChunkTickData,
    /// Inside `Chunks`: scheduled block and fluid ticks.
    ChunkScheduled,
    /// Inside `Chunks`: random ticks (crops, leaves, fire...).
    ChunkRandom,
    /// Inside `Chunks`: choosing and attempting natural mob spawns.
    ChunkSpawning,
    /// Inside `Chunks`: counting inhabited time.
    ChunkInhabited,
    /// Inside `Entities`: deciding which entities are close enough to tick.
    EntityFilter,
    /// Inside `Entities`: working out which entities can tick in parallel.
    EntityPlanning,
    /// Inside `Entities`: the entities' own ticks.
    EntityTicking,
}

impl Phase {
    pub const ALL: [Self; 17] = [
        Self::Server,
        Self::World,
        Self::Prelude,
        Self::Chunks,
        Self::Players,
        Self::Entities,
        Self::EntityTracking,
        Self::BlockEntities,
        Self::ChunkTickets,
        Self::ChunkTickData,
        Self::ChunkScheduled,
        Self::ChunkRandom,
        Self::ChunkSpawning,
        Self::ChunkInhabited,
        Self::EntityFilter,
        Self::EntityPlanning,
        Self::EntityTicking,
    ];

    /// A short, stable name for output and for scripts that read it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::World => "world",
            Self::Prelude => "prelude",
            Self::Chunks => "chunks",
            Self::Players => "players",
            Self::Entities => "entities",
            Self::EntityTracking => "entity_tracking",
            Self::BlockEntities => "block_entities",
            Self::ChunkTickets => "chunk_tickets",
            Self::ChunkTickData => "chunk_tick_data",
            Self::ChunkScheduled => "chunk_scheduled",
            Self::ChunkRandom => "chunk_random",
            Self::ChunkSpawning => "chunk_spawning",
            Self::ChunkInhabited => "chunk_inhabited",
            Self::EntityFilter => "entity_filter",
            Self::EntityPlanning => "entity_planning",
            Self::EntityTicking => "entity_ticking",
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Default)]
struct Counters {
    calls: AtomicU64,
    total_ns: AtomicU64,
    max_ns: AtomicU64,
}

/// The counters of every phase.
#[derive(Debug, Default)]
pub struct TickProfile {
    phases: [Counters; Phase::ALL.len()],
}

/// What one phase has measured since the last reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhaseReading {
    pub phase: Phase,
    pub calls: u64,
    pub total: Duration,
    pub longest: Duration,
}

impl PhaseReading {
    /// Average time per call; zero when there were none.
    #[must_use]
    pub fn mean(&self) -> Duration {
        let nanos = self
            .total
            .as_nanos()
            .checked_div(u128::from(self.calls))
            .unwrap_or(0);
        Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
    }
}

impl fmt::Display for PhaseReading {
    /// `name calls=N mean=X.XXXms longest=Y.YYYms total=Z.ZZZms`, one line, `key=value` pairs.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        write!(
            f,
            "{} calls={} mean={:.3}ms longest={:.3}ms total={:.3}ms",
            self.phase.name(),
            self.calls,
            ms(self.mean()),
            ms(self.longest),
            ms(self.total)
        )
    }
}

impl TickProfile {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Notes that `phase` took `elapsed` once.
    pub fn record(&self, phase: Phase, elapsed: Duration) {
        let nanos = u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX);
        let counters = &self.phases[phase.index()];
        counters.calls.fetch_add(1, Ordering::Relaxed);
        counters.total_ns.fetch_add(nanos, Ordering::Relaxed);
        counters.max_ns.fetch_max(nanos, Ordering::Relaxed);
    }

    /// Every phase's reading, in the order of [`Phase::ALL`].
    #[must_use]
    pub fn snapshot(&self) -> Vec<PhaseReading> {
        Phase::ALL
            .iter()
            .map(|&phase| {
                let counters = &self.phases[phase.index()];
                PhaseReading {
                    phase,
                    calls: counters.calls.load(Ordering::Relaxed),
                    total: Duration::from_nanos(counters.total_ns.load(Ordering::Relaxed)),
                    longest: Duration::from_nanos(counters.max_ns.load(Ordering::Relaxed)),
                }
            })
            .collect()
    }

    /// Starts counting from zero again.
    pub fn reset(&self) {
        for counters in &self.phases {
            counters.calls.store(0, Ordering::Relaxed);
            counters.total_ns.store(0, Ordering::Relaxed);
            counters.max_ns.store(0, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, thread};

    use super::*;

    fn reading(profile: &TickProfile, phase: Phase) -> PhaseReading {
        profile
            .snapshot()
            .into_iter()
            .find(|r| r.phase == phase)
            .unwrap()
    }

    #[test]
    fn calls_total_and_longest_accumulate() {
        let profile = TickProfile::new();
        for ms in [2, 9, 4] {
            profile.record(Phase::Chunks, Duration::from_millis(ms));
        }
        let chunks = reading(&profile, Phase::Chunks);
        assert_eq!(chunks.calls, 3);
        assert_eq!(chunks.total, Duration::from_millis(15));
        assert_eq!(chunks.longest, Duration::from_millis(9));
        assert_eq!(chunks.mean(), Duration::from_millis(5));
    }

    #[test]
    fn phases_do_not_leak_into_each_other() {
        let profile = TickProfile::new();
        profile.record(Phase::Players, Duration::from_millis(7));
        assert_eq!(reading(&profile, Phase::Players).calls, 1);
        for other in Phase::ALL.into_iter().filter(|p| *p != Phase::Players) {
            assert_eq!(reading(&profile, other).calls, 0, "{other:?}");
        }
    }

    #[test]
    fn reset_starts_again_from_zero() {
        let profile = TickProfile::new();
        profile.record(Phase::World, Duration::from_millis(30));
        profile.reset();
        let world = reading(&profile, Phase::World);
        assert_eq!(
            (world.calls, world.total, world.longest),
            (0, Duration::ZERO, Duration::ZERO)
        );
        profile.record(Phase::World, Duration::from_millis(1));
        assert_eq!(
            reading(&profile, Phase::World).longest,
            Duration::from_millis(1)
        );
    }

    #[test]
    fn a_phase_nobody_called_has_a_zero_mean() {
        assert_eq!(
            reading(&TickProfile::new(), Phase::Entities).mean(),
            Duration::ZERO
        );
    }

    #[test]
    fn many_threads_recording_lose_nothing() {
        let profile = Arc::new(TickProfile::new());
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let profile = Arc::clone(&profile);
                thread::spawn(move || {
                    for _ in 0..10_000 {
                        profile.record(Phase::Entities, Duration::from_micros(3));
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        let entities = reading(&profile, Phase::Entities);
        assert_eq!(entities.calls, 80_000);
        assert_eq!(entities.total, Duration::from_millis(240));
    }

    #[test]
    fn a_huge_duration_saturates_instead_of_wrapping() {
        let profile = TickProfile::new();
        profile.record(Phase::Server, Duration::MAX);
        assert_eq!(
            reading(&profile, Phase::Server).longest,
            Duration::from_nanos(u64::MAX)
        );
    }

    #[test]
    fn names_are_unique_and_every_phase_has_one() {
        let mut names: Vec<_> = Phase::ALL.iter().map(|p| p.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Phase::ALL.len());
        assert_eq!(Phase::ALL.len(), TickProfile::new().snapshot().len());
        for (i, phase) in Phase::ALL.iter().enumerate() {
            assert_eq!(phase.index(), i, "ALL must follow the declaration order");
        }
    }

    #[test]
    fn a_line_reads_as_key_value_pairs() {
        let profile = TickProfile::new();
        profile.record(Phase::Chunks, Duration::from_micros(1500));
        profile.record(Phase::Chunks, Duration::from_micros(2500));
        let line = reading(&profile, Phase::Chunks).to_string();
        assert_eq!(
            line,
            "chunks calls=2 mean=2.000ms longest=2.500ms total=4.000ms"
        );
    }
}
