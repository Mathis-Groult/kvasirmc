//! A record of what `ProtoChunk::from_chunk_data` does, made before it is split
//! (`tests/golden/from_chunk_data.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Chunk data is built by hand (full, empty and mixed block sections, fewer biome sections than
//! block sections, sections beyond the dimension height, a section floor that differs from the
//! dimension's, heightmaps that are absent, partial or different per type, light, blending data,
//! block entities and ticks) and resumed under a Noise, a Flat and a Custom generator. Two real
//! generated chunks are resumed at every chunk status. Each line has the stage, the structures
//! rebuilt and a hash of every part of the resumed proto chunk.

use std::fmt::Write;
use std::sync::RwLock;

use pumpkin_data::{Block, chunk::Biome, chunk::ChunkStatus, dimension::Dimension};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_util::{math::position::BlockPos, world_seed::Seed};

use super::feature_golden::fnv;
use crate::chunk::format::LightContainer;
use crate::chunk::palette::{BiomePalette, BlockPalette};
use crate::chunk::{ChunkData, ChunkHeightmapType, ChunkLight, ChunkSections};
use crate::generation::blender::blending_data::BlendingData;
use crate::generation::generator::{CustomChunkGenerator, WorldGenerator};
use crate::generation::{get_world_gen, proto_chunk::ProtoChunk};
use crate::tick::{ScheduledTick, TickPriority};

type Res = Result<(), Box<dyn std::error::Error>>;

struct Custom(Dimension);

impl CustomChunkGenerator for Custom {
    fn dimension(&self) -> &Dimension {
        &self.0
    }

    fn seed(&self) -> u64 {
        5
    }
}

const STATUSES: [ChunkStatus; 10] = [
    ChunkStatus::Empty,
    ChunkStatus::StructureStarts,
    ChunkStatus::StructureReferences,
    ChunkStatus::Biomes,
    ChunkStatus::Terrain,
    ChunkStatus::Features,
    ChunkStatus::InitializeLight,
    ChunkStatus::Light,
    ChunkStatus::Spawn,
    ChunkStatus::Full,
];

fn blocks_of(section: usize, kind: u8) -> BlockPalette {
    let palette = [
        Block::AIR,
        Block::STONE,
        Block::DIRT,
        Block::GRASS_BLOCK,
        Block::WATER,
        Block::OAK_LEAVES,
        Block::GLASS,
        Block::OAK_LOG,
    ];
    match kind {
        0 => BlockPalette::Homogeneous(Block::AIR.default_state.id),
        1 => BlockPalette::Homogeneous(Block::STONE.default_state.id),
        _ => {
            let mut blocks = BlockPalette::Homogeneous(Block::AIR.default_state.id);
            for x in 0..16 {
                for y in 0..16 {
                    for z in 0..16 {
                        let pick = (x * 3 + y * 5 + z * 7 + section * 11 + x * z) % palette.len();
                        blocks.set(x, y, z, palette[pick].default_state.id);
                    }
                }
            }
            blocks
        }
    }
}

fn biomes_of(section: usize, mixed: bool) -> BiomePalette {
    let ids = [
        Biome::PLAINS.id,
        Biome::DESERT.id,
        Biome::FOREST.id,
        Biome::OCEAN.id,
        Biome::TAIGA.id,
    ];
    if !mixed {
        return BiomePalette::Homogeneous(ids[section % ids.len()]);
    }
    let mut biomes = BiomePalette::Homogeneous(ids[0]);
    for x in 0..4 {
        for y in 0..4 {
            for z in 0..4 {
                biomes.set(x, y, z, ids[(x * 2 + y * 3 + z + section) % ids.len()]);
            }
        }
    }
    biomes
}

/// Sections of the given kinds (0 air, 1 stone, 2 mixed); `biome_count` biome sections.
fn sections(kinds: &[u8], biome_count: usize, min_y: i32) -> ChunkSections {
    let blocks: Box<[BlockPalette]> = kinds
        .iter()
        .enumerate()
        .map(|(i, kind)| blocks_of(i, *kind))
        .collect();
    let biomes: Box<[BiomePalette]> = (0..biome_count).map(|i| biomes_of(i, i % 2 == 0)).collect();
    ChunkSections {
        count: kinds.len(),
        block_sections: RwLock::new(blocks),
        random_tick_sections: RwLock::new(None),
        randomly_ticking_mask: std::sync::atomic::AtomicU32::new(0),
        biome_sections: RwLock::new(biomes),
        min_y,
    }
}

