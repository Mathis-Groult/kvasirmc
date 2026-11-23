//! The pieces of `PathFinder::find_path`, following vanilla `PathFinder.findPath`: seed the open
//! set with the start node, run the A* search loop, then pick the best path to a target.

use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector3::Vector3;
use rustc_hash::FxHashMap;

use crate::entity::ai::pathfinder::node::{Node, Target};
use crate::entity::ai::pathfinder::path::Path;
use crate::entity::ai::pathfinder::{EvaluatorKind, PathFinder, TARGET_DISTANCE_MULTIPLIER};

/// A target with the position it was asked for.
pub(super) type TargetEntry = (Target, BlockPos);

/// What stops a search: how far a path may run, how close counts as reached, how many nodes may
/// be visited.
#[derive(Clone, Copy)]
pub(super) struct SearchLimits {
    pub max_path_length: f32,
    pub reach_range: i32,
    pub max_visited_nodes: usize,
}

impl PathFinder {
    /// Resets the start node and puts it in the open set.
    pub(super) fn seed_open_set(
        &mut self,
        evaluator: &mut EvaluatorKind,
        from: &mut Node,
        targets: &mut [TargetEntry],
    ) {
        from.g = 0.0;
        from.h = Self::get_best_h(from, targets);
        from.f = from.h;
        from.walked_dist = 0.0;
        from.came_from = None;
        from.closed = false;

        evaluator.base_mut().update_node(*from);
        self.open_set.insert(*from);
    }

    /// The search loop; returns the indices of the targets within reach of the node that ended it.
    pub(super) fn search(
        &mut self,
        evaluator: &mut EvaluatorKind,
        from: &Node,
        targets: &mut [TargetEntry],
        limits: SearchLimits,
    ) -> Vec<usize> {
        let mut count = 0usize;

        while !self.open_set.is_empty() {
            count += 1;
            if count >= limits.max_visited_nodes {
                break;
            }

            let Some(mut current) = self.open_set.pop() else {
                break;
            };

            current = evaluator.base_mut().get_node(current.pos);
            current.closed = true;
            evaluator.base_mut().update_node(current);

            let reached_targets = Self::reached_targets(&current, targets, limits.reach_range);
            if !reached_targets.is_empty() {
                return reached_targets;
            }

            if current.distance_to_node(from) < limits.max_path_length {
                self.expand(evaluator, &current, targets, limits.max_path_length);
            }
        }
        Vec::new()
    }

    /// Marks and returns the targets within `reach_range` (Manhattan) of `current`.
    fn reached_targets(
        current: &Node,
        targets: &mut [TargetEntry],
        reach_range: i32,
    ) -> Vec<usize> {
        let mut reached = Vec::new();
        for (idx, (target, _)) in targets.iter_mut().enumerate() {
            if current.distance_manhattan_node(&target.node) <= reach_range as f32 {
                target.set_reached();
                reached.push(idx);
            }
        }
        reached
    }

    /// Evaluates the neighbours of `current` and queues the ones that improve.
    fn expand(
        &mut self,
        evaluator: &mut EvaluatorKind,
        current: &Node,
        targets: &mut [TargetEntry],
        max_path_length: f32,
    ) {
        evaluator.get_neighbors(current, &mut self.neighbors_buf);

        for neighbor in self.neighbors_buf.drain(..) {
            let mut neighbor = evaluator.base_mut().get_node(neighbor.pos);
            if neighbor.closed {
                continue;
            }
            let distance = current.distance_to_node(&neighbor);
            neighbor.walked_dist = current.walked_dist + distance;
            let tentative_g = current.g + distance + neighbor.cost_malus;

            let in_open = self.open_set.contains(&neighbor);
            let is_better = !in_open || tentative_g < neighbor.g;

            if neighbor.walked_dist < max_path_length && is_better {
                neighbor.came_from = Some(current.pos.0);
                neighbor.g = tentative_g;
                neighbor.h = Self::get_best_h(&neighbor, targets) * TARGET_DISTANCE_MULTIPLIER;
                neighbor.f = neighbor.g + neighbor.h;

                if in_open {
                    self.open_set.update_node(&neighbor, neighbor);
                } else {
                    self.open_set.insert(neighbor);
                }
            }
            evaluator.base_mut().update_node(neighbor);
        }
    }

    /// The shortest path to a reached target, or else the path ending closest to a target.
    pub(super) fn choose_path(
        all_nodes: &FxHashMap<Vector3<i32>, Node>,
        targets: Vec<TargetEntry>,
        reached_targets: &[usize],
    ) -> Option<Path> {
        if reached_targets.is_empty() {
            targets
                .into_iter()
                .filter_map(|(target, target_pos)| {
                    Self::path_to_best_node(&target, target_pos, false, all_nodes)
                })
                .min_by(|a, b| {
                    a.get_dist_to_target()
                        .total_cmp(&b.get_dist_to_target())
                        .then_with(|| a.get_node_count().cmp(&b.get_node_count()))
                })
        } else {
            reached_targets
                .iter()
                .filter_map(|&idx| {
                    let (target, target_pos) = &targets[idx];
                    Self::path_to_best_node(target, *target_pos, true, all_nodes)
                })
                .min_by_key(Path::get_node_count)
        }
    }

    fn path_to_best_node(
        target: &Target,
        target_pos: BlockPos,
        reached: bool,
        all_nodes: &FxHashMap<Vector3<i32>, Node>,
    ) -> Option<Path> {
        target
            .get_best_node()
            .and_then(|pos| all_nodes.get(&pos.0))
            .map(|best| Self::reconstruct_path(best, target_pos, reached, all_nodes))
    }
}
