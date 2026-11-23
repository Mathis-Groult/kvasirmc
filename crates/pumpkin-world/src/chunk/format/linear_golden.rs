//! A record of what `LinearV2File::parse` returns for byte-level inputs, made before it was split
//! along the file sections (`tests/golden/linear_parse.txt`; rewrite it on purpose with
//! `KVASIR_BLESS=1`).
//!
//! Files are built here, byte by byte, from the layout (superblock, chunk bitmap, features,
//! bucket size entries, bucket data, footer) with their own bucket arithmetic, so the golden does
//! not lean on the code it records. Every scene prints the parsed structure (region, grid, bucket
//! sizes and hashes, every chunk's timestamp, length and hash) or the error variant. One full file
//! is parsed at every truncation length, and the writer's own output is parsed back.

use std::fmt::Write;
use std::path::Path;

use bytes::Bytes;
use pumpkin_util::math::vector2::Vector2;
use ruzstd::encoding::{CompressionLevel, compress_to_vec};
use tempfile::TempDir;
use xxhash_rust::xxh64::xxh64;

use super::{ChunkReadingError, LinearV2File};
use crate::chunk::io::RegionFile;

const SIGNATURE: [u8; 8] = [0xc3, 0xff, 0x13, 0x18, 0x3c, 0xca, 0x9d, 0x9a];

/// FNV-1a: a hash that does not change with the Rust version.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Bytes that do not compress and differ per seed.
fn noise(seed: u64, len: usize) -> Vec<u8> {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

/// A chunk of the region: index, timestamp, data (`None` = absent).
type Chunk = (usize, u64, Option<Vec<u8>>);

fn chunks_at(indexes: &[usize]) -> Vec<Chunk> {
    indexes
        .iter()
        .map(|&index| {
            let seed = index as u64 + 1;
            (
                index,
                1_700_000_000 + seed * 7,
                Some(noise(seed, 1 + index % 37)),
            )
        })
        .collect()
}

/// Chunks in every bucket of a 2 x 2 grid, the last one at the end of the last bucket.
const SPREAD: [usize; 8] = [0, 17, 33, 500, 520, 530, 1000, 1023];

fn record(timestamp: u64, data: Option<&[u8]>) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&data.map_or(0, <[u8]>::len).to_be_bytes()[4..]);
    out.extend_from_slice(&timestamp.to_be_bytes());
    if let Some(data) = data {
        out.extend_from_slice(data);
    }
    out
}

/// The decompressed bytes of every bucket of a `grid` x `grid` file holding `chunks`.
fn raw_buckets(grid: u8, chunks: &[Chunk]) -> Vec<Vec<u8>> {
    let g = usize::from(grid);
    let stride = 32 / g;
    let mut slots: Vec<Vec<Option<&Chunk>>> = vec![vec![None; stride * stride]; g * g];
    for chunk in chunks {
        let (cx, cz) = (chunk.0 % 32, chunk.0 / 32);
        slots[(cz / stride) * g + cx / stride][(cz % stride) * stride + cx % stride] = Some(chunk);
    }
    slots
        .iter()
        .map(|bucket| {
            let mut raw = Vec::new();
            for slot in bucket {
                match slot {
                    Some((_, timestamp, data)) => raw.extend(record(*timestamp, data.as_deref())),
                    None => raw.extend(record(0, None)),
                }
            }
            raw
        })
        .collect()
}

#[derive(Clone)]
struct Bucket {
    data: Vec<u8>,
    level: i8,
    size: Option<u32>,
    hash: Option<u64>,
}

impl Bucket {
    fn new(raw: &[u8]) -> Self {
        Self::stored(compress_to_vec(raw, CompressionLevel::Fastest))
    }

    /// A bucket whose stored bytes are `data` as they are, with the right hash.
    const fn stored(data: Vec<u8>) -> Self {
        Self {
            data,
            level: 1,
            size: None,
            hash: None,
        }
    }
}

#[derive(Clone)]
struct Spec {
    signature: [u8; 8],
    version: u8,
    newest: u64,
    grid: u8,
    region: (i32, i32),
    bitmap: [u8; 128],
    features: Vec<u8>,
    buckets: Vec<Bucket>,
    footer: [u8; 8],
    trailing: Vec<u8>,
}

