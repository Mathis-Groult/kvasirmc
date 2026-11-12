use crate::generation::feature::configured_features::FeatureArgs;
use pumpkin_data::BlockDirection;
use pumpkin_util::random::{RandomGenerator, RandomImpl};

use crate::generation::{
        block_predicate::BlockPredicate,
        block_state_provider::BlockStateProvider,
        feature::placed_features::{PLACED_FEATURES, PlacedFeatureWrapper},
        proto_chunk::GenerationCache,
    };

pub struct SingleBlockPillarFeature {
    pub block: BlockStateProvider,
    pub can_replace: BlockPredicate,
    pub direction: BlockDirection,
    pub chance_to_continue: f32,
    pub cap_feature: Option<PlacedFeatureWrapper>,
}

impl SingleBlockPillarFeature {
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
        let mut pos = origin;

        while self.can_replace.test(block_registry, chunk, &pos)
            && random.next_f32() < self.chance_to_continue
            && !chunk.out_of_height(pos.0.y as i16)
        {
            let state = self.block.get(random, pos, chunk, block_registry);
            chunk.set_block_state(&pos.0, state);
            pos = pos.offset(self.direction.to_offset());
        }

        pos = pos.offset(self.direction.opposite().to_offset());
        if let Some(cap) = &self.cap_feature {
            let (cap_feature_name, placed_feature) = match cap {
                PlacedFeatureWrapper::Named(name) => (*name, PLACED_FEATURES.get(name)),
                PlacedFeatureWrapper::Direct(feature) => (feature_name, Some(feature)),
            };
            if let Some(feature) = placed_feature {
                feature.generate(
                    chunk,
                    block_registry,
                    min_y,
                    height,
                    cap_feature_name,
                    random,
                    pos,
                );
            }
        }

        true
    }
}
