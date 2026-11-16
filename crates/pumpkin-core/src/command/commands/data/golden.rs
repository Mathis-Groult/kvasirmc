//! A record of `snbt_colorful_display` (`tests/golden/snbt_colorful_display.txt`; rewrite it on
//! purpose with `KVASIR_BLESS=1`), made before it was split.
//!
//! One line per tag: its plain text, then the length and FNV hash of the encoded component (the
//! colours and the child structure).

use std::fmt::Write;

use pumpkin_nbt::compound::NbtCompound;
use pumpkin_nbt::tag::NbtTag;

use super::snbt_colorful_display;
use crate::command::commands::script_golden::{self, TestResult, fnv};

fn compound(entries: &[(&str, NbtTag)]) -> NbtTag {
    let mut compound = NbtCompound::new();
    for (key, value) in entries {
        compound.put(key, value.clone());
    }
    NbtTag::Compound(compound)
}

/// A compound of `size` int entries `k000`, `k001`, ...
fn wide_of(size: i32) -> NbtCompound {
    let mut wide = NbtCompound::new();
    for i in 0..size {
        wide.put(&format!("k{i:03}"), NbtTag::Int(i));
    }
    wide
}

fn tags() -> Vec<(&'static str, NbtTag)> {
    vec![
        ("end", NbtTag::End),
        ("byte", NbtTag::Byte(-5)),
        ("short", NbtTag::Short(300)),
        ("int", NbtTag::Int(-70_000)),
        ("long", NbtTag::Long(9_000_000_000)),
        ("float", NbtTag::Float(1.5)),
        ("double", NbtTag::Double(-0.25)),
        ("bytes empty", NbtTag::ByteArray(Box::new([]))),
        ("bytes one", NbtTag::ByteArray(Box::new([7]))),
        ("bytes", NbtTag::ByteArray(Box::new([1, -2, 3]))),
        (
            "bytes 128",
            NbtTag::ByteArray(vec![1; 128].into_boxed_slice()),
        ),
        (
            "bytes 130",
            NbtTag::ByteArray(vec![2; 130].into_boxed_slice()),
        ),
        ("string", NbtTag::String("hello".into())),
        ("string quotes", NbtTag::String("say \"hi\"".into())),
        ("string empty", NbtTag::String("".into())),
        ("list empty", NbtTag::List(vec![])),
        ("list one", NbtTag::List(vec![NbtTag::Int(1)])),
        (
            "list mixed",
            NbtTag::List(vec![
                NbtTag::Int(1),
                NbtTag::String("a".into()),
                NbtTag::List(vec![NbtTag::Byte(1), NbtTag::Byte(2)]),
            ]),
        ),
        ("list 128", NbtTag::List(vec![NbtTag::Short(3); 128])),
        ("list 130", NbtTag::List(vec![NbtTag::Short(4); 130])),
        ("compound empty", compound(&[])),
        ("compound one", compound(&[("a", NbtTag::Byte(1))])),
        (
            "compound sorted",
            compound(&[
                ("zeta", NbtTag::Int(1)),
                ("alpha", NbtTag::String("x".into())),
                ("Mid", compound(&[("inner", NbtTag::Long(2))])),
            ]),
        ),
        ("compound 130", NbtTag::Compound(wide_of(130))),
        ("ints empty", NbtTag::IntArray(vec![])),
        ("ints", NbtTag::IntArray(vec![1, -2, 3])),
        ("ints 130", NbtTag::IntArray(vec![5; 130])),
        ("longs empty", NbtTag::LongArray(vec![])),
        ("longs", NbtTag::LongArray(vec![1, -2])),
        ("longs 128", NbtTag::LongArray(vec![6; 128])),
        ("longs 130", NbtTag::LongArray(vec![6; 130])),
        // Appended after the mutation run: exactly at and one past the 128 fold.
        ("list 129", NbtTag::List(vec![NbtTag::Short(7); 129])),
        ("ints 128", NbtTag::IntArray(vec![8; 128])),
        ("compound 128", NbtTag::Compound(wide_of(128))),
    ]
}

#[test]
fn snbt_display_is_the_same() -> TestResult {
    let mut now = String::new();
    for (name, tag) in tags() {
        let component = snbt_colorful_display(&tag, 0);
        let encoded = component.encode();
        let text = component.get_text();
        let shown: String = if text.len() > 160 {
            format!("{}...({} chars)", &text[..160], text.len())
        } else {
            text
        };
        writeln!(
            now,
            "{name}: {shown} | {}:{:016x}",
            encoded.len(),
            fnv(&encoded)
        )?;
    }
    script_golden::bless_or_compare("snbt_colorful_display.txt", &now)
}
