//! A record of entity selector parsing (`tests/golden/entity_selector_options.txt`; rewrite it
//! on purpose with `KVASIR_BLESS=1`), made before `EntitySelectorOption::modify_parser` was
//! split.
//!
//! One line per selector: the result of `parse` (`ok` or the error text), the reader's cursor,
//! then every parser field the options set. Hash maps (scores, advancements, NBT compounds) are
//! printed with their entries sorted, so the line does not depend on iteration order.

use std::fmt::Write;

use super::{EntitySelectorParser, EntitySelectorPredicate};
use crate::command::string_reader::StringReader;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const OPTIONS: &[&str] = &[
    "distance=5",
    "distance=..5",
    "distance=5..",
    "distance=1..2",
    "distance=0",
    "distance=-1",
    "distance=..-1",
    "distance=-2..-1",
    "distance=-1..1",
    "distance=-1..",
    "distance=abc",
    "level=5",
    "level=..5",
    "level=3..",
    "level=0",
    "level=-1",
    "level=..-1",
    "level=-3..-1",
    "level=-1..2",
    "level=x",
    "x=1.5",
    "y=-3",
    "z=7",
    "x=abc",
    "dx=2",
    "dy=-0.5",
    "dz=4",
    "dy=q",
    "x=1,y=2,z=3,dx=4,dy=5,dz=6",
    "x_rotation=..90",
    "x_rotation=10..20",
    "x_rotation=45",
    "y_rotation=-90..90",
    "y_rotation=abc",
    "limit=1",
    "limit=5",
    "limit=0",
    "limit=-1",
    "limit=abc",
    "sort=nearest",
    "sort=furthest",
    "sort=random",
    "sort=arbitrary",
    "sort=bogus",
    "limit=2,sort=nearest",
    "gamemode=survival",
    "gamemode=!creative",
    "gamemode=adventure,gamemode=creative",
    "gamemode=!creative,gamemode=!spectator",
    "gamemode=!creative,gamemode=survival",
    "gamemode=survival,gamemode=!creative",
    "gamemode=bogus",
    "gamemode=!bogus",
    "type=zombie",
    "type=minecraft:zombie",
    "type=!zombie",
    "type=player",
    "type=!player",
    "type=zombie,type=cow",
    "type=!zombie,type=!cow",
    "type=!zombie,type=cow",
    "type=pumpkin:zombie",
    "type=not_an_entity",
    "type=",
    "name=Bob",
    "name=!Bob",
    "name=Bob,name=Alice",
    "name=!Bob,name=!Alice",
    "name=Bob,name=!Alice",
    "name=",
    "tag=a",
    "tag=!a",
    "tag=",
    "tag=a,tag=b",
    "team=red",
    "team=!red",
    "team=red,team=blue",
    "team=!red,team=!blue",
    "team=red,team=!blue",
    "team=",
    "nbt={a:1b}",
    "nbt=!{a:1b}",
    "nbt={a:1b,b:\"x\"}",
    "nbt=1",
    "nbt={bad",
    "scores={obj=1}",
    "scores={obj=1..5}",
    "scores={ a = ..3 }",
    "scores={a=1,b=2}",
    "scores={a=1,}",
    "scores={}",
    "scores={a=x}",
    "scores={a=1",
    "scores=a",
    "scores={a}",
    "advancements={minecraft:story/root=true}",
    "advancements={a=false}",
    "advancements={ a = true , b = false }",
    "advancements={}",
    "advancements={a=maybe}",
    "advancements={a=true",
    "advancements={a}",
    "predicate=minecraft:tool/can_shear",
    "predicate=!minecraft:tool/can_shear",
    "predicate=",
    "bogus=1",
];

/// Appended after the first mutation run: ranges with exactly one end at zero and the other
/// open, which tell `n < 0` from `n <= 0`.
const ZERO_ENDS: &[&str] = &["distance=0..", "distance=..0", "level=0..", "level=..0"];

const SELECTORS: &[&str] = &["@e", "@a", "@p", "@s", "@r", "@n"];

fn predicate_text(predicate: &EntitySelectorPredicate) -> String {
    match predicate {
        EntitySelectorPredicate::EntityType(entity_type, invert) => {
            format!("EntityType({}, {invert})", entity_type.resource_name)
        }
        EntitySelectorPredicate::Scores(scores) => {
            let mut entries: Vec<String> =
                scores.iter().map(|(k, v)| format!("{k}={v:?}")).collect();
            entries.sort();
            format!("Scores({})", entries.join(","))
        }
        EntitySelectorPredicate::Advancements(advancements) => {
            let mut entries: Vec<String> = advancements
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            entries.sort();
            format!("Advancements({})", entries.join(","))
        }
        EntitySelectorPredicate::Nbt(compound, invert) => {
            let mut entries: Vec<String> = compound
                .child_tags
                .iter()
                .map(|(k, v)| format!("{k}={v:?}"))
                .collect();
            entries.sort();
            format!("Nbt({}, {invert})", entries.join(","))
        }
        other => format!("{other:?}"),
    }
}

fn parser_text(parser: &EntitySelectorParser<'_, '_>) -> String {
    let predicates: Vec<String> = parser.predicates.iter().map(predicate_text).collect();
    format!(
        "max {} distance {:?} level {:?} pos {:?} delta {:?} rotation {:?} order {:?} type {:?} \
         players_only {} current {} suggestions {:?} flags {:?} predicates [{}]",
        parser.max_selected,
        parser.distance,
        parser.experience_level,
        parser.pos,
        parser.delta,
        parser.rotation,
        parser.order,
        parser.entity_type.map(|t| t.resource_name),
        !parser.includes_entities,
        parser.is_current_entity,
        parser.suggestions,
        parser.flags,
        predicates.join("; ")
    )
}

fn record(input: &str) -> String {
    let mut reader = StringReader::new(input);
    let mut parser = EntitySelectorParser::new(&mut reader, true);
    let result = match parser.parse() {
        Ok(()) => "ok".to_owned(),
        Err(error) => format!("err {}", error.message.get_text()),
    };
    let state = parser_text(&parser);
    let cursor = parser.reader.cursor();
    format!("{input} -> {result} at {cursor}; {state}")
}

#[test]
fn entity_selector_options_parse_the_same() -> TestResult {
    let mut now = String::new();
    for option in OPTIONS {
        writeln!(now, "{}", record(&format!("@e[{option}]")))?;
    }
    for selector in SELECTORS {
        for option in [
            "limit=3",
            "type=cow",
            "distance=..2",
            "sort=random",
            "gamemode=survival",
        ] {
            writeln!(now, "{}", record(&format!("{selector}[{option}]")))?;
        }
    }
    for option in ZERO_ENDS {
        writeln!(now, "{}", record(&format!("@e[{option}]")))?;
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/entity_selector_options.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &now)?;
    } else {
        assert_eq!(std::fs::read_to_string(&path)?, now);
    }
    Ok(())
}
