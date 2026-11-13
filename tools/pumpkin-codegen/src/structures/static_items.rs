//! The fixed part of the generated structures file.

use proc_macro2::TokenStream;
use quote::quote;

pub(super) fn imports_and_structure_keys_tokens() -> TokenStream {
    quote! {
        use pumpkin_util::math::floor_div;
        use pumpkin_util::random::{
            RandomGenerator, RandomImpl, get_carver_seed, get_region_seed,
            legacy_rand::LegacyRand, xoroshiro128::Xoroshiro,
        };
        use pumpkin_util::y_offset::{AboveBottom, Absolute, BelowTop, RelativeToSeaLevel, YOffset};

        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
        pub enum StructureKeys {
            PillagerOutpost,
            Mineshaft,
            MineshaftMesa,
            Mansion,
            JunglePyramid,
            DesertPyramid,
            Igloo,
            Shipwreck,
            ShipwreckBeached,
            SwampHut,
            Stronghold,
            Monument,
            OceanRuinCold,
            OceanRuinWarm,
            Fortress,
            NetherFossil,
            EndCity,
            BuriedTreasure,
            BastionRemnant,
            VillagePlains,
            VillageDesert,
            VillageSavanna,
            VillageSnowy,
            VillageTaiga,
            RuinedPortal,
            RuinedPortalDesert,
            RuinedPortalJungle,
            RuinedPortalSwamp,
            RuinedPortalMountain,
            RuinedPortalOcean,
            RuinedPortalNether,
            AncientCity,
            TrailRuins,
            TrialChambers,
            AbandonedCampBambooJungle,
            AbandonedCampBirchForest,
            AbandonedCampCherryGrove,
            AbandonedCampDappledForest,
            AbandonedCampFlowerForest,
            AbandonedCampForest,
            AbandonedCampMeadow,
            AbandonedCampOldGrowthBirchForest,
            AbandonedCampOldGrowthPineTaiga,
            AbandonedCampOldGrowthSpruceTaiga,
            AbandonedCampPaleGarden,
            AbandonedCampSavanna,
            AbandonedCampSnowyTaiga,
            AbandonedCampSparseJungle,
            AbandonedCampSwamp,
            AbandonedCampTaiga,
            AbandonedCampWindsweptForest,
            AbandonedCampWoodedBadlands,
        }
    }
}

