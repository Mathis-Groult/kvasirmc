//! A record of what `AnvilLevelInfo::write_world_info` leaves on disk, made before it is split
//! (`tests/golden/world_info_write.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Every scene prepares a world folder (nothing, a previous save with unmanaged tags, a
//! level.dat without `Data`, a corrupt level.dat, blocked output paths, ...), writes one
//! `LevelData` and records the result plus every file of the folder: `.dat` files as sorted SNBT
//! (`LastPlayed` masked and checked against the clock around the call), other files by size.
//! `hardcore` is always written `false`: `LevelData` has no such field.

use std::{
    fmt::Write as _,
    fs::{self, File},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use pumpkin_data::game_rules::GameRuleRegistry;
use pumpkin_nbt::{
    compound::NbtCompound,
    nbt_compress::{read_gzip_compound_tag, write_gzip_compound_tag},
    tag::NbtTag,
};
use pumpkin_util::{Difficulty, world_seed::Seed};
use tempfile::TempDir;

use super::{AnvilLevelInfo, LEVEL_DAT_BACKUP_FILE_NAME, LEVEL_DAT_FILE_NAME, WorldInfoWriter};
use crate::world_info::{
    DataPacks, LevelData, WorldInfoError, WorldVersion,
    data_files::{
        DimensionClock, WanderingTraderData, WeatherData, WorldClocksData, minecraft_data_dir,
        write_wandering_trader, write_weather, write_world_clocks,
    },
};

type Res = Result<(), Box<dyn std::error::Error>>;

struct Scene {
    name: &'static str,
    setup: fn(&Path) -> Res,
    data: fn() -> LevelData,
}

fn snbt(tag: &NbtTag) -> String {
    match tag {
        NbtTag::End => "end".to_string(),
        NbtTag::Byte(v) => format!("{v}b"),
        NbtTag::Short(v) => format!("{v}s"),
        NbtTag::Int(v) => v.to_string(),
        NbtTag::Long(v) => format!("{v}L"),
        NbtTag::Float(v) => format!("{v:?}f"),
        NbtTag::Double(v) => format!("{v:?}d"),
        NbtTag::ByteArray(v) => format!("[B;{v:?}]"),
        NbtTag::String(v) => format!("{v:?}"),
        NbtTag::List(items) => {
            let items: Vec<String> = items.iter().map(snbt).collect();
            format!("[{}]", items.join(","))
        }
        NbtTag::Compound(c) => snbt_compound(c),
        NbtTag::IntArray(v) => format!("[I;{v:?}]"),
        NbtTag::LongArray(v) => format!("[L;{v:?}]"),
    }
}

fn snbt_compound(compound: &NbtCompound) -> String {
    let mut keys: Vec<&Box<str>> = compound.child_tags.keys().collect();
    keys.sort();
    let fields: Vec<String> = keys
        .into_iter()
        .map(|k| format!("{k}:{}", snbt(&compound.child_tags[k])))
        .collect();
    format!("{{{}}}", fields.join(","))
}

/// Replaces `Data.LastPlayed` by 0 and returns the value it had.
fn mask_last_played(root: &mut NbtCompound) -> Option<i64> {
    let NbtTag::Compound(data) = root.child_tags.get_mut("Data")? else {
        return None;
    };
    let NbtTag::Long(played) = data.child_tags.get_mut("LastPlayed")? else {
        return None;
    };
    Some(std::mem::replace(played, 0))
}

fn dump_file(path: &Path, rel: &str, window: Option<(i64, i64)>, out: &mut String) -> Res {
    let bytes = fs::read(path)?;
    match read_gzip_compound_tag(File::open(path)?) {
        Ok(mut root) => {
            if let (Some(played), Some(window), true) = (
                mask_last_played(&mut root),
                window,
                rel == LEVEL_DAT_FILE_NAME,
            ) {
                assert!(
                    (window.0..=window.1).contains(&played),
                    "LastPlayed {played} outside {window:?}"
                );
                writeln!(out, "  {rel} LastPlayed in the clock window")?;
            }
            writeln!(out, "  {rel} = {}", snbt_compound(&root))?;
        }
        Err(_) => writeln!(out, "  {rel} = unreadable, {} bytes", bytes.len())?,
    }
    Ok(())
}

fn dump_dir(base: &Path, dir: &Path, window: Option<(i64, i64)>, out: &mut String) -> Res {
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let rel = path.strip_prefix(base)?.display().to_string();
        if path.is_dir() {
            writeln!(out, "  {rel}/")?;
            dump_dir(base, &path, window, out)?;
        } else {
            dump_file(&path, &rel, window, out)?;
        }
    }
    Ok(())
}

