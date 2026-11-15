//! Golden of the SNBT number rules (`SnbtParser::parse_integer_literal`, `float_literal`):
//! integer literals over every sign, base, suffix and range boundary, float literals over every
//! pathway (`XXX.[yyy][eZZZ][suffix]`, `.yyy..`, `XXXeZZZ..`, `XXX[eZZZ]suffix`), separators and
//! out-of-range values, then the suggestions for a few prefixes. One line per input: the parse
//! result (`Debug`), how far the reader got, and for errors the message. Rewrite
//! `tests/golden/snbt_numbers.txt` on purpose with `KVASIR_BLESS=1`.

use std::fmt::Write as _;
use std::path::Path;

use pumpkin_command::snbt::SnbtParser;
use pumpkin_command::string_reader::StringReader;
use pumpkin_command::suggestion::suggestions::SuggestionsBuilder;

const SIGNS: &[&str] = &["", "+", "-"];

const INTEGER_SUFFIXES: &[&str] = &[
    "", "b", "B", "s", "S", "i", "I", "l", "L", "ub", "sb", "us", "ss", "ui", "si", "ul", "sl",
    "UB", "SL", "u", "x", "f", "d",
];

/// Decimal magnitudes around every type boundary, signed and unsigned.
const DECIMALS: &[&str] = &[
    "0",
    "1",
    "127",
    "128",
    "255",
    "256",
    "32767",
    "32768",
    "65535",
    "65536",
    "2147483647",
    "2147483648",
    "4294967295",
    "4294967296",
    "9223372036854775807",
    "9223372036854775808",
    "18446744073709551615",
    "18446744073709551616",
    "1_000",
    "1__0",
    "007",
];

/// Hexadecimal and binary magnitudes around the same boundaries.
const PREFIXED: &[&str] = &[
    "0x0",
    "0x7f",
    "0x80",
    "0xff",
    "0x100",
    "0X7FFF",
    "0x8000",
    "0xffff",
    "0x7fffffff",
    "0x80000000",
    "0xffffffff",
    "0x100000000",
    "0x7fffffffffffffff",
    "0xffffffffffffffff",
    "0x1_0000",
    "0b0",
    "0b1",
    "0B1111111",
    "0b10000000",
    "0b11111111",
    "0b100000000",
    "0b1111_1111",
    "0x",
    "0b",
    "0xg",
    "0b2",
];

/// Float literals; each is also tried with every sign.
const FLOATS: &[&str] = &[
    "1.",
    "1.5",
    "1.5f",
    "1.5F",
    "1.5d",
    "1.5D",
    ".5",
    ".5f",
    ".5e2",
    ".5e2d",
    "1e3",
    "1E3",
    "1e+3",
    "1e-3",
    "1e3f",
    "1e3d",
    "1f",
    "1d",
    "1.e3",
    "1.5e-3d",
    "1_0.2_5",
    "1_0.2_5e1_0",
    "1e",
    "1e+",
    "1ef",
    ".",
    ".e3",
    ".f",
    "1e400",
    "1e400d",
    "1e40f",
    "3.4028235e38f",
    "3.5e38f",
    "1e-400",
    "1e-50f",
    "1.7976931348623157e308",
    "1.8e308",
    "0.0",
    "00.5",
    "1 .5",
    "1. 5",
    "1.5 f",
    "1 e3",
    "1.5e 3",
    "1.5x",
    "1.5ub",
];

/// Prefixes whose suggestions are recorded.
const SUGGEST: &[&str] = &[
    "", "1", "1.", "1.5", "1e", "-", ".", "0x", "1u", "1s", "1.5e3",
];

fn fnv(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn record(out: &mut String, input: &str) -> std::fmt::Result {
    let mut reader = StringReader::new(input);
    let result = SnbtParser::parse_for_commands(&mut reader);
    let cursor = reader.cursor();
    match result {
        Ok(tag) => writeln!(out, "{input:?} ok {tag:?} at {cursor}"),
        Err(error) => writeln!(
            out,
            "{input:?} err {:?} ctx {:?} at {cursor} #{:016x}",
            error.message.clone().get_text(),
            error.context.map(|c| c.cursor),
            fnv(&format!("{:?}", error.message))
        ),
    }
}

#[test]
fn snbt_numbers_parse_the_same() -> Result<(), Box<dyn std::error::Error>> {
    let mut now = String::new();
    for sign in SIGNS {
        for digits in DECIMALS.iter().chain(PREFIXED) {
            for suffix in INTEGER_SUFFIXES {
                record(&mut now, &format!("{sign}{digits}{suffix}"))?;
            }
        }
        for float in FLOATS {
            record(&mut now, &format!("{sign}{float}"))?;
        }
    }
    for list in [
        "[1b,2s,3]",
        "[B;1b,-1ub,255ub]",
        "[I;1,0x7fffffff,-1]",
        "[L;1L,-1l,0xffffffffffffffffuL]",
        "[1.5f,2d]",
    ] {
        record(&mut now, list)?;
    }
    for prefix in SUGGEST {
        let suggestions = SnbtParser::parse_for_suggestions(SuggestionsBuilder::new(prefix, 0));
        let texts: Vec<String> = suggestions
            .suggestions
            .into_iter()
            .map(|s| s.text_as_string())
            .collect();
        writeln!(now, "suggest {prefix:?} -> {}", texts.join(" "))?;
    }

    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/snbt_numbers.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(golden.parent().ok_or("no parent")?)?;
        std::fs::write(&golden, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&golden)?, now);
    Ok(())
}
