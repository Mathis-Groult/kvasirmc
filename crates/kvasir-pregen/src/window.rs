//! Chunks that were handed to the generator and have not all come back.
//!
//! Chunks finish out of order. A job saved with the cursor of the next chunk to hand out would
//! forget the ones still in flight if the server stopped; saved with the cursor of the oldest chunk
//! not yet finished, a restart repeats a few chunks that are already on disk (loading a finished
//! chunk is cheap and generates nothing) and skips none.

use std::collections::VecDeque;

use crate::{shape::ChunkPos, spiral::Cursor};

struct Entry {
    pos: ChunkPos,
    /// The spiral's cursor just before this chunk was handed out.
    before: Cursor,
    /// When it was handed out (the caller's clock, in milliseconds).
    at_ms: u64,
    finished: bool,
}

/// The chunks handed out and not yet counted as done, oldest first.
pub struct InFlight {
    entries: VecDeque<Entry>,
    /// The cursor after the last chunk handed out.
    handed_out: Cursor,
    finished_total: u64,
}

impl InFlight {
    /// Nothing in flight, the spiral standing at `start`.
    #[must_use]
    pub const fn new(start: Cursor, already_done: u64) -> Self {
        Self {
            entries: VecDeque::new(),
            handed_out: start,
            finished_total: already_done,
        }
    }

    /// A chunk was handed to the generator at `at_ms`; `before` and `after` are the spiral's
    /// cursor around it.
    pub fn hand_out(&mut self, pos: ChunkPos, before: Cursor, after: Cursor, at_ms: u64) {
        self.entries.push_back(Entry {
            pos,
            before,
            at_ms,
            finished: false,
        });
        self.handed_out = after;
    }

