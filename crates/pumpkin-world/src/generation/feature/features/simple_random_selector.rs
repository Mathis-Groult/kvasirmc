use pumpkin_util::random::{RandomGenerator, RandomImpl};

use crate::generation::feature::configured_features::FeatureArgs;
use crate::generation::proto_chunk::GenerationCache;
use crate::generation::feature::placed_features::PlacedFeature;

pub struct SimpleRandomFeature {
    pub features: Vec<PlacedFeature>,
}

impl SimpleRandomFeature {
    pub fn generate<T: GenerationCache>(
        &self,
        chunk: &mut T,
        args: FeatureArgs<'_>,
        random: &mut RandomGenerator,
    ) -> bool {
        let FeatureArgs {
            block_registry,
            min_y,
            height,
            feature_name,
            pos,
        } = args;
        let i = random.next_bounded_i32(self.features.len() as i32);
        let feature = &self.features[i as usize];
        feature.generate(
            chunk,
            block_registry,
            min_y,
            height,
            feature_name,
            random,
            pos,
        )
    }
}