impl Spec {
    fn from_raws(grid: u8, raws: &[Vec<u8>]) -> Self {
        Self {
            signature: SIGNATURE,
            version: 2,
            newest: 1_700_000_999,
            grid,
            region: (-3, 7),
            bitmap: [0; 128],
            features: vec![0],
            buckets: raws.iter().map(|raw| Bucket::new(raw)).collect(),
            footer: SIGNATURE,
            trailing: Vec::new(),
        }
    }

    fn new(grid: u8, chunks: &[Chunk]) -> Self {
        Self::from_raws(grid, &raw_buckets(grid, chunks))
    }

    fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.signature);
        out.push(self.version);
        out.extend_from_slice(&self.newest.to_be_bytes());
        out.push(self.grid);
        out.extend_from_slice(&self.region.0.to_be_bytes());
        out.extend_from_slice(&self.region.1.to_be_bytes());
        out.extend_from_slice(&self.bitmap);
        out.extend_from_slice(&self.features);
        for bucket in &self.buckets {
            let size = bucket.size.unwrap_or(bucket.data.len() as u32);
            out.extend_from_slice(&size.to_be_bytes());
            out.push(bucket.level as u8);
            let hash = bucket.hash.unwrap_or_else(|| xxh64(&bucket.data, 0));
            out.extend_from_slice(&hash.to_be_bytes());
        }
        for bucket in &self.buckets {
            out.extend_from_slice(&bucket.data);
        }
        out.extend_from_slice(&self.footer);
        out.extend_from_slice(&self.trailing);
        out
    }
}

fn error_name(error: &ChunkReadingError) -> String {
    match error {
        ChunkReadingError::IoError(error) => format!("IoError({:?})", error.kind()),
        other => format!("{other:?}"),
    }
}

fn parse(bytes: &[u8]) -> Result<LinearV2File, ChunkReadingError> {
    LinearV2File::parse(
        Bytes::copy_from_slice(bytes),
        Path::new("dir/r.5.-6.linear"),
    )
}

fn present(file: &LinearV2File) -> usize {
    file.chunks_data.iter().flatten().count()
}

fn buckets_line(file: &LinearV2File) -> String {
    let mut parts = Vec::new();
    let mut all = Vec::new();
    for bucket in &file.buckets {
        let part = bucket.as_ref().map_or_else(
            || "none".to_string(),
            |b| format!("{}/{}/{:016x}", b.data.len(), b.compression_level, b.xxhash),
        );
        all.extend_from_slice(part.as_bytes());
        parts.push(part);
    }
    if file.buckets.len() <= 4 {
        format!("buckets={}", parts.join(" "))
    } else {
        let some = file.buckets.iter().flatten().count();
        format!(
            "buckets={} some={some} fnv={:016x}",
            file.buckets.len(),
            fnv(&all)
        )
    }
}

fn describe(label: &str, result: &Result<LinearV2File, ChunkReadingError>) -> String {
    let mut out = format!("== {label}\n");
    match result {
        Err(error) => {
            let _ = writeln!(out, "err {}", error_name(error));
        }
        Ok(file) => {
            let name = file.path.file_name().map(|n| n.to_string_lossy());
            let _ = writeln!(
                out,
                "ok path={} region=({}, {}) grid={} staged={} unsynced={} chunks={} \
                 timestamps={} {}",
                name.as_deref().unwrap_or("?"),
                file.region_x,
                file.region_z,
                file.grid_size,
                file.staged,
                file.unsynced,
                present(file),
                file.timestamps.len(),
                buckets_line(file)
            );
            for (index, (timestamp, data)) in
                file.timestamps.iter().zip(&file.chunks_data).enumerate()
            {
                if *timestamp != 0 || data.is_some() {
                    let (len, hash) = data.as_ref().map_or((0, 0), |d| (d.len(), fnv(d)));
                    let _ = writeln!(
                        out,
                        "  chunk {index} ts={timestamp} len={len} fnv={hash:016x}"
                    );
                }
            }
        }
    }
    out
}

fn brief(result: &Result<LinearV2File, ChunkReadingError>) -> String {
    match result {
        Err(error) => format!("err {}", error_name(error)),
        Ok(file) => format!("ok chunks={} {}", present(file), buckets_line(file)),
    }
}

fn scene(out: &mut String, label: &str, bytes: &[u8]) {
    out.push_str(&describe(label, &parse(bytes)));
}

