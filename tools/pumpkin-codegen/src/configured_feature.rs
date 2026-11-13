mod terrain;
mod vegetation;

use heck::ToPascalCase;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::placed_feature::{
    int_or_uniform, value_to_block_direction, value_to_block_predicate, value_to_block_state,
    value_to_block_state_codec, value_to_height_provider, value_to_int_provider,
};

fn load_configured_features() -> BTreeMap<String, Value> {
    let dir = Path::new("../../assets/datapack/data/minecraft/worldgen/feature");
    let mut map = BTreeMap::new();
    collect_configured_features(dir, "", &mut map);
    map
}

fn collect_configured_features(dir: &Path, prefix: &str, map: &mut BTreeMap<String, Value>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .expect("Missing worldgen/feature directory")
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.path());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().unwrap().to_string_lossy();
            let nested = format!("{prefix}{name}/");
            collect_configured_features(&path, &nested, map);
        } else if path.extension().is_some_and(|ext| ext == "json") {
            let stem = path.file_stem().unwrap().to_string_lossy();
            let key = format!("{prefix}{stem}");
            let content =
                fs::read_to_string(&path).expect("Failed to read configured_feature file");
            let val: Value =
                serde_json::from_str(&content).expect("Failed to parse configured_feature JSON");
            map.insert(key, val);
        }
    }
}

/// Reads configured_feature files from 26.2 datapack and emits the complete `ConfiguredFeature` enum `TokenStream`.
pub fn build_enum() -> TokenStream {
    let json = load_configured_features();

    let mut from_name_arms = Vec::new();
    let mut to_name_arms = Vec::new();

    let variants: Vec<TokenStream> = json
        .iter()
        .map(|(name, _)| {
            let variant_name = format_ident!("{}", name.to_pascal_case());
            from_name_arms.push(quote! {
                #name => Some(Self::#variant_name),
            });
            to_name_arms.push(quote! {
                Self::#variant_name => #name,
            });
            quote! {
                #variant_name,
            }
        })
        .collect();

    quote! {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum ConfiguredFeature {
            #(#variants)*
        }

        impl ConfiguredFeature {
            pub fn from_name(name: &str) -> Option<Self> {
                let name = name.strip_prefix("minecraft:").unwrap_or(name);
                match name {
                    #(#from_name_arms)*
                    _ => None,
                }
            }

            pub const fn to_name(&self) -> &'static str {
                match self {
                    #(#to_name_arms)*
                }
            }
        }
    }
}

