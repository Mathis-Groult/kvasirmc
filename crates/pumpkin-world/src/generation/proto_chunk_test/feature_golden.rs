//! A record of what `ConfiguredFeature::generate` does, made before the function was split
//! (`tests/golden/configured_features.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Every vanilla configured feature is generated through the dispatcher on a 3 x 3 grid of
//! chunks, on five terrains built once (grass, sea floor, a cave in stone, nether, end) and with
//! two seeds. Each case is hashed: the returned flag, every block of the nine chunks, their
//! heightmaps and the block entities left in the centre chunk (NBT with sorted keys); a panic is
//! recorded as such. One line per feature; `acted` counts the cases that changed a block, so a
//! feature that never acts on these terrains is visible.
//!
//! Not seen: features placed outside the 3 x 3 grid (the cache drops those writes).

use std::collections::HashMap;
use std::fmt::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};

use pumpkin_data::Block;
use pumpkin_data::BlockState;
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_nbt::tag::NbtTag;
use pumpkin_util::{
    math::position::BlockPos,
    random::{RandomGenerator, xoroshiro128::Xoroshiro},
    world_seed::Seed,
};

use super::stage_parity::{TestBlockRegistry, TestGridCache};
use crate::generation::{
    feature::configured_features::CONFIGURED_FEATURES, generator::WorldGenerator, get_world_gen,
    positions::chunk_pos::start_block_x, positions::chunk_pos::start_block_z,
    proto_chunk::ProtoChunk,
};

const SEEDS: [u64; 2] = [1, 20_261_112];

/// The ground a feature is generated on, and where.
#[derive(Clone, Copy)]
enum Terrain {
    /// Stone, dirt and a grass block top at y = 63; generated at y = 64.
    Grass,
    /// Stone, sand at y = 45..46 and water up to y = 62; generated on the sand.
    SeaFloor,
    /// Stone from y = 10 to 90 with an air room around the centre; generated on its floor.
    Cave,
    /// Netherrack up to y = 63 with crimson nylium on top; generated at y = 65.
    Nether,
    /// End stone from y = 50 to 60; generated at y = 61.
    End,
    /// Stone from y = 10 to 90; generated inside it at y = 40. Its lines were appended after
    /// the others (the feature list again, prefixed `deep`), so the earlier lines stayed as they
    /// were; geodes need stone all around their origin.
    Deep,
}

impl Terrain {
    const ALL: [Self; 5] = [
        Self::Grass,
        Self::SeaFloor,
        Self::Cave,
        Self::Nether,
        Self::End,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::Grass => "grass",
            Self::SeaFloor => "sea",
            Self::Cave => "cave",
            Self::Nether => "nether",
            Self::End => "end",
            Self::Deep => "deep",
        }
    }

    const fn origin(self) -> BlockPos {
        match self {
            Self::Grass => BlockPos::new(8, 64, 8),
            Self::SeaFloor => BlockPos::new(8, 47, 8),
            Self::Cave => BlockPos::new(8, 36, 8),
            Self::Nether => BlockPos::new(8, 65, 8),
            Self::End => BlockPos::new(8, 61, 8),
            Self::Deep => BlockPos::new(8, 40, 8),
        }
    }

    /// The block at height `y` of the column (x, z), `None` for air.
    fn block(self, x: i32, y: i32, z: i32) -> Option<&'static BlockState> {
        let state = match self {
            Self::Grass => match y {
                40..=59 => Block::STONE.default_state,
                60..=62 => Block::DIRT.default_state,
                63 => Block::GRASS_BLOCK.default_state,
                _ => return None,
            },
            Self::SeaFloor => match y {
                30..=44 => Block::STONE.default_state,
                45..=46 => Block::SAND.default_state,
                47..=62 => Block::WATER.default_state,
                _ => return None,
            },
            Self::Cave => {
                let in_room = (2..14).contains(&x) && (36..44).contains(&y) && (2..14).contains(&z);
                if !(10..=90).contains(&y) || in_room {
                    return None;
                }
                Block::STONE.default_state
            }
            Self::Nether => match y {
                40..=63 => Block::NETHERRACK.default_state,
                64 => Block::CRIMSON_NYLIUM.default_state,
                _ => return None,
            },
            Self::End => match y {
                50..=60 => Block::END_STONE.default_state,
                _ => return None,
            },
            Self::Deep => match y {
                10..=90 => Block::STONE.default_state,
                _ => return None,
            },
        };
        Some(state)
    }
}