/// 24 sections: air, stone, mixed (and again) in a fixed pattern.
fn kinds_24() -> Vec<u8> {
    (0..24).map(|i| [0, 1, 2, 2, 0, 1][i % 6]).collect()
}

fn fill_heightmaps(chunk: &ChunkData, which: [bool; 3]) {
    let min_y = chunk.section.min_y;
    let mut maps = chunk
        .heightmap
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for x in 0..16 {
        for z in 0..16 {
            if which[0] {
                maps.set(
                    ChunkHeightmapType::WorldSurface,
                    x,
                    z,
                    60 + x + 2 * z,
                    min_y,
                );
            }
            if which[1] {
                maps.set(
                    ChunkHeightmapType::MotionBlocking,
                    x,
                    z,
                    40 + 3 * x + z,
                    min_y,
                );
            }
            if which[2] {
                maps.set(
                    ChunkHeightmapType::MotionBlockingNoLeaves,
                    x,
                    z,
                    20 + x * z % 11,
                    min_y,
                );
            }
        }
    }
}

/// Heightmaps whose three types all differ between a column and its mirror (x and z swapped).
fn fill_skewed_heightmaps(chunk: &ChunkData) {
    let min_y = chunk.section.min_y;
    let mut maps = chunk
        .heightmap
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for x in 0..16 {
        for z in 0..16 {
            maps.set(
                ChunkHeightmapType::WorldSurface,
                x,
                z,
                100 + (x * 5 + z) % 13,
                min_y,
            );
            maps.set(
                ChunkHeightmapType::MotionBlocking,
                x,
                z,
                90 + (x * 3 + z * 7) % 17,
                min_y,
            );
            maps.set(
                ChunkHeightmapType::MotionBlockingNoLeaves,
                x,
                z,
                80 + (x * 5 + z) % 13,
                min_y,
            );
        }
    }
}

fn light_of(count: usize, seed: usize) -> Box<[LightContainer]> {
    (0..count)
        .map(|i| match (i + seed) % 3 {
            0 => LightContainer::Empty(((i + seed) % 16) as u8),
            1 => LightContainer::Full(
                (0..LightContainer::ARRAY_SIZE)
                    .map(|j| (j * 7 + i * 13 + seed) as u8)
                    .collect(),
            ),
            _ => LightContainer::Empty(15),
        })
        .collect()
}

fn entity(id: &str, x: i32, y: i32, z: i32) -> NbtCompound {
    let mut nbt = NbtCompound::new();
    nbt.put_string("id", id.to_owned());
    nbt.put_int("x", x);
    nbt.put_int("y", y);
    nbt.put_int("z", z);
    nbt
}

fn add_extras(chunk: &mut ChunkData) {
    let base = BlockPos::new(chunk.x * 16, 64, chunk.z * 16);
    chunk.block_ticks.schedule_tick(
        &ScheduledTick {
            delay: 3,
            priority: TickPriority::Normal,
            position: BlockPos::new(base.0.x + 1, 64, base.0.z + 2),
            value: &Block::STONE,
        },
        0,
    );
    chunk.fluid_ticks.schedule_tick(
        &ScheduledTick {
            delay: 5,
            priority: TickPriority::Low,
            position: BlockPos::new(base.0.x + 7, 62, base.0.z + 1),
            value: &pumpkin_data::fluid::Fluid::WATER,
        },
        1,
    );
    let mut entities = chunk
        .pending_block_entities
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (id, dx, y, dz) in [
        ("minecraft:chest", 2, 65, 3),
        ("minecraft:furnace", 9, 70, 12),
        ("minecraft:barrel", 15, 20, 0),
    ] {
        let (x, z) = (base.0.x + dx, base.0.z + dz);
        entities.insert(BlockPos::new(x, y, z), entity(id, x, y, z));
    }
    drop(entities);
    chunk.blending_data = Some(BlendingData {
        min_y: -64,
        max_y: 320,
        heights: vec![63.5, 70.25, 80.0, 12.125, -3.0],
        densities: vec![0.5, -0.25, 1.75],
        biomes: vec![&Biome::PLAINS, &Biome::DESERT, &Biome::OCEAN],
    });
    let mut light = chunk
        .light_engine
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *light = ChunkLight {
        sky_light: light_of(chunk.section.count, 0),
        block_light: light_of(chunk.section.count, 1),
    };
}

fn chunk_with(x: i32, z: i32, section: ChunkSections) -> ChunkData {
    let mut chunk = ChunkData::empty(x, z);
    chunk.section = section;
    chunk
}

