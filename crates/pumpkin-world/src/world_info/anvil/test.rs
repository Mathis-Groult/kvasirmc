use pumpkin_data::game_rules::GameRuleRegistry;
use pumpkin_nbt::{
    compound::NbtCompound,
    nbt_compress::{read_gzip_compound_tag, write_gzip_compound_tag},
    tag::NbtTag,
};
use pumpkin_util::{Difficulty, world_seed::Seed};
use std::{
    fs::{self, File},
    io::ErrorKind,
    path::Path,
    sync::LazyLock,
};
use tempfile::TempDir;

use crate::{
    CURRENT_MC_VERSION,
    world_info::{
        DataPacks, LevelData, MAXIMUM_SUPPORTED_LEVEL_VERSION,
        MAXIMUM_SUPPORTED_WORLD_DATA_VERSION, MINIMUM_SUPPORTED_LEVEL_VERSION,
        MINIMUM_SUPPORTED_WORLD_DATA_VERSION, WorldGenSettings, WorldInfoError, WorldVersion,
        data_files::{minecraft_data_dir, read_world_gen_settings, write_world_gen_settings},
    },
};

use super::{AnvilLevelInfo, LEVEL_DAT_FILE_NAME, LevelDat, WorldInfoReader, WorldInfoWriter};

const CONVERTED_LEVEL_NAME: &str = "Converted World";

fn converted_level_dat(seed: Option<i64>) -> NbtCompound {
    let mut world_version = NbtCompound::new();
    world_version.put_string("Name", "1.21.9".to_string());
    world_version.put_int("Id", MINIMUM_SUPPORTED_WORLD_DATA_VERSION);

    let mut data_packs = NbtCompound::new();
    data_packs.put_list("Enabled", vec![NbtTag::from("vanilla")]);

    let mut data = NbtCompound::new();
    data.put_int("DataVersion", MINIMUM_SUPPORTED_WORLD_DATA_VERSION);
    data.put_int("version", MINIMUM_SUPPORTED_LEVEL_VERSION);
    data.put_string("LevelName", CONVERTED_LEVEL_NAME.to_string());
    data.put_bool("allowCommands", true);
    data.put_byte("Difficulty", Difficulty::Hard as i8);
    data.put_int("SpawnX", 128);
    data.put_int("SpawnY", 72);
    data.put_int("SpawnZ", -64);
    data.put_float("SpawnAngle", 90.0);
    data.put_double("BorderSize", 1024.0);
    data.put_long("DayTime", 5000);
    data.put_bool("hardcore", true);
    data.put_compound("Version", world_version);
    data.put_compound("DataPacks", data_packs);
    if let Some(seed) = seed {
        let mut world_gen_settings = NbtCompound::new();
        world_gen_settings.put_long("seed", seed);
        data.put_compound("WorldGenSettings", world_gen_settings);
    }

    let mut root = NbtCompound::new();
    root.put_compound("Data", data);
    root
}

fn write_level_dat(level_folder: &Path, root: NbtCompound) {
    let file = File::create(level_folder.join(LEVEL_DAT_FILE_NAME)).unwrap();
    write_gzip_compound_tag(root, file).unwrap();
}

fn read_level_dat(level_folder: &Path) -> NbtCompound {
    let file = File::open(level_folder.join(LEVEL_DAT_FILE_NAME)).unwrap();
    read_gzip_compound_tag(file).unwrap()
}

#[test]
fn preserve_level_dat_seed() {
    let seed = 1337;

    let data = LevelData::default(Seed(1337));

    let temp_dir = TempDir::new().unwrap();

    AnvilLevelInfo
        .write_world_info(&data, temp_dir.path())
        .unwrap();

    let data = AnvilLevelInfo.read_world_info(temp_dir.path()).unwrap();

    assert_eq!(data.world_gen_settings.seed, seed);
}

#[test]
fn creates_missing_world_directory_when_writing() {
    let temp_dir = TempDir::new().unwrap();
    let level_folder = temp_dir.path().join("world");

    AnvilLevelInfo
        .write_world_info(&LevelData::default(Seed(42)), &level_folder)
        .unwrap();

    assert!(level_folder.join(LEVEL_DAT_FILE_NAME).is_file());
    assert!(!level_folder.join("level.dat_new").exists());
}

#[test]
fn reports_missing_level_dat_as_info_not_found() {
    let temp_dir = TempDir::new().unwrap();

    let error = AnvilLevelInfo.read_world_info(temp_dir.path()).unwrap_err();

    assert!(matches!(error, WorldInfoError::InfoNotFound));
}

