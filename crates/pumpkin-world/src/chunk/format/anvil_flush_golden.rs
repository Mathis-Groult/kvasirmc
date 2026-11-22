//! A record of what `AnvilRegion::flush_with_sync` leaves behind, made before it was split into
//! small functions (`tests/golden/anvil_flush.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene stages chunks of known size in a region file in a temp dir and flushes with a sync
//! closure that counts its calls (and can fail). After every flush the scene prints the result,
//! the sync count, the region's in-memory state (staged chunks, unsynced flag, file length,
//! locations, used sectors, pending `.mcc` deletes) and what is on disk: the files of the
//! directory, the record header of every chunk and a hash of the region file whose timestamps
//! are zeroed (they are the clock and are asserted to lie in the flush's time window instead).

use std::cell::Cell;
use std::fmt::{Debug, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use pumpkin_util::math::vector2::Vector2;
use tempfile::TempDir;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Metadata, Subscriber, span};

use super::anvil::{
    AnvilChunkData, AnvilRegion, CHUNK_COUNT, Compression, HEADER_BYTES, SECTOR_BYTES,
    epoch_seconds, sector_count, sector_offset,
};
use crate::chunk::io::RegionFile;

/// FNV-1a: a hash that does not change with the Rust version.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Bytes that do not compress and differ per seed.
fn noise(seed: u64, len: usize) -> Bytes {
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

/// A chunk that takes `sectors` sectors in a region (its record is `4 + 1 + len` bytes).
fn chunk_of(seed: u64, sectors: usize, compression: Option<Compression>) -> AnvilChunkData {
    AnvilChunkData {
        compression,
        compressed_data: noise(seed, sectors * SECTOR_BYTES - 5 - 7),
    }
}

/// A chunk taking exactly the external threshold in sectors.
fn external_chunk(seed: u64, compression: Option<Compression>) -> AnvilChunkData {
    chunk_of(seed, 256, compression)
}

/// Collects the warnings a flush logs, as `message` text.
#[derive(Clone, Default)]
struct Warnings(Arc<Mutex<Vec<String>>>);

struct Message(String);

impl Visit for Message {
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        if field.name() == "message" {
            let _ = write!(self.0, "{value:?}");
        }
    }
}

impl Subscriber for Warnings {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &span::Attributes<'_>) -> span::Id {
        span::Id::from_u64(1)
    }

    fn record(&self, _: &span::Id, _: &span::Record<'_>) {}

    fn record_follows_from(&self, _: &span::Id, _: &span::Id) {}

    fn event(&self, event: &Event<'_>) {
        if *event.metadata().level() != Level::WARN {
            return;
        }
        let mut message = Message(String::new());
        event.record(&mut message);
        if let Ok(mut all) = self.0.lock() {
            all.push(message.0);
        }
    }

    fn enter(&self, _: &span::Id) {}

    fn exit(&self, _: &span::Id) {}
}

struct Scene {
    dir: TempDir,
    region: AnvilRegion,
    out: String,
    syncs: Cell<u32>,
    warnings: Warnings,
}

impl Scene {
    fn new(name: &str) -> Self {
        let dir = TempDir::new().unwrap_or_else(|err| panic!("temp dir: {err}"));
        let path = dir.path().join("r.0.0.mca");
        let region = AnvilRegion::open(&path, Vector2::new(0, 0))
            .unwrap_or_else(|_| panic!("open the region"));
        Self {
            dir,
            region,
            out: format!("== {name}\n"),
            syncs: Cell::new(0),
            warnings: Warnings::default(),
        }
    }

    fn path(&self) -> PathBuf {
        self.dir.path().join("r.0.0.mca")
    }

    fn stage(&mut self, index: usize, chunk: AnvilChunkData) {
        self.region.stage_chunk(index, chunk);
    }

    fn reopen(&mut self) {
        self.region = AnvilRegion::open(&self.path(), Vector2::new(0, 0))
            .unwrap_or_else(|_| panic!("reopen the region"));
    }