pub(super) fn structure_set_tokens() -> TokenStream {
    quote! {
        pub struct StructureSet {
            pub placement: StructurePlacement,
            pub structures: &'static [WeightedEntry],
        }

        #[derive(Clone)]
        pub struct WeightedEntry {
            pub structure: StructureKeys,
            pub weight: u32,
        }

        pub struct ExclusionZone {
            pub other_set: &'static str,
            pub chunk_count: i32,
        }

        pub struct StructurePlacement {
            pub frequency_reduction_method: Option<FrequencyReductionMethod>,
            pub frequency: Option<f32>,
            pub salt: u32,
            pub exclusion_zone: Option<ExclusionZone>,
            pub placement_type: StructurePlacementType,
        }

        #[derive(Clone, Copy)]
        pub enum FrequencyReductionMethod {
            Default,
            LegacyType1,
            LegacyType2,
            LegacyType3,
        }

        pub enum StructurePlacementType {
            RandomSpread(RandomSpreadStructurePlacement),
            ConcentricRings(ConcentricRingsStructurePlacement),
        }

        pub struct RandomSpreadStructurePlacement {
            pub spacing: i32,
            pub separation: i32,
            pub spread_type: Option<SpreadType>,
        }

        pub struct ConcentricRingsStructurePlacement {
            pub spread: i32,
            pub distance: i32,
            pub count: i32,
            pub preferred_biomes: &'static str,
        }

        #[derive(Clone, Copy)]
        pub enum SpreadType {
            Linear,
            Triangular,
        }

        impl SpreadType {
            pub fn get(&self, random: &mut RandomGenerator, bound: i32) -> i32 {
                match self {
                    Self::Linear => random.next_bounded_i32(bound),
                    Self::Triangular => i32::midpoint(
                        random.next_bounded_i32(bound),
                        random.next_bounded_i32(bound),
                    ),
                }
            }
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum TerrainAdaptation {
            None,
            BeardThin,
            BeardBox,
            Bury,
            Encapsulate,
        }
    }
}

pub(super) fn structure_type_tokens() -> TokenStream {
    quote! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum StructureType {
            Jigsaw,
            BuriedTreasure,
            DesertPyramid,
            EndCity,
            Fortress,
            Igloo,
            JungleTemple,
            WoodlandMansion,
            Mineshaft,
            OceanMonument,
            NetherFossil,
            OceanRuin,
            RuinedPortal,
            Shipwreck,
            Stronghold,
            SwampHut,
            Unknown,
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum BoundingBoxType {
            Piece,
            Full,
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct SpawnEntry {
            pub entity_type: &'static str,
            pub min_count: u32,
            pub max_count: u32,
            pub weight: u32,
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct SpawnOverride {
            pub category: &'static str,
            pub bounding_box: BoundingBoxType,
            pub spawns: &'static [SpawnEntry],
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum HeightProvider {
            Uniform(UniformHeightProvider),
            Trapezoid(TrapezoidHeightProvider),
            VeryBiasedToBottom(VeryBiasedToBottomHeightProvider),
        }

        impl HeightProvider {
            #[must_use]
            pub fn get(&self, random: &mut RandomGenerator, min_y: i8, height: u16) -> i32 {
                match self {
                    Self::Uniform(provider) => provider.get(random, min_y, height),
                    Self::Trapezoid(provider) => provider.get(random, min_y, height),
                    Self::VeryBiasedToBottom(provider) => provider.get(random, min_y, height),
                }
            }
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct UniformHeightProvider {
            pub min_inclusive: YOffset,
            pub max_inclusive: YOffset,
        }

        impl UniformHeightProvider {
            #[must_use]
            pub fn get(&self, random: &mut RandomGenerator, min_y: i8, height: u16) -> i32 {
                let min = self.min_inclusive.get_y(min_y as i16, height);
                let max = self.max_inclusive.get_y(min_y as i16, height);
                if min >= max {
                    min
                } else {
                    random.next_inbetween_i32(min, max)
                }
            }
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct TrapezoidHeightProvider {
            pub min_inclusive: YOffset,
            pub max_inclusive: YOffset,
            pub plateau: Option<i32>,
        }
    }
}

pub(super) fn trapezoid_height_provider_tokens() -> TokenStream {
    quote! {
        impl TrapezoidHeightProvider {
            #[must_use]
            pub fn get(&self, random: &mut RandomGenerator, min_y: i8, height: u16) -> i32 {
                let plateau = self.plateau.unwrap_or(0);
                let i = self.min_inclusive.get_y(min_y as i16, height);
                let j = self.max_inclusive.get_y(min_y as i16, height);

                if i >= j {
                    return i;
                }

                let k = j - i;
                if plateau >= k {
                    return random.next_inbetween_i32(i, j);
                }

                let l = (k - plateau) / 2;
                let m = k - l;
                i + random.next_inbetween_i32(0, m) + random.next_inbetween_i32(0, l)
            }
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct VeryBiasedToBottomHeightProvider {
            pub min_inclusive: YOffset,
            pub max_inclusive: YOffset,
            pub inner: Option<std::num::NonZero<u32>>,
        }

        impl VeryBiasedToBottomHeightProvider {
            #[must_use]
            pub fn get(&self, random: &mut RandomGenerator, min_y: i8, height: u16) -> i32 {
                let min = self.min_inclusive.get_y(min_y as i16, height);
                let max = self.max_inclusive.get_y(min_y as i16, height);
                let inner = self.inner.map_or(1, std::num::NonZero::get) as i32;

                if max - min - inner + 1 <= 0 {
                    return min;
                }

                let upper_inclusive = random.next_inbetween_i32(min + inner, max);
                let biased_upper_inclusive = random.next_inbetween_i32(min, upper_inclusive - 1);
                random.next_inbetween_i32(min, biased_upper_inclusive - 1 + inner)
            }
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum PoolAliasBinding {
            Direct {
                alias: &'static str,
                target: &'static str,
            },
            Random {
                alias: &'static str,
                targets: &'static [WeightedPoolAliasTarget],
            },
            RandomGroup {
                groups: &'static [WeightedPoolAliasGroup],
            },
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct WeightedPoolAliasTarget {
            pub target: &'static str,
            pub weight: u32,
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct WeightedPoolAliasGroup {
            pub bindings: &'static [PoolAliasBinding],
            pub weight: u32,
        }
    }
}

pub(super) fn structure_tokens() -> TokenStream {
    quote! {
        pub struct Structure {
            pub biomes: &'static str,
            pub step: GenerationStep,
            pub start_pool: Option<&'static str>,
            pub start_jigsaw_name: Option<&'static str>,
            pub size: Option<i32>,
            pub terrain_adaptation: TerrainAdaptation,
            pub start_height: Option<HeightProvider>,
            pub project_start_to_heightmap: Option<&'static str>,
            pub max_distance_from_center: Option<i32>,
            pub liquid_settings: Option<&'static str>,
            pub dimension_padding: Option<i32>,
            pub use_expansion_hack: Option<bool>,
            pub pool_aliases: &'static [PoolAliasBinding],
            pub spawn_overrides: &'static [SpawnOverride],
            pub is_beached: Option<bool>,
            pub mineshaft_type: Option<&'static str>,
            pub biome_temp: Option<&'static str>,
            pub cluster_probability: Option<f32>,
            pub large_probability: Option<f32>,
            pub structure_type: StructureType,
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum GenerationStep {
            RawGeneration,
            Lakes,
            LocalModifications,
            UndergroundStructures,
            SurfaceStructures,
            Strongholds,
            UndergroundOres,
            UndergroundDecoration,
            FluidSprings,
            VegetalDecoration,
            TopLayerModification,
        }

        impl GenerationStep {
            #[must_use]
            pub const fn ordinal(&self) -> usize {
                *self as usize
            }
        }

        pub struct StructurePlacementCalculator {
            pub seed: i64,
        }

        impl StructurePlacementCalculator {
            #[must_use]
            pub const fn new(seed: i64) -> Self {
                Self { seed }
            }
        }
    }
}
