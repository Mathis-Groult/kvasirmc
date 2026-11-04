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
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

pub mod kinds;
pub mod motion;
pub mod parts;
pub mod trace;

/// A server tick longer than this makes players feel a stall (20 ticks per second).
pub const TICK_BUDGET: Duration = Duration::from_millis(50);

/// The tick-time histogram has one bucket per whole millisecond; the last one holds everything
/// longer.
const HISTOGRAM_BUCKETS: usize = 512;

/// How many of the longest ticks keep their per-phase breakdown.
const WORST_KEPT: usize = 16;

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
    /// Inside `Prelude`: sending the block updates and block events that piled up.
    PreludeBlocks,
    /// Inside `Prelude`: working out which chunks are close enough to a player to tick.
    PreludeActiveChunks,
    /// Inside `Prelude`: time of day, weather, sleeping.
    PreludeEnvironment,
    /// Inside `Prelude`: raids.
    PreludeRaids,
    /// Inside `Players`: handling the packets the player sent since the last tick (summed over players).
    PlayerInbound,
    /// Inside `Players`: choosing, encoding and queueing the chunks a player has to be sent (summed over players).
    PlayerChunks,
    /// Inside `Server`, after the worlds: sending pending block updates.
    ServerBlockFlush,
    /// Inside `Server`, after the worlds: letting players' packets go out again.
    ServerResume,
    /// Inside `Server`, before the worlds: datapack tick functions, plugin loaders, scheduled functions.
    ServerFunctions,
    /// Inside `Server`, after the worlds: the player data store's per-tick work (saving).
    ServerPlayerData,
    /// Not work of the server: CPU time the thread that runs the tick used during the tick (from
    /// the scheduler's books). Close to the tick's length means the tick really computed; far
    /// below it, the thread was starved or blocked.
    TickThreadCpu,
    /// Not work of the server: time the tick's thread was runnable but not running, because the
    /// host had no free core for it. Large in a slow tick means the host is the cause.
    TickThreadRunQueue,
}

impl Phase {
    pub const ALL: [Self; 29] = [
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
        Self::PreludeBlocks,
        Self::PreludeActiveChunks,
        Self::PreludeEnvironment,
        Self::PreludeRaids,
        Self::PlayerInbound,
        Self::PlayerChunks,
        Self::ServerBlockFlush,
        Self::ServerResume,
        Self::ServerFunctions,
        Self::ServerPlayerData,
        Self::TickThreadCpu,
        Self::TickThreadRunQueue,
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
            Self::TickThreadCpu => "tick_thread_cpu",
            Self::ServerFunctions => "server_functions",
            Self::ServerPlayerData => "server_player_data",
            Self::TickThreadRunQueue => "tick_thread_runqueue",
            Self::PreludeBlocks => "prelude_blocks",
            Self::PreludeActiveChunks => "prelude_active_chunks",
            Self::PreludeEnvironment => "prelude_environment",
            Self::PreludeRaids => "prelude_raids",
            Self::PlayerInbound => "player_inbound",
            Self::PlayerChunks => "player_chunks",
            Self::ServerBlockFlush => "server_block_flush",
            Self::ServerResume => "server_resume",
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

/// The counters of every phase, and the distribution of whole ticks.
///
/// A tick is closed by recording [`Phase::Server`]: the time every other phase recorded since the
/// previous close belongs to that tick. That is how a slow tick gets its breakdown.
#[derive(Debug)]
pub struct TickProfile {
    phases: [Counters; Phase::ALL.len()],
    /// Time recorded per phase since the last tick closed.
    current: [AtomicU64; Phase::ALL.len()],
    /// Per phase, summed over the ticks within / over budget.
    within_budget: [AtomicU64; Phase::ALL.len()],
    over_budget: [AtomicU64; Phase::ALL.len()],
    ticks_within_budget: AtomicU64,
    ticks_over_budget: AtomicU64,
    histogram: [AtomicU64; HISTOGRAM_BUCKETS],
    worst: Mutex<Vec<SlowTick>>,
    /// When set, every closed tick is also written to a file (see [`trace`]).
    trace: Mutex<Option<trace::Sink>>,
    /// Per entity kind tick times, when asked for (see [`kinds`]).
    pub kinds: kinds::KindTimes,
}

impl Default for TickProfile {
    fn default() -> Self {
        Self {
            phases: std::array::from_fn(|_| Counters::default()),
            current: std::array::from_fn(|_| AtomicU64::new(0)),
            within_budget: std::array::from_fn(|_| AtomicU64::new(0)),
            over_budget: std::array::from_fn(|_| AtomicU64::new(0)),
            ticks_within_budget: AtomicU64::new(0),
            ticks_over_budget: AtomicU64::new(0),
            histogram: std::array::from_fn(|_| AtomicU64::new(0)),
            worst: Mutex::new(Vec::new()),
            trace: Mutex::new(None),
            kinds: kinds::KindTimes::new(),
        }
    }
}

/// One tick that went over budget, with what each phase took in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlowTick {
    /// Which tick since the last reset, counting from 1.
    pub number: u64,
    pub total: Duration,
    /// Per phase, in the order of [`Phase::ALL`]; the `Server` entry is the tick itself.
    pub phases: [Duration; Phase::ALL.len()],
}

/// How long ticks are, to the millisecond.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickDistribution {
    pub ticks: u64,
    pub over_budget: u64,
    pub p50: Duration,
    pub p95: Duration,
    pub p99: Duration,
    pub longest: Duration,
}