    /// Flushes, then prints the result and the state under `label`.
    fn flush(&mut self, label: &str, sync: bool, fail_sync: bool) {
        self.syncs.set(0);
        let before = epoch_seconds();
        let syncs = &self.syncs;
        let region = &mut self.region;
        let result = tracing::subscriber::with_default(self.warnings.clone(), || {
            region.flush_with_sync(sync, |_| {
                syncs.set(syncs.get() + 1);
                if fail_sync {
                    Err(std::io::Error::other("sync failed"))
                } else {
                    Ok(())
                }
            })
        });
        let after = epoch_seconds();
        let _ = writeln!(
            self.out,
            "-- {label}: sync={sync} fail_sync={fail_sync} -> {}",
            match &result {
                Ok(()) => "ok".to_string(),
                Err(err) => format!("err {:?}", err.kind()),
            }
        );
        self.report(before, after);
        self.report_warnings();
    }

    /// The warnings of the last flush, with the temp dir's name taken out.
    fn report_warnings(&mut self) {
        let taken: Vec<String> = self
            .warnings
            .0
            .lock()
            .map(|mut all| std::mem::take(&mut *all))
            .unwrap_or_default();
        let dir = self.dir.path().display().to_string();
        for message in taken {
            let _ = writeln!(self.out, "warn={}", message.replace(&dir, "<dir>"));
        }
    }

    fn report(&mut self, before: u32, after: u32) {
        let region = &self.region;
        let mut text = String::new();
        let mut staged: Vec<_> = region.staged.keys().copied().collect();
        staged.sort_unstable();
        let _ = writeln!(
            text,
            "syncs={} staged={staged:?} unsynced={} file_len={} used={} end={}",
            self.syncs.get(),
            region.unsynced,
            region.file_len,
            used_runs(region),
            region.used_sectors.end()
        );
        let mut pending: Vec<_> = region
            .pending_external_deletes
            .iter()
            .filter_map(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect();
        pending.sort_unstable();
        let _ = writeln!(text, "pending={pending:?}");
        let _ = writeln!(text, "locations={}", locations(region, before, after));
        let _ = writeln!(text, "files={}", files(self.dir.path()));
        let _ = writeln!(text, "disk={}", disk(&self.path()));
        self.out.push_str(&text);
    }
}

fn used_runs(region: &AnvilRegion) -> String {
    let end = region.used_sectors.end();
    let mut runs = Vec::new();
    let mut sector = 0;
    while sector < end {
        if region.used_sectors.is_used(sector) {
            let start = sector;
            while sector < end && region.used_sectors.is_used(sector) {
                sector += 1;
            }
            runs.push(format!("{start}-{}", sector - 1));
        } else {
            sector += 1;
        }
    }
    runs.join(",")
}

/// Nonzero locations as `index:offset+count`, with the timestamp as `T` when it lies inside the
/// flush's window, `0` when unset, and the value otherwise.
fn locations(region: &AnvilRegion, before: u32, after: u32) -> String {
    let mut parts = Vec::new();
    for index in 0..CHUNK_COUNT {
        let location = region.locations[index];
        let stamp = region.timestamps[index];
        if location == 0 && stamp == 0 {
            continue;
        }
        let stamp = if stamp == 0 {
            "0".to_string()
        } else if (before..=after).contains(&stamp) {
            "T".to_string()
        } else {
            stamp.to_string()
        };
        parts.push(format!(
            "{index}:{}+{}@{stamp}",
            sector_offset(location),
            sector_count(location)
        ));
    }
    parts.join(" ")
}

/// The directory's entries as `name:len`, `name:dir` sorted.
fn files(dir: &Path) -> String {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        match entry.metadata() {
            Ok(meta) if meta.is_dir() => names.push(format!("{name}:dir")),
            Ok(meta) => names.push(format!("{name}:{}", meta.len())),
            Err(_) => names.push(format!("{name}:?")),
        }
    }
    names.sort_unstable();
    names.join(" ")
}

/// The region file as it is on disk: the record header (length, compression byte) of each
/// chunk the header points at, and a hash of all bytes with the timestamps zeroed.
fn disk(path: &Path) -> String {
    let Ok(mut bytes) = std::fs::read(path) else {
        return "absent".to_string();
    };
    if bytes.len() < HEADER_BYTES {
        return format!("len={} hash={:016x}", bytes.len(), fnv(&bytes));
    }
    let mut heads = Vec::new();
    for index in 0..CHUNK_COUNT {
        let at = index * 4;
        let location = u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
        if location == 0 {
            continue;
        }
        let start = sector_offset(location) as usize * SECTOR_BYTES;
        match bytes.get(start..start + 5) {
            Some(head) => heads.push(format!(
                "{index}=[{} {:#04x}]",
                u32::from_be_bytes([head[0], head[1], head[2], head[3]]),
                head[4]
            )),
            None => heads.push(format!("{index}=beyond")),
        }
    }
    bytes[SECTOR_BYTES..HEADER_BYTES].fill(0);
    format!(
        "len={} hash={:016x} records {}",
        bytes.len(),
        fnv(&bytes),
        heads.join(" ")
    )
}

