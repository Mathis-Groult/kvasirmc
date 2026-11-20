//! A record of what `GeodeFeature::generate` does, made before it is split
//! (`tests/golden/geode.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! The amethyst geode is generated with 64 seeds inside solid stone and inside stone with a
//! water-filled room around the origin. Each line has the seed, the returned flag and a hash of
//! the nine chunks' blocks and heightmaps.

use std::fmt::Write;

use pumpkin_util::{
    random::{RandomGenerator, xoroshiro128::Xoroshiro},
    world_seed::Seed,
};

use super::feature_golden::{Terrain, build_terrain, copy_grid, grid_hash};
use super::stage_parity::TestBlockRegistry;
use crate::generation::{feature::configured_features::CONFIGURED_FEATURES, get_world_gen};

#[test]
fn geode() -> Result<(), Box<dyn std::error::Error>> {
    let world_gen = get_world_gen(
        Seed(1),
        pumpkin_data::dimension::Dimension::OVERWORLD,
        true,
        Vec::new(),
        "minecraft:plains".to_owned(),
    );
    let placed = pumpkin_data::placed_feature::PlacedFeature::from_name(
        pumpkin_data::placed_feature::PlacedFeature::all_names()[0],
    )
    .ok_or("no placed feature")?;
    let (_, feature) = CONFIGURED_FEATURES
        .iter()
        .find(|(key, _)| key.to_name() == "amethyst_geode")
        .ok_or("no amethyst geode")?;
    let mut report = String::new();
    for terrain in [Terrain::Deep, Terrain::Wet, Terrain::Cave, Terrain::SeaFloor] {
        let template = build_terrain(&world_gen, terrain);
        for seed in 1..=64u64 {
            let mut grid = copy_grid(&world_gen, &template);
            let mut random = RandomGenerator::Xoroshiro(Xoroshiro::from_seed(seed));
            let result = feature.generate(
                &mut grid,
                &TestBlockRegistry,
                -64,
                384,
                placed,
                &mut random,
                terrain.origin(),
            );
            let _ = writeln!(
                report,
                "{} {seed} {result} {:016x}",
                terrain.name(),
                grid_hash(&grid)
            );
        }
    }

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("geode.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
