use std::{
    fs::{self, File},
    io::ErrorKind,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use tracing::error;

use pumpkin_nbt::{
    compound::NbtCompound,
    nbt_compress::{read_gzip_compound_tag, write_gzip_compound_tag},
    tag::NbtTag,
};
use pumpkin_util::{Difficulty, world_seed::Seed};
use serde::{Deserialize, Serialize};

use crate::world_info::{
    DataPacks, MAXIMUM_SUPPORTED_LEVEL_VERSION, MAXIMUM_SUPPORTED_WORLD_DATA_VERSION,
    MINIMUM_SUPPORTED_LEVEL_VERSION, MINIMUM_SUPPORTED_WORLD_DATA_VERSION, WorldVersion,
    data_files::{
        minecraft_data_dir, read_game_rules, read_wandering_trader, read_weather,
        read_world_clocks, read_world_gen_settings, write_custom_boss_events_stub,
        write_game_rules, write_random_sequences_stub, write_scheduled_events_stub,
        write_scoreboard_stub, write_stopwatches_stub, write_wandering_trader, write_weather,
        write_world_clocks, write_world_gen_settings,
    },
    default_data_packs,
};

use super::{LevelData, WorldInfoError, WorldInfoReader, WorldInfoWriter};

pub const LEVEL_DAT_FILE_NAME: &str = "level.dat";
pub const LEVEL_DAT_BACKUP_FILE_NAME: &str = "level.dat_old";

const LEVEL_DATA_TAG: &str = "Data";
const WORLD_GEN_SETTINGS_TAG: &str = "WorldGenSettings";

pub struct AnvilLevelInfo;

fn check_data_version(data: &NbtCompound) -> Result<(), WorldInfoError> {
    let Some(data_version) = data.get_int("DataVersion") else {
        error!(
            "The level.dat file does not have a data version! This means it is either corrupt or very old (read unsupported)"
        );
        return Err(WorldInfoError::DeserializationError(
            "Missing DataVersion".into(),
        ));
    };

    if (MINIMUM_SUPPORTED_WORLD_DATA_VERSION..=MAXIMUM_SUPPORTED_WORLD_DATA_VERSION)
        .contains(&data_version)
    {
        Ok(())
    } else {
        Err(WorldInfoError::UnsupportedDataVersion(data_version))
    }
}

fn check_level_version(data: &NbtCompound) -> Result<(), WorldInfoError> {
    let Some(level_version) = data.get_int("version") else {
        error!(
            "The level.dat file does not have a level version! This means it is either corrupt or very old (read unsupported)"
        );
        return Err(WorldInfoError::DeserializationError(
            "Missing version".into(),
        ));
    };

    if (MINIMUM_SUPPORTED_LEVEL_VERSION..=MAXIMUM_SUPPORTED_LEVEL_VERSION).contains(&level_version)
    {
        Ok(())
    } else {
        Err(WorldInfoError::UnsupportedLevelVersion(level_version))
    }
}

const fn difficulty_from_id(id: i8) -> Option<Difficulty> {
    match id {
        0 => Some(Difficulty::Peaceful),
        1 => Some(Difficulty::Easy),
        2 => Some(Difficulty::Normal),
        3 => Some(Difficulty::Hard),
        _ => None,
    }
}

fn strings_from_nbt(tags: &[NbtTag]) -> Vec<String> {
    tags.iter()
        .filter_map(|tag| tag.extract_string().map(ToString::to_string))
        .collect()
}

fn strings_to_nbt(values: &[String]) -> Vec<NbtTag> {
    values
        .iter()
        .map(|value| NbtTag::from(value.as_str()))
        .collect()
}

fn world_version_from_nbt(version: &NbtCompound) -> WorldVersion {
    let mut world_version = WorldVersion::default();
    if let Some(name) = version.get_string("Name") {
        world_version.name = name.to_string();
    }
    if let Some(id) = version.get_int("Id") {
        world_version.id = id;
    }
    if let Some(snapshot) = version.get_bool("Snapshot") {
        world_version.snapshot = snapshot;
    }
    if let Some(series) = version.get_string("Series") {
        world_version.series = series.to_string();
    }
    world_version
}

fn world_version_to_nbt(version: &WorldVersion) -> NbtCompound {
    let mut compound = NbtCompound::new();
    compound.put_string("Name", version.name.clone());
    compound.put_int("Id", version.id);
    compound.put_bool("Snapshot", version.snapshot);
    compound.put_string("Series", version.series.clone());
    compound
}

fn data_packs_from_nbt(packs: &NbtCompound) -> DataPacks {
    let mut data_packs = default_data_packs();
    if let Some(disabled) = packs.get_list("Disabled") {
        data_packs.disabled = strings_from_nbt(disabled);
    }
    if let Some(enabled) = packs.get_list("Enabled") {
        data_packs.enabled = strings_from_nbt(enabled);
    }
    data_packs
}

fn data_packs_to_nbt(packs: &DataPacks) -> NbtCompound {
    let mut compound = NbtCompound::new();
    compound.put_list("Disabled", strings_to_nbt(&packs.disabled));
    compound.put_list("Enabled", strings_to_nbt(&packs.enabled));
    compound
}

fn stored_world_seed(level_folder: &Path, data: &NbtCompound) -> Option<i64> {
    read_world_gen_settings(level_folder)
        .map(|settings| settings.seed)
        .or_else(|| {
            data.get_compound(WORLD_GEN_SETTINGS_TAG)
                .and_then(|settings| settings.get_long("seed"))
        })
}

fn put_world_gen_settings_seed(data: &mut NbtCompound, seed: i64) {
    let mut world_gen_settings = data
        .get_compound(WORLD_GEN_SETTINGS_TAG)
        .cloned()
        .unwrap_or_default();
    world_gen_settings.put_long("seed", seed);
    data.put_compound(WORLD_GEN_SETTINGS_TAG, world_gen_settings);
}

fn update_world_border_from_nbt(level_data: &mut LevelData, data: &NbtCompound) {
    if let Some(border_center_x) = data.get_double("BorderCenterX") {
        level_data.border_center_x = border_center_x;
    }
    if let Some(border_center_z) = data.get_double("BorderCenterZ") {
        level_data.border_center_z = border_center_z;
    }
    if let Some(border_damage_per_block) = data.get_double("BorderDamagePerBlock") {
        level_data.border_damage_per_block = border_damage_per_block;
    }
    if let Some(border_size) = data.get_double("BorderSize") {
        level_data.border_size = border_size;
    }
    if let Some(border_safe_zone) = data.get_double("BorderSafeZone") {
        level_data.border_safe_zone = border_safe_zone;
    }
    if let Some(border_size_lerp_target) = data.get_double("BorderSizeLerpTarget") {
        level_data.border_size_lerp_target = border_size_lerp_target;
    }
    if let Some(border_size_lerp_time) = data.get_long("BorderSizeLerpTime") {
        level_data.border_size_lerp_time = border_size_lerp_time;
    }
    if let Some(border_warning_blocks) = data.get_double("BorderWarningBlocks") {
        level_data.border_warning_blocks = border_warning_blocks;
    }
    if let Some(border_warning_time) = data.get_double("BorderWarningTime") {
        level_data.border_warning_time = border_warning_time;
    }
}

fn update_spawn_from_nbt(level_data: &mut LevelData, data: &NbtCompound) {
    if let Some(spawn_comp) = data.get_compound("spawn") {
        if let Some(pos) = spawn_comp.get_int_array("pos")
            && pos.len() >= 3
        {
            level_data.spawn_x = pos[0];
            level_data.spawn_y = pos[1];
            level_data.spawn_z = pos[2];
        }
        if let Some(spawn_yaw) = spawn_comp
            .get_float("yaw")
            .or_else(|| spawn_comp.get_float("SpawnAngle"))
        {
            level_data.spawn_yaw = spawn_yaw;
        }
        if let Some(spawn_pitch) = spawn_comp.get_float("pitch") {
            level_data.spawn_pitch = spawn_pitch;
        }
    }
    if let Some(spawn_x) = data.get_int("SpawnX") {
        level_data.spawn_x = spawn_x;
    }
    if let Some(spawn_y) = data.get_int("SpawnY") {
        level_data.spawn_y = spawn_y;
    }
    if let Some(spawn_z) = data.get_int("SpawnZ") {
        level_data.spawn_z = spawn_z;
    }
    if let Some(spawn_yaw) = data
        .get_float("SpawnAngle")
        .or_else(|| data.get_float("SpawnYaw"))
    {
        level_data.spawn_yaw = spawn_yaw;
    }
    if let Some(spawn_pitch) = data.get_float("SpawnPitch") {
        level_data.spawn_pitch = spawn_pitch;
    }
}

fn level_data_from_nbt(data: &NbtCompound, seed: i64) -> LevelData {
    let mut level_data = LevelData::default(Seed(seed as u64));

    update_world_border_from_nbt(&mut level_data, data);
    update_spawn_from_nbt(&mut level_data, data);

    if let Some(allow_commands) = data.get_bool("allowCommands") {
        level_data.allow_commands = allow_commands;
    }
    if let Some(data_packs) = data.get_compound("DataPacks") {
        level_data.data_packs = data_packs_from_nbt(data_packs);
    }
    if let Some(data_version) = data.get_int("DataVersion") {
        level_data.data_version = data_version;
    }
    if let Some(diff_comp) = data.get_compound("difficulty_settings") {
        if let Some(diff_str) = diff_comp.get_string("difficulty") {
            match diff_str {
                "peaceful" => level_data.difficulty = Difficulty::Peaceful,
                "easy" => level_data.difficulty = Difficulty::Easy,
                "normal" => level_data.difficulty = Difficulty::Normal,
                "hard" => level_data.difficulty = Difficulty::Hard,
                _ => {}
            }
        }
        if let Some(difficulty_locked) = diff_comp.get_bool("locked") {
            level_data.difficulty_locked = difficulty_locked;
        }
    } else {
        if let Some(difficulty) = data.get_byte("Difficulty").and_then(difficulty_from_id) {
            level_data.difficulty = difficulty;
        }
        if let Some(difficulty_locked) = data.get_bool("DifficultyLocked") {
            level_data.difficulty_locked = difficulty_locked;
        }
    }
    if let Some(last_played) = data.get_long("LastPlayed") {
        level_data.last_played = last_played;
    }
    if let Some(level_name) = data.get_string("LevelName") {
        level_data.level_name = level_name.to_string();
    }
    if let Some(world_version) = data.get_compound("Version") {
        level_data.world_version = world_version_from_nbt(world_version);
    }
    if let Some(level_version) = data.get_int("version") {
        level_data.level_version = level_version;
    }
    if let Some(map_id) = data.get_int("map_id") {
        level_data.map_id = map_id;
    }
    if let Some(day_time) = data.get_long("DayTime") {
        level_data.day_time = day_time;
    }
    if let Some(clear_weather_time) = data.get_int("clearWeatherTime") {
        level_data.clear_weather_time = clear_weather_time;
    }

    level_data
}

fn level_data_to_nbt(info: &LevelData, data: &mut NbtCompound) {
    data.put_bool("allowCommands", info.allow_commands);
    data.put_double("BorderCenterX", info.border_center_x);
    data.put_double("BorderCenterZ", info.border_center_z);
    data.put_double("BorderDamagePerBlock", info.border_damage_per_block);
    data.put_double("BorderSize", info.border_size);
    data.put_double("BorderSafeZone", info.border_safe_zone);
    data.put_double("BorderSizeLerpTarget", info.border_size_lerp_target);
    data.put_long("BorderSizeLerpTime", info.border_size_lerp_time);
    data.put_double("BorderWarningBlocks", info.border_warning_blocks);
    data.put_double("BorderWarningTime", info.border_warning_time);
    data.put_compound("DataPacks", data_packs_to_nbt(&info.data_packs));
    data.put_int("DataVersion", info.data_version);

    // 26.2 difficulty_settings
    let mut diff_comp = NbtCompound::new();
    let diff_name = match info.difficulty {
        Difficulty::Peaceful => "peaceful",
        Difficulty::Easy => "easy",
        Difficulty::Normal => "normal",
        Difficulty::Hard => "hard",
    };
    diff_comp.put_string("difficulty", diff_name.to_string());
    diff_comp.put_bool("hardcore", false);
    diff_comp.put_bool("locked", info.difficulty_locked);
    data.put_compound("difficulty_settings", diff_comp);

    data.put_byte("Difficulty", info.difficulty as i8);
    data.put_bool("DifficultyLocked", info.difficulty_locked);
    data.put_long("LastPlayed", info.last_played);
    data.put_string("LevelName", info.level_name.clone());

    // 26.2 spawn
    let mut spawn_comp = NbtCompound::new();
    spawn_comp.put_string("dimension", "minecraft:overworld".to_string());
    spawn_comp.put(
        "pos",
        NbtTag::IntArray(vec![info.spawn_x, info.spawn_y, info.spawn_z]),
    );
    spawn_comp.put_float("pitch", info.spawn_pitch);
    spawn_comp.put_float("yaw", info.spawn_yaw);
    data.put_compound("spawn", spawn_comp);

    data.put_int("SpawnX", info.spawn_x);
    data.put_int("SpawnY", info.spawn_y);
    data.put_int("SpawnZ", info.spawn_z);
    data.put_float("SpawnAngle", info.spawn_yaw);
    data.put_float("SpawnPitch", info.spawn_pitch);
    data.put_compound("Version", world_version_to_nbt(&info.world_version));
    data.put_int("version", info.level_version);
    data.put_int("map_id", info.map_id);
    put_world_gen_settings_seed(data, info.world_gen_settings.seed);
}

fn stamp_current_version(level_data: &mut LevelData) {
    level_data.data_version = MAXIMUM_SUPPORTED_WORLD_DATA_VERSION;
    level_data.level_version = MAXIMUM_SUPPORTED_LEVEL_VERSION;
    level_data.world_version = WorldVersion::default();
}

fn existing_level_dat_root(path: &Path) -> Result<NbtCompound, WorldInfoError> {
    match File::open(path) {
        Ok(file) => read_gzip_compound_tag(file)
            .map_err(|e| WorldInfoError::DeserializationError(e.to_string())),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(NbtCompound::new()),
        Err(e) => Err(e.into()),
    }
}

impl WorldInfoReader for AnvilLevelInfo {
    fn read_world_info(&self, level_folder: &Path) -> Result<LevelData, WorldInfoError> {
        self.read_world_info_from(level_folder, LEVEL_DAT_FILE_NAME)
    }
}

impl AnvilLevelInfo {
    /// Reads the level data from `file_name` in `level_folder`.
    ///
    /// The file is `level.dat`, or its backup [`LEVEL_DAT_BACKUP_FILE_NAME`] when vanilla falls
    /// back to it; the other files of the folder (world generation settings, game rules) are read
    /// as for `level.dat`.
    pub fn read_world_info_from(
        &self,
        level_folder: &Path,
        file_name: &str,
    ) -> Result<LevelData, WorldInfoError> {
        let path = level_folder.join(file_name);

        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Err(WorldInfoError::InfoNotFound);
            }
            Err(error) => return Err(error.into()),
        };
        let root = read_gzip_compound_tag(file)
            .map_err(|e| WorldInfoError::DeserializationError(e.to_string()))?;
        let Some(data) = root.get_compound(LEVEL_DATA_TAG) else {
            error!("The level.dat file has no {LEVEL_DATA_TAG} compound and is therefore corrupt");
            return Err(WorldInfoError::DeserializationError("Missing Data".into()));
        };

        check_data_version(data)?;
        check_level_version(data)?;

        let Some(seed) = stored_world_seed(level_folder, data) else {
            return Err(WorldInfoError::MissingWorldSeed);
        };

        let mut level_data = level_data_from_nbt(data, seed);

        if let Some(wgs) = read_world_gen_settings(level_folder) {
            level_data.world_gen_settings = wgs;
        }

        // game_rules.dat – prefer the new file; fall back to level.dat values
        if minecraft_data_dir(level_folder)
            .join("game_rules.dat")
            .exists()
        {
            level_data.game_rules = read_game_rules(level_folder);
        }

        if minecraft_data_dir(level_folder)
            .join("world_clocks.dat")
            .exists()
        {
            let clocks = read_world_clocks(level_folder);
            if let Some(overworld) = clocks.clocks.get("minecraft:overworld") {
                level_data.day_time = overworld.total_ticks;
            }
        }

        // weather.dat
        if minecraft_data_dir(level_folder)
            .join("weather.dat")
            .exists()
        {
            let weather = read_weather(level_folder);
            level_data.clear_weather_time = weather.clear_weather_time;
        }

        Ok(level_data)
    }
}

