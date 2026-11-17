//! Collision boxes, free space and ray tracing.

use super::World;
use crate::entity::EntityBase;
use pumpkin_data::{Block, fluid::Fluid};
use pumpkin_data::{BlockDirection, BlockState};
use pumpkin_util::math::{boundingbox::BoundingBox, position::BlockPos, vector3::Vector3};
use std::sync::Arc;

#[cfg(test)]
mod golden;

impl World {
    // FlowingFluid.isSolidFace()
    pub(super) fn is_solid_face(
        &self,
        fluid0_id: u16,
        pos: BlockPos,
        direction: BlockDirection,
    ) -> bool {
        let id = self.get_block_state_id(&pos);

        let fluid = Fluid::from_state_id(id).unwrap_or(&Fluid::EMPTY);

        if Fluid::same_fluid_type(fluid.id, fluid0_id) {
            return false;
        }

        if direction == BlockDirection::Up {
            return true;
        }

        let block = Block::from_state_id(id);
        let state = BlockState::from_id(id);

        // Doesn't count blue ice or packed ice

        if block == &Block::ICE || block == &Block::FROSTED_ICE {
            return false;
        }

        state.is_side_solid(direction)
    }

    pub fn check_outline<F>(
        bounding_box: &BoundingBox,
        pos: BlockPos,
        state: &BlockState,
        use_outline_shape: bool,
        mut using_outline_shape: F,
    ) -> bool
    where
        F: FnMut(&BoundingBox),
    {
        if state.outline_shapes.is_empty() {
            // Apparently we need this for air and moving pistons

            return true;
        }

        let mut inside = false;
        'shapes: for shape in state.get_block_outline_shapes_at(&pos) {
            let outline_shape = shape.at_pos(pos);

            if outline_shape.intersects(bounding_box) {
                inside = true;

                if !use_outline_shape {
                    break 'shapes;
                }

                using_outline_shape(&outline_shape);
            }
        }