fn hand_chunks() -> Vec<(&'static str, ChunkData)> {
    let mut mixed = chunk_with(5, -7, sections(&kinds_24(), 24, -64));
    fill_heightmaps(&mixed, [true, true, true]);
    add_extras(&mut mixed);

    let mut few_biomes = chunk_with(-3, 11, sections(&kinds_24(), 9, -64));
    fill_heightmaps(&few_biomes, [true, true, true]);
    few_biomes.status = ChunkStatus::Features;

    let empty = chunk_with(8, 2, sections(&[0; 24], 24, -64));

    let full = chunk_with(-9, -4, sections(&[1; 24], 24, -64));
    fill_heightmaps(&full, [true, false, false]);

    let partial = chunk_with(13, 6, sections(&kinds_24(), 24, -64));
    fill_heightmaps(&partial, [false, true, false]);

    let shifted = chunk_with(2, 9, sections(&kinds_24(), 24, -32));
    fill_heightmaps(&shifted, [true, true, true]);

    vec![
        ("mixed", mixed),
        ("few_biomes", few_biomes),
        ("empty", empty),
        ("full", full),
        ("partial_heightmaps", partial),
        ("shifted_min_y", shifted),
    ]
}

fn nether_chunk() -> ChunkData {
    let chunk = chunk_with(-6, 3, sections(&kinds_24(), 24, 0));
    fill_heightmaps(&chunk, [true, true, true]);
    chunk
}

fn hash_u16s(values: impl Iterator<Item = u16>) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for value in values {
        hash = fnv(hash, &value.to_le_bytes());
    }
    hash
}

fn light_hash(containers: &[LightContainer]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for container in containers {
        match container {
            LightContainer::Empty(v) => hash = fnv(fnv(hash, &[0]), &[*v]),
            LightContainer::Full(data) => hash = fnv(fnv(hash, &[1]), data),
        }
    }
    hash
}

fn blending_line(proto: &ProtoChunk) -> String {
    proto.blending_data.as_ref().map_or_else(
        || "none".to_owned(),
        |data| {
            format!(
                "{}..{} h{:?} d{:?} b{:?}",
                data.min_y,
                data.max_y,
                data.heights,
                data.densities,
                data.biomes.iter().map(|b| b.id).collect::<Vec<_>>()
            )
        },
    )
}

fn entity_line(proto: &ProtoChunk) -> String {
    let mut lines: Vec<String> = proto
        .pending_block_entities
        .iter()
        .map(|nbt| {
            format!(
                "{}@{},{},{}",
                nbt.get_string("id").unwrap_or("?"),
                nbt.get_int("x").unwrap_or(0),
                nbt.get_int("y").unwrap_or(0),
                nbt.get_int("z").unwrap_or(0)
            )
        })
        .collect();
    lines.sort();
    lines.join(" ")
}

fn structure_line(proto: &ProtoChunk) -> String {
    let mut starts: Vec<String> = proto
        .structure_starts
        .keys()
        .map(|key| format!("{key:?}"))
        .collect();
    starts.sort();
    let mut references: Vec<String> = proto
        .structure_references
        .iter()
        .map(|(key, _)| format!("{key:?}"))
        .collect();
    references.sort();
    format!("starts {starts:?} refs {references:?}")
}

fn dump(report: &mut String, label: &str, proto: &ProtoChunk) -> Res {
    writeln!(
        report,
        "{label}: stage {:?} chunk {},{} h{} b{}",
        proto.stage,
        proto.x,
        proto.z,
        proto.height(),
        proto.bottom_y()
    )?;
    writeln!(report, "  {}", structure_line(proto))?;
    writeln!(
        report,
        "  blocks {} {:016x} biomes {:016x}",
        proto.flat_block_map.len(),
        hash_u16s(proto.flat_block_map.iter().map(|id| id.as_u16())),
        hash_u16s(proto.flat_biome_map.iter().map(|id| u16::from(*id)))
    )?;
    for (name, map) in [
        ("surface", &proto.flat_surface_height_map),
        ("ocean_floor", &proto.flat_ocean_floor_height_map),
        ("surface_wg", &proto.flat_surface_wg_height_map),
        ("ocean_floor_wg", &proto.flat_ocean_floor_wg_height_map),
        ("motion_blocking", &proto.flat_motion_blocking_height_map),
        (
            "motion_blocking_no_leaves",
            &proto.flat_motion_blocking_no_leaves_height_map,
        ),
    ] {
        writeln!(
            report,
            "  {name} {:016x} first {:?}",
            hash_u16s(map.iter().map(|h| *h as u16)),
            &map[..4]
        )?;
    }
    writeln!(
        report,
        "  light {} {:016x} / {} {:016x}",
        proto.light.sky_light.len(),
        light_hash(&proto.light.sky_light),
        proto.light.block_light.len(),
        light_hash(&proto.light.block_light)
    )?;
    writeln!(
        report,
        "  blending {} | entities {} | fluid_ticks {}",
        blending_line(proto),
        entity_line(proto),
        proto.fluid_ticks.len()
    )?;
    Ok(())
}

