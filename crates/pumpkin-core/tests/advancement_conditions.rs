//! A record of what `CriterionConditions::from` makes of every advancement condition in the game
//! data, made before the conversion was split into small functions
//! (`tests/golden/advancement_conditions.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! One line per advancement (sorted by id): how many criteria it has and a hash of the debug text
//! of each converted condition, in order. A second block counts the conditions per kind, so a kind
//! that no advancement uses can be seen; those get a synthetic case at the end.

use std::collections::BTreeMap;
use std::fmt::Write;

use pumpkin_core::entity::player::advancement::predicate::CriterionConditions;
use pumpkin_data::Advancement;
use pumpkin_data::advancement_data::AdvancementCondition;

/// FNV-1a: a hash that does not change with the Rust version.
fn fnv(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in text.bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// The name of an `AdvancementCondition` variant, the first word of its debug text.
fn kind_of(condition: &AdvancementCondition) -> String {
    let text = format!("{condition:?}");
    text.split(|c: char| !c.is_alphanumeric())
        .next()
        .unwrap_or_default()
        .to_string()
}

fn record() -> Result<String, std::fmt::Error> {
    let mut advancements = Advancement::get_advancements_list().to_vec();
    advancements.sort_by_key(|advancement| advancement.id.to_string());

    let mut out = String::new();
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for advancement in advancements {
        let mut texts = String::new();
        for criterion in advancement.criteria_definitions {
            *kinds.entry(kind_of(&criterion.conditions)).or_default() += 1;
            let converted = CriterionConditions::from(criterion.conditions);
            writeln!(texts, "{}: {converted:?}", criterion.name)?;
        }
        writeln!(
            out,
            "{} criteria={} hash={:016x}",
            advancement.id,
            advancement.criteria_definitions.len(),
            fnv(&texts)
        )?;
    }
    for (kind, count) in kinds {
        writeln!(out, "kind {kind} {count}")?;
    }
    for condition in synthetic_conditions() {
        writeln!(
            out,
            "synthetic {condition:?} => {:?}",
            CriterionConditions::from(condition)
        )?;
    }
    Ok(out)
}

/// Kinds that no advancement of the game data uses.
const fn synthetic_conditions() -> [AdvancementCondition; 5] {
    [
        AdvancementCondition::UsedEnderEye,
        AdvancementCondition::Custom("{}"),
        AdvancementCondition::Custom(r#"{"player": {"type_specific": {"type": "player"}}}"#),
        AdvancementCondition::Custom("not json"),
        AdvancementCondition::Empty,
    ]
}

#[test]
fn conditions_are_converted_as_before() -> Result<(), Box<dyn std::error::Error>> {
    let now = record()?;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("advancement_conditions.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, now);
    Ok(())
}
