//! Card #33: amethyst geodes never generated. Vanilla 26.3 `GeodeFeature` (its codec, read with
//! javap) defaults the omitted `distribution_points` to 3-4 and `point_offset` to 1-2, and treats
//! `#minecraft:geode_invalid_blocks` (water, lava, ice, bedrock...) as invalid ground and
//! `#minecraft:features_cannot_replace` (bedrock, spawners, chests...) as untouchable.

use std::collections::HashMap;

use pumpkin_data::{Block, BlockState};
use pumpkin_util::{
    math::position::BlockPos,
    random::{RandomGenerator, xoroshiro128::Xoroshiro},
    world_seed::Seed,
};

use crate::generation::{
    feature::configured_features::CONFIGURED_FEATURES,
    get_world_gen,
    positions::chunk_pos::{start_block_x, start_block_z},
    proto_chunk::{GenerationCache, ProtoChunk},
    proto_chunk_test::stage_parity::{TestBlockRegistry, TestGridCache},
};

const CENTRE: BlockPos = BlockPos::new(8, 40, 8);
/// Inside the geode's shell but beside its distribution points, which lie 4 to 6 blocks from the
/// origin on each axis (a point in bedrock would count as invalid ground and refuse the geode).
const IN_SHELL: BlockPos = BlockPos::new(13, 45, 10);

/// A 3 x 3 grid of chunks filled from y = 10 to 70 with `fill`, and `special` at the cube of
/// half-size `radius` around `IN_SHELL`.
fn grid(fill: &'static BlockState, special: Option<(&'static BlockState, i32)>) -> TestGridCache {
    let world_gen = get_world_gen(
        Seed(1),
        pumpkin_data::dimension::Dimension::OVERWORLD,
        true,
        Vec::new(),
        "minecraft:plains".to_owned(),
    );
    let mut chunks = HashMap::new();
    for chunk_x in -1..=1 {
        for chunk_z in -1..=1 {
            let mut chunk = ProtoChunk::new(chunk_x, chunk_z, &world_gen);
            for local_x in 0..16 {
                for local_z in 0..16 {
                    let x = start_block_x(chunk_x) + local_x;
                    let z = start_block_z(chunk_z) + local_z;
                    for y in 10..=70 {
                        let state = match special {
                            Some((state, radius))
                                if (x - IN_SHELL.0.x).abs() <= radius
                                    && (y - IN_SHELL.0.y).abs() <= radius
                                    && (z - IN_SHELL.0.z).abs() <= radius =>
                            {
                                state
                            }
                            _ => fill,
                        };
                        chunk.set_block_state(x, y, z, state);
                    }
                }
            }
            chunk.flat_surface_wg_height_map = chunk.flat_surface_height_map;
            chunk.flat_ocean_floor_wg_height_map = chunk.flat_ocean_floor_height_map;
            chunks.insert((chunk_x, chunk_z), chunk);
        }
    }
    TestGridCache {
        center_pos: (0, 0),
        chunks,
    }
}

/// Generates the vanilla amethyst geode at the centre; returns its result.
fn generate(grid: &mut TestGridCache, seed: u64) -> Result<bool, String> {
    let feature = CONFIGURED_FEATURES
        .get(&pumpkin_data::configured_feature::ConfiguredFeature::AmethystGeode)
        .ok_or("no amethyst_geode")?;
    let placed = pumpkin_data::placed_feature::PlacedFeature::from_name("amethyst_geode")
        .ok_or("no placed amethyst_geode")?;
    let mut random = RandomGenerator::Xoroshiro(Xoroshiro::from_seed(seed));
    Ok(feature.generate(
        grid,
        &TestBlockRegistry,
        -64,
        384,
        placed,
        &mut random,
        CENTRE,
    ))
}

/// How many blocks of `block` the grid holds within 16 blocks of the centre.
fn count(grid: &TestGridCache, block: &Block) -> usize {
    let mut found = 0;
    for x in -16..=16 {
        for y in -16..=16 {
            for z in -16..=16 {
                let pos = CENTRE
                    .0
                    .add(&pumpkin_util::math::vector3::Vector3::new(x, y, z));
                found += usize::from(
                    GenerationCache::get_block_state(grid, &pos).to_block_id() == block.id,
                );
            }
        }
    }
    found
}

#[test]
fn a_geode_in_stone_places_its_layers() -> Result<(), String> {
    for seed in [1, 2, 3] {
        let mut grid = grid(Block::STONE.default_state, None);
        assert!(generate(&mut grid, seed)?, "seed {seed}");
        for layer in [
            &Block::SMOOTH_BASALT,
            &Block::CALCITE,
            &Block::AMETHYST_BLOCK,
        ] {
            assert!(count(&grid, layer) > 0, "seed {seed}: no {}", layer.name);
        }
    }
    Ok(())
}

#[test]
fn a_geode_in_water_is_refused() -> Result<(), String> {
    let mut grid = grid(Block::WATER.default_state, None);
    assert!(!generate(&mut grid, 1)?);
    assert_eq!(count(&grid, &Block::SMOOTH_BASALT), 0);
    Ok(())
}

#[test]
fn a_geode_does_not_replace_bedrock() -> Result<(), String> {
    let mut grid = grid(
        Block::STONE.default_state,
        Some((Block::BEDROCK.default_state, 1)),
    );
    let bedrock = count(&grid, &Block::BEDROCK);
    assert_eq!(bedrock, 27);
    assert!(generate(&mut grid, 1)?);
    assert!(
        count(&grid, &Block::CALCITE) > 0,
        "the geode was not placed"
    );
    assert_eq!(count(&grid, &Block::BEDROCK), bedrock);
    Ok(())
}