/// The nine chunks of a terrain by chunk position.
type Template = HashMap<(i32, i32), ProtoChunk>;

/// The nine chunks of `terrain`, built block by block.
fn build_terrain(world_gen: &WorldGenerator, terrain: Terrain) -> HashMap<(i32, i32), ProtoChunk> {
    let mut chunks = HashMap::new();
    for chunk_x in -1..=1 {
        for chunk_z in -1..=1 {
            let mut chunk = ProtoChunk::new(chunk_x, chunk_z, world_gen);
            for local_x in 0..16 {
                for local_z in 0..16 {
                    let x = start_block_x(chunk_x) + local_x;
                    let z = start_block_z(chunk_z) + local_z;
                    for y in 0..=90 {
                        if let Some(state) = terrain.block(x, y, z) {
                            chunk.set_block_state(x, y, z, state);
                        }
                    }
                }
            }
            // The worldgen heightmaps are written by the noise stage, not by `set_block_state`;
            // ore and other features read them.
            chunk.flat_surface_wg_height_map = chunk.flat_surface_height_map;
            chunk.flat_ocean_floor_wg_height_map = chunk.flat_ocean_floor_height_map;
            chunks.insert((chunk_x, chunk_z), chunk);
        }
    }
    chunks
}

/// A fresh grid holding a copy of `template`'s blocks and heightmaps.
fn copy_grid(
    world_gen: &WorldGenerator,
    template: &HashMap<(i32, i32), ProtoChunk>,
) -> TestGridCache {
    let mut chunks = HashMap::new();
    for (&(chunk_x, chunk_z), source) in template {
        let mut chunk = ProtoChunk::new(chunk_x, chunk_z, world_gen);
        chunk.flat_block_map.clone_from(&source.flat_block_map);
        chunk.flat_surface_height_map = source.flat_surface_height_map;
        chunk.flat_ocean_floor_height_map = source.flat_ocean_floor_height_map;
        chunk.flat_surface_wg_height_map = source.flat_surface_wg_height_map;
        chunk.flat_ocean_floor_wg_height_map = source.flat_ocean_floor_wg_height_map;
        chunk.flat_motion_blocking_height_map = source.flat_motion_blocking_height_map;
        chunk.flat_motion_blocking_no_leaves_height_map =
            source.flat_motion_blocking_no_leaves_height_map;
        chunks.insert((chunk_x, chunk_z), chunk);
    }
    TestGridCache {
        center_pos: (0, 0),
        chunks,
    }
}

