use pumpkin_data::{BlockDirection, BlockState};
use pumpkin_util::{math::position::BlockPos, random::RandomGenerator};

use crate::generation::proto_chunk::GenerationCache;
use crate::world::WorldPortalExt;

pub struct SpringFeatureFeature {
    pub state: &'static BlockState,
    pub requires_block_below: bool,
    pub rock_count: i32,
    pub hole_count: i32,
    pub valid_blocks: BlockWrapper,
}

pub enum BlockWrapper {
    Single(String),
    Multi(Vec<String>),
}

impl BlockWrapper {
    fn contains(&self, name: &str) -> bool {
        let name_stripped = name.strip_prefix("minecraft:").unwrap_or(name);
        match self {
            Self::Single(s) => s.strip_prefix("minecraft:").unwrap_or(s) == name_stripped,
            Self::Multi(h) => h
                .iter()
                .any(|s| s.strip_prefix("minecraft:").unwrap_or(s) == name_stripped),
        }
    }
}

/// The five neighbours a spring looks at: west, east, north, south and below.
const ROCK_SIDES: [BlockDirection; 5] = [
    BlockDirection::West,
    BlockDirection::East,
    BlockDirection::North,
    BlockDirection::South,
    BlockDirection::Down,
];

impl SpringFeatureFeature {
    pub fn generate<T: GenerationCache>(
        &self,
        _block_registry: &dyn WorldPortalExt,
        chunk: &mut T,
        _random: &mut RandomGenerator,
        pos: BlockPos,
    ) -> bool {
        if !self.may_place(chunk, pos) {
            return false;
        }
        let valid = self.count_valid_neighbours(chunk, pos);
        let air = Self::count_air_neighbours(chunk, pos);
        if valid == self.rock_count && air == self.hole_count {
            chunk.set_block_state(&pos.0, self.state);
            return true;
        }
        false
    }

    fn is_valid_at<T: GenerationCache>(&self, chunk: &T, pos: &BlockPos) -> bool {
        self.valid_blocks.contains(
            GenerationCache::get_block_state(chunk, &pos.0)
                .to_block()
                .name,
        )
    }

    /// The block above must be valid, the block below too when required, and the block itself
    /// air or valid.
    fn may_place<T: GenerationCache>(&self, chunk: &T, pos: BlockPos) -> bool {
        if !self.is_valid_at(chunk, &pos.up()) {
            return false;
        }
        if self.requires_block_below
            && !self.is_valid_at(chunk, &pos.offset(BlockDirection::Down.to_offset()))
        {
            return false;
        }
        let state = GenerationCache::get_block_state(chunk, &pos.0);
        state.to_state().is_air() || self.valid_blocks.contains(state.to_block().name)
    }

    fn count_valid_neighbours<T: GenerationCache>(&self, chunk: &T, pos: BlockPos) -> i32 {
        ROCK_SIDES
            .iter()
            .filter(|side| self.is_valid_at(chunk, &pos.offset(side.to_offset())))
            .count() as i32
    }

    fn count_air_neighbours<T: GenerationCache>(chunk: &T, pos: BlockPos) -> i32 {
        ROCK_SIDES
            .iter()
            .filter(|side| chunk.is_air(&pos.offset(side.to_offset()).0))
            .count() as i32
    }
}

#[cfg(test)]
#[path = "spring_feature_golden.rs"]
mod golden;
