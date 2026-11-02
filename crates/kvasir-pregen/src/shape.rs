//! The area to generate, in chunks around a centre.

use serde::{Deserialize, Serialize};

/// Largest radius accepted, in chunks: a 30 million block world border is 1.875 million chunks
/// each way, and positions must stay inside `i32` after adding the radius to the centre.
pub const MAX_RADIUS: u32 = 1_875_000;

/// A chunk position, in chunks (not blocks).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChunkPos {
    pub x: i32,
    pub z: i32,
}

/// A region around the centre chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Shape {
    /// Every chunk within `radius` chunks along both axes: `(2r + 1)^2` chunks.
    Square { radius: u32 },
    /// Every chunk whose offset `(dx, dz)` has `dx^2 + dz^2 <= radius^2`.
    Circle { radius: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ShapeError {
    #[error("radius {0} chunks is above the limit of {MAX_RADIUS}")]
    TooLarge(u32),
    #[error("the centre {centre:?} with radius {radius} leaves the range of chunk coordinates")]
    OutOfRange { centre: ChunkPos, radius: u32 },
}

impl Shape {
    /// The radius in chunks.
    #[must_use]
    pub const fn radius(self) -> u32 {
        match self {
            Self::Square { radius } | Self::Circle { radius } => radius,
        }
    }

    /// Whether the chunk at offset `(dx, dz)` from the centre belongs to the shape.
    #[must_use]
    pub const fn contains(self, dx: i64, dz: i64) -> bool {
        let radius = self.radius() as i64;
        match self {
            Self::Square { .. } => dx.abs() <= radius && dz.abs() <= radius,
            Self::Circle { .. } => dx * dx + dz * dz <= radius * radius,
        }
    }

    /// How many chunks the shape holds.
    #[must_use]
    pub fn chunk_count(self) -> u64 {
        let radius = u64::from(self.radius());
        match self {
            Self::Square { .. } => (2 * radius + 1) * (2 * radius + 1),
            Self::Circle { .. } => {
                // For each column dx, the rows with dz^2 <= r^2 - dx^2 are 2 * isqrt(..) + 1.
                (0..=radius)
                    .map(|dx| {
                        let rows = 2 * (radius * radius - dx * dx).isqrt() + 1;
                        if dx == 0 { rows } else { 2 * rows }
                    })
                    .sum()
            }
        }
    }

    /// Checks that the shape around `centre` stays inside the coordinates a chunk can have.
    ///
    /// # Errors
    /// The radius is above [`MAX_RADIUS`] or the area leaves the `i32` range.
    pub fn check_around(self, centre: ChunkPos) -> Result<(), ShapeError> {
        let radius = self.radius();
        if radius > MAX_RADIUS {
            return Err(ShapeError::TooLarge(radius));
        }
        let reach = i64::from(radius);
        let inside = |c: i32| {
            let c = i64::from(c);
            i32::try_from(c - reach).is_ok() && i32::try_from(c + reach).is_ok()
        };
        if inside(centre.x) && inside(centre.z) {
            Ok(())
        } else {
            Err(ShapeError::OutOfRange { centre, radius })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_square_holds_the_odd_squares() {
        let counts: Vec<u64> = (0..4)
            .map(|radius| Shape::Square { radius }.chunk_count())
            .collect();
        assert_eq!(counts, [1, 9, 25, 49]);
    }

    /// Gauss's circle problem, OEIS A000328: lattice points within radius r.
    #[test]
    fn a_circle_holds_the_gauss_numbers() {
        let counts: Vec<u64> = (0..=10)
            .map(|radius| Shape::Circle { radius }.chunk_count())
            .collect();
        assert_eq!(counts, [1, 5, 13, 29, 49, 81, 113, 149, 197, 253, 317]);
        assert_eq!(Shape::Circle { radius: 100 }.chunk_count(), 31_417);
    }

    #[test]
    fn the_count_agrees_with_membership() {
        for shape in [Shape::Square { radius: 7 }, Shape::Circle { radius: 7 }] {
            let radius = i64::from(shape.radius());
            let counted = (-radius - 1..=radius + 1)
                .flat_map(|dx| (-radius - 1..=radius + 1).map(move |dz| (dx, dz)))
                .filter(|&(dx, dz)| shape.contains(dx, dz))
                .count() as u64;
            assert_eq!(counted, shape.chunk_count(), "{shape:?}");
        }
    }

    #[test]
    fn the_edge_of_a_circle_is_inside_and_one_past_is_not() {
        let circle = Shape::Circle { radius: 5 };
        assert!(circle.contains(5, 0));
        assert!(circle.contains(3, 4));
        assert!(!circle.contains(4, 4));
        assert!(!circle.contains(6, 0));
    }

    #[test]
    fn a_shape_must_fit_around_its_centre() {
        let shape = Shape::Square { radius: 10 };
        assert_eq!(shape.check_around(ChunkPos { x: 0, z: 0 }), Ok(()));
        let edge = ChunkPos {
            x: i32::MAX - 5,
            z: 0,
        };
        assert!(matches!(
            shape.check_around(edge),
            Err(ShapeError::OutOfRange { .. })
        ));
        assert_eq!(
            Shape::Circle {
                radius: MAX_RADIUS + 1
            }
            .check_around(ChunkPos { x: 0, z: 0 }),
            Err(ShapeError::TooLarge(MAX_RADIUS + 1))
        );
    }

    #[test]
    fn serialises_with_a_readable_kind() {
        let text = serde_json::to_string(&Shape::Circle { radius: 3 }).unwrap();
        assert_eq!(text, r#"{"kind":"circle","radius":3}"#);
        assert_eq!(
            serde_json::from_str::<Shape>(r#"{"kind":"square","radius":2}"#).unwrap(),
            Shape::Square { radius: 2 }
        );
    }
}
