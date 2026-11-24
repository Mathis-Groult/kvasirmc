//! The search half of `NetherPortal::find_safe_location`: which spots a column offers, and the
//! fallback when none does.

use std::sync::Arc;

use pumpkin_data::block_properties::HorizontalAxis;
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};
use pumpkin_world::chunk::ChunkHeightmapType;

use super::NetherPortal;
use crate::world::World;
use crate::world::border::Worldborder;

/// A portal floor, the axis it fits and its squared distance to the target.
type Spot = (BlockPos, HorizontalAxis, f64);

/// The nearest ideal and the nearest acceptable spot seen so far; the first of equal distance
/// wins.
#[derive(Default)]
pub(super) struct SafeSpots {
    ideal: Option<Spot>,
    acceptable: Option<Spot>,
}

impl SafeSpots {
    fn offer(&mut self, spot: Spot, ideal: bool) {
        let slot = if ideal {
            &mut self.ideal
        } else {
            &mut self.acceptable
        };
        if slot.as_ref().is_none_or(|nearest| spot.2 < nearest.2) {
            *slot = Some(spot);
        }
    }

    /// An ideal spot beats any acceptable one, however far.
    pub(super) fn best(self) -> Option<(BlockPos, HorizontalAxis)> {
        self.ideal
            .or(self.acceptable)
            .map(|(pos, axis, _)| (pos, axis))
    }
}

impl NetherPortal {
    /// Walks one column down from its height-map top and offers the floor of every air run tall
    /// enough to hold a portal.
    pub(super) fn scan_column(
        world: &Arc<World>,
        target_pos: BlockPos,
        (x, z): (i32, i32),
        (min_y, top_y_limit): (i32, i32),
        spots: &mut SafeSpots,
    ) {
        let heightmap_y = world.get_heightmap_height(ChunkHeightmapType::MotionBlocking, x, z);
        let mut y = heightmap_y.min(top_y_limit);
        while y >= min_y {
            let state = world.get_block_state(&BlockPos(Vector3::new(x, y, z)));
            if !Self::is_valid_portal_air(state) {
                y -= 1;
                continue;
            }
            let bottom_y = Self::air_run_bottom(world, (x, z), y, min_y);
            let air_height = y - bottom_y;
            if air_height >= 3 && bottom_y + 4 <= top_y_limit {
                let floor_pos = BlockPos(Vector3::new(x, bottom_y, z));
                Self::offer_floor(world, target_pos, floor_pos, spots);
            }
            y = bottom_y - 1;
        }
    }

    /// The lowest `y` of the run of portal air that contains `(x, y, z)`.
    fn air_run_bottom(world: &Arc<World>, (x, z): (i32, i32), y: i32, min_y: i32) -> i32 {
        let mut bottom_y = y;
        while bottom_y > min_y {
            let below = BlockPos(Vector3::new(x, bottom_y - 1, z));
            if !Self::is_valid_portal_air(world.get_block_state(&below)) {
                break;
            }
            bottom_y -= 1;
        }
        bottom_y
    }

    /// Offers `floor_pos` for each axis a portal fits on; ideal when it also fits one block to
    /// either side.
    fn offer_floor(
        world: &Arc<World>,
        target_pos: BlockPos,
        floor_pos: BlockPos,
        spots: &mut SafeSpots,
    ) {
        for check_axis in [HorizontalAxis::X, HorizontalAxis::Z] {
            if !Self::is_valid_portal_pos(world, floor_pos, check_axis, 0) {
                continue;
            }
            let dist = f64::from(target_pos.0.squared_distance_to(
                floor_pos.0.x,
                floor_pos.0.y,
                floor_pos.0.z,
            ));
            let is_ideal = Self::is_valid_portal_pos(world, floor_pos, check_axis, -1)
                && Self::is_valid_portal_pos(world, floor_pos, check_axis, 1);
            spots.offer((floor_pos, check_axis, dist), is_ideal);
        }
    }

    /// No spot found: a fresh portal next to the target, shifted one block against the axis
    /// direction, with the height clamped between `max(bottom, 70)` and the top limit - 9 and the
    /// column clamped into the world border (vanilla `PortalForcer`).
    pub(super) fn fallback_location(
        worldborder: &Worldborder,
        target_pos: BlockPos,
        axis: HorizontalAxis,
        direction: Vector3<i32>,
        (min_y, top_y_limit): (i32, i32),
    ) -> (BlockPos, HorizontalAxis, bool) {
        let fallback_y = target_pos.0.y.clamp(min_y.max(70), top_y_limit - 9);
        let (x, z) =
            worldborder.clamp_block(target_pos.0.x - direction.x, target_pos.0.z - direction.z);
        (BlockPos(Vector3::new(x, fallback_y, z)), axis, true)
    }
}
