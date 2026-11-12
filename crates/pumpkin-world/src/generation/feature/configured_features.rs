use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};

use pumpkin_util::{math::position::BlockPos, random::RandomGenerator};

use super::features::{
    bamboo::BambooFeature,
    basalt_columns::BasaltColumnsFeature,
    basalt_pillar::BasaltPillarFeature,
    block_column::BlockColumnFeature,
    block_pile::BlockPileFeature,
    blue_ice::BlueIceFeature,
    bonus_chest::BonusChestFeature,
    chorus_plant::ChorusPlantFeature,
    coral::{
        coral_claw::CoralClawFeature, coral_mushroom::CoralMushroomFeature,
        coral_tree::CoralTreeFeature,
    },
    delta_feature::DeltaFeatureFeature,
    desert_well::DesertWellFeature,
    disk::DiskFeature,
    drip_stone::{
        cluster::DripstoneClusterFeature, large::LargeDripstoneFeature,
        small::SmallDripstoneFeature,
    },
    end_gateway::EndGatewayFeature,
    end_island::EndIslandFeature,
    end_platform::EndPlatformFeature,
    end_podium::EndPodiumFeature,
    end_spike::EndSpikeFeature,
    fallen_tree::FallenTreeFeature,
    fill_layer::FillLayerFeature,
    forest_rock::ForestRockFeature,
    fossil::FossilFeature,
    freeze_top_layer::FreezeTopLayerFeature,
    geode::GeodeFeature,
    glowstone_blob::GlowstoneBlobFeature,
    huge_brown_mushroom::HugeBrownMushroomFeature,
    huge_fungus::HugeFungusFeature,
    huge_red_mushroom::HugeRedMushroomFeature,
    ice_spike::IceSpikeFeature,
    iceberg::IcebergFeature,
    kelp::KelpFeature,
    lake::LakeFeature,
    monster_room::DungeonFeature,
    multiface_growth::MultifaceGrowthFeature,
    nether_forest_vegetation::NetherForestVegetationFeature,
    netherrack_replace_blobs::ReplaceBlobsFeature,
    ore::OreFeature,
    overlay::OverlayFeature,
    projected_random_patchy_square::ProjectedRandomPatchySquareFeature,
    random_boolean_selector::RandomBooleanFeature,
    random_neighbor_spread::RandomNeighborSpreadFeature,
    random_patch::RandomPatchFeature,
    random_selector::RandomFeature,
    replace_single_block::ReplaceSingleBlockFeature,
    root_system::RootSystemFeature,
    scattered_ore::ScatteredOreFeature,
    sculk_patch::SculkPatchFeature,
    sea_pickle::SeaPickleFeature,
    seagrass::SeagrassFeature,
    simple_block::SimpleBlockFeature,
    simple_random_selector::SimpleRandomFeature,
    single_block_pillar::SingleBlockPillarFeature,
    spring_feature::SpringFeatureFeature,
    stepped_column_cluster::SteppedColumnClusterFeature,
    tree::TreeFeature,
    twisting_vines::TwistingVinesFeature,
    underwater_magma::UnderwaterMagmaFeature,
    vegetation_patch,
    vegetation_patch::VegetationPatchFeature,
    vines::VinesFeature,
    void_start_platform::VoidStartPlatformFeature,
    waterlogged_vegetation_patch,
    waterlogged_vegetation_patch::WaterloggedVegetationPatchFeature,
    weeping_vines::WeepingVinesFeature,
    weighted_random_selector::WeightedRandomFeature,
};
use crate::generation::proto_chunk::GenerationCache;
use crate::world::WorldPortalExt;

/// What `ConfiguredFeature::generate` hands to every feature besides the chunk and the random
/// source.
#[derive(Clone, Copy)]
pub struct FeatureArgs<'a> {
    pub block_registry: &'a dyn WorldPortalExt,
    pub min_y: i8,
    pub height: u16,
    /// The placed feature being generated.
    pub feature_name: pumpkin_data::placed_feature::PlacedFeature,
    pub pos: BlockPos,
}

pub static CONFIGURED_FEATURES: LazyLock<
    HashMap<pumpkin_data::configured_feature::ConfiguredFeature, ConfiguredFeature>,
