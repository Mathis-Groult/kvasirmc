//! One function per foliage type behind `FoliageType::generate`: each forwards the shared call
//! arguments to its placer.

use pumpkin_util::{math::position::BlockPos, random::RandomGenerator};

use super::{
    acacia::AcaciaFoliagePlacer, blob::BlobFoliagePlacer, bush::BushFoliagePlacer,
    cherry::CherryFoliagePlacer, dark_oak::DarkOakFoliagePlacer, fancy::LargeOakFoliagePlacer,
    jungle::JungleFoliagePlacer, mega_pine::MegaPineFoliagePlacer, pine::PineFoliagePlacer,
    poplar::PoplarFoliagePlacer, random_spread::RandomSpreadFoliagePlacer,
    spruce::SpruceFoliagePlacer,
};
use crate::generation::block_state_provider::BlockStateProvider;
use crate::generation::feature::features::tree::TreeNode;
use crate::generation::proto_chunk::GenerationCache;

/// The arguments every foliage placer takes besides its own settings.
pub(super) struct FoliageCall<'a, T: GenerationCache> {
    pub(super) chunk: &'a mut T,
    pub(super) random: &'a mut RandomGenerator,
    pub(super) node: &'a TreeNode,
    pub(super) foliage_height: i32,
    pub(super) radius: i32,
    pub(super) offset: i32,
    pub(super) foliage_provider: &'a BlockStateProvider,
}

impl<T: GenerationCache> FoliageCall<'_, T> {
    pub(super) fn blob(&mut self, placer: &BlobFoliagePlacer) -> Vec<BlockPos> {
        placer.generate(
            self.chunk,
            self.random,
            self.node,
            self.foliage_height,
            self.radius,
            self.offset,
            self.foliage_provider,
        )
    }

    pub(super) fn spruce(&mut self, placer: &SpruceFoliagePlacer) -> Vec<BlockPos> {
        placer.generate(
            self.chunk,
            self.random,
            self.node,
            self.foliage_height,
            self.radius,
            self.offset,
            self.foliage_provider,
        )
    }

    pub(super) fn pine(&mut self, placer: &PineFoliagePlacer) -> Vec<BlockPos> {
        placer.generate(
            self.chunk,
            self.random,
            self.node,
            self.foliage_height,
            self.radius,
            self.offset,
            self.foliage_provider,
        )
    }

    pub(super) fn acacia(&mut self, placer: &AcaciaFoliagePlacer) -> Vec<BlockPos> {
        placer.generate(
            self.chunk,
            self.random,
            self.node,
            self.foliage_height,
            self.radius,
            self.offset,
            self.foliage_provider,
        )
    }

    pub(super) fn bush(&mut self, placer: &BushFoliagePlacer) -> Vec<BlockPos> {
        placer.generate(
            self.chunk,
            self.random,
            self.node,
            self.foliage_height,
            self.radius,
            self.offset,
            self.foliage_provider,
        )
    }

    pub(super) fn fancy(&mut self, placer: &LargeOakFoliagePlacer) -> Vec<BlockPos> {
        placer.generate(
            self.chunk,
            self.random,
            self.node,
            self.foliage_height,
            self.radius,
            self.offset,
            self.foliage_provider,
        )
    }

    pub(super) fn jungle(&mut self, placer: &JungleFoliagePlacer) -> Vec<BlockPos> {
        placer.generate(
            self.chunk,
            self.random,
            self.node,
            self.foliage_height,
            self.radius,
            self.offset,
            self.foliage_provider,
        )
    }

    pub(super) fn mega_pine(&mut self, placer: &MegaPineFoliagePlacer) -> Vec<BlockPos> {
        placer.generate(
            self.chunk,
            self.random,
            self.node,
            self.foliage_height,
            self.radius,
            self.offset,
            self.foliage_provider,
        )
    }

    pub(super) fn dark_oak(&mut self, placer: &DarkOakFoliagePlacer) -> Vec<BlockPos> {
        placer.generate(
            self.chunk,
            self.random,
            self.node,
            self.foliage_height,
            self.radius,
            self.offset,
            self.foliage_provider,
        )
    }

    pub(super) fn random_spread(&mut self, placer: &RandomSpreadFoliagePlacer) -> Vec<BlockPos> {
        placer.generate(
            self.chunk,
            self.random,
            self.node,
            self.foliage_height,
            self.radius,
            self.offset,
            self.foliage_provider,
        )
    }

    pub(super) fn cherry(&mut self, placer: &CherryFoliagePlacer) -> Vec<BlockPos> {
        let mut positions = Vec::new();
        placer.generate(
            &mut positions,
            self.chunk,
            self.random,
            self.node,
            self.foliage_height,
            self.radius,
            self.offset,
            self.foliage_provider,
        );
        positions
    }

    pub(super) fn poplar(&mut self, placer: &PoplarFoliagePlacer) -> Vec<BlockPos> {
        placer.generate(
            self.chunk,
            self.random,
            self.node,
            self.foliage_height,
            self.radius,
            self.offset,
            self.foliage_provider,
        )
    }
}
