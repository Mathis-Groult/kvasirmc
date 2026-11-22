use pumpkin_data::{BlockDirection, BlockState};
use pumpkin_util::{
    math::position::BlockPos,
    random::{RandomGenerator, RandomImpl},
};

use crate::generation::proto_chunk::GenerationCache;
use crate::{
    generation::{
        block_state_provider::BlockStateProvider,
        feature::features::tree::{TreeFeature, TreeNode, trunk::TrunkPlacer},
    },
    world::WorldPortalExt,
};

pub struct DarkOakTrunkPlacer;

impl DarkOakTrunkPlacer {
    #[expect(clippy::too_many_arguments)]
    pub fn generate<T: GenerationCache>(
        block_registry: &dyn WorldPortalExt,
        _placer: &TrunkPlacer,
        height: u32,
        start_pos: BlockPos,
        chunk: &mut T,
        random: &mut RandomGenerator,
        below_trunk_provider: &BlockStateProvider,
        trunk_block: &BlockState,
    ) -> (Vec<TreeNode>, Vec<BlockPos>) {
        let mut trunk_poses = Self::place_ground(
            block_registry,
            chunk,
            random,
            start_pos.down(),
            below_trunk_provider,
        );
        let (tx, tz) = Self::place_leaning_trunk(
            chunk,
            random,
            height,
            start_pos,
            trunk_block,
            &mut trunk_poses,
        );

        let ey = start_pos.0.y + height as i32 - 1;
        let mut nodes = vec![TreeNode {
            center: BlockPos::new(tx, ey, tz),
            foliage_radius: 0,
            giant_trunk: true,
        }];
        Self::place_branches(
            chunk,
            random,
            start_pos,
            ey,
            trunk_block,
            &mut trunk_poses,
            &mut nodes,
        );

        (nodes, trunk_poses)
    }

    /// Sets the ground below the four trunk columns; the columns it accepted.
    fn place_ground<T: GenerationCache>(
        block_registry: &dyn WorldPortalExt,
        chunk: &mut T,
        random: &mut RandomGenerator,
        below: BlockPos,
        below_trunk_provider: &BlockStateProvider,
    ) -> Vec<BlockPos> {
        let mut placed = Vec::new();
        for pos in [below, below.east(), below.south(), below.south().east()] {
            if TrunkPlacer::set_dirt(block_registry, chunk, random, &pos, below_trunk_provider) {
                placed.push(pos);
            }
        }
        placed
    }

    /// Places the 2x2 trunk layer by layer, leaning one step per layer from the lean height up;
    /// the x and z of the last layer.
    fn place_leaning_trunk<T: GenerationCache>(
        chunk: &mut T,
        random: &mut RandomGenerator,
        height: u32,
        start_pos: BlockPos,
        trunk_block: &BlockState,
        trunk_poses: &mut Vec<BlockPos>,
    ) -> (i32, i32) {
        let lean_direction = BlockDirection::random_horizontal(random);
        let lean_height = height as i32 - random.next_bounded_i32(4);
        let mut lean_steps = 2 - random.next_bounded_i32(3);

        let mut tx = start_pos.0.x;
        let mut tz = start_pos.0.z;
        for dy in 0..height as i32 {
            if dy >= lean_height && lean_steps > 0 {
                let offset = lean_direction.to_offset();
                tx += offset.x;
                tz += offset.z;
                lean_steps -= 1;
            }

            let pos = BlockPos::new(tx, start_pos.0.y + dy, tz);
            // The layer is only placed when its main position is air or leaves.
            let state = GenerationCache::get_block_state(chunk, &pos.0);
            if TreeFeature::is_air_or_leaves(state.to_state(), state.to_block_id()) {
                for column in [pos, pos.east(), pos.south(), pos.east().south()] {
                    if TrunkPlacer::try_place(chunk, &column, trunk_block) {
                        trunk_poses.push(column);
                    }
                }
            }
        }
        (tx, tz)
    }

    /// The branches around the trunk: each of the twelve outer cells draws one, a column hanging
    /// down from the top of the trunk with a foliage node at its top.
    fn place_branches<T: GenerationCache>(
        chunk: &mut T,
        random: &mut RandomGenerator,
        start_pos: BlockPos,
        ey: i32,
        trunk_block: &BlockState,
        trunk_poses: &mut Vec<BlockPos>,
        nodes: &mut Vec<TreeNode>,
    ) {
        for ox in -1..=2 {
            for oz in -1..=2 {
                if (!(0..=1).contains(&ox) || !(0..=1).contains(&oz))
                    && random.next_bounded_i32(3) <= 0
                {
                    let length = random.next_bounded_i32(3) + 2;

                    for branch_y in 0..length {
                        let pos = BlockPos::new(
                            start_pos.0.x + ox,
                            ey - branch_y - 1,
                            start_pos.0.z + oz,
                        );
                        if TrunkPlacer::try_place(chunk, &pos, trunk_block) {
                            trunk_poses.push(pos);
                        }
                    }

                    nodes.push(TreeNode {
                        center: BlockPos::new(start_pos.0.x + ox, ey, start_pos.0.z + oz),
                        foliage_radius: 0,
                        giant_trunk: false,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "dark_oak_golden.rs"]
mod golden;
