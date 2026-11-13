//! Configured features that shape terrain: ores, blobs, columns, spikes, end features, lakes and the rest.

use super::{
    value_to_accepted_neighbors, value_to_block_state_provider, value_to_block_wrapper,
    value_to_fossil_processor, value_to_placed_feature_wrapper, value_to_rule_test,
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

pub(super) fn feature_netherrack_replace_blobs(config: &Value) -> TokenStream {
    let target = value_to_block_state(&config["target"]);
    let state = value_to_block_state(&config["state"]);
    let radius = value_to_int_provider(&config["radius"]);
    quote! {
        ConfiguredFeature::NetherrackReplaceBlobs(ReplaceBlobsFeature {
            target: #target,
            state: #state,
            radius: #radius,
        })
    }
}

pub(super) fn feature_simple_block(config: &Value) -> TokenStream {
    let to_place = value_to_block_state_provider(&config["to_place"]);
    let schedule = if config["schedule_tick"].is_boolean() {
        let b = config["schedule_tick"].as_bool().unwrap_or(false);
        quote! { Some(#b) }
    } else {
        quote! { None }
    };
    quote! {
        ConfiguredFeature::SimpleBlock(SimpleBlockFeature {
            to_place: #to_place,
            schedule_tick: #schedule,
        })
    }
}

pub(super) fn feature_ore(config: &Value, type_str: &str) -> TokenStream {
    let size = config["size"].as_i64().unwrap_or(0) as i32;
    let discard = config["discard_chance_on_air_exposure"]
        .as_f64()
        .unwrap_or(0.0) as f32;
    let targets: Vec<TokenStream> = config["targets"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|t| {
                    let rule = value_to_rule_test(&t["target"]);
                    let state = value_to_block_state(&t["state"]);
                    quote! { OreTarget { target: #rule, state: #state } }
                })
                .collect()
        })
        .unwrap_or_default();
    if type_str == "minecraft:scattered_ore" {
        quote! {
            ConfiguredFeature::ScatteredOre(crate::generation::feature::features::scattered_ore::ScatteredOreFeature {
                size: #size,
                discard_chance_on_air_exposure: #discard,
                targets: vec![#(#targets),*],
            })
        }
    } else {
        quote! {
            ConfiguredFeature::Ore(OreFeature {
                size: #size,
                discard_chance_on_air_exposure: #discard,
                targets: vec![#(#targets),*],
            })
        }
    }
}

pub(super) fn feature_spring_feature(config: &Value) -> TokenStream {
    let state = value_to_block_state(&config["state"]);
    let req = config["requires_block_below"].as_bool().unwrap_or(true);
    let rock = config["rock_count"].as_i64().unwrap_or(4) as i32;
    let hole = config["hole_count"].as_i64().unwrap_or(1) as i32;
    let valid = value_to_block_wrapper(&config["valid_blocks"]);
    quote! {
        ConfiguredFeature::SpringFeature(SpringFeatureFeature {
            state: #state,
            requires_block_below: #req,
            rock_count: #rock,
            hole_count: #hole,
            valid_blocks: #valid,
        })
    }
}

pub(super) fn feature_end_spike(config: &Value) -> TokenStream {
    let inv = config["crystal_invulnerable"].as_bool().unwrap_or(false);
    let spikes: Vec<TokenStream> =
        config["spikes"]
            .as_array()
            .map(|arr| {
                arr.iter().map(|s| {
            let cx = s["centerX"].as_i64().unwrap_or(0) as i32;
            let cz = s["centerZ"].as_i64().unwrap_or(0) as i32;
            let r = s["radius"].as_i64().unwrap_or(0) as i32;
            let h = s["height"].as_i64().unwrap_or(0) as i32;
            let g = s["guarded"].as_bool().unwrap_or(false);
            quote! {
                Spike { center_x: #cx, center_z: #cz, radius: #r, height: #h, guarded: #g }
            }
        }).collect()
            })
            .unwrap_or_default();
    quote! {
        ConfiguredFeature::EndSpike(EndSpikeFeature {
            crystal_invulnerable: #inv,
            spikes: vec![#(#spikes),*],
        })
    }
}

pub(super) fn feature_pointed_dripstone(config: &Value) -> TokenStream {
    let taller = config["chance_of_taller_dripstone"].as_f64().unwrap_or(0.2) as f32;
    let dir_spread = config["chance_of_directional_spread"]
        .as_f64()
        .unwrap_or(0.7) as f32;
    let r2 = config["chance_of_spread_radius2"].as_f64().unwrap_or(0.5) as f32;
    let r3 = config["chance_of_spread_radius3"].as_f64().unwrap_or(0.5) as f32;
    quote! {
        ConfiguredFeature::PointedDripstone(SmallDripstoneFeature {
            taller_dripstone: #taller,
            directional_spread: #dir_spread,
            spread_radius2: #r2,
            spread_radius3: #r3,
        })
    }
}

pub(super) fn feature_geode(config: &Value) -> TokenStream {
    let blocks = &config["blocks"];
    let filling_provider = value_to_block_state_provider(&blocks["filling_provider"]);
    let inner_layer_provider = value_to_block_state_provider(&blocks["inner_layer_provider"]);
    let alternate_inner_layer_provider =
        value_to_block_state_provider(&blocks["alternate_inner_layer_provider"]);
    let middle_layer_provider = value_to_block_state_provider(&blocks["middle_layer_provider"]);
    let outer_layer_provider = value_to_block_state_provider(&blocks["outer_layer_provider"]);
    let inner_placements: Vec<TokenStream> = blocks["inner_placements"]
        .as_array()
        .map(|arr| arr.iter().map(value_to_block_state_codec).collect())
        .unwrap_or_default();
    let cannot_replace = value_to_block_wrapper(&blocks["cannot_replace"]);
    let invalid_blocks = value_to_block_wrapper(&blocks["invalid_blocks"]);

    let layers = &config["layers"];
    let filling = layers["filling"].as_f64().unwrap_or(1.7);
    let inner_layer = layers["inner_layer"].as_f64().unwrap_or(2.2);
    let middle_layer = layers["middle_layer"].as_f64().unwrap_or(3.2);
    let outer_layer = layers["outer_layer"].as_f64().unwrap_or(4.2);

    let crack = &config["crack"];
    let generate_crack_chance = crack["generate_crack_chance"].as_f64().unwrap_or(1.0);
    let base_crack_size = crack["base_crack_size"].as_f64().unwrap_or(2.0);
    let crack_point_offset = crack["crack_point_offset"].as_i64().unwrap_or(2) as i32;

    let use_potential_placements_chance = config["use_potential_placements_chance"]
        .as_f64()
        .unwrap_or(0.35);
    let use_alternate_layer0_chance = config["use_alternate_layer0_chance"]
        .as_f64()
        .unwrap_or(0.0);
    let placements_require_layer0_alternate = config["placements_require_layer0_alternate"]
        .as_bool()
        .unwrap_or(true);
    let outer_wall_distance = int_or_uniform(&config["outer_wall_distance"], 4, 5);
    let distribution_points = int_or_uniform(&config["distribution_points"], 3, 4);
    let point_offset = int_or_uniform(&config["point_offset"], 1, 2);
    let min_gen_offset = config["min_gen_offset"].as_i64().unwrap_or(-16) as i32;
    let max_gen_offset = config["max_gen_offset"].as_i64().unwrap_or(16) as i32;
    let noise_multiplier = config["noise_multiplier"].as_f64().unwrap_or(0.05);
    let invalid_blocks_threshold = config["invalid_blocks_threshold"].as_i64().unwrap_or(0) as i32;

    quote! {
        ConfiguredFeature::Geode(Box::new(GeodeFeature {
            filling_provider: #filling_provider,
            inner_layer_provider: #inner_layer_provider,
            alternate_inner_layer_provider: #alternate_inner_layer_provider,
            middle_layer_provider: #middle_layer_provider,
            outer_layer_provider: #outer_layer_provider,
            inner_placements: vec![#(#inner_placements),*],
            cannot_replace: #cannot_replace,
            invalid_blocks: #invalid_blocks,
            filling: #filling,
            inner_layer: #inner_layer,
            middle_layer: #middle_layer,
            outer_layer: #outer_layer,
            generate_crack_chance: #generate_crack_chance,
            base_crack_size: #base_crack_size,
            crack_point_offset: #crack_point_offset,
            use_potential_placements_chance: #use_potential_placements_chance,
            use_alternate_layer0_chance: #use_alternate_layer0_chance,
            placements_require_layer0_alternate: #placements_require_layer0_alternate,
            outer_wall_distance: #outer_wall_distance,
            distribution_points: #distribution_points,
            point_offset: #point_offset,
            min_gen_offset: #min_gen_offset,
            max_gen_offset: #max_gen_offset,
            noise_multiplier: #noise_multiplier,
            invalid_blocks_threshold: #invalid_blocks_threshold,
        }))
    }
}

pub(super) fn feature_monster_room() -> TokenStream {
    quote! { ConfiguredFeature::MonsterRoom(crate::generation::feature::features::monster_room::DungeonFeature {}) }
}

pub(super) fn feature_underwater_magma(config: &Value) -> TokenStream {
    let floor_range = config["floor_search_range"].as_i64().unwrap_or(0) as i32;
    let placement_radius = config["placement_radius_around_floor"]
        .as_i64()
        .unwrap_or(0) as i32;
    let placement_prob = config["placement_probability_per_valid_position"]
        .as_f64()
        .unwrap_or(0.0) as f32;
    quote! {
        ConfiguredFeature::UnderwaterMagma(
            crate::generation::feature::features::underwater_magma::UnderwaterMagmaFeature {
                floor_search_range: #floor_range,
                placement_radius: #placement_radius,
                placement_probability: #placement_prob,
            }
        )
    }
}

pub(super) fn feature_glowstone_blob() -> TokenStream {
    quote! { ConfiguredFeature::GlowstoneBlob(crate::generation::feature::features::glowstone_blob::GlowstoneBlobFeature {}) }
}

pub(super) fn feature_disk(config: &Value) -> TokenStream {
    let state_provider = value_to_block_state_provider(&config["state_provider"]);
    let target = value_to_block_predicate(&config["target"]);
    let radius = value_to_int_provider(&config["radius"]);
    let half_height = config["half_height"].as_i64().unwrap_or(1) as i32;
    quote! {
        ConfiguredFeature::Disk(crate::generation::feature::features::disk::DiskFeature {
            state_provider: #state_provider,
            target: #target,
            radius: #radius,
            half_height: #half_height,
        })
    }
}

pub(super) fn feature_basalt_columns(config: &Value) -> TokenStream {
    let height = value_to_int_provider(&config["height"]);
    let reach = value_to_int_provider(&config["reach"]);
    quote! {
        ConfiguredFeature::BasaltColumns(crate::generation::feature::features::basalt_columns::BasaltColumnsFeature {
            height: #height,
            reach: #reach,
        })
    }
}

pub(super) fn feature_basalt_pillar() -> TokenStream {
    quote! { ConfiguredFeature::BasaltPillar(crate::generation::feature::features::basalt_pillar::BasaltPillarFeature {}) }
}

pub(super) fn feature_block_blob(config: &Value) -> TokenStream {
    let state = value_to_block_state(&config["state"]);
    quote! {
        ConfiguredFeature::ForestRock(crate::generation::feature::features::forest_rock::ForestRockFeature {
            state: #state,
        })
    }
}

pub(super) fn feature_freeze_top_layer() -> TokenStream {
    quote! { ConfiguredFeature::FreezeTopLayer(crate::generation::feature::features::freeze_top_layer::FreezeTopLayerFeature {}) }
}

pub(super) fn feature_ice_spike() -> TokenStream {
    quote! { ConfiguredFeature::IceSpike(crate::generation::feature::features::ice_spike::IceSpikeFeature {}) }
}

pub(super) fn feature_spike() -> TokenStream {
    quote! { ConfiguredFeature::IceSpike(crate::generation::feature::features::ice_spike::IceSpikeFeature {}) }
}

pub(super) fn feature_iceberg(config: &Value) -> TokenStream {
    let state = value_to_block_state_codec(&config["state"]);
    quote! { ConfiguredFeature::Iceberg(crate::generation::feature::features::iceberg::IcebergFeature { main_block: #state }) }
}

pub(super) fn feature_end_platform() -> TokenStream {
    quote! { ConfiguredFeature::EndPlatform(crate::generation::feature::features::end_platform::EndPlatformFeature) }
}

pub(super) fn feature_end_podium(v: &Value, config: &Value) -> TokenStream {
    let active = config
        .get("active")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    quote! { ConfiguredFeature::EndPodium(crate::generation::feature::features::end_podium::EndPodiumFeature::new(#active)) }
}

pub(super) fn feature_end_island() -> TokenStream {
    quote! { ConfiguredFeature::EndIsland(crate::generation::feature::features::end_island::EndIslandFeature {}) }
}

pub(super) fn feature_fossil(config: &Value) -> TokenStream {
    let fossil_structures = config["fossil_structures"]
        .as_array()
        .expect("fossil_structures must be an array")
        .iter()
        .map(|value| {
            let name = value
                .as_str()
                .expect("fossil structure names must be strings");
            quote! { #name }
        });
    let overlay_structures = config["overlay_structures"]
        .as_array()
        .expect("overlay_structures must be an array")
        .iter()
        .map(|value| {
            let name = value
                .as_str()
                .expect("fossil overlay structure names must be strings");
            quote! { #name }
        });
    let fossil_processor = value_to_fossil_processor(&config["fossil_processors"]);
    let overlay_processor = value_to_fossil_processor(&config["overlay_processors"]);
    let max_empty_corners_allowed = config["max_empty_corners_allowed"]
        .as_u64()
        .expect("max_empty_corners_allowed must be an unsigned integer")
        as u8;

    quote! {
        ConfiguredFeature::Fossil(
            crate::generation::feature::features::fossil::FossilFeature {
                fossil_structures: vec![#(#fossil_structures),*],
                overlay_structures: vec![#(#overlay_structures),*],
                fossil_processor: #fossil_processor,
                overlay_processor: #overlay_processor,
                max_empty_corners_allowed: #max_empty_corners_allowed,
            }
        )
    }
}

pub(super) fn feature_lake(config: &Value) -> TokenStream {
    let fluid = value_to_block_state_provider(&config["fluid"]);
    let barrier = value_to_block_state_provider(&config["barrier"]);
    quote! {
        ConfiguredFeature::Lake(crate::generation::feature::features::lake::LakeFeature {
            fluid: #fluid,
            barrier: #barrier,
        })
    }
}

pub(super) fn feature_blue_ice() -> TokenStream {
    quote! { ConfiguredFeature::BlueIce(crate::generation::feature::features::blue_ice::BlueIceFeature {}) }
}

pub(super) fn feature_end_gateway(config: &Value) -> TokenStream {
    let exit = if let Some(exit_arr) = config["exit"].as_array() {
        let x = exit_arr[0].as_i64().unwrap_or(0) as i32;
        let y = exit_arr[1].as_i64().unwrap_or(0) as i32;
        let z = exit_arr[2].as_i64().unwrap_or(0) as i32;
        quote! { Some(pumpkin_util::math::position::BlockPos::new(#x, #y, #z)) }
    } else {
        quote! { None }
    };
    let exact = config["exact"].as_bool().unwrap_or(false);
    quote! {
        ConfiguredFeature::EndGateway(crate::generation::feature::features::end_gateway::EndGatewayFeature {
            exit: #exit,
            exact: #exact,
        })
    }
}

pub(super) fn feature_delta_feature(config: &Value) -> TokenStream {
    let contents = value_to_block_state(&config["contents"]);
    let rim = value_to_block_state(&config["rim"]);
    let size = value_to_int_provider(&config["size"]);
    let rim_size = value_to_int_provider(&config["rim_size"]);
    quote! {
        ConfiguredFeature::DeltaFeature(crate::generation::feature::features::delta_feature::DeltaFeatureFeature {
            contents: #contents,
            rim: #rim,
            size: #size,
            rim_size: #rim_size,
        })
    }
}

pub(super) fn feature_fill_layer(config: &Value) -> TokenStream {
    let height = config["height"].as_i64().unwrap_or(0) as i32;
    let state = value_to_block_state(&config["state"]);
    quote! {
        ConfiguredFeature::FillLayer(crate::generation::feature::features::fill_layer::FillLayerFeature {
            height: #height,
            state: #state,
        })
    }
}

pub(super) fn feature_bonus_chest() -> TokenStream {
    quote! { ConfiguredFeature::BonusChest(crate::generation::feature::features::bonus_chest::BonusChestFeature {}) }
}

pub(super) fn feature_dripstone_cluster() -> TokenStream {
    quote! { ConfiguredFeature::DripstoneCluster(crate::generation::feature::features::drip_stone::cluster::DripstoneClusterFeature {}) }
}

pub(super) fn feature_sequence() -> TokenStream {
    quote! { ConfiguredFeature::NoOp }
}

pub(super) fn feature_stepped_column_cluster(config: &Value) -> TokenStream {
    let block = value_to_block_state_provider(&config["block"]);
    let can_replace = value_to_block_predicate(&config["can_replace"]);
    let cannot_place_on_str = config["cannot_place_on"].as_str().unwrap_or("");
    let cannot_place_on_ident = if let Some(tag) = cannot_place_on_str.strip_prefix('#') {
        quote::format_ident!("{}", tag.to_uppercase().replace([':', '-'], "_"))
    } else {
        panic!("Unsupported cannot_place_on in stepped_column_cluster: {cannot_place_on_str}");
    };
    let cluster_reach = value_to_int_provider(&config["cluster_reach"]);
    let column_count = value_to_int_provider(&config["column_count"]);
    let column_reach = value_to_int_provider(&config["column_reach"]);
    let continue_through = value_to_block_predicate(&config["continue_through"]);
    let height = value_to_int_provider(&config["height"]);
    quote! {
        ConfiguredFeature::SteppedColumnCluster(
            crate::generation::feature::features::stepped_column_cluster::SteppedColumnClusterFeature {
                block: #block,
                continue_through: #continue_through,
                can_replace: #can_replace,
                cannot_place_on: pumpkin_data::tag::Block::#cannot_place_on_ident,
                cluster_reach: #cluster_reach,
                column_count: #column_count,
                column_reach: #column_reach,
                height: #height,
            }
        )
    }
}

pub(super) fn feature_large_dripstone() -> TokenStream {
    quote! { ConfiguredFeature::LargeDripstone(crate::generation::feature::features::drip_stone::large::LargeDripstoneFeature {}) }
}

pub(super) fn feature_sculk_patch(config: &Value) -> TokenStream {
    let charge_count = config["charge_count"].as_u64().unwrap() as i32;
    let amount_per_charge = config["amount_per_charge"].as_u64().unwrap() as i32;
    let spread_attempts = config["spread_attempts"].as_u64().unwrap() as i32;
    let growth_rounds = config["growth_rounds"].as_u64().unwrap() as i32;
    let spread_rounds = config["spread_rounds"].as_u64().unwrap() as i32;
    let extra_rare_growths = value_to_int_provider(&config["extra_rare_growths"]);
    let catalyst_chance = config["catalyst_chance"].as_f64().unwrap() as f32;
    quote! {
        ConfiguredFeature::SculkPatch(crate::generation::feature::features::sculk_patch::SculkPatchFeature {
            charge_count: #charge_count,
            amount_per_charge: #amount_per_charge,
            spread_attempts: #spread_attempts,
            growth_rounds: #growth_rounds,
            spread_rounds: #spread_rounds,
            extra_rare_growths: #extra_rare_growths,
            catalyst_chance: #catalyst_chance,
        })
    }
}

pub(super) fn feature_block_pile(config: &Value) -> TokenStream {
    let provider = value_to_block_state_provider(&config["state_provider"]);
    quote! {
        ConfiguredFeature::BlockPile(crate::generation::feature::features::block_pile::BlockPileFeature {
            state_provider: #provider,
        })
    }
}

pub(super) fn feature_replace_single_block(config: &Value) -> TokenStream {
    let targets: Vec<TokenStream> = config["targets"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|t| {
                    let rule = value_to_rule_test(&t["target"]);
                    let state = value_to_block_state(&t["state"]);
                    quote! { OreTarget { target: #rule, state: #state } }
                })
                .collect()
        })
        .unwrap_or_default();
    quote! {
        ConfiguredFeature::ReplaceSingleBlock(crate::generation::feature::features::replace_single_block::ReplaceSingleBlockFeature {
            targets: vec![#(#targets),*],
        })
    }
}

pub(super) fn feature_void_start_platform() -> TokenStream {
    quote! { ConfiguredFeature::VoidStartPlatform(crate::generation::feature::features::void_start_platform::VoidStartPlatformFeature {}) }
}

pub(super) fn feature_desert_well() -> TokenStream {
    quote! { ConfiguredFeature::DesertWell(crate::generation::feature::features::desert_well::DesertWellFeature) }
}

pub(super) fn feature_overlay(config: &Value) -> TokenStream {
    let features_arr = config["features"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    let features: Vec<TokenStream> = features_arr
        .iter()
        .map(value_to_placed_feature_wrapper)
        .collect();
    quote! {
        ConfiguredFeature::Overlay(crate::generation::feature::features::overlay::OverlayFeature {
            features: vec![#(#features),*],
        })
    }
}

pub(super) fn feature_random_neighbor_spread(config: &Value) -> TokenStream {
    let block = value_to_block_state_provider(&config["block"]);
    let accepted_neighbors = value_to_accepted_neighbors(&config["accepted_neighbors"]);
    let can_replace = value_to_block_predicate(&config["can_replace"]);
    let attempts = value_to_int_provider(&config["attempts"]);
    let xz_offset = value_to_int_provider(&config["xz_offset"]);
    let y_offset = value_to_int_provider(&config["y_offset"]);
    quote! {
        ConfiguredFeature::RandomNeighborSpread(crate::generation::feature::features::random_neighbor_spread::RandomNeighborSpreadFeature {
            block: #block,
            accepted_neighbors: #accepted_neighbors,
            can_replace: #can_replace,
            attempts: #attempts,
            xz_offset: #xz_offset,
            y_offset: #y_offset,
        })
    }
}

pub(super) fn feature_single_block_pillar(config: &Value) -> TokenStream {
    let block = value_to_block_state_provider(&config["block"]);
    let can_replace = if config.get("can_replace").is_some() {
        value_to_block_predicate(&config["can_replace"])
    } else {
        quote! { BlockPredicate::AlwaysTrue }
    };
    let direction = value_to_block_direction(config["direction"].as_str().unwrap_or("down"));
    let chance_to_continue = config["chance_to_continue"].as_f64().unwrap_or(1.0) as f32;
    let cap_feature = if config.get("cap_feature").is_some() && !config["cap_feature"].is_null() {
        let pf = value_to_placed_feature_wrapper(&config["cap_feature"]);
        quote! { Some(#pf) }
    } else {
        quote! { None }
    };
    quote! {
        ConfiguredFeature::SingleBlockPillar(crate::generation::feature::features::single_block_pillar::SingleBlockPillarFeature {
            block: #block,
            can_replace: #can_replace,
            direction: #direction,
            chance_to_continue: #chance_to_continue,
            cap_feature: #cap_feature,
        })
    }
}

pub(super) fn feature_projected_random_patchy_square(config: &Value) -> TokenStream {
    let block = value_to_block_state_provider(&config["block"]);
    let project_through = value_to_block_predicate(&config["project_through"]);
    let size = value_to_int_provider(&config["size"]);
    let max_projection_height = config["max_projection_height"].as_i64().unwrap_or(0) as i32;
    quote! {
        ConfiguredFeature::ProjectedRandomPatchySquare(crate::generation::feature::features::projected_random_patchy_square::ProjectedRandomPatchySquareFeature {
            block: #block,
            project_through: #project_through,
            size: #size,
            max_projection_height: #max_projection_height,
        })
    }
}

pub(super) fn feature_template() -> TokenStream {
    quote! { ConfiguredFeature::NoOp }
}
