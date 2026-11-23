//! A record of what `carve` does, made before it is split
//! (`tests/golden/carve_*.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Chunks of real `VanillaGenerator`s (overworld, nether and end, several seeds) are taken up
//! to the surface stage and carved. Each line has the dimension, seed, chunk, the count of
//! carved cells (cells whose block changed), a hash of those cells (the carving mask as far as
//! it shows in the blocks), the fluid ticks scheduled and a hash per section. Positions
//! include negative and far coordinates; the end has no biome with carvers.

use std::fmt::Write;

use pumpkin_data::dimension::Dimension;
use pumpkin_util::world_seed::Seed;

use super::carve;
use crate::generation::generator::WorldGenerator;
use crate::generation::get_world_gen;
use crate::generation::proto_chunk::ProtoChunk;
use crate::generation::proto_chunk_test::surface_biomes;

const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;
const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_5325;

const OVERWORLD_CHUNKS: [(i32, i32); 14] = [
    (5, -2),
    (-1, -1),
    (3, -5),
    (-12, 7),
    (20, -33),
    (-40, -41),
    (100, 100),
    (-7, 6),
    (15, 15),
    (-64, 3),
    (31, -17),
    (-3, 28),
    (60, -60),
    (-25, -9),
];

const NETHER_CHUNKS: [(i32, i32); 8] = [
    (5, -2),
    (-1, -1),
    (-12, 7),
    (20, -33),
    (-40, 41),
    (9, 9),
    (-6, -22),
    (33, 4),
];

const END_CHUNKS: [(i32, i32); 3] = [(5, -2), (-30, 11), (64, 64)];

fn fnv(hash: u64, value: u64) -> u64 {
    (hash ^ value).wrapping_mul(FNV_PRIME)
}

fn carved_chunk(world_gen: &WorldGenerator, x: i32, z: i32) -> (ProtoChunk, Vec<u16>) {
    let WorldGenerator::Noise(generator) = world_gen else {
        unreachable!()
    };
    let mut chunk = ProtoChunk::new(x, z, world_gen);
    chunk.step_to_biomes(generator);
    chunk.set_structure_starts(generator);
    chunk.set_structure_references(generator);
    chunk.step_to_noise(generator);
    chunk.step_to_surface(generator, &surface_biomes(world_gen, x, z));
    let before: Vec<u16> = chunk.flat_block_map.iter().map(|id| id.as_u16()).collect();
    carve(&mut chunk, generator);
    (chunk, before)
}

fn chunk_line(dimension: &str, seed: u64, x: i32, z: i32, world_gen: &WorldGenerator) -> String {
    let (chunk, before) = carved_chunk(world_gen, x, z);
    let mut carved = 0usize;
    let mut mask_hash = FNV_BASIS;
    let mut sections = Vec::new();
    let mut section_hash = FNV_BASIS;
    for (index, (old, new)) in before.iter().zip(chunk.flat_block_map.iter()).enumerate() {
        let new = new.as_u16();
        if *old != new {
            carved += 1;
            mask_hash = fnv(mask_hash, index as u64);
        }
        section_hash = fnv(section_hash, u64::from(new));
        if index % 4096 == 4095 {
            sections.push(format!("{section_hash:016x}"));
            section_hash = FNV_BASIS;
        }
    }
    format!(
        "{dimension} seed={seed} chunk=({x},{z}) carved={carved} mask={mask_hash:016x} ticks={} sections={}\n",
        chunk.fluid_ticks.len(),
        sections.join(",")
    )
}

fn dimension_lines(
    report: &mut String,
    name: &str,
    dimension: &Dimension,
    seeds: &[u64],
    chunks: &[(i32, i32)],
) {
    for &seed in seeds {
        let world_gen = get_world_gen(
            Seed(seed),
            dimension.clone(),
            false,
            Vec::new(),
            String::new(),
        );
        for &(x, z) in chunks {
            report.push_str(&chunk_line(name, seed, x, z, &world_gen));
        }
    }
}

fn check(name: &str, report: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("carve_{name}.txt"));
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}

fn record(
    name: &str,
    dimension: &Dimension,
    seeds: &[u64],
    chunks: &[(i32, i32)],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut report = String::new();
    dimension_lines(&mut report, name, dimension, seeds, chunks);
    let _ = writeln!(report, "lines {}", report.lines().count());
    check(name, &report)
}

#[test]
fn overworld_seed_1() -> Result<(), Box<dyn std::error::Error>> {
    record(
        "overworld_1",
        &Dimension::OVERWORLD,
        &[1],
        &OVERWORLD_CHUNKS,
    )
}

#[test]
fn overworld_seed_42() -> Result<(), Box<dyn std::error::Error>> {
    record(
        "overworld_42",
        &Dimension::OVERWORLD,
        &[42],
        &OVERWORLD_CHUNKS,
    )
}

#[test]
fn nether() -> Result<(), Box<dyn std::error::Error>> {
    record("nether", &Dimension::THE_NETHER, &[7, 1234], &NETHER_CHUNKS)
}

#[test]
fn end() -> Result<(), Box<dyn std::error::Error>> {
    record("end", &Dimension::THE_END, &[3], &END_CHUNKS)
}
