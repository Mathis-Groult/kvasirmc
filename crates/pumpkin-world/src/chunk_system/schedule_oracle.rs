//! What `GenerationSchedule` (the chunk scheduler thread, `schedule.rs` `work`) must do, made
//! before `work` was split (`tests/golden/chunk_schedule.txt`; rewrite it on purpose with
//! `KVASIR_BLESS=1`).
//!
//! The scheduler's dispatch order is not deterministic (generation threads, async IO readers,
//! 5 ms timeouts), so this records only what must hold whatever the order, through a real
//! `Level` on a temporary folder:
//! - flat world (deterministic content): a 7 x 7 area fetched at once reaches the caller,
//!   every chunk with its status and a hash of all its block states; the same area from a
//!   second fresh folder gives the same hashes; after the scheduler unloads it
//!   (`should_unload`), fetching it again reads it back from disk unchanged; after `shutdown`
//!   and a reopen of the folder, unchanged again;
//! - noise world (content near chunk borders depends on the order): a 3 x 3 area reaches the
//!   caller; after `shutdown` and a reopen, every chunk reads back unchanged.
//!
//! Every wait has a deadline, so a scheduler that stalls fails the test instead of hanging it.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

use pumpkin_config::world::LevelConfig;
use pumpkin_data::dimension::Dimension;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::world_seed::Seed;

use crate::generation::get_world_gen;
use crate::generation::proto_chunk_test::stage_parity::TestBlockRegistry;
use crate::level::{Level, SyncChunk};
use crate::world_info::FlatLevelGeneratorPreset;

type TestResult = Result<(), Box<dyn std::error::Error>>;
/// Each chunk's status and block hash, by position.
type Area = BTreeMap<(i32, i32), (String, u64)>;

const SEED: i64 = 20_261_112;
const FETCH_DEADLINE: Duration = Duration::from_secs(240);
const UNLOAD_DEADLINE: Duration = Duration::from_secs(60);

fn fnv(hash: u64, bytes: &[u8]) -> u64 {
    let mut hash = hash;
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn chunk_summary(chunk: &SyncChunk) -> (String, u64) {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for state in chunk.section.dump_blocks() {
        hash = fnv(hash, &state.as_u16().to_le_bytes());
    }
    (format!("{:?}", chunk.status), hash)
}

fn open(folder: &Path, flat: bool) -> Arc<Level> {
    let level = Level::from_root_folder(
        &LevelConfig::default(),
        folder.to_path_buf(),
        SEED,
        Dimension::OVERWORLD,
    );
    // The engine's server installs its block registry here at start (`server/startup.rs`);
    // without one every generation task fails and is re-queued for ever.
    level
        .world_portal
        .store(Arc::new(Some(Arc::new(TestBlockRegistry))));
    if flat {
        let layers = FlatLevelGeneratorPreset::from_name("classic_flat")
            .map(|preset| preset.settings.to_flat_layers())
            .unwrap_or_default();
        level.set_world_gen(Arc::from(get_world_gen(
            Seed(SEED as u64),
            Dimension::OVERWORLD,
            true,
            layers,
            "minecraft:plains".to_owned(),
        )));
    }
    level
}

fn positions(radius: i32) -> Vec<Vector2<i32>> {
    let mut all = Vec::new();
    for x in -radius..=radius {
        for z in -radius..=radius {
            all.push(Vector2::new(x, z));
        }
    }
    all
}

/// Fetches every position at once (as players joining do) under the deadline.
async fn fetch(level: &Arc<Level>, radius: i32) -> Result<Area, String> {
    let requests = positions(radius).into_iter().map(|pos| {
        let level = Arc::clone(level);
        async move {
            (
                (pos.x, pos.y),
                level.get_or_fetch_chunk(pos, chunk_summary).await,
            )
        }
    });
    let fetched = tokio::time::timeout(FETCH_DEADLINE, futures::future::join_all(requests))
        .await
        .map_err(|_| format!("the scheduler did not deliver a {radius}-radius area in time"))?;
    Ok(fetched.into_iter().collect())
}

/// Asks the scheduler to unload and waits until none of the area is published any more.
async fn unload(level: &Arc<Level>, radius: i32) -> Result<(), String> {
    let start = Instant::now();
    loop {
        level.should_unload.store(true, Relaxed);
        level.level_channel.notify();
        let left = positions(radius)
            .iter()
            .filter(|pos| level.loaded_chunks.contains_key(pos))
            .count();
        if left == 0 {
            return Ok(());
        }
        if start.elapsed() > UNLOAD_DEADLINE {
            return Err(format!(
                "{left} chunks were still loaded after the deadline"
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn area_line(name: &str, area: &Area) -> String {
    let mut statuses: BTreeMap<&str, usize> = BTreeMap::new();
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for ((x, z), (status, chunk_hash)) in area {
        *statuses.entry(status).or_default() += 1;
        hash = fnv(hash, format!("{x},{z}:{chunk_hash:016x};").as_bytes());
    }
    format!(
        "{name}: {} chunks {statuses:?} hash {hash:016x}",
        area.len()
    )
}

#[tokio::test]
async fn chunk_schedule() -> TestResult {
    let mut report = String::new();

    // Flat world.
    let folder = tempfile::tempdir()?;
    let level = open(folder.path(), true);
    let fresh = fetch(&level, 3).await?;
    let _ = writeln!(report, "{}", area_line("flat fresh", &fresh));

    let other_folder = tempfile::tempdir()?;
    let other = open(other_folder.path(), true);
    let fresh_again = fetch(&other, 3).await?;
    other.shutdown().await;
    let _ = writeln!(report, "flat second folder equal: {}", fresh_again == fresh);

    unload(&level, 3).await?;
    let reloaded = fetch(&level, 3).await?;
    let _ = writeln!(report, "flat after unload equal: {}", reloaded == fresh);
    level.shutdown().await;

    let level = open(folder.path(), true);
    let reopened = fetch(&level, 3).await?;
    level.shutdown().await;
    let _ = writeln!(report, "flat after reopen equal: {}", reopened == fresh);

    // Noise world.
    let folder = tempfile::tempdir()?;
    let level = open(folder.path(), false);
    let noise = fetch(&level, 1).await?;
    level.shutdown().await;
    let statuses: Vec<&String> = noise.values().map(|(status, _)| status).collect();
    let _ = writeln!(
        report,
        "noise fresh: {} chunks, statuses {:?}",
        noise.len(),
        statuses.iter().collect::<std::collections::BTreeSet<_>>()
    );
    let level = open(folder.path(), false);
    let noise_reopened = fetch(&level, 1).await?;
    level.shutdown().await;
    let _ = writeln!(
        report,
        "noise after reopen equal: {}",
        noise_reopened == noise
    );

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("chunk_schedule.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
