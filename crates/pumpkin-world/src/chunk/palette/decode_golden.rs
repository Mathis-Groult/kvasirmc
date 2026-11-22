//! A record of what `PalettedContainer::from_palette_and_packed_data` returns, made before it is
//! split (`tests/golden/palette_decode.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! The decoder runs on a corpus of palettes (every length that changes the bit width or the
//! route: empty, one, 2..5, powers of two and their neighbours, 255/256/257 at the indexed
//! limit, 300, 5000, 65537), three minimum widths, and eight packed-data shapes (empty, short
//! by a word, zeros, random, all ones, a proper ascending pack, trailing extra words, indices
//! past the palette), for value types of one, two and four bytes, a duplicate-entry palette set,
//! and cubes of 2, 4 and 16 per side. Each line has the input and the result: the kind, the
//! storage, the palette, the counts and every value read back.

use std::{fmt::Write, hash::Hash};

use pumpkin_util::encompassing_bits;

use super::{PaletteStorage, PalettedContainer};

fn fnv(hash: &mut u64, value: u64) {
    for byte in value.to_le_bytes() {
        *hash ^= u64::from(byte);
        *hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
}

const FNV_START: u64 = 0xcbf2_9ce4_8422_2325;

fn counts_text(counts: &[u16]) -> String {
    let mut hash = FNV_START;
    let mut sum = 0u64;
    let mut nonzero = 0usize;
    for &count in counts {
        fnv(&mut hash, u64::from(count));
        sum += u64::from(count);
        nonzero += usize::from(count > 0);
    }
    let list = if counts.len() <= 8 {
        format!("{counts:?}")
    } else {
        String::from("..")
    };
    format!(
        "counts={list} n={} sum={sum} nonzero={nonzero} h={hash:016x}",
        counts.len()
    )
}

fn describe<V, const DIM: usize>(palette: &[V], data: &[i64], min_bits: u8) -> String
where
    V: Hash + Eq + Copy + Default + Into<u64>,
{
    match PalettedContainer::<V, DIM>::from_palette_and_packed_data(palette, data, min_bits) {
        PalettedContainer::Homogeneous(value) => {
            format!("homogeneous {}", Into::<u64>::into(value))
        }
        PalettedContainer::Heterogeneous(cube) => {
            let mut values = FNV_START;
            let mut first = Vec::new();
            for y in 0..DIM {
                for z in 0..DIM {
                    for x in 0..DIM {
                        let value = Into::<u64>::into(cube.get(x, y, z));
                        fnv(&mut values, value);
                        if first.len() < 6 {
                            first.push(value);
                        }
                    }
                }
            }
            let storage = match &cube.storage {
                PaletteStorage::Dense(_) => String::from("dense"),
                PaletteStorage::Indexed(indices) => {
                    let mut hash = FNV_START;
                    for &index in indices.as_flattened().as_flattened() {
                        fnv(&mut hash, u64::from(index));
                    }
                    format!("indexed idx={hash:016x}")
                }
            };
            let mut palette_hash = FNV_START;
            for &entry in &cube.palette {
                fnv(&mut palette_hash, Into::<u64>::into(entry));
            }
            let same = cube.palette == palette;
            format!(
                "{storage} palette_same={same} palette_h={palette_hash:016x} {} first={first:?} values={values:016x}",
                counts_text(&cube.counts)
            )
        }
    }
}

/// `n`-th palette entry: distinct, non-zero for the first `modulus - 1` entries.
fn entry<V: TryFrom<u32> + Default>(n: usize, modulus: u32) -> V {
    V::try_from((n as u32 % modulus) + 1).unwrap_or_default()
}

fn lcg(seed: u64, count: usize) -> Vec<i64> {
    let mut state = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    (0..count)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state ^ (state >> 29)) as i64
        })
        .collect()
}

/// Indices `i % len` packed the way the decoder reads them.
fn ascending(len: usize, bits: u8, volume: usize) -> Vec<i64> {
    let keys = 64 / usize::from(bits);
    let mut words = vec![0u64; volume.div_ceil(keys)];
    for i in 0..volume {
        words[i / keys] |= ((i % len) as u64) << ((i % keys) * usize::from(bits));
    }
    words.into_iter().map(|word| word as i64).collect()
}

/// The packed-data shapes tried for one palette.
fn shapes(len: usize, min_bits: u8, volume: usize) -> Vec<(&'static str, Vec<i64>)> {
    let bits = encompassing_bits(len.max(1)).max(min_bits);
    let words = volume.div_ceil(64 / usize::from(bits));
    let seed = (len as u64) << 8 | u64::from(min_bits);
    let mut long = lcg(seed ^ 7, words);
    long.extend([-1, 0x1234_5678_9abc_def0, 1]);
    vec![
        ("empty", Vec::new()),
        ("short", lcg(seed ^ 1, words.saturating_sub(1))),
        ("zeros", vec![0; words]),
        ("random", lcg(seed ^ 2, words)),
        ("ones", vec![-1; words]),
        ("ascending", ascending(len.max(1), bits, volume)),
        ("long", long),
    ]
}