/// What a phase costs in an ordinary tick and in a slow one: the phases whose mean grows most
/// are what makes ticks slow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlowPhase {
    pub phase: Phase,
    pub mean_within_budget: Duration,
    pub mean_over_budget: Duration,
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

impl fmt::Display for TickDistribution {
    /// `ticks=N over_budget=N p50=Xms p95=Yms p99=Zms longest=Wms`, one line of `key=value` pairs.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        write!(
            f,
            "ticks={} over_budget={} p50={:.0}ms p95={:.0}ms p99={:.0}ms longest={:.1}ms",
            self.ticks,
            self.over_budget,
            ms(self.p50),
            ms(self.p95),
            ms(self.p99),
            ms(self.longest)
        )
    }
}

impl fmt::Display for SlowPhase {
    /// `name within=X.XXXms over=Y.YYYms`: the mean in ticks within budget and in ticks over it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        write!(
            f,
            "{} within={:.3}ms over={:.3}ms",
            self.phase.name(),
            ms(self.mean_within_budget),
            ms(self.mean_over_budget)
        )
    }
}

impl fmt::Display for SlowTick {
    /// `tick=N total=X.Xms` then every phase that took 1 ms or more as `name=Y.Yms`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        write!(f, "tick={} total={:.1}ms", self.number, ms(self.total))?;
        for (phase, took) in Phase::ALL.iter().zip(self.phases) {
            if *phase != Phase::Server && took >= Duration::from_millis(1) {
                write!(f, " {}={:.1}ms", phase.name(), ms(took))?;
            }
        }
        Ok(())
    }
}

