//! Reads the vanilla enchantment providers and turns each into its kind tokens.

use super::{ProviderEntry, RawProvider, extract_enchantments, scan_dir};
use heck::ToShoutySnakeCase;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;

/// Reads every enchantment provider file into an entry with its kind tokens.
pub(super) fn load_provider_entries() -> Vec<ProviderEntry> {
    let provider_dir = Path::new("../../assets/datapack/data/minecraft/enchantment_provider");
    let tags_dir = Path::new("../../assets/datapack/data/minecraft/tags/enchantment");

    let mut raw_files: BTreeMap<String, (String, String, String)> = BTreeMap::new();
    if provider_dir.is_dir() {
        scan_dir(provider_dir, "", &mut raw_files);
    }

    let mut entries: Vec<ProviderEntry> = Vec::new();

    for (qualified, (bare, const_name, rel_path)) in raw_files {
        // Read and parse JSON
        let full_path = Path::new("../../assets/datapack/data/minecraft/enchantment_provider")
            .join(format!("{bare}.json"));
        let content = fs::read_to_string(&full_path).unwrap_or_else(|e| {
            panic!(
                "failed to read enchantment provider file {}: {}",
                full_path.display(),
                e
            )
        });
        let raw: RawProvider = serde_json::from_str(&content).unwrap_or_else(|e| {
            panic!(
                "failed to parse enchantment provider JSON {}: {}",
                full_path.display(),
                e
            )
        });

        let kind_tokens = provider_kind_tokens(raw, tags_dir);

        entries.push(ProviderEntry {
            qualified,
            bare,
            const_name,
            rel_path,
            kind_tokens,
        });
    }
    entries
}

/// The `EnchantmentProviderKind` value of one provider file.
fn provider_kind_tokens(raw: RawProvider, tags_dir: &Path) -> TokenStream {
    match raw.provider_type.as_str() {
        "minecraft:single" | "single" => {
            let enc_str = raw
                .enchantment
                .expect("single provider requires enchantment");
            let enc_bare = enc_str.strip_prefix("minecraft:").unwrap_or(&enc_str);
            let enc_ident = format_ident!("{}", enc_bare.to_shouty_snake_case());
            let level = match raw.level {
                Some(serde_json::Value::Number(n)) => n.as_i64().unwrap_or(1) as i32,
                _ => 1,
            };
            quote! {
                EnchantmentProviderKind::Single {
                    enchantment: &Enchantment::#enc_ident,
                    level: #level,
                }
            }
        }
        "minecraft:by_cost_with_difficulty" | "by_cost_with_difficulty" => {
            let enc_names = extract_enchantments(raw.enchantments.as_ref(), tags_dir);
            let enc_idents: Vec<_> = enc_names
                .iter()
                .map(|name| {
                    let bare = name.strip_prefix("minecraft:").unwrap_or(name);
                    let ident = format_ident!("{}", bare.to_shouty_snake_case());
                    quote! { &Enchantment::#ident }
                })
                .collect();
            let min_cost = raw.min_cost.unwrap_or(5);
            let max_cost_span = raw.max_cost_span.unwrap_or(17);
            quote! {
                EnchantmentProviderKind::ByCostWithDifficulty {
                    enchantments: &[ #( #enc_idents ),* ],
                    min_cost: #min_cost,
                    max_cost_span: #max_cost_span,
                }
            }
        }
        "minecraft:by_cost" | "by_cost" => {
            let enc_names = extract_enchantments(raw.enchantments.as_ref(), tags_dir);
            let enc_idents: Vec<_> = enc_names
                .iter()
                .map(|name| {
                    let bare = name.strip_prefix("minecraft:").unwrap_or(name);
                    let ident = format_ident!("{}", bare.to_shouty_snake_case());
                    quote! { &Enchantment::#ident }
                })
                .collect();
            let cost = match raw.cost {
                Some(serde_json::Value::Number(n)) => n.as_i64().unwrap_or(1) as i32,
                _ => 1,
            };
            quote! {
                EnchantmentProviderKind::ByCost {
                    enchantments: &[ #( #enc_idents ),* ],
                    cost: #cost,
                }
            }
        }
        other => panic!("unknown enchantment provider type: {other}"),
    }
}
