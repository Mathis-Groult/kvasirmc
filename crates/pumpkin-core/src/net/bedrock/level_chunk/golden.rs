//! A record of the bytes `CLevelChunk::encode_chunk` produces for hand-built chunks, made before
//! the function was split into one piece per payload part (`tests/golden/bedrock_level_chunk.txt`;
//! rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! One line per chunk and cache mode: the section count, the length of the packet and an FNV-1a
//! hash of its bytes, then the blob list of the cache mode (count and a hash over hash+bytes).

use std::fmt::Write;
use std::sync::PoisonError;

use pumpkin_data::{Block, BlockStateId};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_world::chunk::palette::BiomePalette;
use pumpkin_world::chunk::{ChunkData, ChunkSections};

use super::CLevelChunk;

/// FNV-1a: a hash that does not change with the Rust version.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// A chunk with `count` sections starting at block height `min_y`.
fn chunk_with(x: i32, z: i32, count: usize, min_y: i32) -> ChunkData {
    let mut chunk = ChunkData::empty(x, z);
    chunk.section = ChunkSections::new(count, min_y);
    chunk
}

/// Writes `block` into section `index` at the cell (`x`, `y`, `z`) inside it.
fn put(chunk: &ChunkData, index: usize, cell: (usize, usize, usize), id: BlockStateId) {
    let mut sections = chunk
        .section
        .block_sections
        .write()
        .unwrap_or_else(PoisonError::into_inner);
    sections[index].set(cell.0, cell.1, cell.2, id);
}

/// `kinds` different block states spread over section `index`.
fn palette_of(chunk: &ChunkData, index: usize, kinds: u16) {
    for n in 0..kinds {
        let i = usize::from(n);
        if let Some(id) = BlockStateId::new(n + 1) {
            put(chunk, index, (i % 16, (i / 16) % 16, (i / 256) % 16), id);
        }
    }
}

/// Every cell of section `index` set to one state.
fn fill_section(chunk: &ChunkData, index: usize, id: BlockStateId) {
    for x in 0..16 {
        for y in 0..16 {
            for z in 0..16 {
                put(chunk, index, (x, y, z), id);
            }
        }
    }
}

fn set_biomes(chunk: &ChunkData, index: usize, kinds: u8) {
    let mut sections = chunk
        .section
        .biome_sections
        .write()
        .unwrap_or_else(PoisonError::into_inner);
    for n in 0..kinds {
        let i = usize::from(n);
        sections[index].set(i % 4, (i / 4) % 4, (i / 16) % 4, n);
    }
}

fn block_actor(id: &str, x: i32, y: i32, z: i32) -> NbtCompound {
    let mut nbt = NbtCompound::new();
    nbt.put_string("id", id.to_owned());
    nbt.put_int("x", x);
    nbt.put_int("y", y);
    nbt.put_int("z", z);
    nbt.put_string("CustomName", "golden".to_owned());
    nbt
}

/// Poisons a lock by panicking while holding its write guard.
fn poison_blocks(chunk: &ChunkData) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = chunk.section.block_sections.write();
        panic!("poison block_sections");
    }));
}

fn poison_biomes(chunk: &ChunkData) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = chunk.section.biome_sections.write();
        panic!("poison biome_sections");
    }));
}

struct Case {
    name: &'static str,
    chunk: ChunkData,
    dimension: i32,
    actors: Vec<NbtCompound>,
}

fn case(name: &'static str, chunk: ChunkData) -> Case {
    Case {
        name,
        chunk,
        dimension: 0,
        actors: Vec::new(),
    }
}

/// Block sections: empty, single state, and palettes of every packed width.
fn block_cases() -> Vec<Case> {
    let mut cases = vec![case("all_air", ChunkData::empty(0, 0))];

    let stone = ChunkData::empty(1, -1);
    fill_section(&stone, 0, Block::STONE.default_state.id);
    fill_section(&stone, 5, Block::DIRT.default_state.id);
    cases.push(case("single_state_sections", stone));

    // Palette sizes around every packed-width boundary (1..=6, 8 and 16 bits).
    for (name, kinds) in [
        ("palette_2", 1u16),
        ("palette_3", 2),
        ("palette_5", 4),
        ("palette_9", 8),
        ("palette_17", 16),
        ("palette_33", 32),
        ("palette_65", 64),
        ("palette_129", 128),
        ("palette_300", 300),
    ] {
        let chunk = ChunkData::empty(-3, 4);
        palette_of(&chunk, 3, kinds);
        palette_of(&chunk, 20, 1);
        cases.push(case(name, chunk));
    }

    let seagrass = ChunkData::empty(2, 2);
    put(&seagrass, 0, (0, 0, 0), Block::SEAGRASS.default_state.id);
    put(&seagrass, 7, (3, 4, 5), Block::STONE.default_state.id);
    cases.push(case("waterlogged_sections", seagrass));

    let water = ChunkData::empty(5, 5);
    fill_section(&water, 6, Block::WATER.default_state.id);
    put(&water, 8, (1, 1, 1), Block::WATER.default_state.id);
    put(&water, 8, (2, 2, 2), Block::STONE.default_state.id);
    cases.push(case("water_sections", water));
    cases
}