#[test]
fn reports_writer_io_errors_as_io_errors() {
    let temp_dir = TempDir::new().unwrap();
    let level_folder = temp_dir.path().join("world");
    File::create(&level_folder).unwrap();

    let error = AnvilLevelInfo
        .write_world_info(&LevelData::default(Seed(42)), &level_folder)
        .unwrap_err();

    let WorldInfoError::IoError(error) = error else {
        panic!("expected writer I/O error");
    };
    assert_eq!(error.kind(), ErrorKind::AlreadyExists);
}

#[test]
fn read_level_dat_without_series() {
    let temp_dir = TempDir::new().unwrap();
    write_level_dat(temp_dir.path(), converted_level_dat(Some(987_654_321)));

    let data = AnvilLevelInfo.read_world_info(temp_dir.path()).unwrap();

    assert_eq!(data.world_gen_settings.seed, 987_654_321);
    assert_eq!((data.spawn_x, data.spawn_y, data.spawn_z), (128, 72, -64));
    assert_eq!(data.spawn_yaw, 90.0);
    assert_eq!(data.level_name, CONVERTED_LEVEL_NAME);
    assert_eq!(data.difficulty, Difficulty::Hard);
    assert!(data.allow_commands);
    assert_eq!(data.border_size, 1024.0);
    assert_eq!(data.border_safe_zone, 5.0);
    assert_eq!(data.day_time, 5000);
    assert_eq!(data.data_version, MINIMUM_SUPPORTED_WORLD_DATA_VERSION);
    assert_eq!(data.level_version, MINIMUM_SUPPORTED_LEVEL_VERSION);
    assert_eq!(data.world_version.name, "1.21.9");
    assert_eq!(data.world_version.id, MINIMUM_SUPPORTED_WORLD_DATA_VERSION);
    assert_eq!(data.world_version.series, "main");
    assert!(!data.world_version.snapshot);
    assert_eq!(data.data_packs.enabled, vec!["vanilla".to_string()]);
    assert!(data.data_packs.disabled.is_empty());
}

#[test]
fn reject_level_dat_without_seed() {
    let temp_dir = TempDir::new().unwrap();
    write_level_dat(temp_dir.path(), converted_level_dat(None));

    let error = AnvilLevelInfo.read_world_info(temp_dir.path()).unwrap_err();

    assert!(matches!(error, WorldInfoError::MissingWorldSeed));
}

#[test]
fn read_26_x_world_gen_settings_layout() {
    let temp_dir = TempDir::new().unwrap();
    write_level_dat(temp_dir.path(), converted_level_dat(None));

    let data_dir = temp_dir
        .path()
        .join("dimensions")
        .join("minecraft")
        .join("overworld")
        .join("data")
        .join("minecraft");
    fs::create_dir_all(&data_dir).unwrap();

    let mut settings = NbtCompound::new();
    settings.put_long("seed", -2_016_744_919_588_476_706);
    let mut data = NbtCompound::new();
    data.put_compound("data", settings);
    let mut root = NbtCompound::new();
    root.put_compound("Data", data);

    write_gzip_compound_tag(
        root,
        File::create(data_dir.join("world_gen_settings.dat")).unwrap(),
    )
    .unwrap();

    let level_data = AnvilLevelInfo.read_world_info(temp_dir.path()).unwrap();

    assert_eq!(
        level_data.world_gen_settings.seed,
        -2_016_744_919_588_476_706
    );
}

#[test]
fn rewrite_level_dat_keeps_unmanaged_tags() {
    let temp_dir = TempDir::new().unwrap();
    write_level_dat(temp_dir.path(), converted_level_dat(Some(42)));

    let mut level_data = AnvilLevelInfo.read_world_info(temp_dir.path()).unwrap();
    level_data.level_name = "Renamed World".to_string();
    AnvilLevelInfo
        .write_world_info(&level_data, temp_dir.path())
        .unwrap();

    let root = read_level_dat(temp_dir.path());
    let data = root.get_compound("Data").unwrap();
    assert_eq!(data.get_bool("hardcore"), Some(true));
    assert_eq!(data.get_string("LevelName"), Some("Renamed World"));
    assert_eq!(
        data.get_compound("Version").unwrap().get_string("Series"),
        Some("main")
    );
}

