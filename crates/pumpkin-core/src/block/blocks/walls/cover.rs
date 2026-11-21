//! Vanilla `WallBlock.isCovered`: whether the bottom face of the block above covers a test shape.
//!
//! Vanilla takes `aboveState.getCollisionShape(..).getFaceShape(Direction.DOWN)`, the lowest
//! layer of the collision shape, and asks whether `TEST_SHAPE_POST` or one of the
//! `TEST_SHAPES_WALL` (both full height, so only their footprint matters) lies inside it.

use pumpkin_data::BlockState;
use pumpkin_data::block_properties::HorizontalFacing;
use pumpkin_util::math::position::BlockPos;

/// `VoxelShape` merges coordinates closer than this.
const EPSILON: f64 = 1.0e-7;

/// A footprint on the horizontal plane: `[min_x, min_z, max_x, max_z]` in block units.
pub(super) type Footprint = [f64; 4];

/// `TEST_SHAPE_POST`: `Block.column(2, 0, 16)`.
const TEST_POST: Footprint = [7.0 / 16.0, 7.0 / 16.0, 9.0 / 16.0, 9.0 / 16.0];

/// `TEST_SHAPES_WALL`: `Block.boxZ(2, 16, 0, 9)` turned to each horizontal direction.
const fn test_side(direction: HorizontalFacing) -> Footprint {
    const NEAR: f64 = 7.0 / 16.0;
    const FAR: f64 = 9.0 / 16.0;
    match direction {
        HorizontalFacing::North => [NEAR, 0.0, FAR, FAR],
        HorizontalFacing::South => [NEAR, NEAR, FAR, 1.0],
        HorizontalFacing::East => [NEAR, NEAR, 1.0, FAR],
        HorizontalFacing::West => [0.0, NEAR, FAR, FAR],
    }
}

/// `getFaceShape(DOWN)` of the collision shape of `state` at `position`: the footprints of the
/// boxes that reach down to the block's bottom plane.
pub(super) fn bottom_face(state: &BlockState, position: &BlockPos) -> Vec<Footprint> {
    state
        .get_block_collision_shapes_at(position)
        .filter(|shape| shape.min.y <= EPSILON && shape.max.y > EPSILON)
        .map(|shape| [shape.min.x, shape.min.z, shape.max.x, shape.max.z])
        .collect()
}

/// Whether the face covers the side test shape of `direction`.
pub(super) fn covers_side(face: &[Footprint], direction: HorizontalFacing) -> bool {
    covers(face, test_side(direction))
}

/// Whether the face covers the post test shape.
pub(super) fn covers_post(face: &[Footprint]) -> bool {
    covers(face, TEST_POST)
}

/// Every cell of the grid cut by the face's edges inside `test` has its centre in some box.
fn covers(face: &[Footprint], test: Footprint) -> bool {
    let cuts = |low: usize, high: usize| {
        let mut points = vec![test[low], test[high]];
        for rect in face {
            for value in [rect[low], rect[high]] {
                if value > test[low] && value < test[high] {
                    points.push(value);
                }
            }
        }
        points.sort_by(f64::total_cmp);
        points
    };
    let (xs, zs) = (cuts(0, 2), cuts(1, 3));
    xs.windows(2).all(|x| {
        x[1] - x[0] <= EPSILON
            || zs.windows(2).all(|z| {
                let (cx, cz) = (f64::midpoint(x[0], x[1]), f64::midpoint(z[0], z[1]));
                z[1] - z[0] <= EPSILON
                    || face
                        .iter()
                        .any(|r| cx >= r[0] && cx <= r[2] && cz >= r[1] && cz <= r[3])
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: Footprint = [0.0, 0.0, 1.0, 1.0];

    #[test]
    fn a_full_footprint_covers_everything() {
        assert!(covers_post(&[FULL]));
        for direction in HorizontalFacing::all() {
            assert!(covers_side(&[FULL], direction));
        }
    }

    #[test]
    fn nothing_covers_nothing() {
        assert!(!covers_post(&[]));
        assert!(!covers_side(&[], HorizontalFacing::North));
    }

    #[test]
    fn a_fence_post_covers_the_wall_post_but_no_side() {
        let fence_post = [6.0 / 16.0, 6.0 / 16.0, 10.0 / 16.0, 10.0 / 16.0];
        assert!(covers_post(&[fence_post]));
        for direction in HorizontalFacing::all() {
            assert!(!covers_side(&[fence_post], direction));
        }
    }

    #[test]
    fn two_boxes_together_cover_a_side() {
        // The north side test shape is x 7..9, z 0..9: one box for each half of it.
        let near = [0.0, 0.0, 1.0, 4.0 / 16.0];
        let far = [0.0, 4.0 / 16.0, 1.0, 9.0 / 16.0];
        assert!(!covers_side(&[near], HorizontalFacing::North));
        assert!(covers_side(&[near, far], HorizontalFacing::North));
        assert!(!covers_side(&[near, far], HorizontalFacing::South));
    }

    #[test]
    fn a_hole_in_the_middle_is_found() {
        let left = [0.0, 0.0, 8.0 / 16.0, 1.0];
        let right = [8.5 / 16.0, 0.0, 1.0, 1.0];
        assert!(!covers_post(&[left, right]));
    }

    #[test]
    fn faces_beyond_the_test_shape_do_not_matter() {
        let post = [0.4, 0.4, 0.6, 0.6];
        let far_corner = [0.0, 0.0, 0.1, 0.1];
        assert!(covers_post(&[post, far_corner]));
        let other_far_corner = [0.9, 0.9, 1.0, 1.0];
        assert!(covers_post(&[far_corner, post, other_far_corner]));
    }
}
