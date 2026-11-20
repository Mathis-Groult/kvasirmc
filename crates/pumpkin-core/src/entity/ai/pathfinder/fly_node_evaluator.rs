use pumpkin_util::math::{position::BlockPos, vector3::Vector3};
use rustc_hash::FxHashMap;

use crate::entity::ai::pathfinder::{
    node::{Node, PathType, Target},
    node_evaluator::{MobData, NodeEvaluator},
    pathfinding_context::PathfindingContext,
    walk_node_evaluator::WalkNodeEvaluator,
};

/// One of the 26 steps to a neighboring cell. A diagonal step is only taken when the steps
/// it is made of (`needs`, indices into `STEPS`) lead to cells the mob may enter.
struct Step {
    dx: i32,
    dy: i32,
    dz: i32,
    needs: &'static [usize],
}

const fn step(dx: i32, dy: i32, dz: i32, needs: &'static [usize]) -> Step {
    Step { dx, dy, dz, needs }
}

/// In the order vanilla tries them: the six faces, the edges (up, down, around), then the
/// corners.
const STEPS: [Step; 26] = [
    step(0, 0, 1, &[]),                       // 0 south
    step(-1, 0, 0, &[]),                      // 1 west
    step(1, 0, 0, &[]),                       // 2 east
    step(0, 0, -1, &[]),                      // 3 north
    step(0, 1, 0, &[]),                       // 4 up
    step(0, -1, 0, &[]),                      // 5 down
    step(0, 1, 1, &[0, 4]),                   // 6 south up
    step(-1, 1, 0, &[1, 4]),                  // 7 west up
    step(1, 1, 0, &[2, 4]),                   // 8 east up
    step(0, 1, -1, &[3, 4]),                  // 9 north up
    step(0, -1, 1, &[0, 5]),                  // 10 south down
    step(-1, -1, 0, &[1, 5]),                 // 11 west down
    step(1, -1, 0, &[2, 5]),                  // 12 east down
    step(0, -1, -1, &[3, 5]),                 // 13 north down
    step(1, 0, -1, &[3, 2]),                  // 14 north east
    step(1, 0, 1, &[0, 2]),                   // 15 south east
    step(-1, 0, -1, &[3, 1]),                 // 16 north west
    step(-1, 0, 1, &[0, 1]),                  // 17 south west
    step(1, 1, -1, &[14, 3, 2, 4, 9, 8]),     // 18 north east up
    step(1, 1, 1, &[15, 0, 2, 4, 6, 8]),      // 19 south east up
    step(-1, 1, -1, &[16, 3, 1, 4, 9, 7]),    // 20 north west up
    step(-1, 1, 1, &[17, 0, 1, 4, 6, 7]),     // 21 south west up
    step(1, -1, -1, &[14, 3, 2, 5, 13, 12]),  // 22 north east down
    step(1, -1, 1, &[15, 0, 2, 5, 10, 12]),   // 23 south east down
    step(-1, -1, -1, &[16, 3, 1, 5, 13, 11]), // 24 north west down
    step(-1, -1, 1, &[17, 0, 1, 5, 10, 11]),  // 25 south west down
];

pub struct FlyNodeEvaluator {
    pub walk: WalkNodeEvaluator,
    path_types_cache: FxHashMap<Vector3<i32>, PathType>,
}

impl FlyNodeEvaluator {
    #[must_use]
    pub fn new() -> Self {
        Self {
            walk: WalkNodeEvaluator::new(),
            path_types_cache: FxHashMap::default(),
        }
    }

    pub fn get_cached_path_type(&mut self, pos: Vector3<i32>) -> PathType {
        if let Some(&cached) = self.path_types_cache.get(&pos) {
            return cached;
        }

        let path_type = if let Some(mut ctx) = self.walk.base.context.take()
            && let Some(mob_data) = self.walk.base.mob_data
        {
            let res = self.get_path_type_of_mob(&mut ctx, pos, &mob_data);
            self.walk.base.context = Some(ctx);
            res
        } else {
            PathType::Blocked
        };

        self.path_types_cache.insert(pos, path_type);
        path_type
    }

    pub fn can_start_at(&mut self, pos: Vector3<i32>) -> bool {
        let path_type = self.get_cached_path_type(pos);
        self.walk.get_mob_penalty(path_type) >= 0.0
    }