fn empty_scenes(out: &mut String) {
    let mut scene = Scene::new("empty: no file, nothing staged");
    scene.flush("sync on a region without a file", true, false);
    scene.region.unsynced = true;
    scene.flush("unsynced but still no file", true, false);
    out.push_str(&scene.out);

    let mut scene = Scene::new("empty: unsynced region");
    scene.stage(2, chunk_of(1, 1, None));
    scene.flush("first write, no sync", false, false);
    scene.flush("nothing staged, no sync", false, false);
    scene.flush("nothing staged, sync fails", true, true);
    scene.flush("nothing staged, sync", true, false);
    scene.flush("synced already", true, false);
    out.push_str(&scene.out);
}

fn new_chunk_scenes(out: &mut String) {
    for sync in [false, true] {
        let mut scene = Scene::new(&format!("new chunks, sync={sync}"));
        scene.stage(5, chunk_of(2, 1, None));
        scene.flush("one chunk", sync, false);
        scene.stage(700, chunk_of(3, 2, Some(Compression::ZLib)));
        scene.flush("a second, two sectors", sync, false);
        scene.reopen();
        scene.flush("reopened, nothing staged", sync, false);
        out.push_str(&scene.out);
    }
}

fn rewrite_scenes(out: &mut String) {
    let mut scene = Scene::new("overwrite with the same size, grow and shrink");
    scene.stage(3, chunk_of(4, 1, None));
    scene.stage(4, chunk_of(5, 1, None));
    scene.flush("two chunks", false, false);
    scene.stage(3, chunk_of(6, 1, None));
    scene.flush("same size", true, false);
    scene.stage(3, chunk_of(7, 3, None));
    scene.flush("grow to 3 sectors", true, false);
    scene.stage(4, chunk_of(8, 2, None));
    scene.flush("neighbour grows into the hole", false, false);
    scene.stage(3, chunk_of(9, 1, None));
    scene.flush("shrink to 1 sector", true, false);
    scene.reopen();
    scene.stage(4, chunk_of(10, 1, None));
    scene.flush("shrink the last one: the tail goes", true, false);
    out.push_str(&scene.out);
}

fn several_scenes(out: &mut String) {
    let mut scene = Scene::new("several chunks in one flush, holes reused");
    for (index, seed) in (0..6).zip(20..) {
        scene.stage(index, chunk_of(seed, 1, None));
    }
    scene.flush("six single sectors", false, false);
    scene.stage(1, chunk_of(30, 1, None));
    scene.stage(3, chunk_of(31, 1, None));
    scene.flush("two move, leaving holes at 3 and 5", false, false);
    scene.stage(900, chunk_of(32, 1, None));
    scene.stage(4, chunk_of(33, 2, None));
    scene.stage(901, chunk_of(34, 1, None));
    scene.stage(2, chunk_of(35, 1, None));
    scene.stage(1000, chunk_of(36, 3, None));
    scene.flush("mixed sizes into holes and the end", true, false);
    out.push_str(&scene.out);

    let mut scene = Scene::new("a file longer than its sectors");
    scene.stage(8, chunk_of(40, 2, None));
    scene.flush("one chunk", false, false);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(scene.path())
        .unwrap_or_else(|err| panic!("append: {err}"));
    let _ = std::io::Write::write_all(&mut file, &[7; 3 * SECTOR_BYTES]);
    drop(file);
    scene.reopen();
    scene.stage(9, chunk_of(41, 1, None));
    scene.flush("trailing garbage is cut", false, false);
    out.push_str(&scene.out);
}

