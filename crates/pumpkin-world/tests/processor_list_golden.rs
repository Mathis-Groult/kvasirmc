//! Golden of the datapack processor-list reader (`register_dynamic_processor_list`, which turns
//! each raw processor into a `StructureProcessor`): every vanilla processor list shipped in
//! `assets/datapack`, then one synthetic list per processor type and variant the vanilla data
//! never uses. One line per list: result, processor kinds and an FNV-1a hash of the converted
//! value's `Debug` text. Rewrite `tests/golden/processor_lists.txt` on purpose with
//! `KVASIR_BLESS=1`.
//!
//! Hash-map backed values are converted in iteration order: block-state match properties (vanilla
//! zombie village lists) are sorted in the recorded text by [`sorted_properties`]; ignored-block
//! properties and static NBT data appear only in synthetic cases, with at most one entry each.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use pumpkin_world::generation::structure::template::processor::register_dynamic_processor_list;

/// Synthetic lists: (name, JSON).
const SYNTHETIC: &[(&str, &str)] = &[
    (
        "rot_no_blocks",
        r#"[{"processor_type":"minecraft:block_rot","integrity":0.5}]"#,
    ),
    (
        "rot_single_block",
        r#"[{"processor_type":"block_rot","integrity":0.25,"rottable_blocks":"minecraft:stone"}]"#,
    ),
    (
        "rot_single_bare_name",
        r#"[{"processor_type":"block_rot","integrity":0.25,"rottable_blocks":"cobblestone"}]"#,
    ),
    (
        "rot_single_registry_key",
        r#"[{"processor_type":"block_rot","integrity":0.25,"rottable_blocks":"minecraft:oak_planks"}]"#,
    ),
    (
        "rot_single_tag",
        r##"[{"processor_type":"block_rot","integrity":0.75,"rottable_blocks":"#minecraft:logs"}]"##,
    ),
    (
        "rot_single_unknown",
        r#"[{"processor_type":"block_rot","integrity":0.75,"rottable_blocks":"minecraft:no_such_block"}]"#,
    ),
    (
        "rot_list",
        r#"[{"processor_type":"block_rot","integrity":0.1,"rottable_blocks":["minecraft:stone","dirt","minecraft:no_such_block"]}]"#,
    ),
    (
        "rot_empty_list",
        r#"[{"processor_type":"block_rot","integrity":0.1,"rottable_blocks":[]}]"#,
    ),
    (
        "age",
        r#"{"processors":[{"processor_type":"minecraft:block_age","mossiness":0.3}]}"#,
    ),
    (
        "ignore_names",
        r#"[{"processor_type":"minecraft:block_ignore","blocks":["minecraft:structure_block","air","minecraft:no_such_block"]}]"#,
    ),
    (
        "ignore_state_no_props",
        r#"[{"processor_type":"block_ignore","blocks":[{"Name":"minecraft:stone"}]}]"#,
    ),
    (
        "ignore_state_one_prop",
        r#"[{"processor_type":"block_ignore","blocks":[{"Name":"minecraft:oak_log","Properties":{"axis":"x"}}]}]"#,
    ),
    (
        "ignore_state_bad_prop",
        r#"[{"processor_type":"block_ignore","blocks":[{"id":"minecraft:oak_log","properties":{"colour":"red"}}]}]"#,
    ),
    (
        "ignore_state_unknown",
        r#"[{"processor_type":"block_ignore","blocks":[{"Name":"minecraft:no_such_block"},"minecraft:dirt"]}]"#,
    ),
    (
        "gravity_default",
        r#"[{"processor_type":"minecraft:gravity"}]"#,
    ),
    (
        "gravity_world_surface",
        r#"[{"processor_type":"gravity","heightmap":"WORLD_SURFACE","offset":-1}]"#,
    ),
    (
        "gravity_ocean_floor_wg",
        r#"[{"processor_type":"gravity","heightmap":"OCEAN_FLOOR_WG","offset":2}]"#,
    ),
    (
        "gravity_ocean_floor",
        r#"[{"processor_type":"gravity","heightmap":"OCEAN_FLOOR"}]"#,
    ),
    (
        "gravity_motion_blocking",
        r#"[{"processor_type":"gravity","heightmap":"MOTION_BLOCKING"}]"#,
    ),
    (
        "gravity_motion_blocking_no_leaves",
        r#"[{"processor_type":"gravity","heightmap":"MOTION_BLOCKING_NO_LEAVES"}]"#,
    ),
    (
        "gravity_unknown",
        r#"[{"processor_type":"gravity","heightmap":"SOMETHING_ELSE","offset":3}]"#,
    ),
    (
        "protected",
        r##"[{"processor_type":"minecraft:protected_blocks","value":"#minecraft:features_cannot_replace"}]"##,
    ),
    (
        "unit_kinds",
        r#"[{"processor_type":"minecraft:blackstone_replace"},{"processor_type":"jigsaw_replacement"},{"processor_type":"minecraft:lava_submerged_block"},{"processor_type":"nop"}]"#,
    ),
    (
        "rule_minimal",
        r#"[{"processor_type":"minecraft:rule","rules":[{"input_predicate":{"predicate_type":"minecraft:always_true"},"output_state":"minecraft:stone"}]}]"#,
    ),
    (
        "rule_full",
        r#"[{"processor_type":"rule","rules":[{"position_predicate":{"predicate_type":"minecraft:linear_pos","min_dist":1,"max_dist":9,"min_chance":0.1,"max_chance":0.9},"input_predicate":{"predicate_type":"minecraft:random_block_match","block":"minecraft:gravel","probability":0.2},"location_predicate":{"predicate_type":"minecraft:block_match","block":"minecraft:water"},"output_state":{"Name":"minecraft:suspicious_gravel","Properties":{"dusted":"0"}},"block_entity_modifier":{"type":"minecraft:append_loot","loot_table":"minecraft:archaeology/trail_ruins_common"}}]}]"#,
    ),
    (
        "rule_modifiers",
        r#"[{"processor_type":"rule","rules":[
        {"input_predicate":{"predicate_type":"tag_match","tag":"minecraft:logs"},"output_state":"minecraft:dirt","block_entity_modifier":{"type":"minecraft:append_static","data":{"Lock":"key"}}},
        {"input_predicate":{"predicate_type":"always_true"},"output_state":"minecraft:dirt","block_entity_modifier":{"type":"append_static","data":7}},
        {"input_predicate":{"predicate_type":"always_true"},"output_state":"minecraft:dirt","block_entity_modifier":{"type":"minecraft:clear"}},
        {"input_predicate":{"predicate_type":"always_true"},"output_state":"minecraft:dirt","block_entity_modifier":{"type":"passthrough"}}]}]"#,
    ),
    (
        "rule_dropped",
        r#"[{"processor_type":"rule","rules":[
        {"input_predicate":{"predicate_type":"always_true"},"output_state":"minecraft:no_such_block"},
        {"input_predicate":{"predicate_type":"block_match","block":"minecraft:no_such_block"},"output_state":"minecraft:stone"},
        {"input_predicate":{"predicate_type":"always_true"},"location_predicate":{"predicate_type":"block_match","block":"minecraft:no_such_block"},"output_state":"minecraft:stone"},
        {"position_predicate":{"predicate_type":"axis_aligned_linear_pos","axis":"x","max_dist":4,"max_chance":1.0},"input_predicate":{"predicate_type":"blockstate_match","block_state":{"Name":"minecraft:oak_log","Properties":{"axis":"y"}}},"output_state":"minecraft:stone"}]}]"#,
    ),
    (
        "capped",
        r#"[{"processor_type":"minecraft:capped","limit":3,"delegate":{"processor_type":"block_age","mossiness":0.5}}]"#,
    ),
    (
        "capped_nested",
        r#"[{"processor_type":"capped","value":{"type":"minecraft:uniform","min_inclusive":1,"max_inclusive":4},"delegate":{"processor_type":"capped","limit":2,"delegate":{"processor_type":"block_rot","integrity":0.5,"rottable_blocks":"minecraft:stone"}}}]"#,
    ),
    (
        "mixed_order",
        r#"[{"processor_type":"nop"},{"processor_type":"block_age","mossiness":0.1},{"processor_type":"gravity","heightmap":"OCEAN_FLOOR"},{"processor_type":"protected_blocks","value":"minecraft:logs"}]"#,
    ),
    ("not_a_list", r#"{"processor_type":"nop"}"#),
];