impl TickProfile {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Notes that `phase` took `elapsed` once. Recording [`Phase::Server`] closes the tick.
    pub fn record(&self, phase: Phase, elapsed: Duration) {
        let nanos = u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX);
        let counters = &self.phases[phase.index()];
        counters.calls.fetch_add(1, Ordering::Relaxed);
        counters.total_ns.fetch_add(nanos, Ordering::Relaxed);
        counters.max_ns.fetch_max(nanos, Ordering::Relaxed);
        if phase == Phase::Server {
            self.close_tick(nanos);
        } else {
            self.current[phase.index()].fetch_add(nanos, Ordering::Relaxed);
        }
    }

    fn close_tick(&self, tick_nanos: u64) {
        let mut phases = [0u64; Phase::ALL.len()];
        for (slot, current) in phases.iter_mut().zip(&self.current) {
            *slot = current.swap(0, Ordering::Relaxed);
        }
        phases[Phase::Server.index()] = tick_nanos;
        let millis = usize::try_from(tick_nanos / 1_000_000).unwrap_or(usize::MAX);
        self.histogram[millis.min(HISTOGRAM_BUCKETS - 1)].fetch_add(1, Ordering::Relaxed);

        let budget = u64::try_from(TICK_BUDGET.as_nanos()).unwrap_or(u64::MAX);
        let (sums, ticks) = if tick_nanos > budget {
            (&self.over_budget, &self.ticks_over_budget)
        } else {
            (&self.within_budget, &self.ticks_within_budget)
        };
        ticks.fetch_add(1, Ordering::Relaxed);
        for (sum, nanos) in sums.iter().zip(phases) {
            sum.fetch_add(nanos, Ordering::Relaxed);
        }
        if tick_nanos > budget {
            self.remember_worst(tick_nanos, &phases);
        }
        if let Some(sink) = self
            .trace
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
        {
            sink.tick(&phases);
        }
    }

    /// From now on, writes one line per tick to `path` (see [`trace`]).
    ///
    /// # Errors
    /// The file cannot be created.
    pub fn start_trace(&self, path: &std::path::Path) -> std::io::Result<()> {
        let sink = trace::Sink::create(path)?;
        *self.trace.lock().unwrap_or_else(PoisonError::into_inner) = Some(sink);
        Ok(())
    }

    fn remember_worst(&self, tick_nanos: u64, phases: &[u64; Phase::ALL.len()]) {
        let mut worst = self.worst.lock().unwrap_or_else(PoisonError::into_inner);
        let total = Duration::from_nanos(tick_nanos);
        if worst.len() == WORST_KEPT && worst.last().is_some_and(|w| w.total >= total) {
            return;
        }
        let number = self.ticks_over_budget.load(Ordering::Relaxed)
            + self.ticks_within_budget.load(Ordering::Relaxed);
        worst.push(SlowTick {
            number,
            total,
            phases: phases.map(Duration::from_nanos),
        });
        worst.sort_by_key(|w| std::cmp::Reverse(w.total));
        worst.truncate(WORST_KEPT);
    }

    /// The tick-time distribution since the last reset.
    #[must_use]
    pub fn distribution(&self) -> TickDistribution {
        let counts: Vec<u64> = self
            .histogram
            .iter()
            .map(|b| b.load(Ordering::Relaxed))
            .collect();
        let longest = self.phases[Phase::Server.index()]
            .max_ns
            .load(Ordering::Relaxed);
        let at = |fraction: f64| percentile(&counts, fraction, longest);
        TickDistribution {
            ticks: counts.iter().sum(),
            over_budget: self.ticks_over_budget.load(Ordering::Relaxed),
            p50: at(0.50),
            p95: at(0.95),
            p99: at(0.99),
            longest: Duration::from_nanos(longest),
        }
    }

    /// For each phase, its mean in ticks within budget and in ticks over it.
    #[must_use]
    pub fn slow_phases(&self) -> Vec<SlowPhase> {
        let within = self.ticks_within_budget.load(Ordering::Relaxed);
        let over = self.ticks_over_budget.load(Ordering::Relaxed);
        let mean = |sum: &AtomicU64, ticks: u64| {
            Duration::from_nanos(sum.load(Ordering::Relaxed).checked_div(ticks).unwrap_or(0))
        };
        Phase::ALL
            .iter()
            .map(|&phase| SlowPhase {
                phase,
                mean_within_budget: mean(&self.within_budget[phase.index()], within),
                mean_over_budget: mean(&self.over_budget[phase.index()], over),
            })
            .collect()
    }

    /// The longest ticks over budget, longest first, with their breakdown.
    #[must_use]
    pub fn worst_ticks(&self) -> Vec<SlowTick> {
        self.worst
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
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
        self.kinds.reset();
        parts::reset();
        motion::reset();
        for counters in &self.phases {
            counters.calls.store(0, Ordering::Relaxed);
            counters.total_ns.store(0, Ordering::Relaxed);
            counters.max_ns.store(0, Ordering::Relaxed);
        }
        for counter in self
            .current
            .iter()
            .chain(&self.within_budget)
            .chain(&self.over_budget)
            .chain(&self.histogram)
            .chain([&self.ticks_within_budget, &self.ticks_over_budget])
        {
            counter.store(0, Ordering::Relaxed);
        }
        self.worst
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }
}

/// The smallest tick time (to the millisecond, upper edge of its bucket) that `fraction` of the
/// ticks did not exceed. The overflow bucket answers with `longest_nanos`.
fn percentile(counts: &[u64], fraction: f64, longest_nanos: u64) -> Duration {
    let total: u64 = counts.iter().sum();
    if total == 0 {
        return Duration::ZERO;
    }
    let wanted = (fraction * total as f64).ceil().max(1.0) as u64;
    let mut seen = 0;
    for (index, &count) in counts.iter().enumerate() {
        seen += count;
        if seen >= wanted {
            return if index == counts.len() - 1 {
                Duration::from_nanos(longest_nanos)
            } else {
                Duration::from_millis(index as u64 + 1)
            };
        }
    }
    Duration::from_nanos(longest_nanos)
}

/// What the kernel's scheduler books for one thread: time on a core and time runnable but waiting
/// for one (the file `schedstat` of a thread under `/proc`: run time, wait time and a count of timeslices).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SchedStat {
    pub on_cpu: Duration,
    pub waiting_for_cpu: Duration,
}

impl SchedStat {
    /// Reads the calling thread's books; `None` where the kernel does not offer them.
    #[must_use]
    pub fn of_this_thread() -> Option<Self> {
        Self::parse(&std::fs::read_to_string("/proc/thread-self/schedstat").ok()?)
    }

