use crate::generation::feature::configured_features::FeatureArgs;
use pumpkin_util::random::RandomGenerator;

use crate::generation::feature::features::ore::OreTarget;
use crate::generation::proto_chunk::GenerationCache;

pub struct ReplaceSingleBlockFeature {
    pub targets: Vec<OreTarget>,
}

impl ReplaceSingleBlockFeature {
    pub fn generate<T: GenerationCache>(
        &self,
        chunk: &mut T,
        args: FeatureArgs<'_>,
        random: &mut RandomGenerator,
    ) -> bool {
        let FeatureArgs { pos, .. } = args;
        let block_state = GenerationCache::get_block_state(chunk, &pos.0);
        for target in &self.targets {
            if target.target.test(block_state, pos.0.y, random) {
                chunk.set_block_state(&pos.0, target.state);
                break;
            }
        }
        true
    }
}