> = LazyLock::new(build_configured_features);

pub static BONE_MEAL_FEATURES: LazyLock<
    HashSet<pumpkin_data::configured_feature::ConfiguredFeature>,
> = LazyLock::new(|| {
    pumpkin_data::tag::get_tag_values(
        pumpkin_data::tag::RegistryKey::WorldgenConfiguredFeature,
        "minecraft:can_spawn_from_bone_meal",
    )
    .into_iter()
    .flatten()
    .filter_map(|name| pumpkin_data::configured_feature::ConfiguredFeature::from_name(name))
    .collect()
});

pub enum ConfiguredFeature {
    NoOp,
    Tree(Box<TreeFeature>),
    FallenTree(FallenTreeFeature),
    Flower(RandomPatchFeature),
    NoBonemealFlower(RandomPatchFeature),
    RandomPatch(RandomPatchFeature),
    BlockPile(BlockPileFeature),
    SpringFeature(SpringFeatureFeature),
    ChorusPlant(ChorusPlantFeature),
    ReplaceSingleBlock(ReplaceSingleBlockFeature),
    VoidStartPlatform(VoidStartPlatformFeature),
    DesertWell(DesertWellFeature),
    Fossil(FossilFeature),
    HugeRedMushroom(HugeRedMushroomFeature),
    HugeBrownMushroom(HugeBrownMushroomFeature),
    IceSpike(IceSpikeFeature),
    GlowstoneBlob(GlowstoneBlobFeature),
    FreezeTopLayer(FreezeTopLayerFeature),
    Vines(VinesFeature),
    BlockColumn(BlockColumnFeature),
    VegetationPatch(VegetationPatchFeature),
    WaterloggedVegetationPatch(WaterloggedVegetationPatchFeature),
    RootSystem(RootSystemFeature),
    MultifaceGrowth(MultifaceGrowthFeature),
    UnderwaterMagma(UnderwaterMagmaFeature),
    MonsterRoom(DungeonFeature),
    BlueIce(BlueIceFeature),
    Iceberg(IcebergFeature),
    ForestRock(ForestRockFeature),
    Disk(DiskFeature),
    Lake(LakeFeature),
    Ore(OreFeature),
    EndPlatform(EndPlatformFeature),
    EndPodium(EndPodiumFeature),
    EndSpike(EndSpikeFeature),
    EndIsland(EndIslandFeature),
    EndGateway(EndGatewayFeature),
    Seagrass(SeagrassFeature),
    Kelp(KelpFeature),
    CoralTree(CoralTreeFeature),
    CoralMushroom(CoralMushroomFeature),
    CoralClaw(CoralClawFeature),
    SeaPickle(SeaPickleFeature),
    SimpleBlock(SimpleBlockFeature),
    Bamboo(BambooFeature),
    HugeFungus(HugeFungusFeature),
    NetherForestVegetation(NetherForestVegetationFeature),
    WeepingVines(WeepingVinesFeature),
    TwistingVines(TwistingVinesFeature),
    BasaltColumns(BasaltColumnsFeature),
    DeltaFeature(DeltaFeatureFeature),
    NetherrackReplaceBlobs(ReplaceBlobsFeature),
    FillLayer(FillLayerFeature),
    BonusChest(BonusChestFeature),
    BasaltPillar(BasaltPillarFeature),
    ScatteredOre(ScatteredOreFeature),
    RandomSelector(RandomFeature),
    SimpleRandomSelector(SimpleRandomFeature),
    WeightedRandomSelector(WeightedRandomFeature),
    RandomBooleanSelector(RandomBooleanFeature),
    Geode(Box<GeodeFeature>),
    DripstoneCluster(DripstoneClusterFeature),
    LargeDripstone(LargeDripstoneFeature),
    PointedDripstone(SmallDripstoneFeature),
    SculkPatch(SculkPatchFeature),
    Overlay(OverlayFeature),
    ProjectedRandomPatchySquare(ProjectedRandomPatchySquareFeature),
    RandomNeighborSpread(RandomNeighborSpreadFeature),
    SingleBlockPillar(SingleBlockPillarFeature),
    SteppedColumnCluster(SteppedColumnClusterFeature),
}

