//! `stronghold_search_at_load = false` (what test servers use, card #79) only moves WHEN the
//! stronghold ring search runs: the rings a lazy level computes on first request are the rings a
//! level that started the search at load computes.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use pumpkin_config::world::LevelConfig;
use pumpkin_data::dimension::Dimension;
use pumpkin_world::level::Level;

fn rings(stronghold_search_at_load: bool) -> Vec<(i32, i32)> {
    let folder = tempfile::tempdir().unwrap();
    let config = LevelConfig {
        stronghold_search_at_load,
        ..LevelConfig::default()
    };
    let level = Level::from_root_folder(
        &config,
        folder.path().to_path_buf(),
        20_261_112,
        Dimension::OVERWORLD,
    );
    let generator = level.world_gen.load();
    let cache = generator
        .global_structure_cache()
        .expect("the overworld generator has a structure cache");
    cache.get_stronghold_chunks().to_vec()
}

#[tokio::test]
async fn deferred_stronghold_search_gives_the_rings_of_the_search_at_load() {
    let at_load = rings(true);
    let deferred = rings(false);
    assert_eq!(
        at_load.len(),
        128,
        "vanilla places 128 strongholds in rings"
    );
    assert_eq!(deferred, at_load);
}