fn line<V, const DIM: usize>(
    out: &mut String,
    kind: &str,
    palette: &[V],
    min_bits: u8,
    name: &str,
    data: &[i64],
) where
    V: Hash + Eq + Copy + Default + Into<u64>,
{
    let mut palette_hash = FNV_START;
    for &entry in palette {
        fnv(&mut palette_hash, Into::<u64>::into(entry));
    }
    let mut data_hash = FNV_START;
    for &word in data {
        fnv(&mut data_hash, word as u64);
    }
    let _ = writeln!(
        out,
        "{kind} d{DIM} len={} ph={palette_hash:016x} min={min_bits} {name}({}, {data_hash:016x}) => {}",
        palette.len(),
        data.len(),
        describe::<V, DIM>(palette, data, min_bits)
    );
}

fn matrix<V, const DIM: usize>(out: &mut String, kind: &str, lens: &[usize], modulus: u32)
where
    V: Hash + Eq + Copy + Default + Into<u64> + TryFrom<u32>,
{
    for &len in lens {
        let palette: Vec<V> = (0..len).map(|n| entry::<V>(n, modulus)).collect();
        for min_bits in [1u8, 4, 16] {
            for (name, data) in shapes(len, min_bits, PalettedContainer::<V, DIM>::VOLUME) {
                line::<V, DIM>(out, kind, &palette, min_bits, name, &data);
            }
        }
    }
}

fn wide_minimums(out: &mut String) {
    let palette: Vec<u16> = (1..=3).collect();
    for min_bits in [2u8, 3, 5, 7, 8, 12, 13, 31, 32] {
        for (name, data) in shapes(3, min_bits, 64) {
            line::<u16, 4>(out, "u16", &palette, min_bits, name, &data);
        }
    }
}

/// Palettes with repeated entries and with or without the default value (0).
fn odd_palettes(out: &mut String) {
    let cases: [(&str, &[u16]); 5] = [
        ("dup-first", &[5, 5, 7]),
        ("dup-last", &[7, 3, 3]),
        ("zero-first", &[0, 9, 4, 0]),
        ("zero-last", &[9, 4, 6, 0]),
        ("nozero", &[9, 4, 6, 8, 2]),
    ];
    for (name, palette) in cases {
        for min_bits in [1u8, 3] {
            for (shape, data) in shapes(palette.len(), min_bits, 64) {
                let tag = format!("u16:{name}");
                line::<u16, 4>(out, &tag, palette, min_bits, shape, &data);
                let narrow: Vec<u8> = palette.iter().map(|&v| v as u8).collect();
                let tag = format!("u8:{name}");
                line::<u8, 4>(out, &tag, &narrow, min_bits, shape, &data);
            }
        }
    }
}

/// Hand-written inputs: single words with known indices, and the two-per-side cubes of the
/// unit tests.
fn hand_made(out: &mut String) {
    let cases: [(&[u16], &[i64], u8); 8] = [
        (&[10, 20, 30], &[2], 2),
        (&[10, 20, 30], &[0xffff], 2),
        (&[10, 20, 30], &[0b11_10_01_00], 2),
        (&[10, 20, 30], &[0b01_00_10_11_10_01_00_11], 2),
        (&[10, 20, 30, 40], &[i64::MIN], 2),
        (&[10, 20], &[0x55], 1),
        (&[10, 20], &[0x55, 0xff], 1),
        (&[1, 2, 3, 4, 5], &[0x0123_4567], 3),
    ];
    for (palette, data, min_bits) in cases {
        line::<u16, 2>(out, "u16", palette, min_bits, "hand", data);
        let narrow: Vec<u8> = palette.iter().map(|&v| v as u8).collect();
        line::<u8, 2>(out, "u8", &narrow, min_bits, "hand", data);
    }
}

fn report() -> String {
    let lens = [
        0usize, 1, 2, 3, 4, 5, 8, 9, 16, 17, 33, 64, 65, 128, 129, 255, 256, 257, 300, 5000, 65_537,
    ];
    let mut out = String::new();
    matrix::<u16, 4>(&mut out, "u16", &lens, u32::from(u16::MAX));
    matrix::<u8, 4>(
        &mut out,
        "u8",
        &[0, 1, 2, 3, 5, 9, 17, 100, 255, 256, 300],
        255,
    );
    matrix::<u32, 4>(&mut out, "u32", &[2, 3, 17, 256, 257, 300], u32::MAX - 1);
    matrix::<u16, 16>(
        &mut out,
        "u16",
        &[2, 3, 17, 256, 257, 300],
        u32::from(u16::MAX),
    );
    matrix::<u8, 16>(&mut out, "u8", &[2, 3, 17, 200], 255);
    matrix::<u16, 2>(&mut out, "u16", &[2, 3, 5, 9, 300], u32::from(u16::MAX));
    wide_minimums(&mut out);
    odd_palettes(&mut out);
    hand_made(&mut out);
    out
}

#[test]
fn palette_decode() -> Result<(), Box<dyn std::error::Error>> {
    let report = report();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("palette_decode.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
