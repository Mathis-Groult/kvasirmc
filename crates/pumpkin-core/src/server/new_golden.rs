//! Golden of `Server::new` over world folders: fresh, restarted, without nether and end, with
//! a corrupt or an unsupported `level.dat`, and with only a backup. Records the result, the
//! seed, the dimensions and loaded worlds, the files left behind (names only; `level.dat`
//! holds a timestamp), whether the backup equals `level.dat`, and whether a refused
//! `level.dat` kept its bytes.

use std::path::{Path, PathBuf};

use pumpkin_config::{AdvancedConfiguration, BasicConfiguration, TelemetryConfig};
use pumpkin_data::dimension::Dimension;
use pumpkin_world::generation::generator::{GeneratorInit, VanillaGenerator};
use pumpkin_world::world_info::anvil::{
    AnvilLevelInfo, LEVEL_DAT_BACKUP_FILE_NAME, LEVEL_DAT_FILE_NAME,
};
use pumpkin_world::world_info::{LevelData, WorldInfoWriter};

use super::Server;
use crate::data::VanillaData;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const SEED: u64 = 20_261_111;

fn vanilla() -> VanillaData {
    VanillaData {
        banned_ip_list: std::sync::RwLock::default(),
        banned_player_list: std::sync::RwLock::default(),
        operator_config: std::sync::RwLock::default(),
        user_cache: std::sync::RwLock::default(),
        whitelist_config: std::sync::RwLock::default(),
    }
}

fn basic(world: &Path, nether: bool, end: bool, seed: u64) -> BasicConfiguration {
    let mut basic = BasicConfiguration {
        default_level_name: world.to_string_lossy().into_owned(),
        allow_nether: nether,
        allow_end: end,
        ..BasicConfiguration::default()
    };
    basic.seed.0 = seed;
    basic
}

/// Every file and folder under `root`, relative, sorted.
fn tree(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(folder) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .strip_prefix(root)
                .map_or_else(|_| path.display().to_string(), |p| p.display().to_string());
            if path.is_dir() {
                found.push(format!("{name}/"));
                pending.push(path);
            } else {
                found.push(name);
            }
        }
    }
    found.sort();
    found
}

fn read(path: &PathBuf) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

async fn start(world: &Path, nether: bool, end: bool) -> String {
    start_seeded(world, nether, end, SEED).await
}

async fn start_seeded(world: &Path, nether: bool, end: bool, seed: u64) -> String {
    let dat = world.join(LEVEL_DAT_FILE_NAME);
    let before = read(&dat);
    let started = Server::new(
        basic(world, nether, end, seed),
        AdvancedConfiguration::default(),
        TelemetryConfig {
            enabled: false,
            ..TelemetryConfig::default()
        },
        vanilla(),
        Vec::new(),
    )
    .await;
    let mut line = match &started {
        Ok(server) => {
            let dimensions: Vec<&str> =
                server.dimensions.iter().map(|d| d.minecraft_name).collect();
            let worlds: Vec<String> = server
                .worlds
                .load()
                .iter()
                .map(|w| format!("{} seed {}", w.dimension.minecraft_name, w.level.seed.0))
                .collect();
            format!(
                "ok seed {} dimensions {dimensions:?} worlds {worlds:?}",
                server.level_info.load().world_gen_settings.seed,
            )
        }
        Err(error) => format!("error {error:?}"),
    };
    if let Ok(server) = started {
        server.stop();
    }
    let after = read(&dat);
    let backup = read(&world.join(LEVEL_DAT_BACKUP_FILE_NAME));
    line.push_str(&format!(
        " | level.dat {} | backup {} | files {:?}",
        match (&before, &after) {
            (None, None) => "absent",
            (None, Some(_)) => "created",
            (Some(_), None) => "deleted",
            (Some(b), Some(a)) if a == b => "kept",
            (Some(_), Some(_)) => "rewritten",
        },
        match (&backup, &after) {
            (None, _) => "none",
            (Some(b), Some(a)) if a == b => "equals level.dat",
            (Some(_), _) => "differs",
        },
        tree(world),
    ));
    line
}

/// A real `level.dat` (the writer stamps the current version), then, if `data_version` is
/// given, that version written into its `Data` compound.
fn write_level_dat(world: &Path, seed: u64, data_version: Option<i32>) -> TestResult {
    let seed = pumpkin_util::world_seed::Seed(seed);
    let generator = VanillaGenerator::new(seed, Dimension::OVERWORLD);
    let data = LevelData::from_world_generator(seed, &generator);
    AnvilLevelInfo.write_world_info(&data, world)?;
    if let Some(version) = data_version {
        let path = world.join(LEVEL_DAT_FILE_NAME);
        let mut root =
            pumpkin_nbt::nbt_compress::read_gzip_compound_tag(std::fs::File::open(&path)?)?;
        let mut inner = root
            .get_compound("Data")
            .cloned()
            .ok_or("level.dat has no Data")?;
        inner.put_int("DataVersion", version);
        root.put_compound("Data", inner);
        pumpkin_nbt::nbt_compress::write_gzip_compound_tag(root, std::fs::File::create(&path)?)?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn server_new_over_world_folders() -> TestResult {
    let folder = tempfile::tempdir()?;
    let mut report = String::new();
    let mut case = |name: &str, line: String| {
        report.push_str(name);
        report.push_str(": ");
        report.push_str(&line);
        report.push('\n');
    };

    let world = folder.path().join("fresh");
    case("fresh", start(&world, true, true).await);
    case("restarted", start(&world, true, true).await);
    case(
        "restarted, other configured seed",
        start_seeded(&world, true, true, SEED + 7).await,
    );

    let world = folder.path().join("overworld_only");
    case("overworld only", start(&world, false, false).await);

    let world = folder.path().join("corrupt");
    std::fs::create_dir_all(&world)?;
    std::fs::write(world.join(LEVEL_DAT_FILE_NAME), b"not a level.dat")?;
    case("corrupt", start(&world, true, true).await);

    let world = folder.path().join("unsupported");
    std::fs::create_dir_all(&world)?;
    write_level_dat(&world, SEED, Some(1))?;
    case("unsupported", start(&world, true, true).await);

    let world = folder.path().join("backup_only");
    std::fs::create_dir_all(&world)?;
    // The backup's seed differs from the configured one, as an existing world's would.
    write_level_dat(&world, SEED + 1, None)?;
    std::fs::rename(
        world.join(LEVEL_DAT_FILE_NAME),
        world.join(LEVEL_DAT_BACKUP_FILE_NAME),
    )?;
    case("backup only", start(&world, true, true).await);

    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/server_new.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
