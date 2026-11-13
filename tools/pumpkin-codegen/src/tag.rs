mod registry;
mod static_items;

use registry::{TagState, collect_version_tags, load_id_maps};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs,
};

use crate::block::BlockAssets;
use crate::entity_type::EntityType;
use crate::fluid::Fluid;
use crate::item::Item;
use heck::ToPascalCase;
use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};

// Builder that generates an enum with `from_string` and `identifier_string` methods.
pub struct EnumCreator {
    /// Name of the enum to generate (converted to PascalCase).
    pub name: String,
    /// Set of variant names (converted to PascalCase for the enum variants).
    pub values: BTreeSet<String>,
}

impl ToTokens for EnumCreator {
    /// Emits the enum definition and its `from_string`/`identifier_string` impl block.
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let name = format_ident!("{}", self.name.to_pascal_case());

        let variants = self.values.iter().map(|v| {
            let variant_name = format_ident!("{}", v.to_pascal_case());
            quote! { #variant_name }
        });

        let all_variants = self.values.iter().map(|v| {
            let variant_name = format_ident!("{}", v.to_pascal_case());
            quote! { Self::#variant_name }
        });

        let is_network_synced_cat = |v: &str| -> bool {
            if v == "villager_trade" || v == "trade_set" {
                return false;
            }
            if v.starts_with("worldgen/") && v != "worldgen/biome" {
                return false;
            }
            true
        };

        let network_variants = self
            .values
            .iter()
            .filter(|v| is_network_synced_cat(v))
            .map(|v| {
                let variant_name = format_ident!("{}", v.to_pascal_case());
                quote! { Self::#variant_name }
            });

        let unsynced_arms = self
            .values
            .iter()
            .filter(|v| !is_network_synced_cat(v))
            .map(|v| {
                let variant_name = format_ident!("{}", v.to_pascal_case());
                quote! { Self::#variant_name => false }
            });

        let from_string_arms = self.values.iter().map(|v| {
            let variant_name = format_ident!("{}", v.to_pascal_case());
            quote! { #v => Some(Self::#variant_name) }
        });

        let to_string_arms = self.values.iter().map(|v| {
            let variant_name = format_ident!("{}", v.to_pascal_case());
            quote! { Self::#variant_name => #v }
        });

        tokens.extend(quote! {
            #[derive(Eq, PartialEq, Hash, Debug, Clone, Copy)]
            pub enum #name {
                #(#variants),*
            }

            impl #name {
                pub const ALL: &[Self] = &[
                    #(#all_variants),*
                ];

                pub const NETWORK_KEYS: &[Self] = &[
                    #(#network_variants),*
                ];

                #[must_use]
                pub const fn is_network_synced(&self) -> bool {
                    match self {
                        #(#unsynced_arms,)*
                        _ => true,
                    }
                }

                #[must_use]
                pub const fn is_valid_for_version(&self, _version: JavaMinecraftVersion) -> bool {
                    self.is_network_synced()
                }

                #[must_use]
                pub fn from_string(s: &str) -> Option<Self> {
                    match s {
                        #(#from_string_arms,)*
                        _ => None,
                    }
                }

                #[must_use]
                pub const fn identifier_string(&self) -> &str {
                    match self {
                        #(#to_string_arms),*
                    }
                }
            }
        });
    }
}

fn load_datapack_registry_ids(dir: &std::path::Path) -> BTreeMap<String, u16> {
    let mut id_map = BTreeMap::new();
    if dir.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
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
            id_map.insert(format!("minecraft:{stem}"), i as u16);
            id_map.insert(stem, i as u16);
        }
    }
    id_map
}

fn load_datapack_tags(
    data_dir: &std::path::Path,
) -> BTreeMap<String, BTreeMap<String, Vec<String>>> {
    let mut raw_categories: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();

    fn walk_namespace_tags(
        dir: &std::path::Path,
        tags_dir: &std::path::Path,
        namespace: &str,
        raw: &mut BTreeMap<String, BTreeMap<String, Vec<String>>>,
    ) {
        if let Ok(entries) = fs::read_dir(dir) {
            let mut entries: Vec<_> = entries.flatten().collect();
            entries.sort_by_key(|e| e.path());
            for entry in entries {
                let path = entry.path();
                if path.is_dir() {
                    walk_namespace_tags(&path, tags_dir, namespace, raw);
                } else if path.extension().is_some_and(|ext| ext == "json") {
                    if path.file_name().is_some_and(|n| n == "_list.json") {
                        continue;
                    }
                    let rel = path.strip_prefix(tags_dir).unwrap();
                    let components: Vec<_> = rel
                        .iter()
                        .map(|c| c.to_string_lossy().into_owned())
                        .collect();
                    if components.is_empty() {
                        continue;
                    }
                    let (category, tag_rel_path) =
                        if components[0] == "worldgen" && components.len() >= 2 {
                            let sub = match components[1].as_str() {
                                "biomes" => "biome",
                                "feature" => "configured_feature",
                                s => s,
                            };
                            let cat = format!("worldgen/{sub}");
                            let rest = components[2..].join("/");
                            let tag_stem = rest.trim_end_matches(".json").to_string();
                            (cat, tag_stem)
                        } else {
                            let cat_norm = match components[0].as_str() {
                                "blocks" => "block",
                                "items" => "item",
                                "fluids" => "fluid",
                                "entity_types" => "entity_type",
                                "game_events" => "game_event",
                                "biomes" => "biome",
                                s => s,
                            };
                            let rest = components[1..].join("/");
                            let tag_stem = rest.trim_end_matches(".json").to_string();
                            (cat_norm.to_string(), tag_stem)
                        };

                    let tag_name = format!("{namespace}:{tag_rel_path}");
                    if let Ok(content) = fs::read_to_string(&path) {
                        #[derive(serde::Deserialize)]
                        struct TagFile {
                            values: Vec<serde_json::Value>,
                        }
                        if let Ok(tag_file) = serde_json::from_str::<TagFile>(&content) {
                            let mut values = Vec::new();
                            for val in tag_file.values {
                                if let Some(s) = val.as_str() {
                                    values.push(s.to_string());
                                } else if let Some(id) = val.get("id").and_then(|i| i.as_str()) {
                                    values.push(id.to_string());
                                }
                            }
                            raw.entry(category).or_default().insert(tag_name, values);
                        }
                    }
                }
            }
        }
    }

    if let Ok(entries) = fs::read_dir(data_dir) {
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.path());
        for entry in entries {
            let namespace = entry.file_name().to_string_lossy().into_owned();
            let tags_dir = entry.path().join("tags");
            if tags_dir.is_dir() {
                walk_namespace_tags(&tags_dir, &tags_dir, &namespace, &mut raw_categories);
            }
        }
    }