/// Reads configured_feature files from 26.2 datapack and emits a `build_configured_features()` function `TokenStream`.
pub fn build() -> TokenStream {
    let json = load_configured_features();

    let entries: Vec<TokenStream> = json
        .iter()
        .map(|(name, value)| {
            let cf = value_to_configured_feature(value);
            let variant_name = format_ident!("{}", name.to_pascal_case());
            quote! {
                map.insert(pumpkin_data::configured_feature::ConfiguredFeature::#variant_name, #cf);
            }
        })
        .collect();

    quote! {
        #[allow(clippy::all, unused_imports, dead_code, clippy::large_stack_frames, clippy::too_many_lines)]
        fn build_configured_features() -> std::collections::HashMap<pumpkin_data::configured_feature::ConfiguredFeature, ConfiguredFeature> {
            use crate::generation::block_predicate::{
                AllOfBlockPredicate, AnyOfBlockPredicate, BlockPredicate,
                HasSturdyFacePredicate, InsideWorldBoundsBlockPredicate,
                MatchingBlockTagPredicate, MatchingBlocksBlockPredicate, MatchingBlocksWrapper,
                MatchingFluidsBlockPredicate, NotBlockPredicate, OffsetBlocksBlockPredicate,
                ReplaceableBlockPredicate, SolidBlockPredicate, WouldSurviveBlockPredicate,
                HeightRangeBlockPredicate,
            };
            use crate::generation::height_provider::{
                HeightProvider, TrapezoidHeightProvider, UniformHeightProvider,
                VeryBiasedToBottomHeightProvider,
            };
            use pumpkin_util::y_offset::{AboveBottom, Absolute, BelowTop, RelativeToSeaLevel, YOffset};
            use pumpkin_util::math::int_provider::{
                BiasedToBottomIntProvider, ClampedIntProvider, TrapezoidIntProvider, ClampedNormalIntProvider,
                ConstantIntProvider, IntProvider, NormalIntProvider, UniformIntProvider,
                WeightedEntry, WeightedListIntProvider,
            };
            use crate::block::BlockStateCodec;
            use crate::generation::block_state_provider::{
                BlockStateProvider, BlockStateRule, DualNoiseBlockStateProvider,
                NoiseBlockStateProvider, NoiseBlockStateProviderBase,
                NoiseThresholdBlockStateProvider, PillarBlockStateProvider,
                RandomizedIntBlockStateProvider, RuleBasedBlockStateProvider, SimpleStateProvider,
                WeightedBlockStateProvider,
            };
            use pumpkin_util::math::pool::Weighted;
            use pumpkin_util::DoublePerlinNoiseParametersCodec;
            use crate::generation::rule::{
                RuleTest,
                block_match::BlockMatchRuleTest,
                block_state_match::BlockStateMatchRuleTest,
                tag_match::TagMatchRuleTest,
                random_block_match::RandomBlockMatchRuleTest,
                random_block_state_match::RandomBlockStateMatchRuleTest,
            };
            use crate::generation::feature::placed_features::{
                Feature, PlacedFeature, PlacementModifier,
                BiomePlacementModifier, BlockFilterPlacementModifier,
                CountOnEveryLayerPlacementModifier, CountPlacementModifier,
                EnvironmentScanPlacementModifier, HeightRangePlacementModifier,
                HeightmapPlacementModifier, NoiseBasedCountPlacementModifier,
                NoiseThresholdCountPlacementModifier, OffsetPlacementModifier,
                RandomOffsetPlacementModifier,
                RarityFilterPlacementModifier, SquarePlacementModifier,
                SurfaceThresholdFilterPlacementModifier, SurfaceWaterDepthFilterPlacementModifier,
                PlacedFeatureWrapper,
            };
            use crate::generation::feature::features::{
                bamboo::BambooFeature,
                block_column::{BlockColumnFeature, Layer},
                end_spike::{EndSpikeFeature, Spike},
                fallen_tree::FallenTreeFeature,
                nether_forest_vegetation::NetherForestVegetationFeature,
                netherrack_replace_blobs::ReplaceBlobsFeature,
                ore::{OreFeature, OreTarget},
                overlay::OverlayFeature,
                projected_random_patchy_square::ProjectedRandomPatchySquareFeature,
                random_boolean_selector::RandomBooleanFeature,
                random_neighbor_spread::RandomNeighborSpreadFeature,
                random_patch::RandomPatchFeature,
                random_selector::{RandomFeature, RandomFeatureEntry},
                weighted_random_selector::{WeightedRandomFeature, WeightedRandomFeatureEntry},
                sea_pickle::SeaPickleFeature,
                seagrass::SeagrassFeature,
                simple_block::SimpleBlockFeature,
                simple_random_selector::SimpleRandomFeature,
                single_block_pillar::SingleBlockPillarFeature,
                spring_feature::{BlockWrapper, SpringFeatureFeature},
                geode::GeodeFeature,
                tree::TreeFeature,
                vegetation_patch::VegetationPatchFeature,
                waterlogged_vegetation_patch::WaterloggedVegetationPatchFeature,
                tree::trunk::{TrunkPlacer, TrunkType,
                    bending::BendingTrunkPlacer,
                    cherry::CherryTrunkPlacer,
                    dark_oak::DarkOakTrunkPlacer,
                    fancy::FancyTrunkPlacer,
                    forking::ForkingTrunkPlacer,
                    giant::GiantTrunkPlacer,
                    mega_jungle::MegaJungleTrunkPlacer,
                    poplar::PoplarTrunkPlacer,
                    straight::StraightTrunkPlacer,
                    upwards_branching::UpwardsBranchingTrunkPlacer,
                },
                tree::foliage::{FoliagePlacer, FoliageType,
                    acacia::AcaciaFoliagePlacer,
                    blob::BlobFoliagePlacer,
                    bush::BushFoliagePlacer,
                    cherry::CherryFoliagePlacer,
                    dark_oak::DarkOakFoliagePlacer,
                    fancy::LargeOakFoliagePlacer,
                    jungle::JungleFoliagePlacer,
                    mega_pine::MegaPineFoliagePlacer,
                    pine::PineFoliagePlacer,
                    poplar::PoplarFoliagePlacer,
                    random_spread::RandomSpreadFoliagePlacer,
                    spruce::SpruceFoliagePlacer,
                },
                tree::decorator::{
                    TreeDecorator,
                    alter_ground::AlterGroundTreeDecorator,
                    attached_to_leaves::AttachedToLeavesTreeDecorator,
                    attached_to_logs::AttachedToLogsTreeDecorator,
                    beehive::BeehiveTreeDecorator,
                    cocoa::CocoaTreeDecorator,
                    creaking_heart::CreakingHeartTreeDecorator,
                    leave_vine::LeavesVineTreeDecorator,
                    pale_moss::PaleMossTreeDecorator,
                    place_on_ground::PlaceOnGroundTreeDecorator,
                    trunk_vine::TrunkVineTreeDecorator,
                },
                tree::root::{
                    RootPlacer,
                    mangrove::{AboveRootPlacement, MangroveRootPlacement, MangroveRootPlacer},
                },
            };
            use crate::generation::feature::size::{FeatureSize, FeatureSizeType, ThreeLayersFeatureSize, TwoLayersFeatureSize};
            use pumpkin_data::{Block, BlockDirection};
            use pumpkin_util::math::vector3::Vector3;
            use pumpkin_util::HeightMap;
            use crate::generation::feature::features::drip_stone::small::SmallDripstoneFeature;
            let mut map = std::collections::HashMap::new();
            #(#entries)*
            map
        }
    }
}

fn value_to_fossil_processor(value: &Value) -> TokenStream {
    let variant = match value
        .as_str()
        .expect("fossil processor names must be strings")
    {
        "minecraft:fossil_rot" => format_ident!("FossilRot"),
        "minecraft:fossil_coal" => format_ident!("Coal"),
        "minecraft:fossil_diamonds" => format_ident!("Diamonds"),
        processor => panic!("Unsupported fossil processor: {processor}"),
    };

    quote! {
        crate::generation::feature::features::fossil::FossilProcessor::#variant
    }
}

/// Converts a single configured-feature JSON value into its `ConfiguredFeature` token stream.
///
/// # Arguments
/// – `v` – the JSON object for the feature, expected to contain `"type"` and `"config"` fields.
pub fn value_to_configured_feature(v: &Value) -> TokenStream {
    let type_str = v["type"].as_str().unwrap_or("");
    let config = if v.get("config").is_some_and(Value::is_object) {
        &v["config"]
    } else {
        v
    };
    match type_str {
        "minecraft:no_op" => quote! { ConfiguredFeature::NoOp },
        "minecraft:bamboo" => vegetation::feature_bamboo(config),
        "minecraft:seagrass" => vegetation::feature_seagrass(config),
        "minecraft:sea_pickle" => vegetation::feature_sea_pickle(config),
        "minecraft:nether_forest_vegetation" => {
            vegetation::feature_nether_forest_vegetation(config)
        }
        "minecraft:netherrack_replace_blobs" => terrain::feature_netherrack_replace_blobs(config),
        "minecraft:simple_block" => terrain::feature_simple_block(config),
        "minecraft:ore" | "minecraft:scattered_ore" => terrain::feature_ore(config, type_str),
        "minecraft:spring_feature" => terrain::feature_spring_feature(config),
        "minecraft:block_column" => vegetation::feature_block_column(config),
        "minecraft:fallen_tree" => vegetation::feature_fallen_tree(config),
        "minecraft:random_patch" | "minecraft:flower" | "minecraft:no_bonemeal_flower" => {
            vegetation::feature_random_patch(config, type_str)
        }
        "minecraft:random_selector" => vegetation::feature_random_selector(config),
        "minecraft:weighted_random_selector" => {
            vegetation::feature_weighted_random_selector(config)
        }
        "minecraft:simple_random_selector" => vegetation::feature_simple_random_selector(config),
        "minecraft:random_boolean_selector" => vegetation::feature_random_boolean_selector(config),
        "minecraft:end_spike" => terrain::feature_end_spike(config),
        "minecraft:tree" => vegetation::feature_tree(config),
        "minecraft:pointed_dripstone" | "minecraft:speleothem" => {
            terrain::feature_pointed_dripstone(config)
        }
        "minecraft:geode" => terrain::feature_geode(config),
        "minecraft:monster_room" => terrain::feature_monster_room(),
        "minecraft:underwater_magma" => terrain::feature_underwater_magma(config),
        "minecraft:vegetation_patch" | "minecraft:waterlogged_vegetation_patch" => {
            vegetation::feature_vegetation_patch(config, type_str)
        }
        "minecraft:glowstone_blob" => terrain::feature_glowstone_blob(),
        "minecraft:disk" => terrain::feature_disk(config),
        "minecraft:basalt_columns" => terrain::feature_basalt_columns(config),
        "minecraft:basalt_pillar" => terrain::feature_basalt_pillar(),
        "minecraft:block_blob" => terrain::feature_block_blob(config),
        "minecraft:freeze_top_layer" => terrain::feature_freeze_top_layer(),
        "minecraft:ice_spike" => terrain::feature_ice_spike(),
        "minecraft:spike" => terrain::feature_spike(),
        "minecraft:iceberg" => terrain::feature_iceberg(config),
        "minecraft:chorus_plant" => vegetation::feature_chorus_plant(),
        "minecraft:end_platform" => terrain::feature_end_platform(),
        "minecraft:end_podium" => terrain::feature_end_podium(v, config),
        "minecraft:end_island" => terrain::feature_end_island(),
        "minecraft:kelp" => vegetation::feature_kelp(),

        "minecraft:fossil" => terrain::feature_fossil(config),
        "minecraft:lake" => terrain::feature_lake(config),
        "minecraft:huge_brown_mushroom" => vegetation::feature_huge_brown_mushroom(),
        "minecraft:huge_red_mushroom" => vegetation::feature_huge_red_mushroom(),
        "minecraft:vines" => vegetation::feature_vines(),
        "minecraft:root_system" => vegetation::feature_root_system(config),
        "minecraft:multiface_growth" => vegetation::feature_multiface_growth(v, config),
        "minecraft:blue_ice" => terrain::feature_blue_ice(),
        "minecraft:end_gateway" => terrain::feature_end_gateway(config),
        "minecraft:coral_tree" => vegetation::feature_coral_tree(),
        "minecraft:coral_mushroom" => vegetation::feature_coral_mushroom(),
        "minecraft:coral_claw" => vegetation::feature_coral_claw(),
        "minecraft:huge_fungus" => vegetation::feature_huge_fungus(config),
        "minecraft:weeping_vines" => vegetation::feature_weeping_vines(),
        "minecraft:twisting_vines" => vegetation::feature_twisting_vines(config),
        "minecraft:delta_feature" => terrain::feature_delta_feature(config),
        "minecraft:fill_layer" => terrain::feature_fill_layer(config),
        "minecraft:bonus_chest" => terrain::feature_bonus_chest(),
        "minecraft:dripstone_cluster" | "minecraft:speleothem_cluster" => {
            terrain::feature_dripstone_cluster()
        }
        "minecraft:sequence" => terrain::feature_sequence(),
        "minecraft:stepped_column_cluster" => terrain::feature_stepped_column_cluster(config),
        "minecraft:large_dripstone" => terrain::feature_large_dripstone(),
        "minecraft:sculk_patch" => terrain::feature_sculk_patch(config),
        "minecraft:block_pile" => terrain::feature_block_pile(config),
        "minecraft:replace_single_block" => terrain::feature_replace_single_block(config),
        "minecraft:void_start_platform" => terrain::feature_void_start_platform(),
        "minecraft:desert_well" => terrain::feature_desert_well(),
        "minecraft:overlay" => terrain::feature_overlay(config),
        "minecraft:random_neighbor_spread" => terrain::feature_random_neighbor_spread(config),
        "minecraft:single_block_pillar" => terrain::feature_single_block_pillar(config),
        "minecraft:projected_random_patchy_square" => {
            terrain::feature_projected_random_patchy_square(config)
        }
        "minecraft:template" => terrain::feature_template(),
        other => {
            let msg = format!("unknown configured feature type: {other}");
            quote! { compile_error!(#msg) }
        }
    }
}

fn value_to_accepted_neighbors(v: &Value) -> TokenStream {
    let names: Vec<String> = match v {
        Value::String(s) => vec![s.clone()],
        Value::Array(arr) => arr
            .iter()
            .filter_map(|x| x.as_str().map(|s| s.to_string()))
            .collect(),
        _ => Vec::new(),
    };
    let block_ids: Vec<TokenStream> = names
        .iter()
        .map(|name| {
            let stripped = name.strip_prefix("minecraft:").unwrap_or(name);
            let ident =
                quote::format_ident!("{}", stripped.to_uppercase().replace([':', '-'], "_"));
            quote! { pumpkin_data::Block::#ident.id }
        })
        .collect();
    quote! { vec![#(#block_ids),*] }
}

/// Converts a block-state-provider JSON object into its `BlockStateProvider` token stream.
///
/// # Arguments
/// – `v` – the JSON value for the provider, expected to contain a `"type"` field.
///
/// # Returns
/// A `TokenStream` for the appropriate `BlockStateProvider` variant; defaults to `BlockStateProvider::Simple` with air if the type is unrecognised.
fn value_to_block_state_provider(v: &Value) -> TokenStream {
    if let Some(s) = v.as_str() {
        let name = s.strip_prefix("minecraft:").unwrap_or(s);
        let path = format!(
            "../../assets/datapack/data/minecraft/worldgen/block_state_provider/{name}.json"
        );
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(parsed) = serde_json::from_str::<Value>(&content) {
                return value_to_block_state_provider(&parsed);
            }
        }
    }
    if v.get("type").is_none() && (v.get("id").is_some() || v.get("Name").is_some()) {
        let state = value_to_block_state(v);
        return quote! { BlockStateProvider::Simple(SimpleStateProvider { state: #state }) };
    }
    let type_str = v["type"].as_str().unwrap_or("");
    match type_str {
        "minecraft:simple_state_provider" | "minecraft:simple" => {
            let state = if v["state"].is_null() {
                value_to_block_state(v)
            } else {
                value_to_block_state(&v["state"])
            };
            quote! { BlockStateProvider::Simple(SimpleStateProvider { state: #state }) }
        }
        "minecraft:weighted_state_provider" | "minecraft:weighted" => {
            let entries: Vec<TokenStream> = v["entries"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|e| {
                            let data = value_to_block_state(&e["data"]);
                            let weight = e["weight"].as_i64().unwrap_or(1) as i32;
                            quote! { Weighted { data: #data, weight: #weight } }
                        })
                        .collect()
                })
                .unwrap_or_default();
            quote! {
                BlockStateProvider::Weighted(WeightedBlockStateProvider {
                    entries: vec![#(#entries),*],
                })
            }
        }
        "minecraft:rotated_block_provider" | "minecraft:rotated" => {
            let state = value_to_block_state(&v["state"]);
            quote! { BlockStateProvider::Pillar(PillarBlockStateProvider { state: #state }) }
        }
        "minecraft:noise_provider" | "minecraft:noise" => {
            let base = value_to_noise_base(v);
            let states: Vec<TokenStream> = v["states"]
                .as_array()
                .map(|arr| arr.iter().map(value_to_block_state).collect())
                .unwrap_or_default();
            quote! {
                BlockStateProvider::NoiseProvider(NoiseBlockStateProvider {
                    base: #base,
                    states: vec![#(#states),*],
                })
            }
        }
        "minecraft:dual_noise_provider" | "minecraft:dual_noise" => {
            let base_provider = value_to_noise_base(v);
            let states: Vec<TokenStream> = v["states"]
                .as_array()
                .map(|arr| arr.iter().map(value_to_block_state).collect())
                .unwrap_or_default();
            let base_noise_provider = quote! {
                NoiseBlockStateProvider { base: #base_provider, states: vec![#(#states),*] }
            };
            let v0 = v["variety"][0].as_u64().unwrap_or(2) as u32;
            let v1 = v["variety"][1].as_u64().unwrap_or(4) as u32;
            let slow_noise = value_to_dpnp(&v["slow_noise"]);
            let slow_scale = v["slow_scale"].as_f64().unwrap_or(1.0);
            quote! {
                BlockStateProvider::DualNoise(DualNoiseBlockStateProvider {
                    base: #base_noise_provider,
                    variety: [#v0, #v1],
                    slow_noise: #slow_noise,
                    slow_scale: #slow_scale,
                })
            }
        }
        "minecraft:noise_threshold_provider" | "minecraft:noise_threshold" => {
            let base = value_to_noise_base(v);
            let threshold = v["threshold"].as_f64().unwrap_or(0.0) as f32;
            let high_chance = v["high_chance"].as_f64().unwrap_or(0.0) as f32;
            let default = value_to_block_state(&v["default_state"]);
            let low: Vec<TokenStream> = v["low_states"]
                .as_array()
                .map(|a| a.iter().map(value_to_block_state).collect())
                .unwrap_or_default();
            let high: Vec<TokenStream> = v["high_states"]
                .as_array()
                .map(|a| a.iter().map(value_to_block_state).collect())
                .unwrap_or_default();
            quote! {
                BlockStateProvider::NoiseThreshold(NoiseThresholdBlockStateProvider {
                    base: #base,
                    threshold: #threshold,
                    high_chance: #high_chance,
                    default_state: #default,
                    low_states: vec![#(#low),*],
                    high_states: vec![#(#high),*],
                })
            }
        }
        "minecraft:randomized_int_state_provider" | "minecraft:randomized_int" => {
            let src = value_to_block_state_provider(&v["source"]);
            let prop = v["property"].as_str().unwrap_or("");
            let prop_ident = format_ident!("{}", prop.to_pascal_case());
            let vals = value_to_int_provider(&v["values"]);
            quote! {
                BlockStateProvider::RandomizedInt(RandomizedIntBlockStateProvider {
                    source: Box::new(#src),
                    property: pumpkin_data::block_properties::PropertyKey::#prop_ident,
                    values: #vals,
                })
            }
        }
        "minecraft:rule_based_state_provider" | "minecraft:rule_based" => {
            let fallback = if !v["fallback"].is_null() {
                let provider = value_to_block_state_provider(&v["fallback"]);
                quote! { Some(Box::new(#provider))}
            } else {
                quote! { None }
            };

            let rules: Vec<TokenStream> = v["rules"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|rule| {
                            let if_true = value_to_block_predicate(&rule["if_true"]);
                            let then = value_to_block_state_provider(&rule["then"]);
                            quote! { BlockStateRule { if_true: #if_true, then: #then } }
                        })
                        .collect()
                })
                .unwrap_or_default();
            quote! {
                BlockStateProvider::Rule(RuleBasedBlockStateProvider {
                    fallback: #fallback,
                    rules: vec![#(#rules),*],
                })
            }
        }
        _ => {
            // Default to air
            quote! {
                BlockStateProvider::Simple(SimpleStateProvider {
                    state: pumpkin_data::Block::AIR.default_state,
                })
            }
        }
    }
}

/// Converts a noise-provider JSON object into a `NoiseBlockStateProviderBase` token stream.
///
/// # Arguments
/// – `v` – the JSON object containing `"seed"`, `"noise"`, and `"scale"` fields.
fn value_to_noise_base(v: &Value) -> TokenStream {
    let seed = v["seed"].as_i64().unwrap_or(0);
    let noise = value_to_dpnp(&v["noise"]);
    let scale = v["scale"].as_f64().unwrap_or(1.0) as f32;
    quote! {
        NoiseBlockStateProviderBase { seed: #seed, noise: #noise, scale: #scale }
    }
}

/// Converts a double-perlin-noise parameters JSON object into a `DoublePerlinNoiseParametersCodec` token stream.
///
/// # Arguments
/// - `v` - the JSON object containing `"firstOctave"` and `"amplitudes"` fields.
fn value_to_dpnp(v: &Value) -> TokenStream {
    let first_octave = v["firstOctave"]
        .as_i64()
        .or_else(|| v["base_octave"].as_i64())
        .unwrap_or(-7) as i32;
    let amplitudes: Vec<f64> = v["amplitudes"]
        .as_array()
        .or_else(|| v["amplitude_modifiers"].as_array())
        .map(|a| a.iter().filter_map(|x| x.as_f64()).collect())
        .or_else(|| {
            v["octave_count"]
                .as_u64()
                .map(|n| vec![1.0; n.max(1) as usize])
        })
        .unwrap_or_else(|| vec![1.0]);
    quote! {
        DoublePerlinNoiseParametersCodec {
            first_octave: #first_octave,
            amplitudes: vec![#(#amplitudes),*],
        }
    }
}

/// Converts a rule-test JSON object into its `RuleTest` enum variant token stream.
///
/// # Arguments
/// – `v` – the JSON object for the rule test, expected to contain a `"predicate_type"` field.
///
/// # Returns
/// A `TokenStream` for the appropriate `RuleTest` variant; emits a `compile_error!` for unrecognised types.
fn value_to_rule_test(v: &Value) -> TokenStream {
    let type_str = v["predicate_type"].as_str().unwrap_or("");
    match type_str {
        "minecraft:always_true" | "" => quote! { RuleTest::AlwaysTrue },
        "minecraft:block_match" => {
            let block = v["block"].as_str().unwrap_or("minecraft:stone");
            let name_stripped = block.strip_prefix("minecraft:").unwrap_or(block);
            let block_ident =
                quote::format_ident!("{}", name_stripped.to_uppercase().replace([':', '-'], "_"));
            quote! { RuleTest::BlockMatch(BlockMatchRuleTest { block: pumpkin_data::BlockId::#block_ident }) }
        }
        "minecraft:blockstate_match" => {
            let state = value_to_block_state(&v["block_state"]);
            quote! { RuleTest::BlockStateMatch(BlockStateMatchRuleTest { block_state: #state.id }) }
        }
        "minecraft:tag_match" => {
            let tag = v["tag"].as_str().unwrap_or("");
            let tag_ident = quote::format_ident!("{}", tag.to_uppercase().replace([':', '-'], "_"));
            quote! { RuleTest::TagMatch(TagMatchRuleTest { tag: pumpkin_data::tag::Block::#tag_ident }) }
        }
        "minecraft:random_block_match" => {
            let block = v["block"].as_str().unwrap_or("minecraft:stone");
            let prob = v["probability"].as_f64().unwrap_or(0.5) as f32;
            let block_ident =
                quote::format_ident!("{}", block.to_uppercase().replace([':', '-'], "_"));
            quote! { RuleTest::RandomBlockMatch(RandomBlockMatchRuleTest { block: pumpkin_data::BlockId::#block_ident, probability: #prob }) }
        }
        "minecraft:random_blockstate_match" => {
            let state = value_to_block_state(&v["block_state"]);
            let prob = v["probability"].as_f64().unwrap_or(0.5) as f32;
            quote! { RuleTest::RandomBlockStateMatch(RandomBlockStateMatchRuleTest { block_state: #state.id, probability: #prob }) }
        }
        "minecraft:any_of" | "minecraft:all_of" => {
            let rules: Vec<TokenStream> = v["rules"]
                .as_array()
                .map(|rules| rules.iter().map(value_to_rule_test).collect())
                .unwrap_or_default();
            if type_str == "minecraft:any_of" {
                quote! { RuleTest::AnyOf(vec![#(#rules),*]) }
            } else {
                quote! { RuleTest::AllOf(vec![#(#rules),*]) }
            }
        }
        "minecraft:not" => {
            let rule = value_to_rule_test(&v["rule"]);
            quote! { RuleTest::Not(Box::new(#rule)) }
        }
        "minecraft:height_match" => {
            let min = v["min_inclusive"].as_i64().unwrap_or(i64::from(i32::MIN)) as i32;
            let max = v["max_inclusive"].as_i64().unwrap_or(i64::from(i32::MAX)) as i32;
            quote! { RuleTest::HeightMatch { min_inclusive: #min, max_inclusive: #max } }
        }
        other => {
            let msg = format!("unknown rule test: {other}");
            quote! { compile_error!(#msg) }
        }
    }
}

/// Converts a block-list JSON value (string or array) into a `BlockWrapper` token stream.
///
/// # Arguments
/// – `v` – a JSON string (single block) or array of strings (multiple blocks).
///
/// # Returns
/// `BlockWrapper::Single` for a string value or `BlockWrapper::Multi` for an array; defaults to `BlockWrapper::Single("")` for other types.
fn value_to_block_wrapper(v: &Value) -> TokenStream {
    match v {
        Value::String(s) => quote! { BlockWrapper::Single(#s.to_string()) },
        Value::Array(arr) => {
            let items: Vec<TokenStream> = arr
                .iter()
                .filter_map(|s| s.as_str().map(|s| quote! { #s.to_string() }))
                .collect();
            quote! { BlockWrapper::Multi(vec![#(#items),*]) }
        }
        _ => quote! { BlockWrapper::Single(String::new()) },
    }
}

/// Converts a tree-feature config JSON object into a `TreeFeature` token stream.
///
/// # Arguments
/// – `config` – the `"config"` sub-object of a `minecraft:tree` configured feature JSON entry.
fn value_to_tree_feature(config: &Value) -> TokenStream {
    let trunk = value_to_block_state_provider(&config["trunk_provider"]);
    let trunk_placer = value_to_trunk_placer(&config["trunk_placer"]);
    let foliage = value_to_block_state_provider(&config["foliage_provider"]);
    let foliage_placer = value_to_foliage_placer(&config["foliage_placer"]);
    let min_size = value_to_feature_size(&config["minimum_size"]);
    let ignore_vines = config["ignore_vines"].as_bool().unwrap_or(true);
    let below_trunk_provider = value_to_block_state_provider(&config["below_trunk_provider"]);
    let decorators: Vec<TokenStream> = config["decorators"]
        .as_array()
        .map(|arr| arr.iter().filter_map(value_to_tree_decorator).collect())
        .unwrap_or_default();
    let root_placer = match config.get("root_placer") {
        Some(v) if !v.is_null() => {
            let inner = value_to_root_placer(v);
            quote! { Some(#inner) }
        }
        _ => quote! { None },
    };
    quote! {
        TreeFeature {
            trunk_provider: #trunk,
            trunk_placer: #trunk_placer,
            foliage_provider: #foliage,
            foliage_placer: #foliage_placer,
            minimum_size: #min_size,
            ignore_vines: #ignore_vines,
            below_trunk_provider: #below_trunk_provider,
            decorators: vec![#(#decorators),*],
            root_placer: #root_placer,
        }
    }
}

fn value_to_root_placer(v: &Value) -> TokenStream {
    let type_str = v["type"].as_str().unwrap_or("");
    match type_str {
        "minecraft:mangrove_root_placer" => {
            let trunk_offset_y = value_to_int_provider(&v["trunk_offset_y"]);
            let root_provider = value_to_block_state_provider(&v["root_provider"]);
            let above = match v.get("above_root_placement") {
                Some(av) if !av.is_null() => {
                    let provider = value_to_block_state_provider(&av["above_root_provider"]);
                    let chance = av["above_root_placement_chance"].as_f64().unwrap_or(0.0) as f32;
                    quote! {
                        Some(AboveRootPlacement {
                            above_root_provider: #provider,
                            above_root_placement_chance: #chance,
                        })
                    }
                }
                _ => quote! { None },
            };
            let mrp = &v["mangrove_root_placement"];
            let can_grow_through = value_to_block_list(&mrp["can_grow_through"]);
            let muddy_roots_in = value_to_block_list(&mrp["muddy_roots_in"]);
            let muddy_roots_provider = value_to_block_state_provider(&mrp["muddy_roots_provider"]);
            let max_root_width = mrp["max_root_width"].as_i64().unwrap_or(8) as i32;
            let max_root_length = mrp["max_root_length"].as_i64().unwrap_or(15) as i32;
            let random_skew_chance = mrp["random_skew_chance"].as_f64().unwrap_or(0.0) as f32;
            quote! {
                RootPlacer::Mangrove(MangroveRootPlacer {
                    trunk_offset_y: #trunk_offset_y,
                    root_provider: #root_provider,
                    above_root_placement: #above,
                    mangrove_root_placement: MangroveRootPlacement {
                        can_grow_through: #can_grow_through,
                        muddy_roots_in: const { #muddy_roots_in },
                        muddy_roots_provider: #muddy_roots_provider,
                        max_root_width: #max_root_width,
                        max_root_length: #max_root_length,
                        random_skew_chance: #random_skew_chance,
                    },
                })
            }
        }
        other => {
            let msg = format!("Unknown root placer type: {other}");
            quote! { compile_error!(#msg) }
        }
    }
}

fn value_to_block_list(v: &Value) -> TokenStream {
    if let Some(tag) = v.as_str().and_then(|s| s.strip_prefix('#')) {
        let name = format!(
            "MINECRAFT_{}",
            tag.strip_prefix("minecraft:").unwrap_or(tag).to_uppercase()
        );
        let ident = syn::Ident::new(&name, proc_macro2::Span::call_site());
        return quote! { &pumpkin_data::tag::Block::#ident.1 };
    }
    let mut blocks = Vec::new();
    if let Some(arr) = v.as_array() {
        for b in arr {
            if let Some(s) = b.as_str() {
                let name = s.strip_prefix("minecraft:").unwrap_or(s).to_uppercase();
                let ident = syn::Ident::new(&name, proc_macro2::Span::call_site());
                blocks.push(quote! { pumpkin_data::BlockId::#ident.as_u16() });
            }
        }
    }
    quote! { &[#(#blocks),*] }
}

/// Converts a trunk-placer JSON object into a `TrunkPlacer` token stream.
///
/// # Arguments
/// – `v` – the `"trunk_placer"` sub-object from a tree config, containing `"type"`, `"base_height"`, `"height_rand_a"`, and `"height_rand_b"` fields.
fn value_to_trunk_placer(v: &Value) -> TokenStream {
    let base = v["base_height"].as_u64().unwrap_or(5) as u8;
    let rand_a = v["height_rand_a"].as_u64().unwrap_or(0) as u8;
    let rand_b = v["height_rand_b"].as_u64().unwrap_or(0) as u8;
    let type_str = v["type"].as_str().unwrap_or("");
    let trunk_type = match type_str {
        "minecraft:straight_trunk_placer" => quote! { TrunkType::Straight(StraightTrunkPlacer) },
        "minecraft:forking_trunk_placer" => quote! { TrunkType::Forking(ForkingTrunkPlacer) },
        "minecraft:giant_trunk_placer" => quote! { TrunkType::Giant(GiantTrunkPlacer) },
        "minecraft:mega_jungle_trunk_placer" => {
            quote! { TrunkType::MegaJungle(MegaJungleTrunkPlacer) }
        }
        "minecraft:dark_oak_trunk_placer" => quote! { TrunkType::DarkOak(DarkOakTrunkPlacer) },
        "minecraft:fancy_trunk_placer" => quote! { TrunkType::Fancy(FancyTrunkPlacer) },
        "minecraft:bending_trunk_placer" => {
            let min_h = v["min_height_for_leaves"].as_u64().unwrap_or(1) as u32;
            let bend = value_to_int_provider(&v["bend_length"]);
            quote! {
                TrunkType::Bending(BendingTrunkPlacer {
                    min_height_for_leaves: #min_h,
                    bend_length: #bend,
                })
            }
        }
        "minecraft:upwards_branching_trunk_placer" => {
            let extra_branch_steps = value_to_int_provider(&v["extra_branch_steps"]);
            let place_branch_per_log_probability = v["place_branch_per_log_probability"]
                .as_f64()
                .unwrap_or(0.0) as f32;
            let extra_branch_length = value_to_int_provider(&v["extra_branch_length"]);
            let can_grow_through = value_to_block_list(&v["can_grow_through"]);
            quote! {
                TrunkType::UpwardsBranching(UpwardsBranchingTrunkPlacer {
                    extra_branch_steps: #extra_branch_steps,
                    place_branch_per_log_probability: #place_branch_per_log_probability,
                    extra_branch_length: #extra_branch_length,
                    can_grow_through: #can_grow_through,
                })
            }
        }
        "minecraft:cherry_trunk_placer" => {
            let branch_count = value_to_int_provider(&v["branch_count"]);
            let branch_horizontal_length = value_to_int_provider(&v["branch_horizontal_length"]);
            let branch_start_offset_v = &v["branch_start_offset_from_top"];
            let min = branch_start_offset_v["min_inclusive"].as_i64().unwrap_or(0) as i32;
            let max = branch_start_offset_v["max_inclusive"].as_i64().unwrap_or(0) as i32;
            let branch_end_offset_from_top =
                value_to_int_provider(&v["branch_end_offset_from_top"]);
            quote! {
                TrunkType::Cherry(CherryTrunkPlacer {
                    count: #branch_count,
                    horizontal_length: #branch_horizontal_length,
                    start_offset_from_top: UniformIntProvider { min_inclusive: #min, max_inclusive: #max },
                    end_offset_from_top: #branch_end_offset_from_top,
                })
            }
        }
        "minecraft:poplar_trunk_placer" => {
            let above = value_to_int_provider(&v["trunk_height_above_branches"]);
            let branches = value_to_int_provider(&v["branch_amount"]);
            quote! {
                TrunkType::Poplar(PoplarTrunkPlacer {
                    trunk_height_above_branches: #above,
                    branch_amount: #branches,
                })
            }
        }
        _ => quote! { TrunkType::Straight(StraightTrunkPlacer) },
    };
    quote! {
        TrunkPlacer {
            base_height: #base,
            height_rand_a: #rand_a,
            height_rand_b: #rand_b,
            r#type: #trunk_type,
        }
    }
}

/// Converts a foliage-placer JSON object into a `FoliagePlacer` token stream.
///
/// # Arguments
/// – `v` – the `"foliage_placer"` sub-object from a tree config, containing `"type"`, `"radius"`, and `"offset"` fields.
fn value_to_foliage_placer(v: &Value) -> TokenStream {
    let radius = value_to_int_provider(&v["radius"]);
    let offset = value_to_int_provider(&v["offset"]);
    let type_str = v["type"].as_str().unwrap_or("");
    let foliage_type = match type_str {
        "minecraft:blob_foliage_placer" => {
            let h = v["height"].as_i64().unwrap_or(3) as i32;
            quote! { FoliageType::Blob(BlobFoliagePlacer { height: #h }) }
        }
        "minecraft:spruce_foliage_placer" => {
            let th = value_to_int_provider(&v["trunk_height"]);
            quote! { FoliageType::Spruce(SpruceFoliagePlacer { trunk_height: #th }) }
        }
        "minecraft:pine_foliage_placer" => {
            let h = value_to_int_provider(&v["height"]);
            quote! { FoliageType::Pine(PineFoliagePlacer { height: #h }) }
        }
        "minecraft:acacia_foliage_placer" => {
            quote! { FoliageType::Acacia(AcaciaFoliagePlacer) }
        }
        "minecraft:bush_foliage_placer" => {
            let h = v["height"].as_i64().unwrap_or(2) as i32;
            quote! { FoliageType::Bush(BushFoliagePlacer { height: #h }) }
        }
        "minecraft:fancy_foliage_placer" => {
            let h = v["height"].as_i64().unwrap_or(4) as i32;
            quote! { FoliageType::Fancy(LargeOakFoliagePlacer { height: #h }) }
        }
        "minecraft:jungle_foliage_placer" => {
            let h = v["height"].as_i64().unwrap_or(2) as i32;
            quote! { FoliageType::Jungle(JungleFoliagePlacer { height: #h }) }
        }
        "minecraft:mega_pine_foliage_placer" => {
            let ch = value_to_int_provider(&v["crown_height"]);
            quote! { FoliageType::MegaPine(MegaPineFoliagePlacer { crown_height: #ch }) }
        }
        "minecraft:dark_oak_foliage_placer" => {
            quote! { FoliageType::DarkOak(DarkOakFoliagePlacer) }
        }
        "minecraft:random_spread_foliage_placer" => {
            let fh = value_to_int_provider(&v["foliage_height"]);
            let lpa = v["leaf_placement_attempts"].as_i64().unwrap_or(128) as i32;
            quote! {
                FoliageType::RandomSpread(RandomSpreadFoliagePlacer {
                    foliage_height: #fh,
                    leaf_placement_attempts: #lpa,
                })
            }
        }
        "minecraft:cherry_foliage_placer" => {
            let h = value_to_int_provider(&v["height"]);
            let wblh = v["wide_bottom_layer_hole_chance"].as_f64().unwrap_or(0.0) as f32;
            let ch = v["corner_hole_chance"].as_f64().unwrap_or(0.0) as f32;
            let hlc = v["hanging_leaves_chance"].as_f64().unwrap_or(0.0) as f32;
            let hlec = v["hanging_leaves_extension_chance"].as_f64().unwrap_or(0.0) as f32;
            quote! {
                FoliageType::Cherry(CherryFoliagePlacer {
                    height: #h,
                    wide_bottom_layer_hole_chance: #wblh,
                    corner_hole_chance: #ch,
                    hanging_leaves_chance: #hlc,
                    hanging_leaves_extension_chance: #hlec,
                })
            }
        }
        "minecraft:poplar_foliage_placer" => {
            let height = value_to_int_provider(&v["height"]);
            let side_hole_chance = v["side_hole_chance"].as_f64().unwrap_or(0.0) as f32;
            quote! {
                FoliageType::Poplar(PoplarFoliagePlacer {
                    height: #height,
                    side_hole_chance: #side_hole_chance,
                })
            }
        }
        _ => {
            let h = v["height"].as_i64().unwrap_or(3) as i32;
            quote! { FoliageType::Blob(BlobFoliagePlacer { height: #h }) }
        }
    };
    quote! {
        FoliagePlacer { radius: #radius, offset: #offset, r#type: #foliage_type }
    }
}

/// Converts a feature-size JSON object into a `FeatureSize` token stream.
///
/// # Arguments
/// – `v` – the `"minimum_size"` sub-object from a tree config, containing a `"type"` field and size parameters.
fn value_to_feature_size(v: &Value) -> TokenStream {
    let min_clipped = if v["min_clipped_height"].is_number() {
        let val = v["min_clipped_height"].as_u64().unwrap_or(0) as u8;
        quote! { Some(#val) }
    } else {
        quote! { None }
    };
    let type_str = v["type"].as_str().unwrap_or("");
    let size_type = match type_str {
        "minecraft:two_layers_feature_size" => {
            let limit = v["limit"].as_u64().unwrap_or(1) as u8;
            let lower = v["lower_size"].as_u64().unwrap_or(0) as u8;
            let upper = v["upper_size"].as_u64().unwrap_or(1) as u8;
            quote! {
                FeatureSizeType::TwoLayersFeatureSize(TwoLayersFeatureSize {
                    limit: #limit,
                    lower_size: #lower,
                    upper_size: #upper,
                })
            }
        }
        "minecraft:three_layers_feature_size" => {
            let limit = v["limit"].as_u64().unwrap_or(1) as u8;
            let upper_limit = v["upper_limit"].as_u64().unwrap_or(1) as u8;
            let lower = v["lower_size"].as_u64().unwrap_or(0) as u8;
            let middle = v["middle_size"].as_u64().unwrap_or(1) as u8;
            let upper = v["upper_size"].as_u64().unwrap_or(1) as u8;
            quote! {
                FeatureSizeType::ThreeLayersFeatureSize(ThreeLayersFeatureSize {
                    limit: #limit,
                    upper_limit: #upper_limit,
                    lower_size: #lower,
                    middle_size: #middle,
                    upper_size: #upper,
                })
            }
        }
        _ => {
            quote! {
                FeatureSizeType::TwoLayersFeatureSize(TwoLayersFeatureSize {
                    limit: 1,
                    lower_size: 0,
                    upper_size: 1,
                })
            }
        }
    };
    quote! {
        FeatureSize { min_clipped_height: #min_clipped, r#type: #size_type }
    }
}

/// Converts a tree-decorator JSON object into its `TreeDecorator` enum variant token stream.
///
/// # Arguments
/// – `v` – a JSON object for one decorator entry, expected to contain a `"type"` field.
///
/// # Returns
/// A `TokenStream` for the appropriate `TreeDecorator` variant; emits a `compile_error!` for unrecognised types.
fn value_to_tree_decorator(v: &Value) -> Option<TokenStream> {
    let type_str = v["type"].as_str().unwrap_or("");
    Some(match type_str {
        "minecraft:trunk_vine" => quote! { TreeDecorator::TrunkVine(TrunkVineTreeDecorator) },
        "minecraft:leave_vine" => {
            let prob = v["probability"].as_f64().unwrap_or(0.0) as f32;
            quote! { TreeDecorator::LeaveVine(LeavesVineTreeDecorator { probability: #prob }) }
        }
        "minecraft:cocoa" => {
            let prob = v["probability"].as_f64().unwrap_or(0.0) as f32;
            quote! { TreeDecorator::Cocoa(CocoaTreeDecorator { probability: #prob }) }
        }
        "minecraft:beehive" => {
            let prob = v["probability"].as_f64().unwrap_or(0.0) as f32;
            quote! { TreeDecorator::Beehive(BeehiveTreeDecorator { probability: #prob }) }
        }
        "minecraft:alter_ground" => {
            let provider = value_to_block_state_provider(&v["provider"]);
            quote! { TreeDecorator::AlterGround(AlterGroundTreeDecorator { provider: #provider }) }
        }
        "minecraft:attached_to_logs" => {
            let prob = v["probability"].as_f64().unwrap_or(0.0) as f32;
            let bp = value_to_block_state_provider(&v["block_provider"]);
            let dirs: Vec<TokenStream> = v["directions"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|d| d.as_str().map(value_to_block_direction))
                        .collect()
                })
                .unwrap_or_default();
            quote! {
                TreeDecorator::AttachedToLogs(AttachedToLogsTreeDecorator {
                    probability: #prob,
                    block_provider: #bp,
                    directions: vec![#(#dirs),*],
                })
            }
        }
        "minecraft:attached_to_leaves" => {
            let prob = v["probability"].as_f64().unwrap_or(0.0) as f32;
            let exclusion_radius_xz = v["exclusion_radius_xz"].as_i64().unwrap_or(0) as i32;
            let exclusion_radius_y = v["exclusion_radius_y"].as_i64().unwrap_or(0) as i32;
            let bp = value_to_block_state_provider(&v["block_provider"]);
            let required_empty_blocks = v["required_empty_blocks"].as_i64().unwrap_or(1) as i32;
            let dirs: Vec<TokenStream> = v["directions"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|d| d.as_str().map(value_to_block_direction))
                        .collect()
                })
                .unwrap_or_default();
            quote! {
                TreeDecorator::AttachedToLeaves(AttachedToLeavesTreeDecorator {
                    probability: #prob,
                    exclusion_radius_xz: #exclusion_radius_xz,
                    exclusion_radius_y: #exclusion_radius_y,
                    block_provider: #bp,
                    required_empty_blocks: #required_empty_blocks,
                    directions: vec![#(#dirs),*],
                })
            }
        }
        "minecraft:place_on_ground" => {
            let tries = v["tries"].as_i64().unwrap_or(128) as i32;
            let radius = v["radius"].as_i64().unwrap_or(2) as i32;
            let height = v["height"].as_i64().unwrap_or(1) as i32;
            let bsp = value_to_block_state_provider(&v["block_state_provider"]);
            quote! {
                TreeDecorator::PlaceOnGround(PlaceOnGroundTreeDecorator {
                    tries: #tries,
                    radius: #radius,
                    height: #height,
                    block_state_provider: #bsp,
                })
            }
        }
        "minecraft:creaking_heart" => {
            let prob = v["probability"].as_f64().unwrap_or(0.0) as f32;
            quote! { TreeDecorator::CreakingHeart(CreakingHeartTreeDecorator { probability: #prob }) }
        }
        "minecraft:pale_moss" => {
            let leaves_probability = v["leaves_probability"].as_f64().unwrap_or(0.0) as f32;
            let trunk_probability = v["trunk_probability"].as_f64().unwrap_or(0.0) as f32;
            let ground_probability = v["ground_probability"].as_f64().unwrap_or(0.0) as f32;
            quote! {
                TreeDecorator::PaleMoss(PaleMossTreeDecorator {
                    leaves_probability: #leaves_probability,
                    trunk_probability: #trunk_probability,
                    ground_probability: #ground_probability,
                })
            }
        }
        "minecraft:shelf_mushroom" => return None,
        other => {
            let msg = format!("unknown tree decorator: {other}");
            quote! { compile_error!(#msg) }
        }
    })
}

/// Converts an inline placed-feature JSON object into a `PlacedFeature` token stream.
///
/// # Arguments
/// – `v` – the JSON object containing `"feature"` and `"placement"` fields.
fn value_to_inline_placed_feature(v: &Value) -> TokenStream {
    let feature = value_to_feature_ref(&v["feature"]);
    let placement_arr = v["placement"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    let placement: Vec<TokenStream> = placement_arr
        .iter()
        .map(value_to_placement_modifier_cf)
        .collect();
    quote! {
        PlacedFeature {
            feature: #feature,
            placement: vec![#(#placement),*],
        }
    }
}

/// Converts a placed-feature reference JSON value into a `PlacedFeatureWrapper` token stream.
///
/// # Arguments
/// – `v` – a JSON string (named reference) or object (inline placed feature).
///
/// # Returns
/// `PlacedFeatureWrapper::Named` for a string value or `PlacedFeatureWrapper::Direct` for an inline object; defaults to `PlacedFeatureWrapper::Named(pumpkin_data::placed_feature::PlacedFeature::Acacia)` for other types.
fn value_to_placed_feature_wrapper(v: &Value) -> TokenStream {
    match v {
        Value::String(s) => {
            let name = s.strip_prefix("minecraft:").unwrap_or(s);
            let variant_name = format_ident!("{}", name.to_pascal_case());
            quote! { PlacedFeatureWrapper::Named(pumpkin_data::placed_feature::PlacedFeature::#variant_name) }
        }
        Value::Object(_) => {
            // It might be a PlacedFeature object
            let pf = value_to_inline_placed_feature(v);
            quote! { PlacedFeatureWrapper::Direct(#pf) }
        }
        _ => {
            quote! { PlacedFeatureWrapper::Named(pumpkin_data::placed_feature::PlacedFeature::Acacia) }
        }
    }
}

/// Converts a feature reference JSON value into a `Feature` enum token stream.
///
/// # Arguments
/// – `v` – a JSON string (named reference) or object (inline configured feature).
///
/// # Returns
/// `Feature::Named` for a string value or `Feature::Inlined` for an object; defaults to `Feature::Named(pumpkin_data::configured_feature::ConfiguredFeature::NoOp)` for other types.
fn value_to_feature_ref(v: &Value) -> TokenStream {
    match v {
        Value::String(s) => {
            let name = s.strip_prefix("minecraft:").unwrap_or(s);
            let variant_name = format_ident!("{}", name.to_pascal_case());
            quote! { Feature::Named(pumpkin_data::configured_feature::ConfiguredFeature::#variant_name) }
        }
        Value::Object(_) => {
            let cf = value_to_configured_feature(v);
            quote! { Feature::Inlined(Box::new(#cf)) }
        }
        _ => quote! { Feature::Named(pumpkin_data::configured_feature::ConfiguredFeature::NoOp) },
    }
}

// Placement modifier in context of configured_features (same as placed_feature but re-imported)
/// Converts a placement-modifier JSON object into its `PlacementModifier` token stream within the configured-feature context.
///
/// # Arguments
/// – `v` – a JSON object for one modifier entry, expected to contain a `"type"` field.
///
/// # Returns
/// A `TokenStream` for the appropriate `PlacementModifier` variant; defaults to `PlacementModifier::Biome` for unrecognised types.
fn value_to_placement_modifier_cf(v: &Value) -> TokenStream {
    let type_str = v["type"].as_str().unwrap_or("");
    match type_str {
        "minecraft:biome" => quote! { PlacementModifier::Biome(BiomePlacementModifier) },
        "minecraft:in_square" => quote! { PlacementModifier::InSquare(SquarePlacementModifier) },
        "minecraft:fixed_placement" => {
            let positions = v["positions"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|p| {
                            let coords = p.as_array().unwrap();
                            let x = coords[0].as_i64().unwrap_or(0) as i32;
                            let y = coords[1].as_i64().unwrap_or(0) as i32;
                            let z = coords[2].as_i64().unwrap_or(0) as i32;
                            quote! { BlockPos::new(#x, #y, #z) }
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            quote! { PlacementModifier::FixedPlacement(vec![#(#positions),*]) }
        }
        "minecraft:heightmap" => {
            let hm = crate::placed_feature::value_to_height_map(
                v["heightmap"].as_str().unwrap_or("MOTION_BLOCKING"),
            );
            quote! { PlacementModifier::Heightmap(HeightmapPlacementModifier { heightmap: #hm }) }
        }
        "minecraft:height_range" => {
            let h = value_to_height_provider(&v["height"]);
            quote! { PlacementModifier::HeightRange(HeightRangePlacementModifier { height: #h }) }
        }
        "minecraft:count" => {
            let c = value_to_int_provider(&v["count"]);
            quote! { PlacementModifier::Count(CountPlacementModifier { count: #c }) }
        }
        "minecraft:count_on_every_layer" => {
            let c = value_to_int_provider(&v["count"]);
            quote! { PlacementModifier::CountOnEveryLayer(CountOnEveryLayerPlacementModifier { count: #c }) }
        }
        "minecraft:rarity_filter" => {
            let ch = v["chance"].as_u64().unwrap_or(1) as u32;
            quote! { PlacementModifier::RarityFilter(RarityFilterPlacementModifier { chance: #ch }) }
        }
        "minecraft:block_predicate_filter" => {
            let pred = value_to_block_predicate(&v["predicate"]);
            quote! { PlacementModifier::BlockPredicateFilter(BlockFilterPlacementModifier { predicate: #pred }) }
        }
        "minecraft:surface_relative_threshold_filter" => {
            let hm = crate::placed_feature::value_to_height_map(
                v["heightmap"].as_str().unwrap_or("MOTION_BLOCKING"),
            );
            let mn = if v["min_inclusive"].is_number() {
                let x = v["min_inclusive"].as_i64().unwrap() as i32;
                quote! {Some(#x)}
            } else {
                quote! {None}
            };
            let mx = if v["max_inclusive"].is_number() {
                let x = v["max_inclusive"].as_i64().unwrap() as i32;
                quote! {Some(#x)}
            } else {
                quote! {None}
            };
            quote! { PlacementModifier::SurfaceRelativeThresholdFilter(SurfaceThresholdFilterPlacementModifier { heightmap: #hm, min_inclusive: #mn, max_inclusive: #mx }) }
        }
        "minecraft:surface_water_depth_filter" => {
            let d = v["max_water_depth"].as_i64().unwrap_or(0) as i32;
            quote! { PlacementModifier::SurfaceWaterDepthFilter(SurfaceWaterDepthFilterPlacementModifier { max_water_depth: #d }) }
        }
        "minecraft:random_offset" => {
            let xz = value_to_int_provider(&v["xz_spread"]);
            let y = value_to_int_provider(&v["y_spread"]);
            quote! { PlacementModifier::Offset(OffsetPlacementModifier { x: #xz, y: #y, z: #xz }) }
        }
        "minecraft:offset" => {
            let x = value_to_int_provider(&v["x"]);
            let y = value_to_int_provider(&v["y"]);
            let z = value_to_int_provider(&v["z"]);
            quote! { PlacementModifier::Offset(OffsetPlacementModifier { x: #x, y: #y, z: #z }) }
        }
        "minecraft:noise_based_count" => {
            let r = v["noise_to_count_ratio"].as_i64().unwrap_or(0) as i32;
            let f = v["noise_factor"].as_f64().unwrap_or(1.0);
            let o = v["noise_offset"].as_f64().unwrap_or(0.0);
            quote! { PlacementModifier::NoiseBasedCount(NoiseBasedCountPlacementModifier { to_count_ratio: #r, factor: #f, offset: #o }) }
        }
        "minecraft:noise_threshold_count" => {
            let l = v["noise_level"].as_f64().unwrap_or(0.0);
            let b = v["below_noise"].as_i64().unwrap_or(0) as i32;
            let a = v["above_noise"].as_i64().unwrap_or(0) as i32;
            quote! { PlacementModifier::NoiseThresholdCount(NoiseThresholdCountPlacementModifier { noise_level: #l, below_noise: #b, above_noise: #a }) }
        }
        "minecraft:environment_scan" => {
            let dir = value_to_block_direction(v["direction_of_search"].as_str().unwrap_or("down"));
            let tc = value_to_block_predicate(&v["target_condition"]);
            let asc = if v["allowed_search_condition"].is_object() {
                let p = value_to_block_predicate(&v["allowed_search_condition"]);
                quote! { Some(#p) }
            } else {
                quote! { None }
            };
            let steps = v["max_steps"].as_i64().unwrap_or(1) as i32;
            quote! { PlacementModifier::EnvironmentScan(EnvironmentScanPlacementModifier { direction_of_search: #dir, target_condition: #tc, allowed_search_condition: #asc, max_steps: #steps }) }
        }
        _ => quote! { PlacementModifier::Biome(BiomePlacementModifier) },
    }
}
