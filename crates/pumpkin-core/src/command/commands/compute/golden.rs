//! Records of what the compound number providers answer for a corpus of expressions, made before
//! the evaluation functions were split into small ones (`tests/golden/compute_*.txt`; rewrite them
//! on purpose with `KVASIR_BLESS=1`).
//!
//! `uniform` is random, so only its deterministic cases are in the corpus, and `score` needs a
//! scoreboard, so it is not.

use super::*;
use std::fmt::Write;

fn expressions() -> Vec<String> {
    let mut all = binary_expressions();
    all.extend(list_and_power_expressions());
    all.extend(unary_and_other_expressions());
    all
}

fn binary_expressions() -> Vec<String> {
    let mut all = Vec::new();
    for op in [
        "add",
        "sum",
        "sub",
        "mul",
        "product",
        "div",
        "floor_div",
        "floor_mod",
        "mod",
        "min",
        "max",
    ] {
        for (left, right) in [
            ("7", "2"),
            ("-7", "2"),
            ("7", "-2"),
            ("0", "0"),
            ("5", "0"),
            ("1.5d", "2.5d"),
            ("\"3\"", "4"),
            ("\"abc\"", "1"),
            ("1e308d", "1e308d"),
            ("-0.0d", "0"),
        ] {
            all.push(format!("{{type:\"{op}\",left:{left},right:{right}}}"));
        }
        all.push(format!("{{type:\"{op}\",left:1}}"));
        all.push(format!("{{type:\"{op}\",right:1}}"));
        all.push(format!("{{type:\"minecraft:{op}\",left:9,right:4}}"));
        all.push(format!("{{type:\"{op}\",left:{{type:\"constant\",value:6}},right:{{type:\"mul\",left:2,right:3}}}}"));
    }
    all
}

fn list_and_power_expressions() -> Vec<String> {
    let mut all = Vec::new();
    for op in [
        "add", "sum", "mul", "product", "min", "max", "avg", "average",
    ] {
        for inputs in [
            "[]",
            "[5]",
            "[1,2,3]",
            "[1.5d,\"2\"]",
            "[1,\"abc\"]",
            "[-1,-2,-3]",
            "[0.1d,0.2d,0.3d]",
            "[1e308d,1e308d]",
        ] {
            all.push(format!("{{type:\"{op}\",inputs:{inputs}}}"));
        }
        all.push(format!("{{type:\"{op}\",inputs:5}}"));
    }
    for op in ["pow"] {
        for (base, exponent) in [
            ("2", "3"),
            ("0", "0"),
            ("-8", "0.5d"),
            ("2", "-1"),
            ("0", "-1"),
            ("10", "400"),
        ] {
            all.push(format!("{{type:\"{op}\",base:{base},exponent:{exponent}}}"));
        }
        all.push(format!("{{type:\"{op}\",base:2}}"));
        all.push(format!("{{type:\"{op}\",exponent:2}}"));
    }
    all
}

fn unary_and_other_expressions() -> Vec<String> {
    let numbers = [
        "7", "-7", "2", "-2", "0", "1.5d", "2.5d", "\"3\"", "\"abc\"", "1e308d", "-0.0d",
    ];
    let mut all = Vec::new();
    for op in [
        "abs", "negate", "sqrt", "sin", "cos", "ceil", "floor", "round", "truncate",
    ] {
        for input in numbers {
            all.push(format!("{{type:\"{op}\",input:{input}}}"));
        }
        all.push(format!("{{type:\"{op}\"}}"));
    }
    for input in [
        "5",
        "5.7d",
        "-5.7d",
        "\"12\"",
        "\"abc\"",
        "{type:\"constant\",value:3}",
    ] {
        all.push(format!("{{type:\"from_int\",input:{input}}}"));
    }
    all.push("{type:\"from_int\"}".to_owned());
    for (min, max) in [("1", "1"), ("2", "1"), ("1.5d", "1.5d"), ("1", "\"abc\"")] {
        all.push(format!("{{type:\"uniform\",min:{min},max:{max}}}"));
    }
    all.push("{type:\"uniform\",min:1}".to_owned());
    all.push("{type:\"uniform\",max:1}".to_owned());
    for value in [
        "5",
        "2.5d",
        "\"2.5\"",
        "\"abc\"",
        "{type:\"add\",left:1,right:2}",
        "[1,2]",
    ] {
        all.push(format!("{{type:\"constant\",value:{value}}}"));
        all.push(format!("{{value:{value}}}"));
        all.push(format!("{{type:\"unknown_kind\",value:{value}}}"));
    }
    all.push("{type:\"constant\"}".to_owned());
    all.push("{type:\"unknown_kind\"}".to_owned());
    all.push("{}".to_owned());
    all.push("{type:5,value:1}".to_owned());
    all
}

