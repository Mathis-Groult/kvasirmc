//! Golden records of whole structures: their piece layout and the blocks they place.
//!
//! A structure generator lays the pieces out (for mineshafts, strongholds and nether fortresses
//! that is their `add_*_children` recursion); the pieces are then placed chunk by chunk into stone
//! terrain through `StructurePiecesCollector::generate_in_chunk`, as the game does. Rewrite the
//! files on purpose with `KVASIR_BLESS=1`.

use std::fmt::Write;

use pumpkin_util::{
    random::{RandomGenerator, legacy_rand::LegacyRand, xoroshiro128::Xoroshiro},
    world_seed::Seed,
};

use super::{
    piece_golden::{Terrain, block_entities, fill_terrain, fingerprint, write_spawns_and_ticks},
    stage_parity::TestBlockRegistry,
};
use crate::generation::{
    get_world_gen,
    proto_chunk::ProtoChunk,
    structure::structures::{
        StructureGenerator, StructureGeneratorContext, mineshaft::MineshaftGenerator,
        nether_fortress::NetherFortressGenerator, stronghold::StrongholdGenerator,
    },
};

/// (mesa, seed, start chunk x, start chunk z) of each mineshaft in the golden.
const CASES: [(bool, u64, i32, i32); 6] = [
    (false, 1, 0, 0),
    (false, 2, 3, -2),
    (false, 3, -5, 4),
    (true, 1, 0, 0),
    (true, 2, 3, -2),
    (true, 3, -5, 4),
];

/// One structure started in chunk (`chunk_x`, `chunk_z`): a header line, a line per piece and a
/// line per chunk of the layout.
fn record_structure(
    out: &mut String,
    world_gen: &crate::generation::generator::WorldGenerator,
    (header, generator): (&str, &dyn StructureGenerator),
    (seed, chunk_x, chunk_z): (u64, i32, i32),
) -> Result<(), Box<dyn std::error::Error>> {
    let context = StructureGeneratorContext {
        seed: seed as i64,
        chunk_x,
        chunk_z,
        random: RandomGenerator::Legacy(LegacyRand::from_seed(seed)),
        sea_level: 63,
        min_y: -64,
        height: 384,
        height_sampler: None,
        structure_key: None,
    };
    let position = generator
        .get_structure_position(context)
        .ok_or("no structure")?;
    let mut collector = position
        .collector
        .lock()
        .map_err(|_| "collector lock poisoned")?;
    writeln!(
        out,
        "{header} seed={seed} chunk=({chunk_x},{chunk_z}) start={:?} pieces={}",
        position.start_pos.0,
        collector.pieces.len()
    )?;
    for piece in &collector.pieces {
        let piece = piece.get_structure_piece();
        let b = piece.bounding_box;
        writeln!(
            out,
            "  piece {:?} {:?} ({},{},{})..({},{},{})",
            piece.r#type, piece.facing, b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z
        )?;
    }
    let bounds = collector.get_bounding_box();
    for x in (bounds.min.x >> 4)..=(bounds.max.x >> 4) {
        for z in (bounds.min.z >> 4)..=(bounds.max.z >> 4) {
            let mut chunk = ProtoChunk::new(x, z, world_gen);
            fill_terrain(&mut chunk, Terrain::Flat);
            let chunk_seed = seed ^ ((x as u64) << 32) ^ (z as u32 as u64);
            let mut random = RandomGenerator::Xoroshiro(Xoroshiro::from_seed(chunk_seed));
            collector.generate_in_chunk(&mut chunk, &TestBlockRegistry, &mut random, seed as i64);
            let (solid, hash) = fingerprint(&chunk);
            let (entities, entity_hash) = block_entities(&chunk);
            write!(
                out,
                "  chunk=({x},{z}) solid={solid} hash={hash:016x} be={entities}:{entity_hash:016x}"
            )?;
            write_spawns_and_ticks(out, &mut chunk)?;
            out.push('\n');
        }
    }
    Ok(())
}

/// Compares `now` with `tests/golden/<name>.txt`, or rewrites it under `KVASIR_BLESS`.
fn check_golden(name: &str, now: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.txt"));
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, now);
    Ok(())
}

fn overworld() -> crate::generation::generator::WorldGenerator {
    *get_world_gen(
        Seed(1),
        pumpkin_data::dimension::Dimension::OVERWORLD,
        true,
        Vec::new(),
        "minecraft:plains".to_owned(),
    )
}

#[test]
fn mineshafts_lay_out_and_place_the_same_blocks() -> Result<(), Box<dyn std::error::Error>> {
    let world_gen = overworld();
    let mut now = String::new();
    for (is_mesa, seed, chunk_x, chunk_z) in CASES {
        let header = format!("mineshaft mesa={is_mesa}");
        let generator = MineshaftGenerator { is_mesa };
        record_structure(
            &mut now,
            &world_gen,
            (&header, &generator),
            (seed, chunk_x, chunk_z),
        )?;
    }
    check_golden("mineshafts", &now)
}

/// (seed, start chunk x, start chunk z) of the strongholds and fortresses in their goldens.
const STARTS: [(u64, i32, i32); 3] = [(1, 0, 0), (2, 3, -2), (3, -5, 4)];

#[test]
fn strongholds_lay_out_and_place_the_same_blocks() -> Result<(), Box<dyn std::error::Error>> {
    let world_gen = overworld();
    let mut now = String::new();
    for start in STARTS {
        record_structure(
            &mut now,
            &world_gen,
            ("stronghold", &StrongholdGenerator),
            start,
        )?;
    }
    check_golden("strongholds", &now)
}

#[test]
fn nether_fortresses_lay_out_and_place_the_same_blocks() -> Result<(), Box<dyn std::error::Error>> {
    let world_gen = overworld();
    let mut now = String::new();
    for start in STARTS {
        record_structure(
            &mut now,
            &world_gen,
            ("nether_fortress", &NetherFortressGenerator),
            start,
        )?;
    }
    check_golden("nether_fortresses", &now)
}