fn external_scenes(out: &mut String) {
    let mut scene = Scene::new("oversized chunks go to .mcc files");
    scene.stage(7, external_chunk(50, Some(Compression::GZip)));
    scene.flush("external gzip chunk", true, false);
    scene.stage(8, external_chunk(51, None));
    scene.stage(9, chunk_of(52, 1, None));
    scene.flush("external uncompressed beside an inline one", false, false);
    scene.stage(7, external_chunk(53, Some(Compression::LZ4)));
    scene.flush("external rewritten", true, false);
    scene.stage(7, chunk_of(54, 2, None));
    scene.flush("external replaced by an inline chunk", true, false);
    scene.stage(8, chunk_of(55, 1, None));
    scene.flush("second external replaced, no sync", false, false);
    out.push_str(&scene.out);
}

fn pending_scenes(out: &mut String) {
    let mut scene = Scene::new("pending .mcc deletes");
    scene.stage(7, external_chunk(60, Some(Compression::GZip)));
    scene.flush("external chunk", false, false);
    scene.stage(7, chunk_of(61, 1, None));
    scene.flush("inline replaces it, sync fails", true, true);
    scene.stage(7, external_chunk(62, Some(Compression::GZip)));
    scene.flush("external again: the delete is dropped", true, false);
    scene.stage(7, chunk_of(63, 1, None));
    scene.flush("inline again, sync fails", true, true);
    scene.flush("retry: the file goes", true, false);
    out.push_str(&scene.out);

    let mut scene = Scene::new("deleting a .mcc that is gone or cannot go");
    scene.stage(7, external_chunk(64, None));
    scene.stage(8, external_chunk(65, None));
    scene.flush("two external chunks", false, false);
    let mcc = scene.region.external_path(7);
    let _ = std::fs::remove_file(&mcc);
    let blocked = scene.region.external_path(8);
    let _ = std::fs::remove_file(&blocked);
    let _ = std::fs::create_dir(&blocked);
    let _ = std::fs::write(blocked.join("keep"), b"x");
    scene.stage(7, chunk_of(66, 1, None));
    scene.stage(8, chunk_of(67, 1, None));
    scene.flush(
        "both replaced: one file is gone, one is a directory",
        true,
        false,
    );
    out.push_str(&scene.out);
}

fn failing_scenes(out: &mut String) {
    let mut scene = Scene::new("sync fails: the flush can be retried");
    scene.stage(1, chunk_of(70, 1, None));
    scene.flush("first", false, false);
    scene.stage(1, chunk_of(71, 2, None));
    scene.stage(2, chunk_of(72, 1, None));
    scene.flush("rewrite, sync fails", true, true);
    scene.flush("retry", true, false);
    out.push_str(&scene.out);

    let mut scene = Scene::new("the region file cannot be opened");
    scene.stage(1, chunk_of(73, 1, None));
    let blocked = scene.dir.path().join("blocked");
    let _ = std::fs::create_dir(&blocked);
    scene.region.path = blocked;
    scene.flush("path is a directory", true, false);
    out.push_str(&scene.out);

    let mut scene = Scene::new("the .mcc cannot be written");
    scene.stage(7, external_chunk(74, None));
    let blocked = scene.region.external_path(7);
    let _ = std::fs::create_dir(&blocked);
    let _ = std::fs::write(blocked.join("keep"), b"x");
    scene.stage(3, chunk_of(75, 1, None));
    scene.flush("target is a non-empty directory", true, false);
    out.push_str(&scene.out);

    let mut scene = Scene::new("a region on disk keeps its old timestamps");
    scene.stage(3, chunk_of(76, 1, None));
    scene.stage(4, chunk_of(77, 1, None));
    scene.flush("two chunks", false, false);
    scene.region.timestamps[3] = 1234;
    scene.region.timestamps[4] = 5678;
    scene.stage(3, chunk_of(78, 1, None));
    scene.flush("one is rewritten", false, false);
    out.push_str(&scene.out);
}

fn all_lines() -> String {
    let mut out = String::new();
    empty_scenes(&mut out);
    new_chunk_scenes(&mut out);
    rewrite_scenes(&mut out);
    several_scenes(&mut out);
    external_scenes(&mut out);
    pending_scenes(&mut out);
    failing_scenes(&mut out);
    out
}

#[test]
fn region_flushes_leave_the_same_bytes() -> Result<(), Box<dyn std::error::Error>> {
    let now = all_lines();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("anvil_flush.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, now);
    Ok(())
}
