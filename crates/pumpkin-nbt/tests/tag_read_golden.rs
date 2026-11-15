//! Golden of `NbtTag::deserialize_data` / `deserialize_data_depth` over Java (big-endian) payloads:
//! every tag id with a valid payload and with nothing to read, array and list lengths at -1, 0,
//! the limit and past it (with and without the announced bytes), list element types incl. END and
//! unknown ids, wrapper compounds in lists, and nesting at the depth limit. One line per case: the
//! result (`Debug`, hashed when long) and the reader position afterwards. Rewrite
//! `tests/golden/tag_read.txt` on purpose with `KVASIR_BLESS=1`.

use std::fmt::Write as _;
use std::io::Cursor;
use std::path::Path;

use pumpkin_nbt::deserializer::{NbtReadHelper, NbtReadHelperJava};
use pumpkin_nbt::tag::NbtTag;
use pumpkin_nbt::{MAX_ARRAY_LENGTH, MAX_NBT_DEPTH};

const END: u8 = 0;
const BYTE: u8 = 1;
const SHORT: u8 = 2;
const INT: u8 = 3;
const LONG: u8 = 4;
const FLOAT: u8 = 5;
const DOUBLE: u8 = 6;
const BYTE_ARRAY: u8 = 7;
const STRING: u8 = 8;
const LIST: u8 = 9;
const COMPOUND: u8 = 10;
const INT_ARRAY: u8 = 11;
const LONG_ARRAY: u8 = 12;

