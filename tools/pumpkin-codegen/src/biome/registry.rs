//! Reads the vanilla biomes and turns each into its constant and lookup arms.

use super::{Biome, GeyserBiomeMapping, SpawnGroups, Spawner, TemperatureModifier};
use heck::ToPascalCase;
use heck::ToShoutySnakeCase;
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use serde::Deserialize;
use serde_json::Value;
use std::{collections::BTreeMap, fs};
use syn::LitInt;

/// Reads every vanilla biome JSON, numbering them in file-name order.
pub(super) fn load_biomes() -> BTreeMap<String, Biome> {
    let dir = std::path::Path::new("../../assets/datapack/data/minecraft/worldgen/biome");
    let mut biomes: BTreeMap<String, Biome> = BTreeMap::new();
    let mut entries: Vec<_> = fs::read_dir(dir)
        .expect("Missing worldgen/biome directory")
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .collect();
    entries.sort_by_key(|e| e.path());

    for (i, entry) in entries.iter().enumerate() {
        let stem = entry
            .path()
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let content = fs::read_to_string(entry.path()).expect("Failed to read biome file");
        let mut biome: Biome = serde_json::from_str(&content).expect("Failed to parse biome JSON");
        biome.apply_natural_mob_spawns();
        biome.id = i as u8;
        biomes.insert(stem, biome);
    }
    biomes
}

/// Adds one biome's constant, lookup arms and list entry.
pub(super) fn push_biome(
    name: String,
    biome: Biome,
    bedrock_biomes: &BTreeMap<String, GeyserBiomeMapping>,
    variants: &mut TokenStream,
    name_to_type: &mut TokenStream,
    id_to_type: &mut TokenStream,
    all_variants: &mut TokenStream,
) {
    let full_name = format!("minecraft:{name}");
    let be_network_id = bedrock_biomes
        .get(&full_name)
        .map(|mapping| mapping.bedrock_id)
        .unwrap_or(1);

    // let full_name = format!("minecraft:{name}");
    let format_name = format_ident!("{}", name.to_shouty_snake_case());
    let has_precipitation = biome.has_precipitation;
    let temperature = biome.temperature;
    let downfall = biome.downfall;
    let carvers: Vec<TokenStream> = biome
        .carvers
        .iter()
        .map(|c| {
            let name = c.strip_prefix("minecraft:").unwrap_or(c);
            let variant_name = format_ident!("{}", name.to_uppercase());
            quote! { &crate::carver::#variant_name }
        })
        .collect();
    let features: Vec<TokenStream> = biome
        .features
        .iter()
        .map(|step| {
            let step_features: Vec<TokenStream> = step
                .iter()
                .map(|f| {
                    let name = f.strip_prefix("minecraft:").unwrap_or(f);
                    let variant_name = format_ident!("{}", name.to_pascal_case());
                    quote! { crate::placed_feature::PlacedFeature::#variant_name }
                })
                .collect();
            quote! { &[#(#step_features),*] }
        })
        .collect();

    let creature_spawn_probability = &biome.creature_spawn_probability.unwrap_or(0.1);

    let spawners = spawners_tokens(&biome);

    let temperature_modifier = biome
        .temperature_modifier
        .unwrap_or(TemperatureModifier::None);

    let spawn_costs: Vec<_> = biome
        .spawn_costs
        .iter()
        .map(|(name, cost)| {
            let cost_token = cost.to_tokens();
            let entity_type = name.strip_prefix("minecraft:").unwrap();
            quote! {
                #entity_type => #cost_token
            }
        })
        .collect();

    let temperature_modifier = match temperature_modifier {
        TemperatureModifier::Frozen => quote! { TemperatureModifier::Frozen },
        TemperatureModifier::None => quote! { TemperatureModifier::None },
    };
    let index = LitInt::new(&biome.id.to_string(), Span::call_site());

    variants.extend([quote! {
        pub const #format_name: Biome = Biome {
            id: #index,
            registry_id: #name,
            be_network_id: #be_network_id,
            weather: Weather::new(
                 #has_precipitation,
                 #temperature,
                 #temperature_modifier,
                 #downfall
            ),
            features: &[#(#features),*],
            carvers: &[#(#carvers),*],
            creature_spawn_probability: #creature_spawn_probability,
            spawners: #spawners,
            spawn_costs: phf::phf_map! {
                #(#spawn_costs),*
            },
        };
    }]);

    name_to_type.extend(quote! { #name => Some(&Self::#format_name), });
    id_to_type.extend(quote! { #index => Some(&Self::#format_name), });
    all_variants.extend(quote! { &Self::#format_name, });
}

/// The `SpawnGroups` value of one biome.
fn spawners_tokens(biome: &Biome) -> TokenStream {
    let monster: Vec<_> = biome
        .spawners
        .monster
        .iter()
        .map(Spawner::to_tokens)
        .collect();
    let ambient: Vec<_> = biome
        .spawners
        .ambient
        .iter()
        .map(Spawner::to_tokens)
        .collect();
    let axolotls: Vec<_> = biome
        .spawners
        .axolotls
        .iter()
        .map(Spawner::to_tokens)
        .collect();
    let creature: Vec<_> = biome
        .spawners
        .creature
        .iter()
        .map(Spawner::to_tokens)
        .collect();
    let misc: Vec<_> = biome.spawners.misc.iter().map(Spawner::to_tokens).collect();
    let underground_water_creature: Vec<_> = biome
        .spawners
        .underground_water_creature
        .iter()
        .map(Spawner::to_tokens)
        .collect();
    let water_ambient: Vec<_> = biome
        .spawners
        .water_ambient
        .iter()
        .map(Spawner::to_tokens)
        .collect();
    let water_creature: Vec<_> = biome
        .spawners
        .water_creature
        .iter()
        .map(Spawner::to_tokens)
        .collect();

    quote! {
        SpawnGroups {
            monster: &[#(#monster),*],
            ambient: &[#(#ambient),*],
            axolotls: &[#(#axolotls),*],
            creature: &[#(#creature),*],
            misc: &[#(#misc),*],
            underground_water_creature: &[#(#underground_water_creature),*],
            water_ambient: &[#(#water_ambient),*],
            water_creature: &[#(#water_creature),*],
        }
    }
}