fn int_binary_expressions() -> Vec<String> {
    let mut all = Vec::new();
    for op in [
        "add",
        "sum",
        "sub",
        "mul",
        "product",
        "div",
        "floor_div",
        "floor_mod",
        "mod",
        "min",
        "max",
    ] {
        for (left, right) in [
            ("7", "2"),
            ("-7", "2"),
            ("7", "-2"),
            ("-7", "-2"),
            ("0", "0"),
            ("5", "0"),
            ("3", "4"),
            ("\"3\"", "4"),
            ("\"abc\"", "1"),
            ("1.5d", "2"),
            ("-1.5d", "2"),
            ("100000000000L", "3"),
            ("9223372036854775807L", "1L"),
            ("9223372036854775807L", "9223372036854775807L"),
            ("-9223372036854775807L", "2"),
        ] {
            all.push(format!("{{type:\"{op}\",left:{left},right:{right}}}"));
        }
        all.push(format!("{{type:\"{op}\",left:1}}"));
        all.push(format!("{{type:\"{op}\",right:1}}"));
        all.push(format!("{{type:\"minecraft:{op}\",left:9,right:4}}"));
        all.push(format!(
            "{{type:\"{op}\",left:{{type:\"constant\",value:6}},right:{{type:\"mul\",left:2,right:3}}}}"
        ));
    }
    all
}

fn int_list_and_power_expressions() -> Vec<String> {
    let mut all = Vec::new();
    for op in [
        "add", "sum", "mul", "product", "min", "max", "avg", "average",
    ] {
        for inputs in [
            "[]",
            "[5]",
            "[1,2,3]",
            "[1.5d,\"2\"]",
            "[1,\"abc\"]",
            "[-1,-2,-3]",
            "[7,2]",
            "[-7,2]",
            "[9223372036854775807L,1]",
            "[9223372036854775807L,9223372036854775807L]",
        ] {
            all.push(format!("{{type:\"{op}\",inputs:{inputs}}}"));
        }
        all.push(format!("{{type:\"{op}\",inputs:5}}"));
    }
    for (base, exponent) in [
        ("2", "3"),
        ("-2", "3"),
        ("0", "0"),
        ("2", "-1"),
        ("0", "-1"),
        ("10", "30"),
        ("3", "100"),
        ("2", "4294967296L"),
        ("1.5d", "2"),
    ] {
        all.push(format!("{{type:\"pow\",base:{base},exponent:{exponent}}}"));
    }
    all.push("{type:\"pow\",base:2}".to_owned());
    all.push("{type:\"pow\",exponent:2}".to_owned());
    all
}

fn int_unary_and_other_expressions() -> Vec<String> {
    let mut all = Vec::new();
    for op in ["abs", "negate"] {
        for input in [
            "7",
            "-7",
            "0",
            "1.5d",
            "-1.5d",
            "\"3\"",
            "\"abc\"",
            "9223372036854775807L",
        ] {
            all.push(format!("{{type:\"{op}\",input:{input}}}"));
        }
        all.push(format!("{{type:\"{op}\"}}"));
    }
    for (min, max) in [("1", "1"), ("2", "1"), ("-3", "-3"), ("1", "\"abc\"")] {
        all.push(format!("{{type:\"uniform\",min:{min},max:{max}}}"));
    }
    all.push("{type:\"uniform\",min:1}".to_owned());
    all.push("{type:\"uniform\",max:1}".to_owned());
    for value in [
        "5",
        "2.5d",
        "\"2\"",
        "\"abc\"",
        "{type:\"add\",left:1,right:2}",
        "[1,2]",
        "7b",
        "300s",
    ] {
        all.push(format!("{{type:\"constant\",value:{value}}}"));
        all.push(format!("{{value:{value}}}"));
        all.push(format!("{{type:\"unknown_kind\",value:{value}}}"));
    }
    all.push("{type:\"constant\"}".to_owned());
    all.push("{type:\"unknown_kind\"}".to_owned());
    all.push("{}".to_owned());
    all.push("{type:5,value:1}".to_owned());
    all
}

fn int_expressions() -> Vec<String> {
    let mut all = int_binary_expressions();
    all.extend(int_list_and_power_expressions());
    all.extend(int_unary_and_other_expressions());
    all
}

fn parse_compound_for_golden(text: &str) -> Result<NbtCompound, String> {
    match SnbtParser::parse_for_commands(&mut StringReader::new(text)) {
        Ok(NbtTag::Compound(compound)) => Ok(compound),
        Ok(_) => Err(format!("not a compound: {text}")),
        Err(_) => Err(format!("does not parse: {text}")),
    }
}

/// One line per expression: its text and what `evaluate` answers (`show` writes a value).
fn check<T>(
    name: &str,
    expressions: Vec<String>,
    evaluate: impl Fn(&NbtCompound, &ComputeContext) -> Result<T, CommandSyntaxError>,
    show: impl Fn(T) -> String,
) -> Result<(), Box<dyn std::error::Error>> {
    let source = CommandSource::dummy();
    let ctx = ComputeContext {
        source: &source,
        target_type: TargetType::Default,
        block_pos: None,
        target_entity: None,
        datapack_manager: None,
        depth: Cell::new(0),
    };
    let mut now = String::new();
    for text in expressions {
        let compound = parse_compound_for_golden(&text)?;
        match evaluate(&compound, &ctx) {
            Ok(value) => writeln!(now, "{text} => ok {}", show(value))?,
            Err(error) => writeln!(now, "{text} => err {:?}", error.message)?,
        }
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.txt"));
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, now);
    Ok(())
}

#[test]
fn evaluate_compound_float_answers_as_before() -> Result<(), Box<dyn std::error::Error>> {
    check(
        "compute_float",
        expressions(),
        evaluate_compound_float,
        |value| format!("{:016x}", value.to_bits()),
    )
}

#[test]
fn evaluate_compound_int_answers_as_before() -> Result<(), Box<dyn std::error::Error>> {
    check(
        "compute_int",
        int_expressions(),
        evaluate_compound_int,
        |value| value.to_string(),
    )
}