fn fnv(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn len(n: i32) -> Vec<u8> {
    n.to_be_bytes().to_vec()
}

fn string(s: &[u8]) -> Vec<u8> {
    let mut out = (s.len() as u16).to_be_bytes().to_vec();
    out.extend_from_slice(s);
    out
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

/// `depth` lists nested in each other, the innermost an empty END list.
fn nested_lists(depth: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..depth {
        out.push(LIST);
        out.extend(len(1));
    }
    out.push(END);
    out.extend(len(0));
    out
}

/// `depth` compound entries nested under the key "a", followed by `depth + 1` END tags.
fn nested_compounds(depth: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..depth {
        out.push(COMPOUND);
        out.extend(string(b"a"));
    }
    out.extend(std::iter::repeat_n(END, depth + 1));
    out
}

fn scalar_cases(add: &mut impl FnMut(&str, u8, Vec<u8>)) {
    for id in 0..=14u8 {
        add(&format!("empty id {id}"), id, Vec::new());
    }
    add("unknown 255", 255, vec![1, 2, 3]);
    add("byte", BYTE, vec![0x80]);
    add("short", SHORT, vec![0x80, 0x01]);
    add("short short", SHORT, vec![0x80]);
    add("int", INT, vec![0xff, 0xff, 0xff, 0xfe]);
    add("long", LONG, i64::MIN.to_be_bytes().to_vec());
    add("float", FLOAT, 1.5f32.to_be_bytes().to_vec());
    add("float nan", FLOAT, f32::NAN.to_be_bytes().to_vec());
    add("double", DOUBLE, (-0.0f64).to_be_bytes().to_vec());
}

fn array_cases(add: &mut impl FnMut(&str, u8, Vec<u8>)) {
    let max = MAX_ARRAY_LENGTH as i32;
    for (name, id, unit) in [
        ("byte array", BYTE_ARRAY, 1usize),
        ("int array", INT_ARRAY, 4),
        ("long array", LONG_ARRAY, 8),
    ] {
        add(&format!("{name} -1"), id, len(-1));
        add(&format!("{name} min"), id, len(i32::MIN));
        add(&format!("{name} 0"), id, len(0));
        add(
            &format!("{name} 3"),
            id,
            cat(&[&len(3), &vec![0x7f; 3 * unit]]),
        );
        add(
            &format!("{name} 3 short"),
            id,
            cat(&[&len(3), &vec![0x7f; 3 * unit - 1]]),
        );
        add(&format!("{name} limit no data"), id, len(max));
        add(&format!("{name} limit+1"), id, len(max + 1));
    }
    add(
        "byte array at limit",
        BYTE_ARRAY,
        cat(&[&len(max), &vec![1; MAX_ARRAY_LENGTH]]),
    );
}

fn string_cases(add: &mut impl FnMut(&str, u8, Vec<u8>)) {
    add("string empty", STRING, string(b""));
    add("string abc", STRING, string(b"abc"));
    add("string short", STRING, cat(&[&5u16.to_be_bytes(), b"ab"]));
    add("string cesu8 nul", STRING, string(&[0xc0, 0x80]));
    add("string bad utf8", STRING, string(&[0xff, 0xfe]));
}

fn list_cases(add: &mut impl FnMut(&str, u8, Vec<u8>)) {
    let max = MAX_ARRAY_LENGTH as i32;
    add("list end 0", LIST, cat(&[&[END], &len(0)]));
    add("list end 1", LIST, cat(&[&[END], &len(1)]));
    add("list end -1", LIST, cat(&[&[END], &len(-1)]));
    add("list byte 2", LIST, cat(&[&[BYTE], &len(2), &[1, 2]]));
    add("list byte 2 short", LIST, cat(&[&[BYTE], &len(2), &[1]]));
    add("list byte -1", LIST, cat(&[&[BYTE], &len(-1)]));
    add("list byte limit+1", LIST, cat(&[&[BYTE], &len(max + 1)]));
    add("list byte limit no data", LIST, cat(&[&[BYTE], &len(max)]));
    add("list unknown 0", LIST, cat(&[&[99], &len(0)]));
    add("list unknown 1", LIST, cat(&[&[99], &len(1), &[0]]));
    add("list no length", LIST, vec![BYTE]);
    add(
        "list of lists",
        LIST,
        cat(&[&[LIST], &len(2), &[INT], &len(1), &len(7), &[END], &len(0)]),
    );
    add(
        "list of strings",
        LIST,
        cat(&[&[STRING], &len(2), &string(b"x"), &string(b"yz")]),
    );
    add(
        "list of wrapper compounds",
        LIST,
        cat(&[
            &[COMPOUND],
            &len(2),
            &[INT],
            &string(b""),
            &len(5),
            &[END],
            &[STRING],
            &string(b""),
            &string(b"w"),
            &[END],
        ]),
    );
    add(
        "list of plain compounds",
        LIST,
        cat(&[
            &[COMPOUND],
            &len(2),
            &[INT],
            &string(b"k"),
            &len(5),
            &[END],
            &[END],
        ]),
    );
    add(
        "list of compounds with two keys",
        LIST,
        cat(&[
            &[COMPOUND],
            &len(1),
            &[INT],
            &string(b""),
            &len(5),
            &[BYTE],
            &string(b"b"),
            &[1],
            &[END],
        ]),
    );
}

fn compound_and_depth_cases(add: &mut impl FnMut(&str, u8, Vec<u8>)) {
    add("compound empty", COMPOUND, vec![END]);
    add(
        "compound int",
        COMPOUND,
        cat(&[&[INT], &string(b"k"), &len(9), &[END]]),
    );
    add(
        "compound unterminated",
        COMPOUND,
        cat(&[&[INT], &string(b"k"), &len(9)]),
    );
    add(
        "compound unknown child",
        COMPOUND,
        cat(&[&[77], &string(b"k"), &[END]]),
    );

    for depth in [
        MAX_NBT_DEPTH - 1,
        MAX_NBT_DEPTH,
        MAX_NBT_DEPTH + 1,
        MAX_NBT_DEPTH + 2,
    ] {
        // A list payload holding `depth` more lists; a compound payload holding `depth - 1`
        // more compounds (one END left over).
        add(&format!("lists depth {depth}"), LIST, nested_lists(depth));
        add(
            &format!("compounds depth {depth}"),
            COMPOUND,
            nested_compounds(depth)[4..].to_vec(),
        );
    }
}

fn cases() -> Vec<(String, u8, Vec<u8>)> {
    let mut cases: Vec<(String, u8, Vec<u8>)> = Vec::new();
    let mut add = |name: &str, id: u8, bytes: Vec<u8>| cases.push((name.to_owned(), id, bytes));
    scalar_cases(&mut add);
    array_cases(&mut add);
    string_cases(&mut add);
    list_cases(&mut add);
    compound_and_depth_cases(&mut add);
    cases
}

fn record(out: &mut String, name: &str, id: u8, bytes: &[u8]) -> std::fmt::Result {
    let mut reader = NbtReadHelperJava::new(Cursor::new(bytes));
    let result = NbtTag::deserialize_data(&mut reader, id);
    let position = reader.reader().position();
    let mut text = match result {
        Ok(tag) => format!("ok {tag:?}"),
        Err(error) => format!("err {error:?}"),
    };
    if name.contains("two keys") {
        // A compound's keys come out in hash-map order: record its characters sorted.
        let mut chars: Vec<char> = text.chars().collect();
        chars.sort_unstable();
        text = chars.into_iter().collect();
    }
    if text.len() > 160 {
        writeln!(
            out,
            "{name}: {}.. #{:016x} at {position}",
            &text[..80],
            fnv(&text)
        )
    } else {
        writeln!(out, "{name}: {text} at {position}")
    }
}

#[test]
fn tags_read_the_same() -> Result<(), Box<dyn std::error::Error>> {
    // Nesting at the depth limit overflows the 2 MiB test thread in a debug build (a release
    // build reads it on 2 MiB), so the cases run on a thread with a larger stack.
    let now = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(|| {
            let mut now = String::new();
            for (name, id, bytes) in cases() {
                record(&mut now, &name, id, &bytes)?;
            }
            Ok::<_, std::fmt::Error>(now)
        })?
        .join()
        .map_err(|_| "reader thread panicked")??;
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/tag_read.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(golden.parent().ok_or("no parent")?)?;
        std::fs::write(&golden, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&golden)?, now);
    Ok(())
}