    /// The generator finished `pos`. Returns false when `pos` is not in flight (a late or
    /// repeated report), which changes nothing.
    pub fn finish(&mut self, pos: ChunkPos) -> bool {
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.pos == pos && !entry.finished)
        else {
            return false;
        };
        entry.finished = true;
        self.finished_total += 1;
        while self.entries.front().is_some_and(|entry| entry.finished) {
            self.entries.pop_front();
        }
        true
    }

    /// Chunks handed out and not finished.
    #[must_use]
    pub fn unfinished(&self) -> usize {
        self.entries.iter().filter(|entry| !entry.finished).count()
    }

    /// Entries kept in memory: unfinished chunks plus finished ones queued behind an older
    /// unfinished chunk. Stays small however long the job runs.
    #[must_use]
    pub fn tracked(&self) -> usize {
        self.entries.len()
    }

    /// Every chunk handed out and not finished, oldest first.
    #[must_use]
    pub fn unfinished_positions(&self) -> Vec<ChunkPos> {
        self.entries
            .iter()
            .filter(|entry| !entry.finished)
            .map(|entry| entry.pos)
            .collect()
    }

    /// The chunk that has waited longest without finishing, and when it was handed out.
    #[must_use]
    pub fn oldest_unfinished(&self) -> Option<(ChunkPos, u64)> {
        self.entries
            .iter()
            .find(|entry| !entry.finished)
            .map(|entry| (entry.pos, entry.at_ms))
    }

    /// Chunks finished over the whole job, including before a resume.
    #[must_use]
    pub const fn finished_total(&self) -> u64 {
        self.finished_total
    }

    /// Where a restart must continue: at the oldest unfinished chunk, or after the last one
    /// handed out when everything handed out is finished.
    #[must_use]
    pub fn resume_cursor(&self) -> Cursor {
        self.entries
            .iter()
            .find(|entry| !entry.finished)
            .map_or(self.handed_out, |entry| entry.before)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{shape::Shape, spiral::Spiral};

    const SHAPE: Shape = Shape::Square { radius: 3 };
    const ORIGIN: ChunkPos = ChunkPos { x: 0, z: 0 };

    /// Hands out `count` chunks of the spiral, returning them.
    fn hand_out(flight: &mut InFlight, spiral: &mut Spiral, count: usize) -> Vec<ChunkPos> {
        (0..count)
            .map(|_| {
                let before = spiral.cursor();
                let pos = spiral.next().expect("chunks left");
                flight.hand_out(pos, before, spiral.cursor(), 0);
                pos
            })
            .collect()
    }

    #[test]
    fn everything_finished_resumes_after_the_last_chunk_handed_out() {
        let mut spiral = Spiral::new(SHAPE, ORIGIN);
        let mut flight = InFlight::new(spiral.cursor(), 0);
        let chunks = hand_out(&mut flight, &mut spiral, 5);
        for pos in chunks {
            assert!(flight.finish(pos));
        }
        assert_eq!(flight.unfinished(), 0);
        assert_eq!(flight.finished_total(), 5);
        assert_eq!(flight.resume_cursor(), spiral.cursor());
    }

    #[test]
    fn an_unfinished_old_chunk_holds_the_resume_point_back() {
        let mut spiral = Spiral::new(SHAPE, ORIGIN);
        let mut flight = InFlight::new(spiral.cursor(), 0);
        let chunks = hand_out(&mut flight, &mut spiral, 6);
        // chunks 0, 1, 3, 4, 5 finish; chunk 2 is still running
        for index in [0, 1, 3, 4, 5] {
            flight.finish(chunks[index]);
        }
        let resumed: Vec<_> = Spiral::resume(SHAPE, ORIGIN, flight.resume_cursor()).collect();
        let all: Vec<_> = Spiral::new(SHAPE, ORIGIN).collect();
        assert_eq!(
            resumed,
            all[2..],
            "restarts at chunk 2, the oldest unfinished"
        );
        assert_eq!(flight.unfinished(), 1);
        // when it finishes, the resume point jumps past everything
        flight.finish(chunks[2]);
        assert_eq!(flight.resume_cursor(), spiral.cursor());
    }

    #[test]
    fn a_resume_never_skips_a_chunk_whatever_order_they_finish_in() {
        let all: Vec<_> = Spiral::new(SHAPE, ORIGIN).collect();
        // finish orders chosen to be awkward: reversed, interleaved, one straggler
        let orders: [&[usize]; 3] = [
            &[7, 6, 5, 4, 3, 2, 1, 0],
            &[1, 0, 3, 2, 5, 4, 7, 6],
            &[1, 2, 3, 4, 5, 6, 7],
        ];
        for order in orders {
            let mut spiral = Spiral::new(SHAPE, ORIGIN);
            let mut flight = InFlight::new(spiral.cursor(), 0);
            let chunks = hand_out(&mut flight, &mut spiral, 8);
            for (step, &index) in order.iter().enumerate() {
                flight.finish(chunks[index]);
                let resumed: Vec<_> =
                    Spiral::resume(SHAPE, ORIGIN, flight.resume_cursor()).collect();
                let unfinished_first = (0..8).find(|i| !order[..=step].contains(i));
                let from = unfinished_first.unwrap_or(8);
                assert_eq!(
                    resumed,
                    all[from..],
                    "order {order:?} after {step} finished"
                );
            }
        }
    }

    #[test]
    fn finished_chunks_are_forgotten_so_memory_stays_bounded() {
        let big = Shape::Square { radius: 80 };
        let mut spiral = Spiral::new(big, ORIGIN);
        let mut flight = InFlight::new(spiral.cursor(), 0);
        let mut peak = 0;
        for _ in 0..5000 {
            let chunks = hand_out(&mut flight, &mut spiral, 4);
            peak = peak.max(flight.tracked());
            for pos in chunks.into_iter().rev() {
                flight.finish(pos);
            }
        }
        assert_eq!(flight.tracked(), 0);
        assert_eq!(flight.finished_total(), 20_000);
        assert!(peak <= 4, "peak {peak}");
    }

    #[test]
    fn the_oldest_unfinished_chunk_is_named_with_its_hand_out_time() {
        let mut spiral = Spiral::new(SHAPE, ORIGIN);
        let mut flight = InFlight::new(spiral.cursor(), 0);
        assert_eq!(flight.oldest_unfinished(), None);
        let mut chunks = Vec::new();
        for at_ms in [100, 250, 400] {
            let before = spiral.cursor();
            let pos = spiral.next().unwrap();
            flight.hand_out(pos, before, spiral.cursor(), at_ms);
            chunks.push(pos);
        }
        assert_eq!(flight.oldest_unfinished(), Some((chunks[0], 100)));
        assert_eq!(flight.unfinished_positions(), chunks);
        // a younger chunk finishing first changes nothing; the oldest finishing moves it on
        flight.finish(chunks[1]);
        assert_eq!(flight.oldest_unfinished(), Some((chunks[0], 100)));
        flight.finish(chunks[0]);
        assert_eq!(flight.unfinished_positions(), [chunks[2]]);
        assert_eq!(flight.oldest_unfinished(), Some((chunks[2], 400)));
        flight.finish(chunks[2]);
        assert_eq!(flight.oldest_unfinished(), None);
    }

    #[test]
    fn a_repeated_or_unknown_report_changes_nothing() {
        let mut spiral = Spiral::new(SHAPE, ORIGIN);
        let mut flight = InFlight::new(spiral.cursor(), 10);
        let chunks = hand_out(&mut flight, &mut spiral, 2);
        assert!(flight.finish(chunks[1]));
        assert!(!flight.finish(chunks[1]), "twice");
        assert!(
            !flight.finish(ChunkPos { x: 99, z: 99 }),
            "never handed out"
        );
        assert_eq!(flight.finished_total(), 11);
    }

    #[test]
    fn the_total_includes_what_was_done_before_a_resume() {
        let flight = InFlight::new(Cursor { ring: 2, index: 3 }, 1234);
        assert_eq!(flight.finished_total(), 1234);
        assert_eq!(flight.resume_cursor(), Cursor { ring: 2, index: 3 });
    }
}
