use pumpkin_util::random::{RandomGenerator, RandomImpl};

use crate::generation::feature::configured_features::FeatureArgs;
use crate::generation::proto_chunk::GenerationCache;
use crate::generation::feature::placed_features::PlacedFeatureWrapper;

pub struct RandomBooleanFeature {
    pub feature_true: Box<PlacedFeatureWrapper>,
    pub feature_false: Box<PlacedFeatureWrapper>,
}

impl RandomBooleanFeature {
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
        let val = random.next_bool();
        let feature = if val {
            &self.feature_true
        } else {
            &self.feature_false
        };
        let Some(feature) = feature.get() else {
            return false;
        };
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
