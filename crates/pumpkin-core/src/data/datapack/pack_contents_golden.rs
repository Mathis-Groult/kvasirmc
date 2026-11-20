//! A record of what `load_pack_contents` collects, made before it is split
//! (`tests/golden/pack_contents.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! The vanilla pack and the bundled ones are loaded; for each, every count it returns, the size
//! of every accumulator, and a hash of each accumulator's sorted keys (and values where they
//! are plain data) are written.

use std::collections::HashMap;
use std::fmt::Write;
use std::path::Path;

use crate::data::datapack::{
    PackContentAccumulators, TradeRegistry, find_bundled_datapack_dir, load_pack_contents,
};

fn fnv(hash: u64, text: &str) -> u64 {
    text.bytes().fold(hash, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// `len` and a hash of the keys, each with its value's text when `value` gives one.
fn map_text<V>(map: &HashMap<String, V>, value: impl Fn(&V) -> String) -> String {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for key in keys {
        hash = fnv(hash, key);
        if let Some(v) = map.get(key) {
            hash = fnv(hash, &value(v));
        }
        hash = fnv(hash, "\n");
    }
    format!("{} {hash:016x}", map.len())
}

fn key_list<V>(map: &HashMap<String, V>) -> String {
    let mut ids: Vec<&String> = map.keys().collect();
    ids.sort();
    format!("{ids:?}")
}

/// Everything a pack's loaders fill.
#[derive(Default)]
struct Held {
    recipes: Vec<crate::data::datapack::DynamicRecipe>,
    functions: HashMap<String, Vec<String>>,
    function_tags: HashMap<String, Vec<String>>,
    test_instances: HashMap<String, crate::data::datapack::test_loader::TestInstance>,
    damage_types: HashMap<String, crate::data::datapack::damage_type_loader::DamageTypeDefinition>,
    context_int_providers: crate::data::datapack::context_provider_loader::ContextProviderRegistry,
    context_float_providers:
        crate::data::datapack::context_provider_loader::ContextProviderRegistry,
    loot_tables: HashMap<String, std::sync::Arc<pumpkin_util::loot_table::DynamicLootTable>>,
    dynamic_registries: HashMap<String, HashMap<String, crate::data::datapack::RegistryEntryData>>,
    trade_registry: TradeRegistry,
    advancements:
        HashMap<String, crate::data::datapack::advancement_loader::DynamicAdvancementDefinition>,
    predicates: HashMap<String, serde_json::Value>,
    item_modifiers: HashMap<String, serde_json::Value>,
    slot_sources: HashMap<String, serde_json::Value>,
    tags: HashMap<String, HashMap<String, crate::data::datapack::tag_loader::DynamicTagEntry>>,
}

impl Held {
    fn load(&mut self, path: &Path) -> crate::data::datapack::PackContentCounts {
        load_pack_contents(
            path,
            &mut PackContentAccumulators {
                recipes: &mut self.recipes,
                functions: &mut self.functions,
                function_tags: &mut self.function_tags,
                test_instances: &mut self.test_instances,
                damage_types: &mut self.damage_types,
                context_int_providers: &mut self.context_int_providers,
                context_float_providers: &mut self.context_float_providers,
                loot_tables: &mut self.loot_tables,
                dynamic_registries: &mut self.dynamic_registries,
                trade_registry: &mut self.trade_registry,
                advancements: &mut self.advancements,
                predicates: &mut self.predicates,
                item_modifiers: &mut self.item_modifiers,
                slot_sources: &mut self.slot_sources,
                tags: &mut self.tags,
            },
        )
    }

    /// One line per accumulator. Values are left out where their text is not stable (NBT
    /// compounds are hash maps).
    fn lines(&self) -> Vec<(&'static str, String)> {
        let json = |v: &serde_json::Value| v.to_string();
        vec![
            ("recipes", self.recipes.len().to_string()),
            ("functions", map_text(&self.functions, |v| format!("{v:?}"))),
            (
                "function_tags",
                map_text(&self.function_tags, |v| format!("{v:?}")),
            ),
            (
                "test_instances",
                map_text(&self.test_instances, |_| String::new()),
            ),
            (
                "damage_types",
                map_text(&self.damage_types, |_| String::new()),
            ),
            (
                "context_int",
                map_text(&self.context_int_providers, |_| String::new()),
            ),
            (
                "context_float",
                map_text(&self.context_float_providers, |_| String::new()),
            ),
            (
                "loot_tables",
                map_text(&self.loot_tables, |_| String::new()),
            ),
            ("dynamic", map_text(&self.dynamic_registries, key_list)),
            (
                "trades",
                map_text(&self.trade_registry.trades, |_| String::new()),
            ),
            (
                "trade_sets",
                map_text(&self.trade_registry.trade_sets, |_| String::new()),
            ),
            (
                "trade_tags",
                map_text(&self.trade_registry.trade_tags, |v| format!("{v:?}")),
            ),
            (
                "advancements",
                map_text(&self.advancements, |_| String::new()),
            ),
            ("predicates", map_text(&self.predicates, json)),
            ("item_modifiers", map_text(&self.item_modifiers, json)),
            ("slot_sources", map_text(&self.slot_sources, json)),
            ("tags", map_text(&self.tags, key_list)),
        ]
    }
}

fn counts_text(counts: &crate::data::datapack::PackContentCounts) -> String {
    format!(
        "recipes {} functions {} test_instances {} damage_types {} int {} float {} loot {} \
         structures {} pools {} processors {} dynamic {} trade_sets {} trades {} trade_tags {} \
         advancements {} predicates {} item_modifiers {} slot_sources {} tags {}",
        counts.recipes,
        counts.functions,
        counts.test_instances,
        counts.damage_types,
        counts.int_providers,
        counts.float_providers,
        counts.loot_tables,
        counts.structures,
        counts.template_pools,
        counts.processor_lists,
        counts.dynamic_registries,
        counts.trade_sets,
        counts.trades,
        counts.trade_tags,
        counts.advancements,
        counts.predicates,
        counts.item_modifiers,
        counts.slot_sources,
        counts.tags
    )
}

fn pack(name: &str, path: &Path, report: &mut String) {
    let mut held = Held::default();
    let counts = held.load(path);
    let _ = writeln!(report, "{name} counts {}", counts_text(&counts));
    for (what, text) in held.lines() {
        let _ = writeln!(report, "{name} {what}: {text}");
    }
}

/// A pack of one small file of each kind, so that a mix-up between loaders shows.
fn synthetic_pack(root: &std::path::Path) -> std::io::Result<()> {
    let files: [(&str, &str); 16] = [
        ("function/a.mcfunction", "say hi\nsay there\n"),
        ("tags/function/t.json", r#"{"values":["demo:a"]}"#),
        ("tags/item/i.json", r#"{"values":["minecraft:stone"]}"#),
        ("tags/villager_trade/v.json", r#"{"values":["demo:x"]}"#),
        (
            "item_modifier/m.json",
            r#"{"function":"minecraft:set_count","count":2}"#,
        ),
        ("slot_source/s.json", r#"{"type":"minecraft:empty"}"#),
        (
            "predicate/p.json",
            r#"{"condition":"minecraft:random_chance","chance":0.5}"#,
        ),
        (
            "loot_table/l.json",
            r#"{"type":"minecraft:generic","pools":[]}"#,
        ),
        (
            "damage_type/d.json",
            r#"{"message_id":"demo","scaling":"never","exhaustion":0.1}"#,
        ),
        (
            "context_int_provider/i.json",
            r#"{"type":"minecraft:constant","value":3}"#,
        ),
        (
            "context_float_provider/f.json",
            r#"{"type":"minecraft:constant","value":1.5}"#,
        ),
        (
            "recipe/r.json",
            r#"{"type":"minecraft:crafting_shapeless","ingredients":["minecraft:stone"],"result":{"id":"minecraft:cobblestone"}}"#,
        ),
        ("trade_set/ts.json", r##"{"trades":"#demo:x","amount":1}"##),
        (
            "worldgen/template_pool/tp.json",
            r#"{"fallback":"minecraft:empty","elements":[]}"#,
        ),
        ("worldgen/processor_list/pl.json", r#"{"processors":[]}"#),
        (
            "advancement/a.json",
            r#"{"criteria":{"c":{"trigger":"minecraft:impossible"}}}"#,
        ),
    ];
    for (name, text) in files {
        let path = root.join("data/demo").join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)?;
    }
    Ok(())
}

#[test]
fn pack_contents() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let vanilla = manifest.join("../../assets/datapack");
    let mut report = String::new();
    if vanilla.join("data/minecraft").is_dir() {
        pack("vanilla", &vanilla, &mut report);
    }
    for name in [
        "trade_rebalance",
        "redstone_experiments",
        "minecart_improvements",
    ] {
        let path = find_bundled_datapack_dir(name).ok_or(name)?;
        pack(name, &path, &mut report);
    }

    let folder = tempfile::tempdir()?;
    synthetic_pack(folder.path())?;
    pack("synthetic", folder.path(), &mut report);

    let path = manifest.join("tests/golden/pack_contents.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