fn error_name(error: &WorldInfoError) -> String {
    match error {
        WorldInfoError::IoError(e) => format!("IoError({:?})", e.kind()),
        WorldInfoError::DeserializationError(_) => "DeserializationError".to_string(),
        WorldInfoError::SerializationError(_) => "SerializationError".to_string(),
        other => format!("other {other}"),
    }
}

fn run_scene(scene: &Scene, out: &mut String) -> Res {
    let temp = TempDir::new()?;
    let folder = temp.path().join("nested").join("world");
    (scene.setup)(&folder)?;
    let info = (scene.data)();
    let before = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
    let result = AnvilLevelInfo.write_world_info(&info, &folder);
    let after = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
    writeln!(out, "== {}", scene.name)?;
    match &result {
        Ok(()) => writeln!(out, "  result Ok")?,
        Err(e) => writeln!(out, "  result {}", error_name(e))?,
    }
    if folder.exists() {
        dump_dir(
            &folder,
            &folder,
            result.is_ok().then_some((before, after)),
            out,
        )?;
    } else {
        writeln!(out, "  folder missing")?;
    }
    Ok(())
}

// --- inputs ---------------------------------------------------------------------------------

fn custom() -> LevelData {
    let mut data = LevelData::default(Seed(9_000_000_000));
    data.allow_commands = true;
    data.border_center_x = 12.5;
    data.border_center_z = -33.25;
    data.border_damage_per_block = 0.75;
    data.border_size = 1024.0;
    data.border_safe_zone = 3.5;
    data.border_size_lerp_target = 2048.0;
    data.border_size_lerp_time = 4_000;
    data.border_warning_blocks = 9.0;
    data.border_warning_time = 21.0;
    data.data_packs = DataPacks {
        disabled: vec!["bundle".to_string()],
        enabled: vec!["vanilla".to_string(), "file/extra".to_string()],
    };
    data.difficulty = Difficulty::Hard;
    data.difficulty_locked = true;
    data.level_name = "Custom World".to_string();
    data.spawn_x = -128;
    data.spawn_y = 71;
    data.spawn_z = 256;
    data.spawn_yaw = 135.5;
    data.spawn_pitch = -12.25;
    data.map_id = 17;
    data.game_rules = GameRuleRegistry {
        keep_inventory: true,
        fall_damage: false,
        random_tick_speed: 7,
        respawn_radius: 3,
        ..GameRuleRegistry::default()
    };
    data.day_time = 123_456;
    data.clear_weather_time = 777;
    data
}

/// Values the writer must overwrite: the stamp of the running version.
fn stale_version() -> LevelData {
    let mut data = custom();
    data.data_version = 1;
    data.level_version = 2;
    data.world_version = WorldVersion {
        name: "old".to_string(),
        id: 3,
        snapshot: true,
        series: "ccpreview".to_string(),
    };
    data
}

fn defaults() -> LevelData {
    LevelData::default(Seed(1))
}

fn peaceful() -> LevelData {
    let mut data = LevelData::default(Seed(5));
    data.difficulty = Difficulty::Peaceful;
    data
}

fn easy_locked() -> LevelData {
    let mut data = LevelData::default(Seed(6));
    data.difficulty = Difficulty::Easy;
    data.difficulty_locked = true;
    data
}

fn second_save() -> LevelData {
    let mut data = LevelData::default(Seed(2));
    data.level_name = "Second".to_string();
    data.day_time = 99;
    data.clear_weather_time = 55;
    data
}

// --- setups ---------------------------------------------------------------------------------

#[allow(clippy::unnecessary_wraps)] // same signature as the other setups
fn nothing(_: &Path) -> Res {
    Ok(())
}

fn write_root(folder: &Path, root: NbtCompound) -> Res {
    fs::create_dir_all(folder)?;
    write_gzip_compound_tag(root, File::create(folder.join(LEVEL_DAT_FILE_NAME))?)?;
    Ok(())
}

fn previous_save(folder: &Path) -> Res {
    AnvilLevelInfo.write_world_info(&custom(), folder)?;
    let mut root = read_gzip_compound_tag(File::open(folder.join(LEVEL_DAT_FILE_NAME))?)?;
    root.put_int("RootExtra", 11);
    let mut data = root.get_compound("Data").cloned().unwrap_or_default();
    data.put_string("Unmanaged", "kept".to_string());
    data.put_string("LevelName", "Replaced".to_string());
    root.put_compound("Data", data);
    write_root(folder, root)?;
    let mut clocks = WorldClocksData::default();
    clocks.clocks.insert(
        "minecraft:overworld".to_string(),
        DimensionClock { total_ticks: 5 },
    );
    clocks.clocks.insert(
        "minecraft:the_nether".to_string(),
        DimensionClock { total_ticks: 6 },
    );
    write_world_clocks(folder, &clocks)?;
    let weather = WeatherData {
        rain_time: 5,
        raining: true,
        thunder_time: 9,
        thundering: true,
        clear_weather_time: 1,
        data_version: 1,
    };
    write_weather(folder, &weather)?;
    let trader = WanderingTraderData {
        spawn_delay: 111,
        spawn_chance: 7,
        data_version: 1,
    };
    write_wandering_trader(folder, &trader)?;
    Ok(())
}