fn valid_scenes(out: &mut String) {
    let spread = chunks_at(&SPREAD);
    scene(out, "grid 2, no chunks", &Spec::new(2, &[]).bytes());
    scene(
        out,
        "grid 2, one chunk first",
        &Spec::new(2, &chunks_at(&[0])).bytes(),
    );
    scene(
        out,
        "grid 2, one chunk last",
        &Spec::new(2, &chunks_at(&[1023])).bytes(),
    );
    for grid in [1, 2, 4, 8, 16, 32] {
        let mut spec = Spec::new(
            grid,
            &chunks_at(&[0, 1, 31, 32, 63, 300, 481, 700, 1022, 1023]),
        );
        spec.region = (i32::MIN, i32::MAX);
        scene(out, &format!("grid {grid}, many chunks"), &spec.bytes());
    }
    let mut absent = spread.clone();
    absent.push((40, 1_234_567, None));
    absent.push((41, 7_654_321, Some(Vec::new())));
    scene(
        out,
        "absent chunks keep their timestamps",
        &Spec::new(2, &absent).bytes(),
    );
    let mut spec = Spec::new(2, &spread);
    spec.newest = u64::MAX;
    spec.region = (123_456, -654_321);
    scene(out, "extreme newest timestamp, region", &spec.bytes());
    let mut spec = Spec::new(2, &[]);
    spec.bitmap = [0xFF; 128];
    scene(out, "bitmap says all present, none are", &spec.bytes());
    let mut spec = Spec::new(2, &spread);
    spec.bitmap = [0; 128];
    scene(out, "bitmap says none present, 8 are", &spec.bytes());
    let mut spec = Spec::new(2, &spread);
    spec.features = vec![
        3, b'a', b'b', b'c', 0, 0, 0, 9, //
        1, b'z', 0xFF, 0xFF, 0xFF, 0xFF, //
        0,
    ];
    scene(out, "two features before the buckets", &spec.bytes());
    let mut spec = Spec::new(2, &spread);
    spec.buckets[1].level = -1;
    spec.buckets[2].level = 22;
    scene(out, "compression levels are kept", &spec.bytes());
}

fn trailing_and_footer_scenes(out: &mut String) {
    let spread = chunks_at(&SPREAD);
    let mut spec = Spec::new(2, &spread);
    spec.trailing = vec![0xAA; 5];
    scene(out, "five trailing bytes", &spec.bytes());
    let mut spec = Spec::new(2, &spread);
    spec.trailing = SIGNATURE.to_vec();
    scene(out, "a second footer after the footer", &spec.bytes());
    for (label, edit) in [("first byte", 0usize), ("last byte", 7)] {
        let mut spec = Spec::new(2, &spread);
        spec.footer[edit] ^= 1;
        scene(out, &format!("footer {label} wrong"), &spec.bytes());
    }
    let mut spec = Spec::new(2, &spread);
    spec.footer = [0; 8];
    scene(out, "footer zeroed", &spec.bytes());
    let mut spec = Spec::new(2, &spread);
    spec.footer = [0; 8];
    spec.trailing = vec![1, 2, 3, 4, 5, 6, 7, 8];
    scene(out, "footer zeroed, bytes after it", &spec.bytes());
}

fn superblock_scenes(out: &mut String) {
    let spread = chunks_at(&SPREAD);
    for (label, byte) in [("first", 0usize), ("last", 7)] {
        let mut spec = Spec::new(2, &spread);
        spec.signature[byte] ^= 0x80;
        scene(out, &format!("signature {label} byte wrong"), &spec.bytes());
    }
    for version in [0u8, 1, 3, 0x82, 255] {
        let mut spec = Spec::new(2, &spread);
        spec.version = version;
        scene(out, &format!("version {version}"), &spec.bytes());
    }
    for grid in [0u8, 3, 5, 6, 7, 9, 12, 33, 64, 255] {
        let mut spec = Spec::new(2, &spread);
        spec.grid = grid;
        scene(out, &format!("grid byte {grid}"), &spec.bytes());
    }
    let mut spec = Spec::new(2, &spread);
    spec.signature[3] ^= 1;
    spec.version = 9;
    scene(out, "signature and version wrong", &spec.bytes());
    let header = Spec::new(2, &spread).bytes()[..26 + 128].to_vec();
    scene(out, "header and bitmap only", &header);
    let mut spec = Spec::new(2, &spread);
    spec.features = vec![2, 0xFF, 0xFE, 0, 0, 0, 1, 0];
    scene(out, "feature key is not UTF-8", &spec.bytes());
    let mut spec = Spec::new(2, &spread);
    spec.features = vec![2, b'o', b'k', 0, 0, 0, 1, 7];
    scene(out, "feature key runs into the buckets", &spec.bytes());
}