    // Recursively resolve references for each category
    let mut resolved_categories: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();

    for (cat, tag_map) in &raw_categories {
        let mut cat_resolved = BTreeMap::new();

        fn resolve_tag(
            tag_name: &str,
            tag_map: &BTreeMap<String, Vec<String>>,
            visited: &mut HashSet<String>,
        ) -> Vec<String> {
            if visited.contains(tag_name) {
                return Vec::new();
            }
            visited.insert(tag_name.to_string());
            let mut result = Vec::new();
            if let Some(raw_values) = tag_map.get(tag_name) {
                for v in raw_values {
                    if let Some(sub_tag) = v.strip_prefix('#') {
                        let full_sub = if sub_tag.contains(':') {
                            sub_tag.to_string()
                        } else {
                            format!("minecraft:{sub_tag}")
                        };
                        let mut sub_visited = visited.clone();
                        for child in resolve_tag(&full_sub, tag_map, &mut sub_visited) {
                            if !result.contains(&child) {
                                result.push(child);
                            }
                        }
                    } else {
                        let clean = v.strip_prefix("minecraft:").unwrap_or(v).to_string();
                        if !result.contains(&clean) {
                            result.push(clean);
                        }
                    }
                }
            }
            result
        }

        for tag_name in tag_map.keys() {
            let mut visited = HashSet::new();
            let resolved = resolve_tag(tag_name, tag_map, &mut visited);
            cat_resolved.insert(tag_name.clone(), resolved);
        }

        resolved_categories.insert(cat.clone(), cat_resolved);
    }

    resolved_categories
}

/// Generates the `TokenStream` for the `Tag` type, `RegistryKey` enum, tag
/// modules, and the `Taggable` trait with its lookup helpers.
pub(crate) fn build() -> TokenStream {
    let versions = [("26_3", "V_26_3")];

    let version_mapping = [("V_26_3", "V_26_3")];

    let maps = load_id_maps();
    let TagState {
        all_registry_keys,
        latest_tag_modules,
        latest_match_arms,
        all_version_code,
        ..
    } = collect_version_tags(versions, &maps);

    let mut version_fn_match_arms = Vec::new();

    for (ver_variant, ver_ident_str) in version_mapping {
        let ver_ident = format_ident!("{ver_variant}");
        if ver_ident_str == "V_26_3" {
            version_fn_match_arms.push(quote! {
                JavaMinecraftVersion::#ver_ident => get_latest_map(tag_category)
            });
        } else {
            let fn_name = format_ident!("get_tags_{}", ver_ident_str);
            version_fn_match_arms.push(quote! {
                JavaMinecraftVersion::#ver_ident => #fn_name(tag_category)
            });
        }
    }

    // --- Generate RegistryKey Enum ---
    let registry_key_enum = EnumCreator {
        name: "RegistryKey".to_string(),
        values: all_registry_keys.into_iter().collect(),
    }
    .to_token_stream();

    let get_tag_values_tokens = static_items::get_tag_values_tokens();
    quote! {
        use pumpkin_util::version::JavaMinecraftVersion;

        pub type Tag = (&'static [&'static str], &'static [u16]);

        #registry_key_enum

        #(#latest_tag_modules)*

        #(#all_version_code)*

        #[must_use]
        pub const fn get_latest_map(key: RegistryKey) -> &'static phf::Map<&'static str, &'static Tag> {
            match key {
                #(#latest_match_arms,)*
            }
        }

        #get_tag_values_tokens
    }
}
