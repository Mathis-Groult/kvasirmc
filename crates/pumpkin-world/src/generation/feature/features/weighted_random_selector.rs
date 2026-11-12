use pumpkin_util::random::{RandomGenerator, RandomImpl};

use crate::generation::feature::configured_features::FeatureArgs;
use crate::generation::proto_chunk::GenerationCache;
use crate::generation::feature::placed_features::PlacedFeatureWrapper;

pub struct WeightedRandomFeature {
    pub features: Vec<WeightedRandomFeatureEntry>,
    pub total_weight: i32,
}

pub struct WeightedRandomFeatureEntry {
    pub feature: PlacedFeatureWrapper,
    pub weight: i32,
}

impl WeightedRandomFeature {
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
        if self.total_weight <= 0 {
            return false;
        }
        let mut roll = random.next_bounded_i32(self.total_weight);
        for entry in &self.features {
            roll -= entry.weight;
            if roll < 0 {
                if let Some(feature) = entry.feature.get() {
                    let sub_name = match entry.feature {
                        PlacedFeatureWrapper::Named(name) => name,
                        PlacedFeatureWrapper::Direct(_) => feature_name,
                    };
                    return feature.generate(
                        chunk,
                        block_registry,
                        min_y,
                        height,
                        sub_name,
                        random,
                        pos,
                    );
                }
                return false;
            }
        }
        false
    }
}
