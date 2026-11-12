use crate::generation::feature::configured_features::FeatureArgs;
use pumpkin_data::Block;
use pumpkin_util::{
    math::vector3::Vector3,
    random::RandomGenerator,
};

use crate::generation::proto_chunk::GenerationCache;

pub struct EndPlatformFeature;

impl EndPlatformFeature {
    pub fn generate<T: GenerationCache>(
        chunk: &mut T,
        args: FeatureArgs<'_>,
        _random: &mut RandomGenerator,
    ) -> bool {
        let FeatureArgs { pos, .. } = args;
        for dx in -2i32..=2 {
            for dz in -2i32..=2 {
                for dy in -1i32..3 {
                    let state = if dy == -1 {
                        Block::OBSIDIAN.default_state
                    } else {
                        Block::AIR.default_state
                    };
                    let target = Vector3::new(pos.0.x + dx, pos.0.y + dy, pos.0.z + dz);
                    chunk.set_block_state(&target, state);
                }
            }
        }
        true
    }
}
