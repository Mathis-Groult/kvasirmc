//! Vanilla 26.3 falls back to `level.dat_old` when `level.dat` is missing or unreadable
//! (`LevelStorageAccess.getUnfixedDataTagWithFallback`, then `restoreLevelDataFromOld`).

use std::path::Path;

use pumpkin_config::BasicConfiguration;
use pumpkin_data::dimension::Dimension;
use pumpkin_world::generation::generator::{GeneratorInit, VanillaGenerator};
use pumpkin_world::world_info::anvil::{
    AnvilLevelInfo, LEVEL_DAT_BACKUP_FILE_NAME, LEVEL_DAT_FILE_NAME,
};
use pumpkin_world::world_info::{LevelData, WorldInfoWriter};

use super::load_level_info;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const CONFIGURED: u64 = 20_261_111;
const STORED: u64 = 20_261_113;
const GARBAGE: &[u8] = b"not a level.dat";

fn configured() -> BasicConfiguration {
    let mut basic = BasicConfiguration::default();
    basic.seed.0 = CONFIGURED;
    basic
}

/// A world whose only level file is a good `level.dat_old` holding [`STORED`].
fn world_with_backup(world: &Path) -> TestResult {
    let seed = pumpkin_util::world_seed::Seed(STORED);
    let generator = VanillaGenerator::new(seed, Dimension::OVERWORLD);
    AnvilLevelInfo.write_world_info(&LevelData::from_world_generator(seed, &generator), world)?;
    std::fs::rename(
        world.join(LEVEL_DAT_FILE_NAME),
        world.join(LEVEL_DAT_BACKUP_FILE_NAME),
    )?;
    Ok(())
}

fn moved_aside(world: &Path) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error>> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(world)? {
        let entry = entry?;
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with("level.dat_corrupted_")
        {
            found.push(std::fs::read(entry.path())?);
        }
    }
    Ok(found)
}

#[test]
fn a_missing_level_dat_is_restored_from_the_backup() -> TestResult {
    let folder = tempfile::tempdir()?;
    let world = folder.path();
    world_with_backup(world)?;
    let level = load_level_info(world, &configured())?;
    assert_eq!(
        level.world_gen_settings.seed, STORED as i64,
        "a new world was made"
    );
    assert!(world.join(LEVEL_DAT_FILE_NAME).exists());
    assert!(moved_aside(world)?.is_empty());
    Ok(())
}

#[test]
fn a_corrupt_level_dat_is_moved_aside_and_the_backup_used() -> TestResult {
    let folder = tempfile::tempdir()?;
    let world = folder.path();
    world_with_backup(world)?;
    std::fs::write(world.join(LEVEL_DAT_FILE_NAME), GARBAGE)?;
    let level = load_level_info(world, &configured())?;
    assert_eq!(level.world_gen_settings.seed, STORED as i64);
    assert_eq!(moved_aside(world)?, vec![GARBAGE.to_vec()]);
    Ok(())
}

/// Vanilla shuts down when the fallback cannot be read either; it never makes a new world over
/// an existing one.
#[test]
fn an_unreadable_backup_stops_the_start() -> TestResult {
    let folder = tempfile::tempdir()?;
    let world = folder.path();
    std::fs::write(world.join(LEVEL_DAT_BACKUP_FILE_NAME), GARBAGE)?;
    assert!(
        load_level_info(world, &configured()).is_err(),
        "a new world was made"
    );
    assert!(!world.join(LEVEL_DAT_FILE_NAME).exists());

    std::fs::write(world.join(LEVEL_DAT_FILE_NAME), GARBAGE)?;
    assert!(load_level_info(world, &configured()).is_err());
    assert_eq!(std::fs::read(world.join(LEVEL_DAT_FILE_NAME))?, GARBAGE);
    assert_eq!(
        std::fs::read(world.join(LEVEL_DAT_BACKUP_FILE_NAME))?,
        GARBAGE
    );
    assert!(moved_aside(world)?.is_empty());
    Ok(())
}

/// Without any level file the world is new, from the configured seed.
#[test]
fn no_level_file_makes_a_new_world() -> TestResult {
    let folder = tempfile::tempdir()?;
    let level = load_level_info(folder.path(), &configured())?;
    assert_eq!(level.world_gen_settings.seed, CONFIGURED as i64);
    Ok(())
}