fn fnv(hash: u64, bytes: &[u8]) -> u64 {
    let mut hash = hash;
    for &byte in bytes {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// The blocks and heightmaps of the nine chunks in a fixed order.
fn grid_hash(grid: &TestGridCache) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for chunk_x in -1..=1 {
        for chunk_z in -1..=1 {
            let Some(chunk) = grid.chunks.get(&(chunk_x, chunk_z)) else {
                continue;
            };
            for id in &chunk.flat_block_map {
                hash = fnv(hash, &id.as_u16().to_le_bytes());
            }
            for map in [
                &chunk.flat_surface_height_map,
                &chunk.flat_ocean_floor_height_map,
                &chunk.flat_surface_wg_height_map,
                &chunk.flat_ocean_floor_wg_height_map,
                &chunk.flat_motion_blocking_height_map,
                &chunk.flat_motion_blocking_no_leaves_height_map,
            ] {
                for height in map {
                    hash = fnv(hash, &height.to_le_bytes());
                }
            }
        }
    }
    hash
}

/// NBT as text with the compound keys sorted (a compound is a `HashMap`).
fn nbt_text(tag: &NbtTag, text: &mut String) {
    match tag {
        NbtTag::Compound(compound) => compound_text(compound, text),
        NbtTag::List(items) => {
            text.push('[');
            for item in items {
                nbt_text(item, text);
                text.push(',');
            }
            text.push(']');
        }
        other => {
            let _ = write!(text, "{other:?}");
        }
    }
}

fn compound_text(compound: &NbtCompound, text: &mut String) {
    let mut keys: Vec<&str> = compound.child_tags.keys().map(AsRef::as_ref).collect();
    keys.sort_unstable();
    text.push('{');
    for key in keys {
        let _ = write!(text, "{key}:");
        if let Some(tag) = compound.child_tags.get(key) {
            nbt_text(tag, text);
        }
        text.push(',');
    }
    text.push('}');
}

/// The terrains of one section of the report, built once, with the hash of each bare grid.
struct Section {
    prefix: &'static str,
    templates: Vec<(Terrain, Template)>,
    bare: Vec<u64>,
}

impl Section {
    fn new(world_gen: &WorldGenerator, prefix: &'static str, terrains: &[Terrain]) -> Self {
        let templates: Vec<_> = terrains
            .iter()
            .map(|&terrain| (terrain, build_terrain(world_gen, terrain)))
            .collect();
        let bare = templates
            .iter()
            .map(|(_, template)| grid_hash(&copy_grid(world_gen, template)))
            .collect();
        Self {
            prefix,
            templates,
            bare,
        }
    }

    /// One line per feature: every terrain of the section with every seed.
    fn report(
        &self,
        world_gen: &WorldGenerator,
        fallback: pumpkin_data::placed_feature::PlacedFeature,
        report: &mut String,
    ) {
        let mut features: Vec<_> = CONFIGURED_FEATURES.iter().collect();
        features.sort_by_key(|(key, _)| key.to_name());
        for (key, feature) in features {
            let name = key.to_name();
            let placed =
                pumpkin_data::placed_feature::PlacedFeature::from_name(name).unwrap_or(fallback);
            let mut hash = 0xcbf2_9ce4_8422_2325u64;
            let (mut placed_true, mut acted, mut panics, mut entities) = (0, 0, 0, 0);
            for ((terrain, template), bare_hash) in self.templates.iter().zip(&self.bare) {
                for seed in SEEDS {
                    let mut grid = copy_grid(world_gen, template);
                    let mut random = RandomGenerator::Xoroshiro(Xoroshiro::from_seed(seed));
                    let outcome = catch_unwind(AssertUnwindSafe(|| {
                        feature.generate(
                            &mut grid,
                            &TestBlockRegistry,
                            -64,
                            384,
                            placed,
                            &mut random,
                            terrain.origin(),
                        )
                    }));
                    let mut text = format!("{} {seed} ", terrain.name());
                    if let Ok(result) = outcome {
                        placed_true += usize::from(result);
                        let blocks = grid_hash(&grid);
                        acted += usize::from(blocks != *bare_hash);
                        let _ = write!(text, "{result} {blocks:016x}");
                        if let Some(centre) = grid.chunks.get(&(0, 0)) {
                            entities += centre.pending_block_entities.len();
                            for nbt in &centre.pending_block_entities {
                                compound_text(nbt, &mut text);
                            }
                        }
                    } else {
                        panics += 1;
                        text.push_str("panic");
                    }
                    hash = fnv(hash, text.as_bytes());
                }
            }
            let _ = writeln!(
                report,
                "{}{name}: {hash:016x} true {placed_true} acted {acted} entities {entities}{}",
                self.prefix,
                if panics > 0 {
                    format!(" PANICS {panics}")
                } else {
                    String::new()
                }
            );
        }
    }
}

#[test]
fn configured_features() -> Result<(), Box<dyn std::error::Error>> {
    let world_gen = get_world_gen(
        Seed(1),
        pumpkin_data::dimension::Dimension::OVERWORLD,
        true,
        Vec::new(),
        "minecraft:plains".to_owned(),
    );
    let fallback = pumpkin_data::placed_feature::PlacedFeature::from_name(
        pumpkin_data::placed_feature::PlacedFeature::all_names()[0],
    )
    .ok_or("no placed feature")?;
    let mut report = String::new();
    Section::new(&world_gen, "", &Terrain::ALL).report(&world_gen, fallback, &mut report);
    Section::new(&world_gen, "deep ", &[Terrain::Deep]).report(&world_gen, fallback, &mut report);

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("configured_features.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