#[test]
fn rewrite_level_dat_stamps_current_version() {
    let temp_dir = TempDir::new().unwrap();
    write_level_dat(temp_dir.path(), converted_level_dat(Some(42)));

    let imported = AnvilLevelInfo.read_world_info(temp_dir.path()).unwrap();
    AnvilLevelInfo
        .write_world_info(&imported, temp_dir.path())
        .unwrap();

    let root = read_level_dat(temp_dir.path());
    let data = root.get_compound("Data").unwrap();
    assert_eq!(
        data.get_int("DataVersion"),
        Some(MAXIMUM_SUPPORTED_WORLD_DATA_VERSION)
    );
    assert_eq!(
        data.get_int("version"),
        Some(MAXIMUM_SUPPORTED_LEVEL_VERSION)
    );

    let version = data.get_compound("Version").unwrap();
    assert_eq!(version.get_string("Name"), Some(CURRENT_MC_VERSION));
    assert_eq!(
        version.get_int("Id"),
        Some(MAXIMUM_SUPPORTED_WORLD_DATA_VERSION)
    );

    let game_rules_path = minecraft_data_dir(temp_dir.path()).join("game_rules.dat");
    let game_rules = read_gzip_compound_tag(File::open(game_rules_path).unwrap()).unwrap();
    assert_eq!(
        game_rules
            .get_compound("data")
            .unwrap()
            .get_int("DataVersion"),
        Some(MAXIMUM_SUPPORTED_WORLD_DATA_VERSION)
    );
}

#[test]
fn recover_seed_from_level_dat_after_losing_world_gen_settings() {
    let temp_dir = TempDir::new().unwrap();

    AnvilLevelInfo
        .write_world_info(&LevelData::default(Seed(8_675_309)), temp_dir.path())
        .unwrap();
    fs::remove_dir_all(temp_dir.path().join("data")).unwrap();

    let reloaded = AnvilLevelInfo.read_world_info(temp_dir.path()).unwrap();

    assert_eq!(reloaded.world_gen_settings.seed, 8_675_309);
}

#[test]
fn round_trip_level_dat() {
    let temp_dir = TempDir::new().unwrap();

    let mut original = LevelData::default(Seed(4242));
    original.level_name = "Round Trip".to_string();
    original.difficulty = Difficulty::Easy;
    original.difficulty_locked = true;
    original.set_pos(16, -32);
    original.spawn_y = 64;
    original.spawn_yaw = 45.0;
    original.border_size = 2048.0;
    original.border_center_x = 8.0;
    original.day_time = 12_345;
    original.map_id = 3;

    AnvilLevelInfo
        .write_world_info(&original, temp_dir.path())
        .unwrap();

    let root = read_level_dat(temp_dir.path());
    let data = root.get_compound("Data").unwrap();
    assert_eq!(
        data.get_int("DataVersion"),
        Some(MAXIMUM_SUPPORTED_WORLD_DATA_VERSION)
    );
    assert_eq!(
        data.get_int("version"),
        Some(MAXIMUM_SUPPORTED_LEVEL_VERSION)
    );
    assert_eq!(data.get_string("LevelName"), Some("Round Trip"));
    assert_eq!(data.get_byte("Difficulty"), Some(Difficulty::Easy as i8));
    assert_eq!(data.get_bool("DifficultyLocked"), Some(true));
    assert_eq!(data.get_bool("allowCommands"), Some(true));
    assert_eq!(data.get_int("SpawnX"), Some(16));
    assert_eq!(data.get_int("SpawnY"), Some(64));
    assert_eq!(data.get_int("SpawnZ"), Some(-32));
    assert_eq!(data.get_float("SpawnAngle"), Some(45.0));
    assert_eq!(data.get_double("BorderSize"), Some(2048.0));
    assert_eq!(data.get_double("BorderCenterX"), Some(8.0));
    assert_eq!(data.get_int("map_id"), Some(3));
    assert!(data.get_long("LastPlayed").is_some_and(|played| played > 0));

    let version = data.get_compound("Version").unwrap();
    assert_eq!(version.get_string("Name"), Some(CURRENT_MC_VERSION));
    assert_eq!(
        version.get_int("Id"),
        Some(MAXIMUM_SUPPORTED_WORLD_DATA_VERSION)
    );
    assert_eq!(version.get_bool("Snapshot"), Some(false));
    assert_eq!(version.get_string("Series"), Some("main"));

    let data_packs = data.get_compound("DataPacks").unwrap();
    assert_eq!(
        data_packs.get_list("Enabled"),
        Some([NbtTag::from("vanilla")].as_slice())
    );
    assert!(
        data_packs
            .get_list("Disabled")
            .is_some_and(<[NbtTag]>::is_empty)
    );

    let mut reloaded = AnvilLevelInfo.read_world_info(temp_dir.path()).unwrap();
    assert!(reloaded.last_played > 0);
    reloaded.last_played = original.last_played;

    assert_eq!(reloaded, original);
}

