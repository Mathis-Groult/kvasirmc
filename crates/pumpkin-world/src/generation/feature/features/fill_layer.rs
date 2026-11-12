use crate::generation::feature::configured_features::FeatureArgs;
use pumpkin_data::BlockState;
use pumpkin_util::{math::position::BlockPos, random::RandomGenerator};

use crate::generation::proto_chunk::GenerationCache;

pub struct FillLayerFeature {
    pub height: i32,
    pub state: &'static BlockState,
}

impl FillLayerFeature {
    pub fn generate<T: GenerationCache>(
        &self,
        chunk: &mut T,
        args: FeatureArgs<'_>,
        _random: &mut RandomGenerator,
    ) -> bool {
        let FeatureArgs { min_y, pos, .. } = args;
        let y = min_y as i32 + self.height;
        for dx in 0..16 {
            for dz in 0..16 {
                let target = BlockPos::new(pos.0.x + dx, y, pos.0.z + dz);
                if chunk.is_air(&target.0) {
                    chunk.set_block_state(&target.0, self.state);
                }
            }
        }
        true
    }
}
