use crate::generation::feature::configured_features::FeatureArgs;
use pumpkin_data::{Block, tag};
use pumpkin_util::random::{RandomGenerator, RandomImpl};

use crate::generation::proto_chunk::GenerationCache;
use crate::generation::block_state_provider::BlockStateProvider;

pub struct NetherForestVegetationFeature {
    pub state_provider: BlockStateProvider,
    pub spread_width: i32,
    pub spread_height: i32,
}

impl NetherForestVegetationFeature {
    pub fn generate<T: GenerationCache>(
        &self,
        chunk: &mut T,
        args: FeatureArgs<'_>,
        random: &mut RandomGenerator,
    ) -> bool {
        let FeatureArgs {
            block_registry,
            pos,
            ..
        } = args;
        let origin_pos = pos;
        let below_state = GenerationCache::get_block_state(chunk, &origin_pos.down().0);

        if !below_state
            .to_block_id()
            .has_tag(tag::Block::MINECRAFT_NYLIUM)
        {
            return false;
        }

        // Origin must be within (minY + 1, maxY - 1) inclusive.
        if origin_pos.0.y <= chunk.bottom_y() as i32 || origin_pos.0.y > chunk.top_y() as i32 {
            return false;
        }

        let mut result = false;

        for _ in 0..self.spread_width * self.spread_width {
            let pos = origin_pos.add(
                random.next_bounded_i32(self.spread_width)
                    - random.next_bounded_i32(self.spread_width),
                random.next_bounded_i32(self.spread_height)
                    - random.next_bounded_i32(self.spread_height),
                random.next_bounded_i32(self.spread_width)
                    - random.next_bounded_i32(self.spread_width),
            );

            let nether_state = self.state_provider.get(random, pos, chunk, block_registry);
            let nether_block = Block::from_state_id(nether_state.id);

            // Only place if the spot is air, above the bottom, and the block can survive here.
            if !chunk.is_air(&pos.0)
                || pos.0.y <= chunk.bottom_y() as i32
                || !block_registry.can_place_at(nether_block, nether_state, chunk, &pos)
            {
                continue;
            }

            chunk.set_block_state(&pos.0, nether_state);
            result = true;
        }

        result
    }
}