fn clocks_without_overworld(folder: &Path) -> Res {
    let mut clocks = WorldClocksData::default();
    clocks.clocks.insert(
        "minecraft:the_end".to_string(),
        DimensionClock { total_ticks: 8 },
    );
    Ok(write_world_clocks(folder, &clocks)?)
}

fn level_dat_without_data(folder: &Path) -> Res {
    let mut root = NbtCompound::new();
    root.put_int("Other", 1);
    write_root(folder, root)
}

fn corrupt_level_dat(folder: &Path) -> Res {
    fs::create_dir_all(folder)?;
    fs::write(folder.join(LEVEL_DAT_FILE_NAME), b"not gzip at all")?;
    Ok(())
}

fn level_dat_new_blocked(folder: &Path) -> Res {
    fs::create_dir_all(folder.join("level.dat_new"))?;
    Ok(())
}

fn backup_blocked(folder: &Path) -> Res {
    AnvilLevelInfo.write_world_info(&defaults(), folder)?;
    fs::create_dir(folder.join(LEVEL_DAT_BACKUP_FILE_NAME))?;
    Ok(())
}

const DATA_FILES: [&str; 10] = [
    "game_rules.dat",
    "world_gen_settings.dat",
    "world_clocks.dat",
    "weather.dat",
    "wandering_trader.dat",
    "custom_boss_events.dat",
    "scheduled_events.dat",
    "random_sequences.dat",
    "scoreboard.dat",
    "stopwatches.dat",
];

fn data_files_blocked(folder: &Path) -> Res {
    for name in DATA_FILES {
        fs::create_dir_all(minecraft_data_dir(folder).join(name))?;
    }
    Ok(())
}

fn one_data_file_blocked(folder: &Path) -> Res {
    fs::create_dir_all(minecraft_data_dir(folder).join("weather.dat"))?;
    Ok(())
}

fn data_dir_is_a_file(folder: &Path) -> Res {
    fs::create_dir_all(folder.join("data"))?;
    fs::write(minecraft_data_dir(folder), b"file")?;
    Ok(())
}

fn scenes() -> Vec<Scene> {
    vec![
        Scene {
            name: "defaults into a missing nested folder",
            setup: nothing,
            data: defaults,
        },
        Scene {
            name: "custom values, stale version stamp",
            setup: nothing,
            data: stale_version,
        },
        Scene {
            name: "custom values again, difficulty hard",
            setup: nothing,
            data: custom,
        },
        Scene {
            name: "peaceful",
            setup: nothing,
            data: peaceful,
        },
        Scene {
            name: "easy, locked",
            setup: nothing,
            data: easy_locked,
        },
        Scene {
            name: "over a previous save",
            setup: previous_save,
            data: second_save,
        },
        Scene {
            name: "clocks without overworld",
            setup: clocks_without_overworld,
            data: second_save,
        },
        Scene {
            name: "level.dat without Data",
            setup: level_dat_without_data,
            data: defaults,
        },
        Scene {
            name: "corrupt level.dat",
            setup: corrupt_level_dat,
            data: defaults,
        },
        Scene {
            name: "level.dat_new blocked",
            setup: level_dat_new_blocked,
            data: defaults,
        },
        Scene {
            name: "backup blocked",
            setup: backup_blocked,
            data: second_save,
        },
        Scene {
            name: "every data file blocked",
            setup: data_files_blocked,
            data: custom,
        },
        Scene {
            name: "only weather.dat blocked",
            setup: one_data_file_blocked,
            data: custom,
        },
        Scene {
            name: "data/minecraft is a file",
            setup: data_dir_is_a_file,
            data: custom,
        },
    ]
}

fn report() -> Result<String, Box<dyn std::error::Error>> {
    let mut out = String::new();
    for scene in scenes() {
        run_scene(&scene, &mut out)?;
    }
    Ok(out)
}

#[test]
fn world_info_write() -> Res {
    let report = report()?;
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("world_info_write.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(fs::read_to_string(&path)?, report);
    Ok(())
}
