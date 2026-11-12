use crate::generation::feature::configured_features::FeatureArgs;
use pumpkin_data::{Block, BlockState};
use pumpkin_util::{
    math::{int_provider::IntProvider, position::BlockPos},
    random::RandomGenerator,
};

use crate::generation::proto_chunk::GenerationCache;

pub struct ReplaceBlobsFeature {
    pub target: &'static BlockState,
    pub state: &'static BlockState,
    pub radius: IntProvider,
}

impl ReplaceBlobsFeature {
    pub fn generate<T: GenerationCache>(
        &self,
        chunk: &mut T,
        args: FeatureArgs<'_>,
        random: &mut RandomGenerator,
    ) -> bool {
        let FeatureArgs { pos, .. } = args;
        let target = Block::from_state_id(self.target.id);
        let state = &self.state;
        let Some(pos) = Self::move_down_to_target(pos, chunk, target) else {
            return false;
        };
        let x = self.radius.get(random);
        let y = self.radius.get(random);
        let z = self.radius.get(random);
        let distance = x.max(y.max(z));

        let mut result = false;

        for iter_pos in BlockPos::iterate_outwards(pos, x, y, z) {
            if iter_pos.manhattan_distance(pos) > distance {
                break;
            }
            let current_state = GenerationCache::get_block_state(chunk, &iter_pos.0);
            if current_state.to_block_id() != target.id {
                continue;
            }
            chunk.set_block_state(&iter_pos.0, state);
            result = true;
        }

        result
    }

    fn move_down_to_target<T: GenerationCache>(
        mut pos: BlockPos,
        chunk: &T,
        target: &'static Block,
    ) -> Option<BlockPos> {
        while pos.0.y > chunk.bottom_y() as i32 + 1 {
            let state = GenerationCache::get_block_state(chunk, &pos.0);
            if state.to_block_id() == target.id {
                return Some(pos);
            }

            pos = pos.down();
        }
        None
    }
}