/// Biome sections: single, valid and unknown ids, palettes of growing size.
fn biome_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, kinds) in [
        ("biomes_2", 2u8),
        ("biomes_3", 3),
        ("biomes_9", 9),
        ("biomes_20", 20),
    ] {
        let chunk = ChunkData::empty(7, -8);
        set_biomes(&chunk, 0, kinds);
        set_biomes(&chunk, 23, 2);
        cases.push(case(name, chunk));
    }
    let unknown = ChunkData::empty(8, 8);
    let mut sections = unknown
        .section
        .biome_sections
        .write()
        .unwrap_or_else(PoisonError::into_inner);
    sections[2] = BiomePalette::Homogeneous(250);
    sections[3] = BiomePalette::Homogeneous(1);
    drop(sections);
    cases.push(case("biome_unknown_id", unknown));
    cases
}

/// Section layouts: top and bottom heights, negative y, a count that differs from the list.
fn layout_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, count, min_y) in [
        ("one_section_y0", 1usize, 0i32),
        ("bottom_section_y-128", 1, -2048),
        ("top_section_y127", 1, 2032),
        ("two_sections_y-1", 2, -16),
        ("no_sections", 0, 0),
        ("overworld_like_16", 16, 0),
    ] {
        let chunk = chunk_with(1, 1, count, min_y);
        if count > 0 {
            palette_of(&chunk, 0, 5);
            set_biomes(&chunk, 0, 3);
        }
        cases.push(case(name, chunk));
    }
    let mut mismatch = chunk_with(0, 0, 3, -64);
    mismatch.section.count = 5;
    cases.push(case("count_differs_from_list", mismatch));
    cases
}

/// Block entities and coordinates that need a sign bit or several bytes.
fn extra_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    let mut one = case("one_block_actor", ChunkData::empty(0, 0));
    one.actors = vec![block_actor("Chest", 1, 64, 2)];
    cases.push(one);

    let mut several = case("block_actors_nether", ChunkData::empty(-40, 300));
    several.dimension = -1;
    several.actors = vec![
        block_actor("Furnace", -640, -20, 4800),
        block_actor("Sign", -639, 100, 4801),
        NbtCompound::new(),
    ];
    several.chunk.section = ChunkSections::new(8, 0);
    palette_of(&several.chunk, 1, 12);
    cases.push(several);

    let mut end = case("end_dimension", ChunkData::empty(100_000, -100_000));
    end.dimension = 2;
    cases.push(end);

    let mut custom = case("dimension_1000", ChunkData::empty(1, 2));
    custom.dimension = 1000;
    cases.push(custom);
    cases
}

/// Locks poisoned by a panic: the encoder reports an error instead of panicking.
fn poisoned_cases() -> Vec<Case> {
    let blocks = ChunkData::empty(0, 0);
    poison_blocks(&blocks);
    let biomes = ChunkData::empty(0, 0);
    poison_biomes(&biomes);
    vec![
        case("poisoned_blocks", blocks),
        case("poisoned_biomes", biomes),
    ]
}

fn corpus() -> Vec<Case> {
    let mut cases = block_cases();
    cases.extend(biome_cases());
    cases.extend(layout_cases());
    cases.extend(extra_cases());
    cases.extend(poisoned_cases());
    cases
}

fn describe(case: &Case, cache_enabled: bool) -> String {
    let result =
        CLevelChunk::encode_chunk(&case.chunk, case.dimension, cache_enabled, &case.actors);
    let head = format!(
        "{} cache={cache_enabled} sections={}",
        case.name, case.chunk.section.count
    );
    match result {
        Ok((bytes, blobs)) => {
            let mut blob_bytes = Vec::new();
            for (hash, data) in &blobs {
                blob_bytes.extend_from_slice(&hash.to_le_bytes());
                blob_bytes.extend_from_slice(data);
            }
            format!(
                "{head} len={} hash={:016x} blobs={} blobhash={:016x}",
                bytes.len(),
                fnv(&bytes),
                blobs.len(),
                fnv(&blob_bytes)
            )
        }
        Err(error) => format!("{head} err {error}"),
    }
}

#[test]
fn level_chunk_payload_is_written_as_before() -> Result<(), Box<dyn std::error::Error>> {
    let mut now = String::new();
    for case in corpus() {
        for cache_enabled in [false, true] {
            writeln!(now, "{}", describe(&case, cache_enabled))?;
        }
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("bedrock_level_chunk.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, now);
    Ok(())
}
