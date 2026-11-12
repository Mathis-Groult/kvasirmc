use crate::generation::feature::configured_features::FeatureArgs;
use pumpkin_util::random::RandomGenerator;

use crate::generation::{
        feature::placed_features::{PLACED_FEATURES, PlacedFeatureWrapper},
        proto_chunk::GenerationCache,
    };

pub struct OverlayFeature {
    pub features: Vec<PlacedFeatureWrapper>,
}

impl OverlayFeature {
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
            pos: origin,
        } = args;
        let mut placed_any = false;
        for feature_wrapper in &self.features {
            let (target_feature_name, placed_feature) = match feature_wrapper {
                PlacedFeatureWrapper::Named(name) => (*name, PLACED_FEATURES.get(name)),
                PlacedFeatureWrapper::Direct(feature) => (feature_name, Some(feature)),
            };
            if let Some(placed_feature) = placed_feature {
                placed_any |= placed_feature.generate(
                    chunk,
                    block_registry,
                    min_y,
                    height,
                    target_feature_name,
                    random,
                    origin,
                );
            }
        }
        placed_any
    }
}
