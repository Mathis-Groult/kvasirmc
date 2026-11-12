use pumpkin_util::random::{RandomGenerator, RandomImpl};

use crate::generation::feature::configured_features::FeatureArgs;
use crate::generation::proto_chunk::GenerationCache;
use crate::generation::feature::placed_features::PlacedFeatureWrapper;

pub struct RandomFeature {
    pub features: Vec<RandomFeatureEntry>,
    pub default: Box<PlacedFeatureWrapper>,
}

pub struct RandomFeatureEntry {
    pub feature: PlacedFeatureWrapper,
    pub chance: f32,
}

impl RandomFeature {
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
        for feature in &self.features {
            if random.next_f32() >= feature.chance {
                continue;
            }
            if let Some(f) = feature.feature.get() {
                let sub_name = match feature.feature {
                    PlacedFeatureWrapper::Named(name) => name,
                    PlacedFeatureWrapper::Direct(_) => feature_name,
                };
                return f.generate(chunk, block_registry, min_y, height, sub_name, random, pos);
            }
        }
        let default_name = match self.default.as_ref() {
            PlacedFeatureWrapper::Named(name) => *name,
            PlacedFeatureWrapper::Direct(_) => feature_name,
        };
        self.default.get().is_some_and(|default_feature| {
            default_feature.generate(
                chunk,
                block_registry,
                min_y,
                height,
                default_name,
                random,
                pos,
            )
        })
    }
}