fn size_scenes(out: &mut String) {
    let spread = chunks_at(&SPREAD);
    let mut spec = Spec::new(2, &spread);
    spec.buckets[0].size = Some(u32::MAX);
    scene(out, "first bucket size u32::MAX", &spec.bytes());
    let mut spec = Spec::new(2, &spread);
    spec.buckets[3].size = Some(spec.buckets[3].data.len() as u32 + 1);
    scene(out, "last bucket size one too big", &spec.bytes());
    let mut spec = Spec::new(2, &spread);
    spec.buckets[3].size = Some(spec.buckets[3].data.len() as u32 + 1);
    spec.trailing = vec![0; 4];
    scene(
        out,
        "last bucket size one too big, bytes after the footer",
        &spec.bytes(),
    );
    let mut spec = Spec::new(2, &spread);
    spec.buckets[0].size = Some(spec.buckets[0].data.len() as u32 - 1);
    scene(out, "first bucket size one too small", &spec.bytes());
    let mut spec = Spec::new(2, &spread);
    spec.buckets[1].size = Some(0);
    spec.buckets[1].hash = Some(xxh64(&[], 0));
    scene(
        out,
        "second bucket size zero, data still there",
        &spec.bytes(),
    );
    let mut spec = Spec::new(2, &spread);
    spec.buckets[2] = Bucket::stored(Vec::new());
    scene(out, "third bucket empty", &spec.bytes());
    // Sizes shifted: bucket 0 loses its last two bytes to bucket 1, hashes fit the new slices.
    let mut spec = Spec::new(2, &spread);
    let cut = spec.buckets[0].data.len() - 2;
    let tail = spec.buckets[0].data[cut..].to_vec();
    let head = spec.buckets[0].data[..cut].to_vec();
    spec.buckets[0].size = Some(cut as u32);
    spec.buckets[0].hash = Some(xxh64(&head, 0));
    let mut second = tail;
    second.extend_from_slice(&spec.buckets[1].data);
    spec.buckets[1].size = Some(second.len() as u32);
    spec.buckets[1].hash = Some(xxh64(&second, 0));
    scene(out, "sizes shifted between buckets 0 and 1", &spec.bytes());
}

fn bucket_data_scenes(out: &mut String) {
    let spread = chunks_at(&SPREAD);
    let mut spec = Spec::new(2, &spread);
    spec.buckets[1] = Bucket::stored(vec![1, 2, 3, 4, 5, 6, 7, 8]);
    scene(out, "second bucket is not zstd", &spec.bytes());
    let mut spec = Spec::new(2, &spread);
    let data = &spec.buckets[2].data;
    spec.buckets[2] = Bucket::stored(data[..data.len() - 3].to_vec());
    scene(
        out,
        "third bucket frame cut short, hash fits",
        &spec.bytes(),
    );
    let mut spec = Spec::new(2, &spread);
    let mut data = spec.buckets[0].data.clone();
    data.push(0xAB);
    spec.buckets[0] = Bucket::stored(data);
    scene(
        out,
        "first bucket has a byte after its frame",
        &spec.bytes(),
    );
    let mut spec = Spec::new(2, &spread);
    spec.buckets[0] = Bucket::stored(Vec::new());
    spec.buckets[1] = Bucket::stored(Vec::new());
    scene(out, "first two buckets empty", &spec.bytes());
}

fn hash_scenes(out: &mut String) {
    let spread = chunks_at(&SPREAD);
    for (label, wrong) in [
        ("first", vec![0usize]),
        ("second", vec![1]),
        ("last", vec![3]),
        ("first and last", vec![0, 3]),
        ("all", vec![0, 1, 2, 3]),
    ] {
        let mut spec = Spec::new(2, &spread);
        for index in wrong {
            spec.buckets[index].hash = Some(xxh64(&spec.buckets[index].data, 0) ^ 1);
        }
        scene(
            out,
            &format!("hash of {label} bucket(s) wrong"),
            &spec.bytes(),
        );
    }
    let mut spec = Spec::new(2, &spread);
    spec.buckets[1] = Bucket::stored(vec![9; 40]);
    spec.buckets[1].hash = Some(5);
    scene(out, "wrong hash on bytes that are not zstd", &spec.bytes());
}