// Yes this may look ugly and you wonder why this is hard coded, but it makes sense to hardcode since we have to add logic for these in code

impl ConfiguredFeature {
    #[expect(clippy::too_many_arguments)]
    pub fn generate<T: GenerationCache>(
        &self,
        chunk: &mut T,
        block_registry: &dyn WorldPortalExt,
        min_y: i8,
        height: u16,
        feature_name: pumpkin_data::placed_feature::PlacedFeature, // This placed feature
        random: &mut RandomGenerator,
        pos: BlockPos,
    ) -> bool {
        let args = FeatureArgs {
            block_registry,
            min_y,
            height,
            feature_name,
            pos,
        };
        match self {
            Self::NetherrackReplaceBlobs(feature) => feature.generate(chunk, args, random),
            Self::NetherForestVegetation(feature) => feature.generate(chunk, args, random),
            Self::VegetationPatch(feature) => feature.generate(chunk, args, random),
            Self::WaterloggedVegetationPatch(feature) => feature.generate(chunk, args, random),
            Self::PointedDripstone(feature) => feature.generate(chunk, random, pos),
            Self::CoralMushroom(_feature) => CoralMushroomFeature::generate(chunk, args, random),
            Self::CoralTree(_feature) => CoralTreeFeature::generate(chunk, args, random),
            Self::CoralClaw(_feature) => {
                CoralClawFeature::generate(chunk, block_registry, random, pos)
            }
            Self::EndPlatform(_feature) => EndPlatformFeature::generate(chunk, args, random),
            Self::EndPodium(feature) => feature.generate(chunk, pos),
            Self::EndSpike(feature) => feature.generate(chunk, args, random),
            Self::SpringFeature(feature) => feature.generate(block_registry, chunk, random, pos),
            Self::SimpleBlock(feature) => feature.generate(block_registry, chunk, random, pos),
            Self::RandomPatch(feature)
            | Self::Flower(feature)
            | Self::NoBonemealFlower(feature) => feature.generate(chunk, args, random),
            Self::DesertWell(_feature) => DesertWellFeature::generate(chunk, args, random),
            Self::Fossil(feature) => feature.generate(chunk, args, random),
            Self::Bamboo(feature) => feature.generate(chunk, args, random),
            Self::BlockColumn(feature) => feature.generate(chunk, args, random),
            Self::RandomBooleanSelector(feature) => feature.generate(chunk, args, random),
            Self::Tree(feature) => feature.generate(block_registry, chunk, random, pos),
            Self::RandomSelector(feature) => feature.generate(chunk, args, random),
            Self::SimpleRandomSelector(feature) => feature.generate(chunk, args, random),
            Self::WeightedRandomSelector(feature) => feature.generate(chunk, args, random),
            Self::Vines(_feature) => VinesFeature::generate(chunk, args, random),
            Self::Seagrass(feature) => feature.generate(chunk, args, random),
            Self::TwistingVines(feature) => feature.generate(chunk, args, random),
            Self::UnderwaterMagma(feature) => feature.generate(chunk, args, random),
            Self::SeaPickle(feature) => feature.generate(chunk, args, random),
            Self::Geode(feature) => feature.generate(chunk, args, random),
            Self::Kelp(_feature) => KelpFeature::generate(chunk, args, random),
            Self::Ore(feature) => feature.generate(chunk, args, random),
            Self::ScatteredOre(feature) => feature.generate(chunk, args, random),
            Self::MonsterRoom(_feature) => DungeonFeature::generate(chunk, args, random),
            Self::BlueIce(_feature) => BlueIceFeature::generate(chunk, random, pos),
            Self::GlowstoneBlob(_feature) => GlowstoneBlobFeature::generate(chunk, args, random),
            Self::Disk(feature) => feature.generate(chunk, args, random),
            Self::Lake(feature) => feature.generate(block_registry, chunk, random, pos),
            Self::BasaltColumns(feature) => feature.generate(chunk, args, random),
            Self::BasaltPillar(_feature) => BasaltPillarFeature::generate(chunk, random, pos),
            Self::ForestRock(feature) => feature.generate(chunk, random, pos),
            Self::FreezeTopLayer(_feature) => FreezeTopLayerFeature::generate(chunk, args, random),
            Self::IceSpike(_feature) => IceSpikeFeature::generate(chunk, args, random),
            Self::Iceberg(feature) => feature.generate(chunk, args, random),
            Self::ChorusPlant(_feature) => ChorusPlantFeature::generate(chunk, args, random),
            Self::EndIsland(_feature) => EndIslandFeature::generate(chunk, args, random),
            Self::SculkPatch(feature) => feature.generate(block_registry, chunk, random, pos),
            Self::RootSystem(feature) => feature.generate(chunk, args, random),
            Self::BonusChest(_feature) => BonusChestFeature::generate(chunk, args, random),
            Self::DeltaFeature(feature) => feature.generate(chunk, args, random),
            Self::BlockPile(feature) => feature.generate(chunk, args, random),
            Self::DripstoneCluster(feature) => feature.generate(chunk, pos),
            Self::LargeDripstone(feature) => feature.generate(chunk, random, pos),
            Self::EndGateway(feature) => feature.generate(chunk, pos),
            Self::FillLayer(feature) => feature.generate(chunk, args, random),
            Self::FallenTree(feature) => feature.generate(chunk, args, random),
            Self::HugeBrownMushroom(feature) => feature.generate(chunk, args, random),
            Self::HugeFungus(feature) => feature.generate(chunk, args, random),
            Self::HugeRedMushroom(feature) => feature.generate(chunk, args, random),
            Self::MultifaceGrowth(feature) => feature.generate(chunk, args, random),
            Self::ReplaceSingleBlock(feature) => feature.generate(chunk, args, random),
            Self::VoidStartPlatform(feature) => feature.generate(chunk, args, random),
            Self::WeepingVines(feature) => feature.generate(chunk, args, random),
            Self::Overlay(feature) => feature.generate(chunk, args, random),
            Self::ProjectedRandomPatchySquare(feature) => feature.generate(chunk, args, random),
            Self::RandomNeighborSpread(feature) => feature.generate(chunk, args, random),
            Self::SingleBlockPillar(feature) => feature.generate(chunk, args, random),
            Self::SteppedColumnCluster(feature) => {
                feature.generate(chunk, block_registry, random, pos)
            }
            Self::NoOp => false,
        }
    }
}