    fn find_accepted_node(&mut self, x: i32, y: i32, z: i32) -> Option<Node> {
        let path_type = self.get_cached_path_type(Vector3::new(x, y, z));
        let path_cost = self.walk.get_mob_penalty(path_type);
        (path_cost >= 0.0).then(|| {
            let mut best = self.walk.base.get_node(BlockPos::new(x, y, z));
            best.path_type = path_type;
            best.cost_malus = best.cost_malus.max(path_cost);
            if path_type == PathType::Walkable {
                best.cost_malus += 1.0;
            }
            self.walk.base.update_node(best)
        })
    }

    const fn has_malus(node: Option<&Node>) -> bool {
        if let Some(n) = node {
            n.cost_malus >= 0.0
        } else {
            false
        }
    }

    const fn is_open(node: Option<&Node>) -> bool {
        if let Some(n) = node { !n.closed } else { false }
    }
}

impl NodeEvaluator for FlyNodeEvaluator {
    fn prepare(&mut self, context: PathfindingContext, mob_data: MobData) {
        self.walk.prepare(context, mob_data);
        self.path_types_cache.clear();
    }

    fn done(&mut self) {
        self.path_types_cache.clear();
        self.walk.done();
    }

    fn get_start(&mut self) -> Option<Node> {
        let mob_data = self.walk.base.mob_data?;
        let mob_x = mob_data.position.x;
        let mob_y = mob_data.position.y;
        let mob_z = mob_data.position.z;
        let block_x = mob_x.floor() as i32;
        let block_z = mob_z.floor() as i32;

        let start_y = if self.walk.base.can_float && mob_data.is_in_water {
            let mut y = mob_y.floor() as i32;
            if let Some(ref ctx) = self.walk.base.context {
                while ctx.is_water(&BlockPos::new(block_x, y + 1, block_z)) {
                    y += 1;
                }
            }
            y
        } else {
            (mob_y + 0.5).floor() as i32
        };

        let start_pos = Vector3::new(block_x, start_y, block_z);
        if !self.can_start_at(start_pos) {
            let is_small_mob = mob_data.width < 1.0 || mob_data.height < 1.0;
            if is_small_mob {
                let x_pad = (1.1 - f64::from(mob_data.width)).max(0.0);
                let y_pad = (1.1 - f64::from(mob_data.height)).max(0.0);
                let z_pad = (1.1 - f64::from(mob_data.width)).max(0.0);
                let half_w = f64::from(mob_data.width) / 2.0;
                let min_x = (mob_x - half_w - x_pad).floor() as i32;
                let max_x = (mob_x + half_w + x_pad).floor() as i32;
                let min_y = (mob_y - y_pad).floor() as i32;
                let max_y = (mob_y + f64::from(mob_data.height) + y_pad).floor() as i32;
                let min_z = (mob_z - half_w - z_pad).floor() as i32;
                let max_z = (mob_z + half_w + z_pad).floor() as i32;

                let mut candidate_count = 0;
                for cx in min_x..=max_x {
                    for cy in min_y..=max_y {
                        for cz in min_z..=max_z {
                            let candidate = Vector3::new(cx, cy, cz);
                            if self.can_start_at(candidate) {
                                return Some(self.walk.get_start_node(candidate));
                            }
                            candidate_count += 1;
                            if candidate_count >= 10 {
                                break;
                            }
                        }
                        if candidate_count >= 10 {
                            break;
                        }
                    }
                    if candidate_count >= 10 {
                        break;
                    }
                }
            } else {
                let half_width = f64::from(mob_data.width) / 2.0;
                let min_x = (mob_x - half_width).floor() as i32;
                let max_x = (mob_x + half_width).floor() as i32;
                let min_z = (mob_z - half_width).floor() as i32;
                let max_z = (mob_z + half_width).floor() as i32;

                for candidate in [
                    Vector3::new(min_x, start_y, min_z),
                    Vector3::new(min_x, start_y, max_z),
                    Vector3::new(max_x, start_y, min_z),
                    Vector3::new(max_x, start_y, max_z),
                ] {
                    if self.can_start_at(candidate) {
                        return Some(self.walk.get_start_node(candidate));
                    }
                }
            }
        }

        Some(self.walk.get_start_node(start_pos))
    }

    fn get_target(&mut self, pos: BlockPos) -> Target {
        Target::new(self.walk.base.get_node(pos))
    }

    fn get_neighbors(&mut self, current: &Node, out: &mut Vec<Node>) {
        let pos = current.pos.0;
        let mut found: [Option<Node>; STEPS.len()] = [None; STEPS.len()];
        for (slot, step) in STEPS.iter().enumerate() {
            let node = self.find_accepted_node(pos.x + step.dx, pos.y + step.dy, pos.z + step.dz);
            let reachable = step
                .needs
                .iter()
                .all(|&need| Self::has_malus(found[need].as_ref()));
            if reachable && Self::is_open(node.as_ref()) {
                out.extend(node);
            }
            found[slot] = node;
        }
    }

