mod registry;
mod static_items;

use registry::load_provider_entries;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;

use heck::ToShoutySnakeCase;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use serde::Deserialize;

const DEFAULT_NAMESPACE: &str = "minecraft";

#[derive(Deserialize, Debug)]
struct RawProvider {
    #[serde(rename = "type")]
    provider_type: String,
    #[serde(default)]
    enchantment: Option<String>,
    #[serde(default)]
    level: Option<serde_json::Value>,
    #[serde(default)]
    enchantments: Option<serde_json::Value>,
    #[serde(default)]
    min_cost: Option<i32>,
    #[serde(default)]
    max_cost_span: Option<i32>,
    #[serde(default)]
    cost: Option<serde_json::Value>,
}

struct ProviderEntry {
    qualified: String,
    bare: String,
    const_name: String,
    rel_path: String,
    kind_tokens: TokenStream,
}

pub fn build() -> TokenStream {
    let entries = load_provider_entries();

    let mut const_decls = Vec::new();
    let mut from_name_arms = Vec::new();
    let mut raw_json_arms = Vec::new();
    let mut all_const_idents = Vec::new();
    let mut all_qualified_names = Vec::new();

    for entry in &entries {
        let const_ident = format_ident!("{}", entry.const_name);
        let qualified = &entry.qualified;
        let bare = &entry.bare;
        let rel_path = &entry.rel_path;
        let kind_tokens = &entry.kind_tokens;

        const_decls.push(quote! {
            pub const #const_ident: Self = Self {
                name: #qualified,
                kind: #kind_tokens,
            };
        });

        from_name_arms.push(quote! {
            #qualified | #bare => Some(&Self::#const_ident),
        });

        raw_json_arms.push(quote! {
            #qualified | #bare => Some(include_str!(#rel_path)),
        });

        all_const_idents.push(quote! { Self::#const_ident });
        all_qualified_names.push(quote! { #qualified });
    }

    let imports_and_enchantment_provider_kind_tokens =
        static_items::imports_and_enchantment_provider_kind_tokens();
    let name_tokens = static_items::name_tokens();
    let select_enchantments_tokens = static_items::select_enchantments_tokens();
    let select_by_cost_tokens = static_items::select_by_cost_tokens();
    let tests_tokens = static_items::tests_tokens();
    quote! {
        #imports_and_enchantment_provider_kind_tokens

        impl EnchantmentProvider {
            #( #const_decls )*

            pub const ALL: &'static [Self] = &[
                #( #all_const_idents ),*
            ];

            #name_tokens

            #[allow(clippy::too_many_lines)]
            #[allow(clippy::match_same_arms)]
            #[must_use]
            pub fn from_name(name: &str) -> Option<&'static Self> {
                match name {
                    #( #from_name_arms )*
                    _ => None,
                }
            }

            #[allow(clippy::too_many_lines)]
            #[allow(clippy::match_same_arms)]
            #[must_use]
            pub fn get_raw_json(name: &str) -> Option<&'static str> {
                match name {
                    #( #raw_json_arms )*
                    _ => None,
                }
            }

            #[must_use]
            pub const fn all() -> &'static [Self] {
                Self::ALL
            }

            #[must_use]
            pub const fn all_provider_names() -> &'static [&'static str] {
                &[
                    #( #all_qualified_names ),*
                ]
            }

            #select_enchantments_tokens

            #select_by_cost_tokens
        }

        #tests_tokens
    }
}

fn scan_dir(dir: &Path, prefix: &str, registry: &mut BTreeMap<String, (String, String, String)>) {
    let mut entries = fs::read_dir(dir)
        .expect("read dir")
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if path.is_dir() {
            let new_prefix = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            scan_dir(&path, &new_prefix, registry);
            continue;
        }

        let Some(stem) = name.strip_suffix(".json") else {
            continue;
        };

        let id = if prefix.is_empty() {
            stem.to_string()
        } else {
            format!("{prefix}/{stem}")
        };

        let qualified = format!("{DEFAULT_NAMESPACE}:{id}");
        let bare = id.clone();
        let const_name = id.replace(['/', '-', '.'], "_").to_uppercase();

        let rel_to_repo = path.strip_prefix("../../").expect("strip ../../");
        let rel_path = format!(
            "../../../../{}",
            rel_to_repo.to_string_lossy().replace('\\', "/")
        );

        registry.insert(qualified, (bare, const_name, rel_path));
    }
}

fn extract_enchantments(val: Option<&serde_json::Value>, tags_dir: &Path) -> Vec<String> {
    let Some(val) = val else {
        return Vec::new();
    };

    let mut result = Vec::new();
    match val {
        serde_json::Value::String(s) => {
            if s.starts_with('#') {
                let mut visited = HashSet::new();
                resolve_tag_recursive(s, tags_dir, &mut visited, &mut result);
            } else {
                let bare = s.strip_prefix("minecraft:").unwrap_or(s);
                result.push(bare.to_string());
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                if let Some(s) = item.as_str() {
                    if s.starts_with('#') {
                        let mut visited = HashSet::new();
                        resolve_tag_recursive(s, tags_dir, &mut visited, &mut result);
                    } else {
                        let bare = s.strip_prefix("minecraft:").unwrap_or(s);
                        result.push(bare.to_string());
                    }
                }
            }
        }
        _ => {}
    }

    result.sort();
    result.dedup();
    result
}

fn resolve_tag_recursive(
    tag: &str,
    tags_dir: &Path,
    visited: &mut HashSet<String>,
    output: &mut Vec<String>,
) {
    let tag_name = tag.strip_prefix('#').unwrap_or(tag);
    if !visited.insert(tag_name.to_string()) {
        return;
    }

    let bare = tag_name.strip_prefix("minecraft:").unwrap_or(tag_name);
    let tag_file = tags_dir.join(format!("{bare}.json"));
    if !tag_file.exists() {
        return;
    }

    let content = fs::read_to_string(&tag_file).unwrap_or_default();
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
        return;
    };

    if let Some(values) = json.get("values").and_then(|v| v.as_array()) {
        for v in values {
            if let Some(s) = v.as_str() {
                if s.starts_with('#') {
                    resolve_tag_recursive(s, tags_dir, visited, output);
                } else {
                    let enc = s.strip_prefix("minecraft:").unwrap_or(s);
                    output.push(enc.to_string());
                }
            }
        }
    }
}