// generated code is now placed alongside other codegen outputs
// in `src/generated` so we don’t hide it deep under `generation/feature`.
include!("../../../../pumpkin-data/src/generated/configured_features_generated.rs");

#[cfg(test)]
mod tests {
    use super::{BONE_MEAL_FEATURES, CONFIGURED_FEATURES, ConfiguredFeature};
    use pumpkin_util::{
        math::position::BlockPos,
        random::{RandomGenerator, xoroshiro128::Xoroshiro},
    };

    #[test]
    fn bonemeal_feature_tag_resolves_to_placeable_blocks() {
        assert_eq!(BONE_MEAL_FEATURES.len(), 8);
        let mut random = RandomGenerator::Xoroshiro(Xoroshiro::from_seed(0));
        for key in BONE_MEAL_FEATURES.iter() {
            let Some(ConfiguredFeature::SimpleBlock(feature)) = CONFIGURED_FEATURES.get(key) else {
                panic!("bonemeal feature {key:?} must place a block");
            };
            assert!(
                feature
                    .to_place
                    .get_for_bonemeal(&mut random, BlockPos::new(0, 64, 0))
                    .is_some()
            );
        }
    }

    #[test]
    fn bonemeal_features_use_tag_output() {
        let tag_values = pumpkin_data::tag::get_tag_values(
            pumpkin_data::tag::RegistryKey::WorldgenConfiguredFeature,
            "minecraft:can_spawn_from_bone_meal",
        );
        assert!(tag_values.is_some());
        let values = tag_values.unwrap();
        assert_eq!(values.len(), 8);
        assert!(values.contains(&"flower_default"));
        assert!(values.contains(&"wildflower"));
    }
}