    /// Reads one `schedstat` line.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut fields = text.split_whitespace();
        let on_cpu = fields.next()?.parse().ok()?;
        let waiting = fields.next()?.parse().ok()?;
        Some(Self {
            on_cpu: Duration::from_nanos(on_cpu),
            waiting_for_cpu: Duration::from_nanos(waiting),
        })
    }

    /// What happened between `earlier` and `self`; zero where the books went backwards.
    #[must_use]
    pub const fn since(self, earlier: Self) -> Self {
        Self {
            on_cpu: self.on_cpu.saturating_sub(earlier.on_cpu),
            waiting_for_cpu: self.waiting_for_cpu.saturating_sub(earlier.waiting_for_cpu),
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

    fn tick(profile: &TickProfile, phases: &[(Phase, u64)], total_ms: u64) {
        for &(phase, ms) in phases {
            profile.record(phase, Duration::from_millis(ms));
        }
        profile.record(Phase::Server, Duration::from_millis(total_ms));
    }

    #[test]
    fn percentiles_come_from_whole_millisecond_buckets() {
        let profile = TickProfile::new();
        for _ in 0..90 {
            tick(&profile, &[], 10);
        }
        for _ in 0..10 {
            tick(&profile, &[], 80);
        }
        let d = profile.distribution();
        assert_eq!(d.ticks, 100);
        assert_eq!(d.over_budget, 10);
        assert_eq!(
            d.p50,
            Duration::from_millis(11),
            "upper edge of the 10 ms bucket"
        );
        assert_eq!(d.p95, Duration::from_millis(81));
        assert_eq!(d.p99, Duration::from_millis(81));
        assert_eq!(d.longest, Duration::from_millis(80));
    }

    #[test]
    fn a_tick_exactly_on_budget_is_not_over_it() {
        let profile = TickProfile::new();
        tick(&profile, &[], 50);
        assert_eq!(profile.distribution().over_budget, 0);
        tick(&profile, &[], 51);
        assert_eq!(profile.distribution().over_budget, 1);
    }

    #[test]
    fn a_tick_longer_than_the_histogram_reports_its_real_length() {
        let profile = TickProfile::new();
        tick(&profile, &[], 3000);
        let d = profile.distribution();
        assert_eq!(
            (d.p50, d.p99, d.longest),
            (
                Duration::from_secs(3),
                Duration::from_secs(3),
                Duration::from_secs(3)
            )
        );
    }

    #[test]
    fn an_empty_profile_has_a_zero_distribution() {
        let d = TickProfile::new().distribution();
        assert_eq!(
            (d.ticks, d.over_budget, d.p95, d.longest),
            (0, 0, Duration::ZERO, Duration::ZERO)
        );
    }

    #[test]
    fn the_phases_of_a_slow_tick_are_told_apart_from_those_of_a_normal_one() {
        let profile = TickProfile::new();
        for _ in 0..3 {
            tick(&profile, &[(Phase::Chunks, 4), (Phase::Entities, 6)], 12);
        }
        tick(&profile, &[(Phase::Chunks, 100), (Phase::Entities, 8)], 120);
        let by_phase = |p: Phase| {
            profile
                .slow_phases()
                .into_iter()
                .find(|s| s.phase == p)
                .unwrap()
        };
        let chunks = by_phase(Phase::Chunks);
        assert_eq!(chunks.mean_within_budget, Duration::from_millis(4));
        assert_eq!(chunks.mean_over_budget, Duration::from_millis(100));
        let entities = by_phase(Phase::Entities);
        assert_eq!(entities.mean_within_budget, Duration::from_millis(6));
        assert_eq!(entities.mean_over_budget, Duration::from_millis(8));
    }

    #[test]
    fn phases_recorded_between_two_closes_belong_to_the_tick_that_closes() {
        let profile = TickProfile::new();
        tick(&profile, &[(Phase::Players, 7)], 60);
        tick(&profile, &[], 60);
        let worst = profile.worst_ticks();
        assert_eq!(worst.len(), 2);
        let players = |w: &SlowTick| w.phases[Phase::Players.index()];
        assert_eq!(
            worst.iter().map(players).sum::<Duration>(),
            Duration::from_millis(7)
        );
        assert_eq!(
            worst.iter().filter(|w| players(w) > Duration::ZERO).count(),
            1
        );
    }

    #[test]
    fn only_the_longest_ticks_keep_their_breakdown_longest_first() {
        let profile = TickProfile::new();
        for ms in 51..51 + 40 {
            tick(&profile, &[(Phase::Chunks, ms)], ms);
        }
        let worst = profile.worst_ticks();
        assert_eq!(worst.len(), WORST_KEPT);
        assert_eq!(worst[0].total, Duration::from_millis(90));
        assert_eq!(worst[WORST_KEPT - 1].total, Duration::from_millis(75));
        assert!(worst.windows(2).all(|w| w[0].total >= w[1].total));
        assert_eq!(
            worst[0].phases[Phase::Chunks.index()],
            Duration::from_millis(90)
        );
        assert_eq!(
            worst[0].phases[Phase::Server.index()],
            Duration::from_millis(90)
        );
        assert_eq!(worst[0].number, 40, "the 40th tick");
    }

    #[test]
    fn ticks_within_budget_keep_no_breakdown() {
        let profile = TickProfile::new();
        tick(&profile, &[(Phase::Chunks, 5)], 10);
        assert!(profile.worst_ticks().is_empty());
    }

    #[test]
    fn reset_clears_the_distribution_and_the_breakdowns() {
        let profile = TickProfile::new();
        tick(&profile, &[(Phase::Chunks, 70)], 80);
        profile.record(Phase::Players, Duration::from_millis(3));
        profile.reset();
        assert_eq!(profile.distribution().ticks, 0);
        assert!(profile.worst_ticks().is_empty());
        tick(&profile, &[], 10);
        let players = profile
            .slow_phases()
            .into_iter()
            .find(|s| s.phase == Phase::Players)
            .unwrap();
        assert_eq!(
            players.mean_within_budget,
            Duration::ZERO,
            "nothing carried over from before the reset"
        );
        assert_eq!(profile.distribution().over_budget, 0);
    }

    #[test]
    fn the_report_lines_read_as_key_value_pairs() {
        let profile = TickProfile::new();
        tick(&profile, &[(Phase::Chunks, 70), (Phase::Players, 0)], 80);
        tick(&profile, &[], 10);
        assert_eq!(
            profile.distribution().to_string(),
            "ticks=2 over_budget=1 p50=11ms p95=81ms p99=81ms longest=80.0ms"
        );
        let slow = &profile.worst_ticks()[0];
        assert_eq!(slow.to_string(), "tick=1 total=80.0ms chunks=70.0ms");
        let chunks = profile
            .slow_phases()
            .into_iter()
            .find(|p| p.phase == Phase::Chunks)
            .unwrap();
        assert_eq!(chunks.to_string(), "chunks within=0.000ms over=70.000ms");
    }

    #[test]
    fn a_schedstat_line_is_two_durations_and_a_count() {
        let stat = SchedStat::parse("7134565 250 12\n").unwrap();
        assert_eq!(stat.on_cpu, Duration::from_nanos(7_134_565));
        assert_eq!(stat.waiting_for_cpu, Duration::from_nanos(250));
        assert_eq!(SchedStat::parse(""), None);
        assert_eq!(SchedStat::parse("12"), None);
        assert_eq!(SchedStat::parse("a b c"), None);
    }

    #[test]
    fn the_difference_of_two_readings_never_goes_negative() {
        let early = SchedStat::parse("100 40 1").unwrap();
        let late = SchedStat::parse("160 100 2").unwrap();
        let d = late.since(early);
        assert_eq!(
            (d.on_cpu, d.waiting_for_cpu),
            (Duration::from_nanos(60), Duration::from_nanos(60))
        );
        assert_eq!(early.since(late), SchedStat::default());
    }

    #[test]
    fn this_thread_has_books_on_linux_and_they_grow_when_it_works() {
        if cfg!(target_os = "linux") {
            let before = SchedStat::of_this_thread().unwrap();
            let mut x = 0u64;
            for i in 0..20_000_000u64 {
                x = x.wrapping_add(std::hint::black_box(i));
            }
            std::hint::black_box(x);
            let spent = SchedStat::of_this_thread().unwrap().since(before);
            assert!(spent.on_cpu > Duration::from_millis(1), "{spent:?}");
        }
    }

    #[test]
    fn the_scheduler_phases_are_ordinary_phases_in_the_breakdown() {
        let profile = TickProfile::new();
        profile.record(Phase::TickThreadCpu, Duration::from_millis(20));
        profile.record(Phase::TickThreadRunQueue, Duration::from_millis(70));
        profile.record(Phase::Server, Duration::from_millis(100));
        let slow = &profile.worst_ticks()[0];
        assert_eq!(
            slow.phases[Phase::TickThreadRunQueue.index()],
            Duration::from_millis(70)
        );
        assert_eq!(
            slow.to_string(),
            "tick=1 total=100.0ms tick_thread_cpu=20.0ms tick_thread_runqueue=70.0ms"
        );
    }
}
