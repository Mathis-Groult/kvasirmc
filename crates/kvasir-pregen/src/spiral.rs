//! The order chunks are generated in: rings around the centre, nearest first, so that the middle
//! of the area is finished first and an interrupted job leaves a usable disc.

use serde::{Deserialize, Serialize};

use crate::shape::{ChunkPos, Shape};

/// Where a spiral stands: ring `ring` (Chebyshev distance from the centre), cell `index` of it.
/// Ring 0 has 1 cell, ring `k` has `8k`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Cursor {
    pub ring: u32,
    pub index: u32,
}

/// The offset of cell `index` on ring `ring`, walking clockwise from the top-left corner:
/// along the top edge, down the right, back along the bottom, up the left.
#[must_use]
pub fn ring_cell(ring: u32, index: u32) -> (i64, i64) {
    if ring == 0 {
        return (0, 0);
    }
    let (k, i) = (i64::from(ring), i64::from(index));
    let side = 2 * k;
    match i / side {
        0 => (-k + i, -k),
        1 => (k, -k + (i - side)),
        2 => (k - (i - 2 * side), k),
        _ => (-k, k - (i - 3 * side)),
    }
}

/// Number of cells on a ring.
#[must_use]
pub const fn ring_len(ring: u32) -> u32 {
    if ring == 0 { 1 } else { 8 * ring }
}

/// The chunks of a shape around a centre, nearest rings first.
#[derive(Debug, Clone)]
pub struct Spiral {
    shape: Shape,
    centre: ChunkPos,
    next: Cursor,
}

impl Spiral {
    /// A spiral from the centre outwards. The shape must fit around the centre (see
    /// [`Shape::check_around`]).
    #[must_use]
    pub const fn new(shape: Shape, centre: ChunkPos) -> Self {
        Self::resume(shape, centre, Cursor { ring: 0, index: 0 })
    }

    /// A spiral that continues at `cursor`, as [`cursor`](Self::cursor) reported it earlier.
    #[must_use]
    pub const fn resume(shape: Shape, centre: ChunkPos, cursor: Cursor) -> Self {
        Self {
            shape,
            centre,
            next: cursor,
        }
    }

    /// Where the next chunk will come from. Saving it and resuming there continues exactly.
    #[must_use]
    pub const fn cursor(&self) -> Cursor {
        self.next
    }
}

impl Iterator for Spiral {
    type Item = ChunkPos;

    fn next(&mut self) -> Option<ChunkPos> {
        let radius = self.shape.radius();
        while self.next.ring <= radius {
            let Cursor { ring, index } = self.next;
            if index >= ring_len(ring) {
                self.next = Cursor {
                    ring: ring + 1,
                    index: 0,
                };
                continue;
            }
            self.next.index += 1;
            let (dx, dz) = ring_cell(ring, index);
            if self.shape.contains(dx, dz) {
                // `check_around` guarantees centre + offset stays in range.
                return Some(ChunkPos {
                    x: i32::try_from(i64::from(self.centre.x) + dx).ok()?,
                    z: i32::try_from(i64::from(self.centre.z) + dz).ok()?,
                });
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    const ORIGIN: ChunkPos = ChunkPos { x: 0, z: 0 };

    #[test]
    fn rings_have_eight_k_distinct_cells_all_at_distance_k() {
        for ring in 1..=6 {
            let cells: Vec<_> = (0..ring_len(ring)).map(|i| ring_cell(ring, i)).collect();
            let distinct: HashSet<_> = cells.iter().collect();
            assert_eq!(distinct.len() as u32, 8 * ring);
            assert!(
                cells
                    .iter()
                    .all(|&(dx, dz)| dx.abs().max(dz.abs()) == i64::from(ring))
            );
        }
    }

    #[test]
    fn a_ring_is_walked_without_jumps() {
        for ring in 1..=6 {
            let len = ring_len(ring);
            for i in 0..len {
                let (ax, az) = ring_cell(ring, i);
                let (bx, bz) = ring_cell(ring, (i + 1) % len);
                assert!(
                    (ax - bx).abs() <= 1 && (az - bz).abs() <= 1 && (ax, az) != (bx, bz),
                    "ring {ring}: {i} -> {}",
                    (i + 1) % len
                );
            }
        }
    }

    #[test]
    fn ring_one_in_order() {
        let cells: Vec<_> = (0..8).map(|i| ring_cell(1, i)).collect();
        assert_eq!(
            cells,
            [
                (-1, -1),
                (0, -1),
                (1, -1),
                (1, 0),
                (1, 1),
                (0, 1),
                (-1, 1),
                (-1, 0)
            ]
        );
    }

    #[test]
    fn a_circle_of_radius_one_is_the_centre_and_its_four_neighbours() {
        let chunks: Vec<_> =
            Spiral::new(Shape::Circle { radius: 1 }, ChunkPos { x: 10, z: -4 }).collect();
        assert_eq!(
            chunks,
            [
                ChunkPos { x: 10, z: -4 },
                ChunkPos { x: 10, z: -5 },
                ChunkPos { x: 11, z: -4 },
                ChunkPos { x: 10, z: -3 },
                ChunkPos { x: 9, z: -4 },
            ]
        );
    }

    #[test]
    fn yields_each_chunk_of_the_shape_exactly_once() {
        for shape in [Shape::Square { radius: 6 }, Shape::Circle { radius: 6 }] {
            let chunks: Vec<_> = Spiral::new(shape, ChunkPos { x: -3, z: 9 }).collect();
            let distinct: HashSet<_> = chunks.iter().collect();
            assert_eq!(chunks.len() as u64, shape.chunk_count(), "{shape:?}");
            assert_eq!(distinct.len(), chunks.len(), "{shape:?}");
            assert!(
                chunks
                    .iter()
                    .all(|c| shape.contains(i64::from(c.x) + 3, i64::from(c.z) - 9))
            );
        }
    }

    #[test]
    fn goes_outwards_never_back() {
        let mut last = 0;
        for chunk in Spiral::new(Shape::Circle { radius: 9 }, ORIGIN) {
            let ring = chunk.x.abs().max(chunk.z.abs());
            assert!(ring >= last, "ring {ring} after {last}");
            last = ring;
        }
    }

    #[test]
    fn resuming_from_a_saved_cursor_continues_exactly() {
        let shape = Shape::Circle { radius: 8 };
        let all: Vec<_> = Spiral::new(shape, ORIGIN).collect();
        for done in [0, 1, 4, 5, 37, 100, all.len() - 1, all.len()] {
            let mut spiral = Spiral::new(shape, ORIGIN);
            for _ in 0..done {
                spiral.next();
            }
            let saved = spiral.cursor();
            let rest: Vec<_> = Spiral::resume(shape, ORIGIN, saved).collect();
            assert_eq!(rest, all[done..], "after {done} chunks");
        }
    }

    #[test]
    fn a_cursor_survives_json() {
        let cursor = Cursor { ring: 4, index: 17 };
        let text = serde_json::to_string(&cursor).unwrap();
        assert_eq!(serde_json::from_str::<Cursor>(&text).unwrap(), cursor);
    }

    #[test]
    fn a_cursor_past_the_end_yields_nothing() {
        let shape = Shape::Square { radius: 2 };
        let end = Cursor { ring: 3, index: 0 };
        assert_eq!(Spiral::resume(shape, ORIGIN, end).count(), 0);
    }
}