    fn get_path_type_of_mob(
        &mut self,
        context: &mut PathfindingContext,
        pos: Vector3<i32>,
        mob_data: &MobData,
    ) -> PathType {
        let block_types = self
            .walk
            .get_path_type_within_mob_bb(context, pos.x, pos.y, pos.z, mob_data);
        if block_types.len() == 1 {
            return block_types[0];
        }

        if block_types.contains(&PathType::Fence) {
            return PathType::Fence;
        }

        if block_types.contains(&PathType::UnpassableRail) {
            return PathType::UnpassableRail;
        }

        let mut highest_malus_path_type_within_bb = PathType::Blocked;
        let mut highest_malus_within_bb =
            mob_data.get_pathfinding_malus(highest_malus_path_type_within_bb);

        for &path_type in &block_types {
            let malus = mob_data.get_pathfinding_malus(path_type);
            if malus < 0.0 {
                return path_type;
            }

            if malus >= highest_malus_within_bb {
                highest_malus_within_bb = malus;
                highest_malus_path_type_within_bb = path_type;
            }
        }

        let current_node_path_type = self.get_path_type(context, pos);
        let is_large_mob = self.walk.base.entity_width > 1;
        if is_large_mob {
            let is_current_node_cheaper =
                mob_data.get_pathfinding_malus(current_node_path_type) < highest_malus_within_bb;
            let cap_malus_due_to_cheap_node = is_current_node_cheaper
                && mob_data.get_pathfinding_malus(PathType::BigMobsCloseToDanger)
                    < highest_malus_within_bb;
            if cap_malus_due_to_cheap_node {
                PathType::BigMobsCloseToDanger
            } else {
                highest_malus_path_type_within_bb
            }
        } else if current_node_path_type == PathType::Open
            && highest_malus_path_type_within_bb != PathType::Open
            && highest_malus_within_bb == 0.0
        {
            PathType::Open
        } else {
            highest_malus_path_type_within_bb
        }
    }

    fn get_path_type(&mut self, context: &mut PathfindingContext, pos: Vector3<i32>) -> PathType {
        let mut block_path_type = context.get_path_type_from_state(pos);
        if block_path_type == PathType::Open && pos.y > context.min_y() {
            let below_pos = Vector3::new(pos.x, pos.y - 1, pos.z);
            let below_type = context.get_path_type_from_state(below_pos);
            if below_type == PathType::DamageFire || below_type == PathType::Lava {
                block_path_type = PathType::DamageFire;
            } else if below_type == PathType::DamageOther {
                block_path_type = PathType::DamageOther;
            } else if below_type == PathType::Cocoa {
                block_path_type = PathType::Cocoa;
            } else if below_type == PathType::Fence {
                if below_pos != context.mob_position() {
                    block_path_type = PathType::Fence;
                }
            } else {
                block_path_type = if below_type != PathType::Walkable
                    && below_type != PathType::Open
                    && below_type != PathType::Water
                {
                    PathType::Walkable
                } else {
                    PathType::Open
                };
            }
        }

        if block_path_type == PathType::Walkable || block_path_type == PathType::Open {
            block_path_type = context.get_node_type_from_neighbors(pos, block_path_type);
        }

        block_path_type
    }

    fn set_can_pass_doors(&mut self, can_pass: bool) {
        self.walk.set_can_pass_doors(can_pass);
    }

    fn set_can_open_doors(&mut self, can_open: bool) {
        self.walk.set_can_open_doors(can_open);
    }

    fn set_can_float(&mut self, can_float: bool) {
        self.walk.set_can_float(can_float);
    }

    fn set_can_walk_over_fences(&mut self, can_walk: bool) {
        self.walk.set_can_walk_over_fences(can_walk);
    }

    fn can_pass_doors(&self) -> bool {
        self.walk.can_pass_doors()
    }

    fn can_open_doors(&self) -> bool {
        self.walk.can_open_doors()
    }

    fn can_float(&self) -> bool {
        self.walk.can_float()
    }

    fn can_walk_over_fences(&self) -> bool {
        self.walk.can_walk_over_fences()
    }
}

impl Default for FlyNodeEvaluator {
    fn default() -> Self {
        Self::new()
    }
}
