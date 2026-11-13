mod registry;
mod static_items;

use heck::{ToPascalCase, ToShoutySnakeCase};
use indexmap::IndexMap;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use registry::{
    ProfessionTokens, generate_trade_sets, load_raw_trade_tags, load_trade_sets,
    load_villager_trades, profession_items_tokens, profession_tokens, villager_type_tokens,
};
use serde::Deserialize;
use serde_json::Value;
use std::fs;

#[derive(Deserialize)]
struct VillagerDataJson {
    professions: IndexMap<String, ProfessionJson>,
    types: IndexMap<String, String>,
}

#[derive(Deserialize)]
struct ProfessionJson {
    name: NameJson,
    requested_items: Vec<String>,
    work_sound: Option<String>,
    #[serde(default)]
    trade_sets: IndexMap<String, String>,
}

#[derive(Deserialize)]
struct NameJson {
    translate: String,
}

#[derive(Deserialize, Clone)]
struct TradeSetJson {
    trades: String, // Tag like "#minecraft:armorer/level_1"
    #[serde(default = "default_amount")]
    amount: f32,
}

fn default_amount() -> f32 {
    1.0
}

#[derive(Deserialize, Clone)]
struct TradeJson {
    wants: TradeItemJson,
    #[serde(alias = "wants_b")]
    additional_wants: Option<TradeItemJson>,
    gives: TradeItemJson,
    max_uses: Option<f32>,
    xp: Option<f32>,
    #[serde(alias = "price_multiplier")]
    reputation_discount: Option<f32>,
    #[serde(
        rename = "given_item_modifier",
        default,
        deserialize_with = "crate::loot_table::one_or_many"
    )]
    given_item_modifiers: Vec<Value>,
    merchant_predicate: Option<Value>,
}

#[derive(Deserialize, Clone)]
struct TradeItemJson {
    id: String,
    count: Option<f32>,
}

fn walk_json_files<F>(dir: &std::path::Path, base_dir: &std::path::Path, callback: &mut F)
where
    F: FnMut(String, String),
{
    if let Ok(entries) = fs::read_dir(dir) {
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.path());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                walk_json_files(&path, base_dir, callback);
            } else if path.extension().is_some_and(|ext| ext == "json") {
                let rel = path.strip_prefix(base_dir).unwrap();
                let stem = rel.with_extension("");
                let key = stem.to_string_lossy().replace('\\', "/");
                let content = fs::read_to_string(&path).expect("Failed to read JSON file");
                callback(key, content);
            }
        }
    }
}

pub fn build() -> TokenStream {
    let data: VillagerDataJson =
        serde_json::from_str(&fs::read_to_string("../../assets/villager_data.json").unwrap())
            .expect("Failed to parse villager_data.json");

    let trade_sets = load_trade_sets();
    let raw_trade_tags = load_raw_trade_tags();
    let villager_trades = load_villager_trades();

    let (trade_set_data, generated_trade_sets) =
        generate_trade_sets(&trade_sets, &raw_trade_tags, &villager_trades);
    let professions = profession_tokens(&data, &trade_sets, &generated_trade_sets);
    let profession_items = profession_items_tokens(&professions);
    let villager_types = villager_type_tokens(&data);

    let imports_and_villager_trade_item_tokens =
        static_items::imports_and_villager_trade_item_tokens();
    quote! {
        #imports_and_villager_trade_item_tokens

        #(#trade_set_data)*

        #profession_items

        #villager_types
    }
}