        inside
    }

    pub fn check_collision<F>(
        bounding_box: &BoundingBox,
        pos: BlockPos,
        state: &BlockState,
        use_collision_shape: bool,
        mut on_collision: F,
    ) -> bool
    where
        F: FnMut(&BoundingBox),
    {
        if state.is_air() || !state.is_solid() {
            return false;
        }

        let mut shapes = state
            .get_block_collision_shapes_at(&pos)
            .map(|shape| shape.at_pos(pos));

        if use_collision_shape {
            let mut collided = false;
            for collision_shape in shapes {
                if collision_shape.intersects(bounding_box) {
                    collided = true;
                    // Convert to BB and trigger the callback
                    on_collision(&collision_shape);
                }
            }
            collided
        } else {
            shapes.any(|s| s.intersects(bounding_box))
        }
    }

    // For adjusting movement
    pub fn get_block_collisions(
        &self,
        bounding_box: BoundingBox,
        entity: &dyn EntityBase,
    ) -> (Vec<BoundingBox>, Vec<(usize, BlockPos)>) {
        let mut collisions = Vec::new();

        let mut positions = Vec::new();

        let min = BlockPos::floored_v(bounding_box.min.add_raw(0.0, -0.50001, 0.0));
        let max = bounding_box.max_block_pos();
        let pos_iter = BlockPos::iterate(min, max);

        for pos in pos_iter {
            let state = self.get_block_state(&pos);

            if state.is_air() {
                continue;
            }

            let block = Block::from_state_id(state.id);
            let mut collided = false;

            if block == &Block::POWDER_SNOW {
                if let Some(shape) =
                    crate::block::blocks::powder_snow::collision_shape_for_entity(entity, &pos)
                {
                    let shape = shape.at_pos(pos);
                    if shape.intersects(&bounding_box) {
                        collided = true;
                        collisions.push(shape);
                    }
                }
            } else {
                for shape in state.get_block_collision_shapes_at(&pos) {
                    let shape = shape.at_pos(pos);
                    if shape.intersects(&bounding_box) {
                        collided = true;
                        collisions.push(shape);
                    }
                }
            }

            if collided {
                positions.push((collisions.len(), pos));
            }
        }

        (collisions, positions)
    }

    pub fn find_free_position(
        &self,
        source: &dyn EntityBase,
        allowed_centers: &BoundingBox,
        preferred_center: Vector3<f64>,
        size: Vector3<f64>,
    ) -> Option<Vector3<f64>> {
        let (collisions, _) =
            self.get_block_collisions(allowed_centers.expand(size.x, size.y, size.z), source);
        let mut free_spots = vec![*allowed_centers];
        {
            let border = self
                .worldborder
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for shape in collisions {
                if !border.contains(shape.min.x, shape.min.z)
                    || !border.contains(shape.max.x - 1.0E-5, shape.max.z - 1.0E-5)
                {
                    continue;
                }
                let blocked = shape.expand(size.x / 2.0, size.y / 2.0, size.z / 2.0);
                free_spots = free_spots
                    .iter()
                    .flat_map(|spot| spot.subtract(&blocked))
                    .collect();
            }
        }
        free_spots
            .iter()
            .map(|spot| spot.closest_point_to(preferred_center))
            .min_by(|a, b| {
                (*a - preferred_center)
                    .length_squared()
                    .total_cmp(&(*b - preferred_center).length_squared())
            })
    }

    pub fn is_space_empty(&self, bounding_box: BoundingBox) -> bool {
        let min = bounding_box.min_block_pos();
        let max = bounding_box.max_block_pos();

        for pos in BlockPos::iterate(min, max) {
            let state = self.get_block_state(&pos);
            let collided = Self::check_collision(&bounding_box, pos, state, false, |_| ());

            if collided {
                return false;
            }
        }
        true
    }

    /// Vanilla's `BlockView.getDismountHeight()`.
    /// Returns the Y surface height for dismounting at the given block position,
    /// or `f64::NEG_INFINITY` if no valid surface exists.
    pub fn get_dismount_height(&self, pos: &BlockPos) -> f64 {
        let state = self.get_block_state(pos);
        let max_y = state
            .get_block_collision_shapes_at(pos)
            .map(|s| s.max.y)
            .fold(f64::NEG_INFINITY, f64::max);
        if max_y != f64::NEG_INFINITY {
            return max_y;
        }
        // No collision at pos — check block below
        let below = BlockPos(Vector3::new(pos.0.x, pos.0.y - 1, pos.0.z));
        let below_state = self.get_block_state(&below);
        let below_max_y = below_state
            .get_block_collision_shapes_at(&below)
            .map(|s| s.max.y)
            .fold(f64::NEG_INFINITY, f64::max);
        if below_max_y >= 1.0 {
            below_max_y - 1.0
        } else {
            f64::NEG_INFINITY
        }
    }

    #[must_use]
    pub fn intersects_aabb_with_hit(
        from: Vector3<f64>,
        to: Vector3<f64>,
        min: Vector3<f64>,
        max: Vector3<f64>,
    ) -> Option<(f64, BlockDirection, Vector3<f64>)> {
        let dir = to.sub(&from);
        let mut tmin: f64 = 0.0;
        let mut tmax: f64 = 1.0;

        let mut hit_axis = None;
        let mut hit_is_min = false;

        macro_rules! check_axis {
            ($axis:ident, $dir_axis:ident, $min_axis:ident, $max_axis:ident) => {{
                if dir.$dir_axis.abs() < 1e-8 {
                    if from.$dir_axis < min.$min_axis || from.$dir_axis > max.$max_axis {
                        return None;
                    }
                } else {
                    let inv_d = 1.0 / dir.$dir_axis;
                    let t_near = (min.$min_axis - from.$dir_axis) * inv_d;
                    let t_far = (max.$max_axis - from.$dir_axis) * inv_d;

                    let (t_entry, t_exit, is_min_face) = if inv_d >= 0.0 {
                        (t_near, t_far, true)
                    } else {
                        (t_far, t_near, false)
                    };

                    if t_entry > tmin {
                        tmin = t_entry;
                        hit_axis = Some(stringify!($axis));
                        hit_is_min = is_min_face;
                    }
                    tmax = tmax.min(t_exit);
                    if tmax < tmin {
                        return None;
                    }
                }
            }};
        }

        check_axis!(x, x, x, x);
        check_axis!(y, y, y, y);
        check_axis!(z, z, z, z);

        if tmax < 0.0 || tmin > 1.0 {
            return None;
        }

        let direction = match (hit_axis, hit_is_min) {
            (Some("x"), true) => BlockDirection::West,
            (Some("x"), false) => BlockDirection::East,
            (Some("y"), true) => BlockDirection::Down,
            (Some("y"), false) => BlockDirection::Up,
            (Some("z"), true) => BlockDirection::North,
            (Some("z"), false) => BlockDirection::South,
            _ => {
                if dir.y < 0.0 {
                    BlockDirection::Up
                } else if dir.y > 0.0 {
                    BlockDirection::Down
                } else {
                    BlockDirection::North
                }
            }
        };

        let t_hit = tmin.max(0.0);
        let hit_pos = from + dir * t_hit;
        Some((t_hit, direction, hit_pos))
    }

    /// Clips the segment against the outline shapes of `state`. A shapeless block,
    /// air above all, cannot be hit.
    pub(super) fn clip_outline_shapes(
        state: &BlockState,
        block_pos: &BlockPos,
        from: Vector3<f64>,
        to: Vector3<f64>,
    ) -> Option<(BlockDirection, Vector3<f64>)> {
        let mut closest_hit: Option<(f64, BlockDirection, Vector3<f64>)> = None;

        for shape in state.get_block_outline_shapes_at(block_pos) {
            let world_min = shape.min.add(&block_pos.0.to_f64());
            let world_max = shape.max.add(&block_pos.0.to_f64());

            if let Some((t, dir, hit_pos)) =
                Self::intersects_aabb_with_hit(from, to, world_min, world_max)
                && closest_hit
                    .as_ref()
                    .is_none_or(|(closest_t, _, _)| t < *closest_t)
            {
                closest_hit = Some((t, dir, hit_pos));
            }
        }

        closest_hit.map(|(_, dir, hit_pos)| (dir, hit_pos))
    }

    pub fn ray_outline_check_detailed(
        &self,
        block_pos: &BlockPos,
        from: Vector3<f64>,
        to: Vector3<f64>,
    ) -> Option<(BlockDirection, Vector3<f64>)> {
        Self::clip_outline_shapes(self.get_block_state(block_pos), block_pos, from, to)
    }

    pub(super) fn ray_outline_check(
        &self,
        block_pos: &BlockPos,
        from: Vector3<f64>,
        to: Vector3<f64>,
    ) -> (bool, Option<BlockDirection>) {
        if let Some((dir, _)) = self.ray_outline_check_detailed(block_pos, from, to) {
            (true, Some(dir))
        } else {
            let state = self.get_block_state(block_pos);
            if state.outline_shapes.is_empty() {
                (true, None)
            } else {
                (false, None)
            }
        }
    }

    /// Traces a ray through the world, testing blocks and fluids according to `context`.
    /// Vanilla: `BlockGetter.clip(ClipContext)`.
    pub fn clip(
        &self,
        context: &pumpkin_world::clip::ClipContext,
    ) -> pumpkin_world::clip::BlockHitResult {
        pumpkin_world::clip::clip(self, context)
    }

    pub fn ray_trace_block(
        &self,
        start_pos: Vector3<f64>,
        end_pos: Vector3<f64>,
        include_fluids: bool,
    ) -> Option<(BlockPos, BlockDirection, Vector3<f64>)> {
        let fluid = if include_fluids {
            pumpkin_world::clip::ClipFluid::Any
        } else {
            pumpkin_world::clip::ClipFluid::None
        };
        let hit = self.clip(&pumpkin_world::clip::ClipContext::new(
            start_pos,
            end_pos,
            pumpkin_world::clip::ClipBlock::Outline,
            fluid,
        ));
        if hit.is_miss {
            None
        } else {
            Some((hit.block_pos, hit.direction, hit.location))
        }
    }

    pub fn ray_trace_entities(
        &self,
        start: Vector3<f64>,
        end: Vector3<f64>,
    ) -> Vec<(Arc<dyn EntityBase>, Vector3<f64>, f64)> {
        if start == end {
            return Vec::new();
        }

        let min_x = start.x.min(end.x) - 1.0;
        let max_x = start.x.max(end.x) + 1.0;
        let min_y = start.y.min(end.y) - 1.0;
        let max_y = start.y.max(end.y) + 1.0;
        let min_z = start.z.min(end.z) - 1.0;
        let max_z = start.z.max(end.z) + 1.0;
        let ray_box = BoundingBox::new(
            Vector3::new(min_x, min_y, min_z),
            Vector3::new(max_x, max_y, max_z),
        );

        let mut hits = Vec::new();

        for entity in self.entities.load().iter() {
            let bb = entity.get_entity().bounding_box.load();
            if bb.intersects(&ray_box)
                && let Some((t, _, hit_pos)) =
                    Self::intersects_aabb_with_hit(start, end, bb.min, bb.max)
            {
                let distance = (hit_pos - start).length();
                hits.push((entity.clone(), hit_pos, distance, t));
            }
        }

        for player in self.players.load().iter() {
            let bb = player.get_entity().bounding_box.load();
            if bb.intersects(&ray_box)
                && let Some((t, _, hit_pos)) =
                    Self::intersects_aabb_with_hit(start, end, bb.min, bb.max)
            {
                let distance = (hit_pos - start).length();
                hits.push((player.clone() as Arc<dyn EntityBase>, hit_pos, distance, t));
            }
        }

        hits.sort_by(|a, b| a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal));
        hits.into_iter()
            .map(|(ent, hit_pos, dist, _)| (ent, hit_pos, dist))
            .collect()
    }

    /// Returns the closest entity the segment from `start` to `end` hits, or
    /// `None`. Convenience wrapper over [`Self::ray_trace_entities`].
    pub fn ray_trace_entity(
        &self,
        start: Vector3<f64>,
        end: Vector3<f64>,
    ) -> Option<(Arc<dyn EntityBase>, Vector3<f64>, f64)> {
        self.ray_trace_entities(start, end).into_iter().next()
    }

    /// Traces the block grid from `start_pos` to `end_pos` (vanilla
    /// `Block.clip` semantics) and returns the first block the ray actually
    /// passes through whose outline collides and whose `hit_check` returns
    /// true, together with the direction reported for that hit. The start
    /// block is tested like any other; since the ray begins inside it, the
    /// reported direction there is a fallback rather than a true entry face.
    /// Returns `None` when nothing is hit or the ray starts and ends in the
    /// same block.
    pub fn raycast(
        self: &Arc<Self>,
        start_pos: Vector3<f64>,
        end_pos: Vector3<f64>,
        hit_check: impl Fn(&BlockPos, &Arc<Self>) -> bool,
    ) -> Option<(BlockPos, BlockDirection)> {
        if start_pos == end_pos {
            return None;
        }

        let adjust = -1.0e-7f64;
        let to = end_pos.lerp(&start_pos, adjust);
        let from = start_pos.lerp(&end_pos, adjust);

        let mut block = BlockPos::floored(from.x, from.y, from.z);

        if hit_check(&block, self) {
            let (collision, direction) = self.ray_outline_check(&block, from, to);
            if let Some(dir) = direction
                && collision
            {
                return Some((block, dir));
            }
        }

        let (step, delta, mut next) = ray_steps(from, to);

        while next.x <= 1.0 || next.y <= 1.0 || next.z <= 1.0 {
            let block_direction = step_to_next_block(&mut block, &mut next, step, delta);

            if hit_check(&block, self) {
                let (collision, direction) = self.ray_outline_check(&block, from, to);
                if collision {
                    if let Some(dir) = direction {
                        return Some((block, dir));
                    }
                    return Some((block, block_direction));
                }
            }
        }

        None
    }
}