impl WorldInfoWriter for AnvilLevelInfo {
    fn write_world_info(
        &self,
        info: &LevelData,
        level_folder: &Path,
    ) -> Result<(), WorldInfoError> {
        fs::create_dir_all(level_folder)?;

        let start = SystemTime::now();
        let since_the_epoch = start.duration_since(UNIX_EPOCH).unwrap_or_default();
        let mut level_data = info.clone();
        level_data.last_played = since_the_epoch.as_millis() as i64;
        stamp_current_version(&mut level_data);

        write_level_dat(level_folder, &level_data)?;

        // data/minecraft/*.dat files: a failure is logged, the save still counts.
        let data_version = level_data.data_version;
        write_state_files(level_folder, info, data_version);
        write_stub_files(level_folder, data_version);

        Ok(())
    }
}

/// Writes `level.dat` through `level.dat_new`, keeping the previous file as the backup and
/// every tag of the old file this writer does not manage.
fn write_level_dat(level_folder: &Path, level_data: &LevelData) -> Result<(), WorldInfoError> {
    let path = level_folder.join(LEVEL_DAT_FILE_NAME);
    let path_new = level_folder.join("level.dat_new");
    let path_old = level_folder.join(LEVEL_DAT_BACKUP_FILE_NAME);

    let mut root = existing_level_dat_root(&path)?;
    let mut data_comp = root
        .get_compound(LEVEL_DATA_TAG)
        .cloned()
        .unwrap_or_default();
    level_data_to_nbt(level_data, &mut data_comp);
    root.put_compound(LEVEL_DATA_TAG, data_comp);

    write_gzip_compound_tag(root, File::create(&path_new)?)
        .map_err(|e| WorldInfoError::SerializationError(e.to_string()))?;

    if path.exists() {
        fs::copy(&path, &path_old)?;
    }
    fs::rename(&path_new, &path)?;
    Ok(())
}