fn fnv(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// The `Debug` text with the items of every `properties: [..]` list sorted, so a list built from a
/// hash map reads the same in every run.
fn sorted_properties(text: &str) -> String {
    const OPEN: &str = "properties: [";
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find(OPEN) {
        let (head, tail) = rest.split_at(start + OPEN.len());
        out.push_str(head);
        let mut depth = 0usize;
        let mut end = tail.len();
        let mut items = Vec::new();
        let mut item_start = 0;
        for (i, c) in tail.char_indices() {
            match c {
                '[' | '(' | '{' => depth += 1,
                ']' if depth == 0 => {
                    end = i;
                    break;
                }
                ']' | ')' | '}' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    items.push(tail[item_start..i].trim());
                    item_start = i + 1;
                }
                _ => {}
            }
        }
        let last = tail[item_start..end].trim();
        if !last.is_empty() {
            items.push(last);
        }
        items.sort_unstable();
        out.push_str(&items.join(", "));
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

fn record(out: &mut String, name: &str, json: &str) -> std::fmt::Result {
    match register_dynamic_processor_list(&format!("kvasir_golden:{name}"), json) {
        Ok(list) => {
            let kinds: Vec<String> = list
                .iter()
                .map(|p| {
                    let text = format!("{p:?}");
                    text.split([' ', '(', '{']).next().unwrap_or("").to_owned()
                })
                .collect();
            writeln!(
                out,
                "{name} ok n={} kinds={} hash={:016x}",
                list.len(),
                kinds.join(","),
                fnv(&sorted_properties(&format!("{list:?}")))
            )
        }
        Err(_) => writeln!(out, "{name} err"),
    }
}

fn vanilla_lists(
    dir: &Path,
    prefix: &str,
    found: &mut Vec<(String, PathBuf)>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if path.is_dir() {
            vanilla_lists(&path, &format!("{prefix}{stem}/"), found)?;
        } else if path.extension().is_some_and(|e| e == "json") {
            found.push((format!("{prefix}{stem}"), path));
        }
    }
    Ok(())
}

#[test]
fn processor_lists_convert_the_same() -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/datapack/data/minecraft/worldgen/processor_list");
    let mut lists = Vec::new();
    vanilla_lists(&root, "", &mut lists)?;
    lists.sort();
    assert!(
        lists.len() >= 40,
        "vanilla processor lists missing: {}",
        lists.len()
    );

    let mut now = String::new();
    for (name, path) in &lists {
        record(
            &mut now,
            &format!("vanilla {name}"),
            &std::fs::read_to_string(path)?,
        )?;
    }
    for (name, json) in SYNTHETIC {
        record(&mut now, &format!("synthetic {name}"), json)?;
    }

    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/processor_lists.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&golden, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&golden)?, now);
    Ok(())
}