/// The voxel walk of a ray from `from` to `to`: the step sign per axis, the fraction of the ray
/// one block takes per axis (`f64::MAX` on an axis it does not move along), and the fraction at
/// which it first crosses a block border per axis.
fn ray_steps(from: Vector3<f64>, to: Vector3<f64>) -> (Vector3<i32>, Vector3<f64>, Vector3<f64>) {
    let difference = to.sub(&from);

    let step = difference.sign();

    let delta = Vector3::new(
        if step.x == 0 {
            f64::MAX
        } else {
            (f64::from(step.x)) / difference.x
        },
        if step.y == 0 {
            f64::MAX
        } else {
            (f64::from(step.y)) / difference.y
        },
        if step.z == 0 {
            f64::MAX
        } else {
            (f64::from(step.z)) / difference.z
        },
    );

    let next = Vector3::new(
        delta.x
            * (if step.x > 0 {
                1.0 - (from.x - from.x.floor())
            } else {
                from.x - from.x.floor()
            }),
        delta.y
            * (if step.y > 0 {
                1.0 - (from.y - from.y.floor())
            } else {
                from.y - from.y.floor()
            }),
        delta.z
            * (if step.z > 0 {
                1.0 - (from.z - from.z.floor())
            } else {
                from.z - from.z.floor()
            }),
    );
    (step, delta, next)
}

/// Moves `block` one block along the axis whose border the ray crosses first, and returns the
/// face it enters through.
fn step_to_next_block(
    block: &mut BlockPos,
    next: &mut Vector3<f64>,
    step: Vector3<i32>,
    delta: Vector3<f64>,
) -> BlockDirection {
    match (next.x, next.y, next.z) {
        (x, y, z) if x < y && x < z => {
            block.0.x += step.x;
            next.x += delta.x;
            if step.x > 0 {
                BlockDirection::West
            } else {
                BlockDirection::East
            }
        }
        (_, y, z) if y < z => {
            block.0.y += step.y;
            next.y += delta.y;
            if step.y > 0 {
                BlockDirection::Down
            } else {
                BlockDirection::Up
            }
        }
        _ => {
            block.0.z += step.z;
            next.z += delta.z;
            if step.z > 0 {
                BlockDirection::North
            } else {
                BlockDirection::South
            }
        }
    }
}
