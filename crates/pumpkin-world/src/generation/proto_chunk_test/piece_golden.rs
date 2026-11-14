//! Golden records of what the `place` of a structure piece writes into chunks.
//!
//! `place` is a long, deterministic function of a piece, a chunk, a random generator and a seed, so
//! the blocks it writes are a fingerprint of its behaviour. A piece is placed into every chunk its
//! bounding box touches, on three kinds of terrain, facing the four directions and with two seeds;
//! each chunk of each case becomes one line (its number of non-air blocks and a hash of all of
//! them). The file is compared by [`check`]; rewrite it on purpose with `KVASIR_BLESS=1`.

use std::fmt::Write;

use pumpkin_data::Block;
use pumpkin_util::{
    BlockDirection,
    math::block_box::BlockBox,
    random::{RandomGenerator, xoroshiro128::Xoroshiro},
    world_seed::Seed,
};

use super::{feature_golden::compound_text, stage_parity::TestBlockRegistry};
use crate::generation::{
    get_world_gen,
    positions::chunk_pos::start_block_x,
    positions::chunk_pos::start_block_z,
    proto_chunk::ProtoChunk,
    structure::structures::{StructurePieceBase, stronghold::EntranceType},
};

const FACINGS: [BlockDirection; 4] = [
    BlockDirection::North,
    BlockDirection::South,
    BlockDirection::East,
    BlockDirection::West,
];

/// The ground the piece is placed on.
#[derive(Clone, Copy, Debug)]
enum Terrain {
    /// Stone up to y = 62 everywhere.
    Flat,
    /// Stone whose top varies between y = 58 and y = 66 from column to column.
    Hilly,
    /// Nothing at all.
    Empty,
}

impl Terrain {
    const ALL: [Self; 3] = [Self::Flat, Self::Hilly, Self::Empty];

    fn top(self, x: i32, z: i32) -> Option<i32> {
        match self {
            Self::Flat => Some(62),
            Self::Hilly => Some(58 + (x * 7 + z * 13).rem_euclid(9)),
            Self::Empty => None,
        }
    }
}

fn fill_terrain(chunk: &mut ProtoChunk, terrain: Terrain) {
    let stone = Block::STONE.default_state;
    for local_x in 0..16 {
        for local_z in 0..16 {
            let x = start_block_x(chunk.x) + local_x;
            let z = start_block_z(chunk.z) + local_z;
            if let Some(top) = terrain.top(x, z) {
                for y in i32::from(chunk.bottom_y())..=top {
                    chunk.set_block_state(x, y, z, stone);
                }
            }
        }
    }
}

/// FNV-1a over the state ids of every block of the chunk, with the number of blocks that are not
/// air.
fn fingerprint(chunk: &ProtoChunk) -> (usize, u64) {
    let air = Block::AIR.default_state.id;
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut solid = 0;
    for y in 0..i32::from(chunk.height()) {
        for x in 0..16 {
            for z in 0..16 {
                let id = chunk.get_block_state_raw(x, y, z);
                solid += usize::from(id != air);
                for byte in id.as_u16().to_le_bytes() {
                    hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
                }
            }
        }
    }
    (solid, hash)
}

/// The number of block entities the chunk holds and an FNV-1a hash of their NBT, keys sorted.
fn block_entities(chunk: &ProtoChunk) -> (usize, u64) {
    let mut text = String::new();
    for nbt in &chunk.pending_block_entities {
        compound_text(nbt, &mut text);
    }
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    (chunk.pending_block_entities.len(), hash)
}

/// The chunk coordinates the bounding box of `piece` touches.
fn touched_chunks(piece: &dyn StructurePieceBase) -> Vec<(i32, i32)> {
    let bounds = piece.get_structure_piece().bounding_box;
    let mut chunks = Vec::new();
    for chunk_x in (bounds.min.x >> 4)..=(bounds.max.x >> 4) {
        for chunk_z in (bounds.min.z >> 4)..=(bounds.max.z >> 4) {
            chunks.push((chunk_x, chunk_z));
        }
    }
    chunks
}

/// One line per chunk the piece touches, after placing the piece into each in turn (the piece
/// keeps its state between chunks, as it does in the game).
fn place_everywhere(
    label: &str,
    mut piece: Box<dyn StructurePieceBase>,
    terrain: Terrain,
    seed: i64,
) -> Result<String, std::fmt::Error> {
    let world_gen = get_world_gen(
        Seed(1),
        pumpkin_data::dimension::Dimension::OVERWORLD,
        true,
        Vec::new(),
        "minecraft:plains".to_owned(),
    );
    let registry = TestBlockRegistry;
    let mut out = String::new();
    let mut random = RandomGenerator::Xoroshiro(Xoroshiro::from_seed(seed as u64));
    for (chunk_x, chunk_z) in touched_chunks(piece.as_ref()) {
        let mut chunk = ProtoChunk::new(chunk_x, chunk_z, &world_gen);
        fill_terrain(&mut chunk, terrain);
        let chunk_box = BlockBox::new(
            start_block_x(chunk_x),
            i32::from(chunk.bottom_y()),
            start_block_z(chunk_z),
            start_block_x(chunk_x) + 15,
            i32::from(chunk.bottom_y()) + i32::from(chunk.height()) - 1,
            start_block_z(chunk_z) + 15,
        );
        piece.place(&mut chunk, &registry, &mut random, seed, &chunk_box);
        let (solid, hash) = fingerprint(&chunk);
        let (entities, entity_hash) = block_entities(&chunk);
        let bounds = piece.get_structure_piece().bounding_box;
        writeln!(
            out,
            "{label} chunk=({chunk_x},{chunk_z}) solid={solid} hash={hash:016x} box=({},{},{})..({},{},{}) be={entities}:{entity_hash:016x}",
            bounds.min.x, bounds.min.y, bounds.min.z, bounds.max.x, bounds.max.y, bounds.max.z
        )?;
    }
    Ok(out)
}

/// A different stronghold entrance for each facing, so every entrance shape is placed.
pub(crate) const fn stronghold_door(facing: BlockDirection) -> EntranceType {
    match facing {
        BlockDirection::North => EntranceType::Opening,
        BlockDirection::South => EntranceType::WoodDoor,
        BlockDirection::East => EntranceType::Grates,
        _ => EntranceType::IronDoor,
    }
}

/// Places the pieces `make` builds for every facing, terrain and seed, and compares the result with
/// `tests/golden/<name>.txt` of the crate.
pub(crate) fn check(
    name: &str,
    make: impl Fn(BlockDirection) -> Box<dyn StructurePieceBase>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut now = String::new();
    for facing in FACINGS {
        for terrain in Terrain::ALL {
            for seed in [1i64, 987_654_321] {
                let label = format!("{facing:?}/{terrain:?}/{seed}");
                now.push_str(&place_everywhere(&label, make(facing), terrain, seed)?);
            }
        }
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.txt"));
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, now);
    Ok(())
}