static LEVEL_DAT: LazyLock<LevelDat> = LazyLock::new(|| LevelDat {
    data: LevelData {
        allow_commands: true,
        border_center_x: 0.0,
        border_center_z: 0.0,
        border_damage_per_block: 0.2,
        border_size: 59_999_968.0,
        border_safe_zone: 5.0,
        border_size_lerp_target: 59_999_968.0,
        border_size_lerp_time: 0,
        border_warning_blocks: 5.0,
        border_warning_time: 15.0,
        clear_weather_time: 0,
        data_packs: DataPacks {
            disabled: vec![
                "minecart_improvements".to_string(),
                "redstone_experiments".to_string(),
                "trade_rebalance".to_string(),
            ],
            enabled: vec!["vanilla".to_string()],
        },
        data_version: 4189,
        day_time: 1727,
        difficulty: Difficulty::Normal,
        difficulty_locked: false,
        game_rules: GameRuleRegistry {
            block_explosion_drop_decay: true,
            command_block_output: true,
            drowning_damage: true,
            ender_pearls_vanish_on_death: true,
            fall_damage: true,
            fire_damage: true,
            forgive_dead_players: true,
            freeze_damage: true,
            global_sound_events: true,
            keep_inventory: false,
            lava_source_conversion: false,
            log_admin_commands: true,
            max_entity_cramming: 24,
            mob_explosion_drop_decay: true,
            mob_griefing: true,
            players_nether_portal_creative_delay: 0,
            players_nether_portal_default_delay: 80,
            players_sleeping_percentage: 100,
            projectiles_can_break_blocks: true,
            random_tick_speed: 3,
            reduced_debug_info: false,
            send_command_feedback: true,
            show_death_messages: true,
            spectators_generate_chunks: true,
            tnt_explosion_drop_decay: false,
            universal_anger: false,
            water_source_conversion: true,
            ..Default::default()
        },
        world_gen_settings: WorldGenSettings::new(Seed(1)),
        last_played: 1733847709327,
        level_name: "New World".to_string(),
        spawn_x: 160,
        spawn_y: 70,
        spawn_z: 160,
        spawn_yaw: 0.0,
        spawn_pitch: 0.0,
        level_version: 19133,
        world_version: WorldVersion {
            name: "1.21.4".to_string(),
            id: 4189,
            snapshot: false,
            series: "main".to_string(),
        },
        map_id: 0,
    },
});

// #[test]
// fn deserialize_level_dat() {
//     let raw_compressed_nbt = fs::read("assets/level_1_21_4.dat").unwrap();
//     assert!(!raw_compressed_nbt.is_empty());

//     let mut decoder = GzDecoder::new(&raw_compressed_nbt[..]);
//     let mut buf = Vec::new();
//     decoder.read_to_end(&mut buf).unwrap();
//     let level_dat: LevelDat = from_bytes(Cursor::new(buf)).expect("Failed to decode from file");

//     assert_eq!(level_dat, *LEVEL_DAT);
// }

#[test]
fn serialize_level_dat() {
    let mut data_comp = pumpkin_nbt::compound::NbtCompound::new();
    data_comp.put_int("DataVersion", LEVEL_DAT.data.data_version);
    data_comp.put_long("LastPlayed", LEVEL_DAT.data.last_played);

    let mut root = pumpkin_nbt::compound::NbtCompound::new();
    root.put_compound("Data", data_comp);

    let bytes = pumpkin_nbt::Nbt::from(root).write();
    assert!(!bytes.is_empty());
}

