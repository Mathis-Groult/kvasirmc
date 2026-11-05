//! The chunk a thread read last, kept so that the next read of the same chunk skips the map.
//!
//! Redstone, fluid and neighbour updates read many blocks in a row, almost all in one chunk, and
//! each read paid for a hash and a shard lock in the map of loaded chunks (17 % of the cycles of a
//! redstone-heavy tick). This module owns the rule for when the remembered chunk may still be
//! used; the map itself stays in [`Level`](crate::level::Level).
//!
//! A remembered chunk is valid while three things match what the reader asks about: the level it
//! came from, its position, and the level's *epoch*. The epoch is raised whenever a loaded chunk
//! is removed or replaced, so a chunk that has left the map is never read again from here.

use std::{
    cell::RefCell,
    sync::atomic::{AtomicU64, Ordering},
};

use std::sync::Arc;

use crossbeam::queue::SegQueue;
use pumpkin_util::math::vector2::Vector2;

use crate::level::{ChunkMap, LoadedChunkChange, SyncChunk};

/// Which level a chunk belongs to and how many times its set of loaded chunks has lost or swapped
/// one. Every level has one, shared with the code that unloads chunks.
#[derive(Debug)]
pub struct Version {
    level: u64,
    epoch: AtomicU64,
}

impl Version {
    #[must_use]
    pub fn new() -> Self {
        static NEXT_LEVEL: AtomicU64 = AtomicU64::new(1);
        Self {
            level: NEXT_LEVEL.fetch_add(1, Ordering::Relaxed),
            epoch: AtomicU64::new(0),
        }
    }

    /// A loaded chunk was removed or replaced: nothing remembered before this may be used.
    pub fn chunk_left(&self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
    }

    fn stamp(&self) -> (u64, u64) {
        (self.level, self.epoch.load(Ordering::Acquire))
    }
}

impl Default for Version {
    fn default() -> Self {
        Self::new()
    }
}

/// The map of loaded chunks, changed only through the rules below.
///
/// A chunk that leaves or is replaced raises the epoch, and a change is queued for those who
/// follow which chunks are loaded. Every addition and removal goes through here.
#[derive(Clone)]
pub struct PublicChunks {
    map: Arc<ChunkMap<SyncChunk>>,
    version: Arc<Version>,
    changes: Arc<SegQueue<LoadedChunkChange>>,
}

impl PublicChunks {
    pub const fn new(
        map: Arc<ChunkMap<SyncChunk>>,
        version: Arc<Version>,
        changes: Arc<SegQueue<LoadedChunkChange>>,
    ) -> Self {
        Self {
            map,
            version,
            changes,
        }
    }

    /// Makes `chunk` the loaded chunk at `position`; gives back the one it replaced.
    pub fn publish(&self, position: Vector2<i32>, chunk: SyncChunk) -> Option<SyncChunk> {
        let previous = self.map.insert(position, chunk);
        if previous.is_none() {
            self.changes.push(LoadedChunkChange::Loaded(position));
        } else {
            self.version.chunk_left();
        }
        previous
    }

    /// The loaded chunk at `position`, if any.
    #[must_use]
    pub fn get(&self, position: &Vector2<i32>) -> Option<SyncChunk> {
        self.map.get(position).map(|found| found.value().clone())
    }

    /// Removes the loaded chunk at `position`, if any, and gives it back.
    #[expect(
        clippy::must_use_candidate,
        reason = "the scheduler unloads chunks and does not need the old one back"
    )]
    pub fn unpublish(&self, position: Vector2<i32>) -> Option<SyncChunk> {
        let removed = self.map.remove(&position).map(|(_, chunk)| chunk);
        if removed.is_some() {
            self.version.chunk_left();
            self.changes.push(LoadedChunkChange::Unloaded(position));
        }
        removed
    }
}

struct Remembered {
    stamp: (u64, u64),
    position: Vector2<i32>,
    chunk: SyncChunk,
}

thread_local! {
    static LAST: RefCell<Option<Remembered>> = const { RefCell::new(None) };
}

