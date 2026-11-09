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
use pumpkin_data::advancement_data::{
    AdvancementCondition, InventoryChangedCondition, InventorySlotsCondition, ItemCondition,
};

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

/// The two ends of a bound, either of which may be missing.
type Ends = (Option<i64>, Option<i64>);

/// Conditions the game data does not contain: kinds that no advancement uses, and bounds with
/// only one end given (the data always gives both or neither).
fn synthetic_conditions() -> Vec<AdvancementCondition> {
    let item = |count_min, count_max, durability_min, durability_max| ItemCondition {
        items: &["minecraft:stone"],
        count_min,
        count_max,
        durability_min,
        durability_max,
        ..ItemCondition::default()
    };
    let slots = |empty: Ends, full: Ends, occupied: Ends| InventorySlotsCondition {
        empty_min: empty.0,
        empty_max: empty.1,
        full_min: full.0,
        full_max: full.1,
        occupied_min: occupied.0,
        occupied_max: occupied.1,
    };
    let inventory: &'static InventoryChangedCondition =
        Box::leak(Box::new(InventoryChangedCondition {
            items: Box::leak(Box::new([
                item(Some(2), None, None, None),
                item(None, Some(5), None, None),
                ItemCondition {
                    tag: Some("c:ores"),
                    ..ItemCondition::default()
                },
            ])),
            slots: Some(slots((Some(1), None), (None, Some(3)), (Some(4), None))),
        }));
    let other_slots: &'static InventoryChangedCondition =
        Box::leak(Box::new(InventoryChangedCondition {
            items: &[],
            slots: Some(slots((None, Some(6)), (Some(7), None), (None, Some(8)))),
        }));
    let interacted = |item| AdvancementCondition::PlayerInteractedWithEntity {
        entity: None,
        item: Some(item),
        player: None,
    };
    vec![
        AdvancementCondition::UsedEnderEye,
        AdvancementCondition::Custom("{}"),
        AdvancementCondition::Custom(r#"{"player": {"type_specific": {"type": "player"}}}"#),
        AdvancementCondition::Custom("not json"),
        AdvancementCondition::Empty,
        AdvancementCondition::InventoryChanged(inventory),
        AdvancementCondition::InventoryChanged(other_slots),
        interacted(item(Some(1), None, None, None)),
        interacted(item(None, Some(4), None, None)),
        interacted(item(None, None, Some(3), None)),
        interacted(item(None, None, None, Some(7))),
        interacted(item(Some(1), Some(2), Some(3), Some(4))),
        interacted(item(None, None, None, None)),
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