#[test]
fn spawn_and_difficulty_format_26_3() {
    let temp_dir = TempDir::new().unwrap();

    let mut data_comp = NbtCompound::new();
    data_comp.put_int("DataVersion", 5023);
    data_comp.put_int("version", 19133);
    data_comp.put_string("LevelName", "26.2 Test".to_string());
    data_comp.put_long("LastPlayed", 123456789);
    data_comp.put_long("DayTime", 1000);

    // 26.2 difficulty_settings compound
    let mut diff_comp = NbtCompound::new();
    diff_comp.put_string("difficulty", "hard".to_string());
    diff_comp.put_bool("locked", true);
    diff_comp.put_bool("hardcore", false);
    data_comp.put_compound("difficulty_settings", diff_comp);

    // 26.2 spawn compound
    let mut spawn_comp = NbtCompound::new();
    spawn_comp.put_string("dimension", "minecraft:overworld".to_string());
    spawn_comp.put("pos", NbtTag::IntArray(vec![42, -60, 99]));
    spawn_comp.put_float("pitch", 15.0);
    spawn_comp.put_float("yaw", 180.0);
    data_comp.put_compound("spawn", spawn_comp);

    let mut root = NbtCompound::new();
    root.put_compound("Data", data_comp);
    write_level_dat(temp_dir.path(), root);

    // Write world_gen_settings.dat
    let mut wgs = WorldGenSettings::new(Seed(777));
    wgs.dimensions.insert(
        "minecraft:overworld".to_string(),
        crate::world_info::Dimension {
            generator: crate::world_info::Generator {
                settings: Some(crate::world_info::GeneratorSettings::Compound(
                    serde_json::json!({
                        "biome": "minecraft:the_void",
                        "layers": [
                            { "block": "minecraft:air", "height": 1 }
                        ]
                    }),
                )),
                biome_source: None,
                generator_type: "minecraft:flat".to_string(),
            },
            dimension_type: "minecraft:overworld".to_string(),
        },
    );
    write_world_gen_settings(temp_dir.path(), &wgs, 5023).unwrap();

    let loaded = AnvilLevelInfo.read_world_info(temp_dir.path()).unwrap();
    assert_eq!(loaded.spawn_x, 42);
    assert_eq!(loaded.spawn_y, -60);
    assert_eq!(loaded.spawn_z, 99);
    assert_eq!(loaded.spawn_pitch, 15.0);
    assert_eq!(loaded.spawn_yaw, 180.0);
    assert_eq!(loaded.difficulty, Difficulty::Hard);
    assert!(loaded.difficulty_locked);

    let read_wgs = read_world_gen_settings(temp_dir.path()).unwrap();
    assert_eq!(read_wgs.seed, 777);
    let overworld_dim = read_wgs.dimensions.get("minecraft:overworld").unwrap();
    assert_eq!(overworld_dim.generator.generator_type, "minecraft:flat");
    if let Some(crate::world_info::GeneratorSettings::Compound(val)) =
        &overworld_dim.generator.settings
    {
        assert_eq!(val["biome"], "minecraft:the_void");
    } else {
        panic!("Expected Compound generator settings");
    }
}

#[test]
fn all_26_3_minecraft_data_files_written_and_read() {
    let temp_dir = TempDir::new().unwrap();
    let mut level_data = LEVEL_DAT.data.clone();
    level_data.data_version = 5023;
    level_data.day_time = 24000;
    level_data.clear_weather_time = 100;
    level_data.world_gen_settings = WorldGenSettings::new(Seed(9999));

    AnvilLevelInfo
        .write_world_info(&level_data, temp_dir.path())
        .unwrap();

    let data_dir = temp_dir.path().join("data").join("minecraft");
    let expected_files = [
        "game_rules.dat",
        "random_sequences.dat",
        "scoreboard.dat",
        "stopwatches.dat",
        "wandering_trader.dat",
        "world_clocks.dat",
        "world_gen_settings.dat",
        "scheduled_events.dat",
        "custom_boss_events.dat",
        "weather.dat",
    ];

    for file_name in &expected_files {
        let file_path = data_dir.join(file_name);
        assert!(
            file_path.exists(),
            "Expected file {file_name} to exist in data/minecraft/"
        );
    }

    // Verify world_gen_settings.dat read
    let loaded_wgs =
        crate::world_info::data_files::read_world_gen_settings(temp_dir.path()).unwrap();
    assert_eq!(loaded_wgs.seed, 9999);

    // Verify weather.dat read
    let loaded_weather = crate::world_info::data_files::read_weather(temp_dir.path());
    assert_eq!(loaded_weather.clear_weather_time, 100);

    // Verify world_clocks.dat read
    let loaded_clocks = crate::world_info::data_files::read_world_clocks(temp_dir.path());
    assert_eq!(
        loaded_clocks
            .clocks
            .get("minecraft:overworld")
            .unwrap()
            .total_ticks,
        24000
    );

    // Verify wandering_trader.dat read
    let loaded_wt = crate::world_info::data_files::read_wandering_trader(temp_dir.path());
    assert_eq!(loaded_wt.spawn_delay, 24000);
    assert_eq!(loaded_wt.spawn_chance, 25);
}