fn resume(report: &mut String, label: &str, chunk: &ChunkData, generator: &WorldGenerator) -> Res {
    dump(
        report,
        label,
        &ProtoChunk::from_chunk_data(chunk, generator),
    )
}

fn hand_scenes(report: &mut String) -> Res {
    let noise = get_world_gen(
        Seed(1_782_124_772_053_846_960),
        Dimension::OVERWORLD,
        false,
        Vec::new(),
        String::new(),
    );
    for (label, chunk) in hand_chunks() {
        resume(report, &format!("noise {label}"), &chunk, &noise)?;
    }
    let nether = get_world_gen(
        Seed(7),
        Dimension::THE_NETHER,
        false,
        Vec::new(),
        String::new(),
    );
    resume(report, "nether tall data", &nether_chunk(), &nether)
}

fn other_generators(report: &mut String) -> Res {
    let flat = get_world_gen(
        Seed(3),
        Dimension::OVERWORLD,
        true,
        Vec::new(),
        "minecraft:plains".to_owned(),
    );
    let custom = WorldGenerator::Custom(std::sync::Arc::new(Custom(Dimension::OVERWORLD)));
    for status in [
        ChunkStatus::StructureStarts,
        ChunkStatus::StructureReferences,
        ChunkStatus::Terrain,
        ChunkStatus::Full,
    ] {
        for (name, generator) in [("flat", &*flat), ("custom", &custom)] {
            let mut mixed = chunk_with(5, -7, sections(&kinds_24(), 24, -64));
            fill_heightmaps(&mixed, [true, true, true]);
            mixed.status = status;
            resume(
                report,
                &format!("{name} mixed {status:?}"),
                &mixed,
                generator,
            )?;
        }
    }
    Ok(())
}

/// A chunk generated up to the noise stage and turned into chunk data, as the world saves it.
fn real_chunk(
    generator: &WorldGenerator,
    world_gen: &WorldGenerator,
    x: i32,
    z: i32,
) -> Result<ChunkData, &'static str> {
    use crate::chunk_system::chunk_state::Chunk;
    use pumpkin_config::lighting::LightingEngineConfig;

    let WorldGenerator::Noise(noise) = generator else {
        return Err("not a noise generator");
    };
    let mut proto = ProtoChunk::new(x, z, world_gen);
    proto.step_to_biomes(noise);
    proto.set_structure_starts(noise);
    proto.set_structure_references(noise);
    proto.step_to_noise(noise);
    let mut staged = Chunk::Proto(Box::new(proto));
    staged.upgrade_to_level_chunk(&Dimension::OVERWORLD, &LightingEngineConfig::Default);
    let Chunk::Level(data) = staged else {
        return Err("not a level chunk");
    };
    std::sync::Arc::try_unwrap(data).map_err(|_| "chunk data is shared")
}

fn real_scenes(report: &mut String) -> Res {
    let world_gen = get_world_gen(
        Seed(1_782_124_772_053_846_960),
        Dimension::OVERWORLD,
        false,
        Vec::new(),
        String::new(),
    );
    // A monument starts at (-553, 173) and reaches into (-553, 174).
    for (x, z) in [(-553, 173), (-553, 174)] {
        let mut chunk = real_chunk(&world_gen, &world_gen, x, z)?;
        for status in STATUSES {
            chunk.status = status;
            resume(
                report,
                &format!("real {x},{z} {status:?}"),
                &chunk,
                &world_gen,
            )?;
        }
    }
    Ok(())
}

/// Appended in the widening round: heightmaps that are not symmetric in x and z.
fn skewed_scenes(report: &mut String) -> Res {
    let noise = get_world_gen(
        Seed(11),
        Dimension::OVERWORLD,
        false,
        Vec::new(),
        String::new(),
    );
    let chunk = chunk_with(-14, 21, sections(&kinds_24(), 24, -64));
    fill_skewed_heightmaps(&chunk);
    resume(report, "noise skewed heightmaps", &chunk, &noise)
}

#[test]
fn from_chunk_data() -> Res {
    let mut report = String::new();
    hand_scenes(&mut report)?;
    other_generators(&mut report)?;
    real_scenes(&mut report)?;
    skewed_scenes(&mut report)?;

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("from_chunk_data.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