/// A scene label, the bucket to edit and the edit.
type Edit = (&'static str, usize, fn(&mut Vec<u8>));

fn record_scenes(out: &mut String) {
    let spread = chunks_at(&SPREAD);
    let raws = raw_buckets(2, &spread);
    let edits: [Edit; 7] = [
        ("last bucket has no bytes", 3, |raw| raw.clear()),
        ("last bucket misses its last record", 3, |raw| {
            raw.truncate(raw.len() - 12 - 25);
        }),
        ("last chunk's data cut by a byte", 3, |raw| {
            raw.truncate(raw.len() - 1);
        }),
        ("last record has only its size", 3, |raw| {
            raw.truncate(raw.len() - 33);
        }),
        ("last record has no data", 3, |raw| {
            raw.truncate(raw.len() - 25);
        }),
        ("first bucket loses its first 11 bytes", 0, |raw| {
            raw.drain(..4);
            raw.drain(..7);
        }),
        ("bytes after the last record", 0, |raw| raw.extend([7; 9])),
    ];
    for (label, bucket, edit) in edits {
        let mut raws = raws.clone();
        edit(&mut raws[bucket]);
        scene(out, label, &Spec::from_raws(2, &raws).bytes());
    }
    let mut raws = raws;
    raws[1][..4].copy_from_slice(&u32::MAX.to_be_bytes());
    scene(
        out,
        "first record claims u32::MAX bytes",
        &Spec::from_raws(2, &raws).bytes(),
    );
}

/// Every prefix of one full file.
fn truncation_scenes(out: &mut String) {
    let full = Spec::new(2, &chunks_at(&SPREAD)).bytes();
    let _ = writeln!(out, "== every prefix of a full file ({} bytes)", full.len());
    for len in 0..=full.len() {
        let _ = writeln!(out, "  {len}: {}", brief(&parse(&full[..len])));
    }
}

fn writer_scenes(out: &mut String) -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let path = dir.path().join("r.5.-6.linear");
    let mut file = LinearV2File::open(&path, Vector2::new(5, -6))?;
    for index in SPREAD {
        file.stage_chunk(index, Bytes::from(noise(index as u64 + 1, 1 + index % 37)));
        file.timestamps[index] = 1_800_000_000 + index as u64;
    }
    file.flush(false)?;
    let written = std::fs::read(&path)?;
    let _ = writeln!(
        out,
        "== writer output: {} bytes fnv={:016x}",
        written.len(),
        fnv(&written)
    );
    out.push_str(&describe("the writer's file parsed", &parse(&written)));
    out.push_str(&describe(
        "the writer's file opened",
        &LinearV2File::open(&path, Vector2::new(5, -6)),
    ));

    let mut parsed = parse(&Spec::new(4, &chunks_at(&[0, 70, 300, 1023])).bytes())?;
    parsed.path = dir.path().join("parsed.linear");
    parsed.stage_chunk(70, Bytes::from(noise(99, 20)));
    for index in [0, 70, 300, 1023] {
        parsed.timestamps[index] = 1_900_000_000 + index as u64;
    }
    parsed.flush(false)?;
    let again = std::fs::read(&parsed.path)?;
    out.push_str(&describe(
        "parsed, one chunk replaced, written, parsed",
        &parse(&again),
    ));
    Ok(())
}

fn all_lines() -> Result<String, Box<dyn std::error::Error>> {
    let mut out = String::new();
    valid_scenes(&mut out);
    trailing_and_footer_scenes(&mut out);
    superblock_scenes(&mut out);
    size_scenes(&mut out);
    bucket_data_scenes(&mut out);
    hash_scenes(&mut out);
    record_scenes(&mut out);
    writer_scenes(&mut out)?;
    truncation_scenes(&mut out);
    Ok(out)
}

#[test]
fn parse_returns_the_same_structure() -> Result<(), Box<dyn std::error::Error>> {
    let now = all_lines()?;
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("linear_parse.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, now);
    Ok(())
}
