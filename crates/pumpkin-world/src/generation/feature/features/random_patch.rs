use pumpkin_util::{
    math::{position::BlockPos, vector3::Vector3},
    random::{RandomGenerator, RandomImpl},
};

use crate::generation::feature::configured_features::FeatureArgs;
use crate::generation::proto_chunk::GenerationCache;
use crate::generation::feature::placed_features::PlacedFeature;

pub struct RandomPatchFeature {
    pub tries: u8,
    pub xz_spread: u8,
    pub y_spread: u8,
    pub feature: Box<PlacedFeature>,
}

impl RandomPatchFeature {
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
            feature_name: feature,
            pos,
        } = args;
        let xz = self.xz_spread as i32 + 1;
        let y = self.y_spread as i32 + 1;
        let mut is_some = false;
        for _ in 0..self.tries {
            let pos = Vector3::new(
                pos.0.x + random.next_bounded_i32(xz) - random.next_bounded_i32(xz),
                pos.0.y + random.next_bounded_i32(y) - random.next_bounded_i32(y),
                pos.0.z + random.next_bounded_i32(xz) - random.next_bounded_i32(xz),
            );
            if !self.feature.generate(
                chunk,
                block_registry,
                min_y,
                height,
                feature,
                random,
                BlockPos(pos),
            ) {
                continue;
            }
            is_some = true;
        }
        is_some
    }
}
