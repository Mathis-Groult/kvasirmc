//! Configured features that place vegetation, and the selectors that choose between placed features.

use super::{
    value_to_block_state_provider, value_to_inline_placed_feature, value_to_placed_feature_wrapper,
    value_to_tree_decorator, value_to_tree_feature,
};
use crate::placed_feature::{
    int_or_uniform, value_to_block_direction, value_to_block_predicate, value_to_block_state,
    value_to_block_state_codec, value_to_height_provider, value_to_int_provider,
};
use heck::ToPascalCase;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub(super) fn feature_bamboo(config: &Value) -> TokenStream {
    let prob = config["probability"].as_f64().unwrap_or(0.0) as f32;
    quote! { ConfiguredFeature::Bamboo(BambooFeature { probability: #prob }) }
}

pub(super) fn feature_seagrass(config: &Value) -> TokenStream {
    let prob = config["probability"].as_f64().unwrap_or(0.0) as f32;
    quote! { ConfiguredFeature::Seagrass(SeagrassFeature { probability: #prob }) }
}

pub(super) fn feature_sea_pickle(config: &Value) -> TokenStream {
    let count = value_to_int_provider(&config["count"]);
    quote! { ConfiguredFeature::SeaPickle(SeaPickleFeature { count: #count }) }
}

pub(super) fn feature_nether_forest_vegetation(config: &Value) -> TokenStream {
    let provider = value_to_block_state_provider(&config["state_provider"]);
    let w = config["spread_width"].as_i64().unwrap_or(8) as i32;
    let h = config["spread_height"].as_i64().unwrap_or(4) as i32;
    quote! {
        ConfiguredFeature::NetherForestVegetation(NetherForestVegetationFeature {
            state_provider: #provider,
            spread_width: #w,
            spread_height: #h,
        })
    }
}

pub(super) fn feature_block_column(config: &Value) -> TokenStream {
    let layers: Vec<TokenStream> = config["layers"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|l| {
                    let h = value_to_int_provider(&l["height"]);
                    let p = value_to_block_state_provider(&l["provider"]);
                    quote! { Layer { height: #h, provider: #p } }
                })
                .collect()
        })
        .unwrap_or_default();
    let dir = value_to_block_direction(config["direction"].as_str().unwrap_or("up"));
    let allowed = value_to_block_predicate(&config["allowed_placement"]);
    let tip = config["prioritize_tip"].as_bool().unwrap_or(false);
    quote! {
        ConfiguredFeature::BlockColumn(BlockColumnFeature {
            layers: vec![#(#layers),*],
            direction: #dir,
            allowed_placement: #allowed,
            prioritize_tip: #tip,
        })
    }
}

pub(super) fn feature_fallen_tree(config: &Value) -> TokenStream {
    let trunk = value_to_block_state_provider(&config["trunk_provider"]);
    let log_length = value_to_int_provider(&config["log_length"]);
    let stump_decorators: Vec<TokenStream> = config["stump_decorators"]
        .as_array()
        .map(|arr| arr.iter().filter_map(value_to_tree_decorator).collect())
        .unwrap_or_default();
    let log_decorators: Vec<TokenStream> = config["log_decorators"]
        .as_array()
        .map(|arr| arr.iter().filter_map(value_to_tree_decorator).collect())
        .unwrap_or_default();
    quote! {
        ConfiguredFeature::FallenTree(FallenTreeFeature {
            trunk_provider: #trunk,
            log_length: #log_length,
            stump_decorators: vec![#(#stump_decorators),*],
            log_decorators: vec![#(#log_decorators),*],
        })
    }
}

pub(super) fn feature_random_patch(config: &Value, type_str: &str) -> TokenStream {
    let tries = config["tries"].as_u64().unwrap_or(128) as u8;
    let xz = config["xz_spread"].as_u64().unwrap_or(7) as u8;
    let y = config["y_spread"].as_u64().unwrap_or(3) as u8;
    let feature = value_to_inline_placed_feature(&config["feature"]);
    let variant = match type_str {
        "minecraft:flower" => quote! { ConfiguredFeature::Flower },
        "minecraft:no_bonemeal_flower" => quote! { ConfiguredFeature::NoBonemealFlower },
        _ => quote! { ConfiguredFeature::RandomPatch },
    };
    quote! {
        #variant(RandomPatchFeature {
            tries: #tries,
            xz_spread: #xz,
            y_spread: #y,
            feature: Box::new(#feature),
        })
    }
}

pub(super) fn feature_random_selector(config: &Value) -> TokenStream {
    let entries: Vec<TokenStream> = config["features"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|e| {
                    let feat = value_to_placed_feature_wrapper(&e["feature"]);
                    let chance = e["chance"].as_f64().unwrap_or(0.1) as f32;
                    quote! { RandomFeatureEntry { feature: #feat, chance: #chance } }
                })
                .collect()
        })
        .unwrap_or_default();
    let default = value_to_placed_feature_wrapper(&config["default"]);
    quote! {
        ConfiguredFeature::RandomSelector(RandomFeature {
            features: vec![#(#entries),*],
            default: Box::new(#default),
        })
    }
}

pub(super) fn feature_weighted_random_selector(config: &Value) -> TokenStream {
    let weights: Vec<i32> = config["features"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|e| e["weight"].as_i64().unwrap_or(1).max(0) as i32)
                .collect()
        })
        .unwrap_or_default();
    let total_weight: i32 = weights.iter().sum();
    let entries: Vec<TokenStream> = config["features"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .zip(&weights)
                .map(|(e, weight)| {
                    let feat = value_to_placed_feature_wrapper(&e["data"]);
                    quote! { WeightedRandomFeatureEntry { feature: #feat, weight: #weight } }
                })
                .collect()
        })
        .unwrap_or_default();
    quote! {
        ConfiguredFeature::WeightedRandomSelector(WeightedRandomFeature {
            features: vec![#(#entries),*],
            total_weight: #total_weight,
        })
    }
}

pub(super) fn feature_simple_random_selector(config: &Value) -> TokenStream {
    let features: Vec<TokenStream> = config["features"]
        .as_array()
        .map(|arr| arr.iter().map(value_to_inline_placed_feature).collect())
        .unwrap_or_default();
    quote! {
        ConfiguredFeature::SimpleRandomSelector(SimpleRandomFeature {
            features: vec![#(#features),*],
        })
    }
}

pub(super) fn feature_random_boolean_selector(config: &Value) -> TokenStream {
    let ft = value_to_placed_feature_wrapper(&config["feature_true"]);
    let ff = value_to_placed_feature_wrapper(&config["feature_false"]);
    quote! {
        ConfiguredFeature::RandomBooleanSelector(RandomBooleanFeature {
            feature_true: Box::new(#ft),
            feature_false: Box::new(#ff),
        })
    }
}

pub(super) fn feature_tree(config: &Value) -> TokenStream {
    let tree = value_to_tree_feature(config);
    quote! { ConfiguredFeature::Tree(Box::new(#tree)) }
}

pub(super) fn feature_vegetation_patch(config: &Value, type_str: &str) -> TokenStream {
    let replaceable = value_to_block_predicate(&config["replaceable"]);
    let ground_state = value_to_block_state_provider(&config["ground_state"]);
    let vegetation_feature = value_to_inline_placed_feature(&config["vegetation_feature"]);
    let surface = match config["surface"].as_str().unwrap_or("floor") {
        "ceiling" => {
            quote! { pumpkin_util::math::vertical_surface_type::VerticalSurfaceType::Ceiling }
        }
        _ => {
            quote! { pumpkin_util::math::vertical_surface_type::VerticalSurfaceType::Floor }
        }
    };
    let depth = value_to_int_provider(&config["depth"]);
    let extra_bottom = config["extra_bottom_block_chance"].as_f64().unwrap_or(0.0) as f32;
    let vert_range = config["vertical_range"].as_i64().unwrap_or(0) as i32;
    let veg_chance = config["vegetation_chance"].as_f64().unwrap_or(0.0) as f32;
    let xz = value_to_int_provider(&config["xz_radius"]);
    let extra_edge = config["extra_edge_column_chance"].as_f64().unwrap_or(0.0) as f32;

    if type_str == "minecraft:vegetation_patch" {
        quote! {
            ConfiguredFeature::VegetationPatch(vegetation_patch::VegetationPatchFeature {
                replaceable: #replaceable,
                ground_state: #ground_state,
                vegetation_feature: Box::new(#vegetation_feature),
                surface: #surface,
                depth: #depth,
                extra_bottom_block_chance: #extra_bottom,
                vertical_range: #vert_range,
                vegetation_chance: #veg_chance,
                xz_radius: #xz,
                extra_edge_column_chance: #extra_edge,
            })
        }
    } else {
        quote! {
            ConfiguredFeature::WaterloggedVegetationPatch(waterlogged_vegetation_patch::WaterloggedVegetationPatchFeature {
                base: vegetation_patch::VegetationPatchFeature {
                    replaceable: #replaceable,
                    ground_state: #ground_state,
                    vegetation_feature: Box::new(#vegetation_feature),
                    surface: #surface,
                    depth: #depth,
                    extra_bottom_block_chance: #extra_bottom,
                    vertical_range: #vert_range,
                    vegetation_chance: #veg_chance,
                    xz_radius: #xz,
                    extra_edge_column_chance: #extra_edge,
                }
            })
        }
    }
}

pub(super) fn feature_chorus_plant() -> TokenStream {
    quote! { ConfiguredFeature::ChorusPlant(crate::generation::feature::features::chorus_plant::ChorusPlantFeature {}) }
}

pub(super) fn feature_kelp() -> TokenStream {
    quote! { ConfiguredFeature::Kelp(crate::generation::feature::features::kelp::KelpFeature {}) }
}

pub(super) fn feature_huge_brown_mushroom() -> TokenStream {
    quote! { ConfiguredFeature::HugeBrownMushroom(crate::generation::feature::features::huge_brown_mushroom::HugeBrownMushroomFeature {}) }
}

pub(super) fn feature_huge_red_mushroom() -> TokenStream {
    quote! { ConfiguredFeature::HugeRedMushroom(crate::generation::feature::features::huge_red_mushroom::HugeRedMushroomFeature {}) }
}

pub(super) fn feature_vines() -> TokenStream {
    quote! { ConfiguredFeature::Vines(crate::generation::feature::features::vines::VinesFeature) }
}

pub(super) fn feature_root_system(config: &Value) -> TokenStream {
    let feature = value_to_inline_placed_feature(&config["feature"]);
    let required_vertical_space_for_tree = config["required_vertical_space_for_tree"]
        .as_i64()
        .unwrap_or(0) as i32;
    let root_radius = config["root_radius"].as_i64().unwrap_or(0) as i32;
    let root_replaceable = value_to_block_predicate(&config["root_replaceable"]);
    let root_state_provider = value_to_block_state_provider(&config["root_state_provider"]);
    let root_placement_attempts = config["root_placement_attempts"].as_i64().unwrap_or(0) as i32;
    let root_column_max_height = config["root_column_max_height"].as_i64().unwrap_or(0) as i32;
    let hanging_root_radius = config["hanging_root_radius"].as_i64().unwrap_or(0) as i32;
    let hanging_roots_vertical_span = config["hanging_roots_vertical_span"]
        .as_i64()
        .or(config["hanging_root_vertical_span"].as_i64())
        .unwrap_or(0) as i32;
    let hanging_root_state_provider =
        value_to_block_state_provider(&config["hanging_root_state_provider"]);
    let hanging_root_placement_attempts = config["hanging_root_placement_attempts"]
        .as_i64()
        .unwrap_or(0) as i32;
    let allowed_vertical_water_for_tree = config["allowed_vertical_water_for_tree"]
        .as_i64()
        .unwrap_or(0) as i32;
    let allowed_tree_position = value_to_block_predicate(&config["allowed_tree_position"]);

    quote! {
        ConfiguredFeature::RootSystem(crate::generation::feature::features::root_system::RootSystemFeature {
            feature: Box::new(#feature),
            required_vertical_space_for_tree: #required_vertical_space_for_tree,
            root_radius: #root_radius,
            root_replaceable: #root_replaceable,
            root_state_provider: #root_state_provider,
            root_placement_attempts: #root_placement_attempts,
            root_column_max_height: #root_column_max_height,
            hanging_root_radius: #hanging_root_radius,
            hanging_roots_vertical_span: #hanging_roots_vertical_span,
            hanging_root_state_provider: #hanging_root_state_provider,
            hanging_root_placement_attempts: #hanging_root_placement_attempts,
            allowed_vertical_water_for_tree: #allowed_vertical_water_for_tree,
            allowed_tree_position: #allowed_tree_position,
        })
    }
}

pub(super) fn feature_multiface_growth(v: &Value, config: &Value) -> TokenStream {
    let block_name = config["block"].as_str().unwrap_or("minecraft:glow_lichen");
    let stripped = block_name.strip_prefix("minecraft:").unwrap_or(block_name);
    let block_ident = quote::format_ident!("{}", stripped.to_uppercase().replace([':', '-'], "_"));
    let search_range = config["search_range"].as_i64().unwrap_or(10) as i32;
    let can_place_on_floor = config["can_place_on_floor"].as_bool().unwrap_or(false);
    let can_place_on_ceiling = config["can_place_on_ceiling"].as_bool().unwrap_or(false);
    let can_place_on_wall = config["can_place_on_wall"].as_bool().unwrap_or(false);
    let chance_of_spreading = config["chance_of_spreading"].as_f64().unwrap_or(0.5) as f32;
    let can_be_placed_on: Vec<TokenStream> = config["can_be_placed_on"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|name| {
                    let stripped = name.strip_prefix("minecraft:").unwrap_or(name);
                    let ident = quote::format_ident!(
                        "{}",
                        stripped.to_uppercase().replace([':', '-'], "_")
                    );
                    quote! { pumpkin_data::BlockId::#ident }
                })
                .collect()
        })
        .unwrap_or_default();
    quote! {
        ConfiguredFeature::MultifaceGrowth(crate::generation::feature::features::multiface_growth::MultifaceGrowthFeature {
            place_block: pumpkin_data::BlockId::#block_ident,
            search_range: #search_range,
            can_place_on_floor: #can_place_on_floor,
            can_place_on_ceiling: #can_place_on_ceiling,
            can_place_on_wall: #can_place_on_wall,
            chance_of_spreading: #chance_of_spreading,
            can_be_placed_on: vec![#(#can_be_placed_on),*],
        })
    }
}

pub(super) fn feature_coral_tree() -> TokenStream {
    quote! { ConfiguredFeature::CoralTree(crate::generation::feature::features::coral::coral_tree::CoralTreeFeature) }
}

pub(super) fn feature_coral_mushroom() -> TokenStream {
    quote! { ConfiguredFeature::CoralMushroom(crate::generation::feature::features::coral::coral_mushroom::CoralMushroomFeature) }
}

pub(super) fn feature_coral_claw() -> TokenStream {
    quote! { ConfiguredFeature::CoralClaw(crate::generation::feature::features::coral::coral_claw::CoralClawFeature) }
}

pub(super) fn feature_huge_fungus(config: &Value) -> TokenStream {
    let valid_base_block = value_to_block_state(&config["valid_base_block"]);
    let stem_state = value_to_block_state(&config["stem_state"]);
    let hat_state = value_to_block_state(&config["hat_state"]);
    let decor_state = value_to_block_state(&config["decor_state"]);
    let replaceable_blocks = value_to_block_predicate(&config["replaceable_blocks"]);
    let planted = config["planted"].as_bool().unwrap_or(false);
    quote! {
        ConfiguredFeature::HugeFungus(crate::generation::feature::features::huge_fungus::HugeFungusFeature {
            valid_base_block: #valid_base_block,
            stem_state: #stem_state,
            hat_state: #hat_state,
            decor_state: #decor_state,
            replaceable_blocks: #replaceable_blocks,
            planted: #planted,
        })
    }
}

pub(super) fn feature_weeping_vines() -> TokenStream {
    quote! { ConfiguredFeature::WeepingVines(crate::generation::feature::features::weeping_vines::WeepingVinesFeature {}) }
}

pub(super) fn feature_twisting_vines(config: &Value) -> TokenStream {
    let spread_width = config["spread_width"].as_i64().unwrap_or(0) as i32;
    let spread_height = config["spread_height"].as_i64().unwrap_or(0) as i32;
    let max_height = config["max_height"].as_i64().unwrap_or(0) as i32;
    quote! {
        ConfiguredFeature::TwistingVines(crate::generation::feature::features::twisting_vines::TwistingVinesFeature {
            spread_width: #spread_width,
            spread_height: #spread_height,
            max_height: #max_height,
        })
    }
}