/// Runs `read` on the chunk at `position`: on the remembered one when it is still valid, else on
/// the one `lookup` finds (which is then remembered). `None` when there is no such chunk.
///
/// `read` may itself read chunks; it then simply finds the remembered one or goes to the map.
pub fn read_through<R>(
    version: &Version,
    position: &Vector2<i32>,
    lookup: impl FnOnce() -> Option<SyncChunk>,
    read: impl FnOnce(&SyncChunk) -> R,
) -> Option<R> {
    let stamp = version.stamp();
    let mut read = Some(read);
    let hit = LAST.with(|cell| {
        let slot = cell.try_borrow().ok()?;
        let remembered = slot.as_ref()?;
        if remembered.stamp != stamp || remembered.position != *position {
            return None;
        }
        read.take().map(|read| read(&remembered.chunk))
    });
    if hit.is_some() {
        return hit;
    }
    let chunk = lookup()?;
    let result = read.take().map(|read| read(&chunk));
    LAST.with(|cell| {
        // Busy only when this call runs inside another `read_through`; that one remembers its own.
        if let Ok(mut slot) = cell.try_borrow_mut() {
            *slot = Some(Remembered {
                stamp,
                position: *position,
                chunk,
            });
        }
    });
    result
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn chunk(x: i32) -> SyncChunk {
        std::sync::Arc::new(crate::chunk::ChunkData::empty(x, 0))
    }

    fn at(x: i32) -> Vector2<i32> {
        Vector2::new(x, 0)
    }

    /// Reads chunk `x` of `version`, counting the map lookups.
    fn read(version: &Version, x: i32, lookups: &Cell<u32>) -> Option<i32> {
        read_through(
            version,
            &at(x),
            || {
                lookups.set(lookups.get() + 1);
                Some(chunk(x))
            },
            |found| found.x,
        )
    }

    fn published() -> (PublicChunks, Arc<Version>, Arc<SegQueue<LoadedChunkChange>>) {
        let version = Arc::new(Version::new());
        let changes = Arc::new(SegQueue::new());
        let public = PublicChunks::new(
            Arc::new(ChunkMap::default()),
            version.clone(),
            changes.clone(),
        );
        (public, version, changes)
    }

    #[test]
    fn a_new_chunk_is_announced_and_leaves_remembered_chunks_valid() {
        let (public, version, changes) = published();
        let before = version.stamp();
        assert!(public.publish(at(1), chunk(1)).is_none());
        assert_eq!(version.stamp(), before);
        assert_eq!(changes.pop(), Some(LoadedChunkChange::Loaded(at(1))));
        assert_eq!(changes.pop(), None);
    }

    #[test]
    fn replacing_a_chunk_ends_what_was_remembered_and_announces_nothing_new() {
        let (public, version, changes) = published();
        public.publish(at(1), chunk(1));
        changes.pop();
        let before = version.stamp();
        assert!(public.publish(at(1), chunk(1)).is_some());
        assert_ne!(version.stamp(), before);
        assert_eq!(changes.pop(), None);
    }

    #[test]
    fn removing_a_chunk_ends_what_was_remembered_and_is_announced() {
        let (public, version, changes) = published();
        public.publish(at(1), chunk(1));
        changes.pop();
        let before = version.stamp();
        assert!(public.unpublish(at(1)).is_some());
        assert_ne!(version.stamp(), before);
        assert_eq!(changes.pop(), Some(LoadedChunkChange::Unloaded(at(1))));
    }

    #[test]
    fn removing_a_chunk_that_is_not_there_changes_nothing() {
        let (public, version, changes) = published();
        let before = version.stamp();
        assert!(public.unpublish(at(8)).is_none());
        assert_eq!(version.stamp(), before);
        assert_eq!(changes.pop(), None);
    }

    #[test]
    fn the_second_read_of_a_chunk_does_not_go_to_the_map() {
        let version = Version::new();
        let lookups = Cell::new(0);
        assert_eq!(read(&version, 3, &lookups), Some(3));
        assert_eq!(read(&version, 3, &lookups), Some(3));
        assert_eq!(read(&version, 3, &lookups), Some(3));
        assert_eq!(lookups.get(), 1);
    }

    #[test]
    fn another_chunk_goes_to_the_map_and_then_is_the_one_remembered() {
        let version = Version::new();
        let lookups = Cell::new(0);
        read(&version, 1, &lookups);
        assert_eq!(read(&version, 2, &lookups), Some(2));
        assert_eq!(lookups.get(), 2);
        read(&version, 2, &lookups);
        assert_eq!(lookups.get(), 2);
        read(&version, 1, &lookups);
        assert_eq!(
            lookups.get(),
            3,
            "chunk 1 was forgotten when chunk 2 was read"
        );
    }

    #[test]
    fn a_chunk_leaving_the_map_is_not_read_again_from_here() {
        let version = Version::new();
        let lookups = Cell::new(0);
        read(&version, 5, &lookups);
        version.chunk_left();
        read(&version, 5, &lookups);
        assert_eq!(lookups.get(), 2);
    }

    #[test]
    fn another_level_never_sees_a_chunk_remembered_for_this_one() {
        let (first, second) = (Version::new(), Version::new());
        let lookups = Cell::new(0);
        read(&first, 7, &lookups);
        read(&second, 7, &lookups);
        assert_eq!(lookups.get(), 2);
    }

    #[test]
    fn a_chunk_that_is_not_loaded_is_none_and_remembers_nothing() {
        let version = Version::new();
        let lookups = Cell::new(0);
        let missing = read_through(
            &version,
            &at(9),
            || {
                lookups.set(lookups.get() + 1);
                None
            },
            |_: &SyncChunk| 0,
        );
        assert_eq!(missing, None);
        read(&version, 9, &lookups);
        assert_eq!(lookups.get(), 2, "the absent chunk was looked up again");
    }

    #[test]
    fn a_read_inside_a_read_works_on_a_hit_and_on_a_miss() {
        let version = Version::new();
        let lookups = Cell::new(0);
        read(&version, 1, &lookups);
        // inside a hit on chunk 1, read chunk 2 (a miss, so the map) and chunk 1 again (a hit)
        let nested = read_through(
            &version,
            &at(1),
            || None,
            |outer| {
                let inner = read(&version, 2, &lookups);
                let again = read(&version, 1, &lookups);
                (outer.x, inner, again)
            },
        );
        assert_eq!(nested, Some((1, Some(2), Some(1))));
        // the inner miss could not be remembered while the outer hit held the slot
        assert_eq!(lookups.get(), 1 + 1);
    }

    #[test]
    fn a_reader_that_sees_a_stale_epoch_remembers_the_new_chunk() {
        let version = Version::new();
        let lookups = Cell::new(0);
        read(&version, 4, &lookups);
        version.chunk_left();
        read(&version, 4, &lookups);
        read(&version, 4, &lookups);
        assert_eq!(
            lookups.get(),
            2,
            "after the refresh the chunk is remembered again"
        );
    }

    /// Cost of a read with and without the memory, over 25 loaded chunks where 9 of 10 reads hit
    /// the chunk read before it (a neighbour update cascade). Run with
    /// `cargo test --profile kvasir-fast -p pumpkin-world --lib chunk_cache::tests::cost -- --ignored --nocapture`.
    #[test]
    #[ignore = "a measurement, not a check"]
    fn cost_of_a_read_with_and_without_the_memory() {
        use std::time::Instant;

        let map = ChunkMap::<SyncChunk>::default();
        for x in 0..5 {
            for z in 0..5 {
                map.insert(
                    Vector2::new(x, z),
                    std::sync::Arc::new(crate::chunk::ChunkData::empty(x, z)),
                );
            }
        }
        let version = Version::new();
        let reads = 20_000_000_u32;
        // 9 reads in a chunk, then one elsewhere; deterministic
        let position = |n: u32| {
            let chunk = (n / 10) % 25;
            Vector2::new((chunk % 5) as i32, (chunk / 5) as i32)
        };
        let mut sum = 0_i64;
        let started = Instant::now();
        for n in 0..reads {
            let at = position(n);
            sum += i64::from(map.get(&at).map(|found| found.value().x).unwrap_or(-1));
        }
        let plain = started.elapsed();
        let started = Instant::now();
        for n in 0..reads {
            let at = position(n);
            sum += i64::from(
                read_through(
                    &version,
                    &at,
                    || map.get(&at).map(|found| found.value().clone()),
                    |found| found.x,
                )
                .unwrap_or(-1),
            );
        }
        let remembered = started.elapsed();
        println!(
            "map only: {:.1} ns per read; with the memory: {:.1} ns per read (checksum {sum})",
            plain.as_nanos() as f64 / f64::from(reads),
            remembered.as_nanos() as f64 / f64::from(reads)
        );
    }
}
