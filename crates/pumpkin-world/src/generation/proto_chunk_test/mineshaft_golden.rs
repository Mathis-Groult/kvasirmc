//! Golden record of whole mineshafts: their piece layout and the blocks they place.
//!
//! `MineshaftGenerator` lays the pieces out (running the room, corridor, crossing and stairs
//! `add_*_children` recursions); the pieces are then placed chunk by chunk into stone terrain
//! through `StructurePiecesCollector::generate_in_chunk`, as the game does. Rewrite the file on
//! purpose with `KVASIR_BLESS=1`.

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

fn record_mineshaft(
    out: &mut String,
    world_gen: &crate::generation::generator::WorldGenerator,
    (is_mesa, seed, chunk_x, chunk_z): (bool, u64, i32, i32),
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
    let position = MineshaftGenerator { is_mesa }
        .get_structure_position(context)
        .ok_or("no mineshaft")?;
    let mut collector = position
        .collector
        .lock()
        .map_err(|_| "collector lock poisoned")?;
    writeln!(
        out,
        "mineshaft mesa={is_mesa} seed={seed} chunk=({chunk_x},{chunk_z}) start={:?} pieces={}",
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

#[test]
fn mineshafts_lay_out_and_place_the_same_blocks() -> Result<(), Box<dyn std::error::Error>> {
    let world_gen = get_world_gen(
        Seed(1),
        pumpkin_data::dimension::Dimension::OVERWORLD,
        true,
        Vec::new(),
        "minecraft:plains".to_owned(),
    );
    let mut now = String::new();
    for case in CASES {
        record_mineshaft(&mut now, &world_gen, case)?;
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/mineshafts.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, now);
    Ok(())
}