/// Logs a failed `data/minecraft/<file>` write; the other files are still written.
fn log_write_failure(file: &str, result: Result<(), WorldInfoError>) {
    if let Err(e) = result {
        error!("Failed to write {file}: {e}");
    }
}

/// The files holding world state: game rules, world generation settings, the overworld clock
/// (other clocks are preserved), weather and the wandering trader (other fields preserved).
fn write_state_files(level_folder: &Path, info: &LevelData, data_version: i32) {
    log_write_failure(
        "game_rules.dat",
        write_game_rules(level_folder, &info.game_rules, data_version),
    );
    log_write_failure(
        "world_gen_settings.dat",
        write_world_gen_settings(level_folder, &info.world_gen_settings, data_version),
    );

    let mut clocks = read_world_clocks(level_folder);
    clocks.data_version = data_version;
    clocks
        .clocks
        .entry("minecraft:overworld".to_string())
        .and_modify(|c| c.total_ticks = info.day_time)
        .or_insert(crate::world_info::data_files::DimensionClock {
            total_ticks: info.day_time,
        });
    log_write_failure(
        "world_clocks.dat",
        write_world_clocks(level_folder, &clocks),
    );

    let mut weather = read_weather(level_folder);
    weather.clear_weather_time = info.clear_weather_time;
    weather.data_version = data_version;
    log_write_failure("weather.dat", write_weather(level_folder, &weather));

    let mut wandering_trader = read_wandering_trader(level_folder);
    wandering_trader.data_version = data_version;
    log_write_failure(
        "wandering_trader.dat",
        write_wandering_trader(level_folder, &wandering_trader),
    );
}

/// The files this engine writes empty (no feature fills them yet).
fn write_stub_files(level_folder: &Path, data_version: i32) {
    log_write_failure(
        "custom_boss_events.dat",
        write_custom_boss_events_stub(level_folder, data_version),
    );
    log_write_failure(
        "scheduled_events.dat",
        write_scheduled_events_stub(level_folder, data_version),
    );
    log_write_failure(
        "random_sequences.dat",
        write_random_sequences_stub(level_folder, data_version),
    );
    log_write_failure(
        "scoreboard.dat",
        write_scoreboard_stub(level_folder, data_version),
    );
    log_write_failure(
        "stopwatches.dat",
        write_stopwatches_stub(level_folder, data_version),
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct LevelDat {
    // This tag contains all the level data.
    #[serde(rename = "Data")]
    pub data: LevelData,
}

#[cfg(test)]
mod test;
#[cfg(test)]
mod write_golden;
