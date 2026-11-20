//! A record of what `SkyLightPropagator::convert_light` does, made before it is split
//! (`tests/golden/sky_light.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene builds a three by three square of chunks, some of them proto chunks and some
//! finished ones, puts blocks in it, spreads the sky light, and writes down the light of every
//! section and the light along a few columns.

use std::fmt::Write;
use std::sync::Arc;

use pumpkin_data::Block;
use pumpkin_util::world_seed::Seed;

use super::engine::SkyLightPropagator;
use crate::chunk::ChunkData;
use crate::chunk::format::LightContainer;
use crate::chunk_system::chunk_state::Chunk;
use crate::chunk_system::generation_cache::Cache;
use crate::generation::generator::WorldGenerator;
use crate::generation::get_world_gen;
use crate::generation::proto_chunk::ProtoChunk;

const MIN_Y: i32 = -64;
const SECTIONS: usize = 24;

/// The chunk in the middle of the square, away from the origin so that no coordinate is zero.
const ORIGIN: (i32, i32) = (5, -2);

type Place = (i32, i32, i32, &'static Block);

/// Which of the nine chunks, in cache order, are finished chunks. The centre one never is.
#[derive(Clone, Copy)]
struct Kinds([bool; 9]);

const PROTO: Kinds = Kinds([false; 9]);
const RING: Kinds = Kinds([true, true, true, true, false, true, true, true, true]);
const MIXED: Kinds = Kinds([true, false, true, false, false, true, false, true, false]);

struct Scene {
    name: &'static str,
    kinds: Kinds,
    blocks: fn() -> Vec<Place>,
}

fn floor(top: i32, block: &'static Block) -> Vec<Place> {
    let mut out = Vec::new();
    for x in -16..32 {
        for z in -16..32 {
            for y in MIN_Y..=top {
                out.push((x, y, z, block));
            }
        }
    }
    out
}

/// A box of `block` from `from` to `to`, both ends included.
fn cuboid(from: (i32, i32, i32), to: (i32, i32, i32), block: &'static Block) -> Vec<Place> {
    let mut out = Vec::new();
    for x in from.0..=to.0 {
        for y in from.1..=to.1 {
            for z in from.2..=to.2 {
                out.push((x, y, z, block));
            }
        }
    }
    out
}

/// Walls of different heights in the rows and columns just outside, and just inside, the centre.
fn ridges() -> Vec<Place> {
    let mut out = floor(30, &Block::STONE);
    out.extend(cuboid((-1, 31, -16), (-1, 60, 31), &Block::STONE));
    out.extend(cuboid((16, 31, -16), (16, 45, 31), &Block::STONE));
    out.extend(cuboid((-16, 31, -1), (31, 52, -1), &Block::STONE));
    out.extend(cuboid((-16, 31, 16), (31, 38, 16), &Block::STONE));
    out.extend(cuboid((0, 31, 0), (0, 70, 0), &Block::STONE));
    out.extend(cuboid((15, 31, 15), (15, 66, 15), &Block::STONE));
    out
}

/// A sea of the given depth over stone, as wide as the cache.
fn wide_water(depth: i32) -> Vec<Place> {
    let mut out = floor(40, &Block::STONE);
    out.extend(cuboid((-16, 41, -16), (31, 40 + depth, 31), &Block::WATER));
    out
}

/// Open ground in the centre chunk and a roof over everything around it.
fn overhang() -> Vec<Place> {
    let mut out = floor(30, &Block::STONE);
    for (from, to) in [
        ((-16, 40, -16), (-1, 42, 31)),
        ((16, 40, -16), (31, 42, 31)),
        ((0, 40, -16), (15, 42, -1)),
        ((0, 40, 16), (15, 42, 31)),
    ] {
        out.extend(cuboid(from, to, &Block::STONE));
    }
    out
}

fn scenes() -> Vec<Scene> {
    vec![
        Scene {
            name: "empty_proto",
            kinds: PROTO,
            blocks: Vec::new,
        },
        Scene {
            name: "empty_ring",
            kinds: RING,
            blocks: Vec::new,
        },
        Scene {
            name: "flat_proto",
            kinds: PROTO,
            blocks: || floor(63, &Block::STONE),
        },
        Scene {
            name: "flat_ring",
            kinds: RING,
            blocks: || floor(63, &Block::STONE),
        },
        Scene {
            name: "flat_mixed",
            kinds: MIXED,
            blocks: || floor(40, &Block::STONE),
        },
        Scene {
            name: "water_over_stone",
            kinds: MIXED,
            blocks: || {
                let mut out = floor(40, &Block::STONE);
                out.extend(cuboid((-4, 41, -4), (12, 55, 12), &Block::WATER));
                out
            },
        },
        Scene {
            name: "glass_and_leaves",
            kinds: PROTO,
            blocks: || {
                let mut out = floor(40, &Block::STONE);
                out.extend(cuboid((0, 41, 0), (5, 45, 5), &Block::GLASS));
                out.extend(cuboid((8, 41, 0), (13, 60, 5), &Block::OAK_LEAVES));
                out
            },
        },
    ]
}

fn cave_scenes() -> Vec<Scene> {
    vec![
        Scene {
            name: "wide_water_fourteen_deep",
            kinds: PROTO,
            blocks: || wide_water(14),
        },
        Scene {
            name: "wide_water_fifteen_deep_ring",
            kinds: RING,
            blocks: || wide_water(15),
        },
        Scene {
            name: "wide_water_thirteen_deep",
            kinds: MIXED,
            blocks: || wide_water(13),
        },
        Scene {
            name: "overhang_all_around",
            kinds: PROTO,
            blocks: overhang,
        },
        Scene {
            name: "overhang_all_around_mixed",
            kinds: MIXED,
            blocks: overhang,
        },
        Scene {
            name: "water_fourteen_deep",
            kinds: PROTO,
            blocks: || {
                let mut out = floor(40, &Block::STONE);
                out.extend(cuboid((0, 41, 0), (7, 54, 7), &Block::WATER));
                out
            },
        },
        Scene {
            name: "water_fourteen_deep_ring",
            kinds: RING,
            blocks: || {
                let mut out = floor(40, &Block::STONE);
                out.extend(cuboid((0, 41, 0), (7, 54, 7), &Block::WATER));
                out
            },
        },
        Scene {
            name: "ridges_on_the_edge_of_the_window",
            kinds: PROTO,
            blocks: ridges,
        },
        Scene {
            name: "ridges_on_the_edge_of_the_window_mixed",
            kinds: MIXED,
            blocks: ridges,
        },
        Scene {
            name: "roof_over_cave",
            kinds: MIXED,
            blocks: || {
                let mut out = floor(30, &Block::STONE);
                out.extend(cuboid((-3, 50, -3), (19, 52, 19), &Block::STONE));
                out
            },
        },
        Scene {
            name: "roof_over_cave_ring",
            kinds: RING,
            blocks: || {
                let mut out = floor(30, &Block::STONE);
                out.extend(cuboid((-3, 50, -3), (19, 52, 19), &Block::STONE));
                out
            },
        },
        Scene {
            name: "pit_and_tower",
            kinds: PROTO,
            blocks: || {
                let mut out = floor(50, &Block::STONE);
                out.retain(|&(x, y, z, _)| {
                    !((4..=8).contains(&x) && (4..=8).contains(&z) && y > 20)
                });
                out.extend(cuboid((12, 51, 12), (13, 90, 13), &Block::STONE));
                out
            },
        },
        Scene {
            name: "pit_and_tower_mixed",
            kinds: MIXED,
            blocks: || {
                let mut out = floor(50, &Block::STONE);
                out.retain(|&(x, y, z, _)| {
                    !((4..=8).contains(&x) && (4..=8).contains(&z) && y > 20)
                });
                out.extend(cuboid((12, 51, 12), (13, 90, 13), &Block::STONE));
                out
            },
        },
        Scene {
            name: "cliff_on_chunk_border",
            kinds: MIXED,
            blocks: || {
                let mut out = floor(30, &Block::STONE);
                out.extend(cuboid((16, 31, -16), (31, 70, 31), &Block::STONE));
                out.extend(cuboid((-16, 31, -16), (-1, 45, 31), &Block::STONE));
                out
            },
        },
        Scene {
            name: "sealed_room_deep",
            kinds: PROTO,
            blocks: || {
                let mut out = floor(100, &Block::STONE);
                out.retain(|&(x, y, z, _)| {
                    !((2..=12).contains(&x) && (2..=12).contains(&z) && (60..80).contains(&y))
                });
                out
            },
        },
        Scene {
            name: "world_top",
            kinds: PROTO,
            blocks: || cuboid((0, 290, 0), (3, 316, 3), &Block::STONE),
        },
    ]
}

fn all_scenes() -> Vec<Scene> {
    let mut all = scenes();
    all.extend(cave_scenes());
    all
}

fn world_gen() -> Arc<WorldGenerator> {
    Arc::from(get_world_gen(
        Seed(1),
        pumpkin_data::dimension::Dimension::OVERWORLD,
        true,
        Vec::new(),
        "minecraft:plains".to_owned(),
    ))
}

fn finished_chunk(x: i32, z: i32) -> Chunk {
    let chunk = ChunkData::empty(x, z);

    let mut light = chunk
        .light_engine
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    light.sky_light = (0..SECTIONS)
        .map(|_| LightContainer::new_empty(0))
        .collect();
    light.block_light = (0..SECTIONS)
        .map(|_| LightContainer::new_empty(0))
        .collect();
    drop(light);
    Chunk::Level(Arc::new(chunk))
}

fn build(scene: &Scene, world_gen: &WorldGenerator) -> Cache {
    let mut cache = Cache::new(ORIGIN.0 - 1, ORIGIN.1 - 1, 3);
    for dx in 0..3 {
        for dz in 0..3 {
            let (x, z) = (ORIGIN.0 + dx - 1, ORIGIN.1 + dz - 1);
            let index = (dx * 3 + dz) as usize;
            cache.chunks.push(if scene.kinds.0[index] {
                finished_chunk(x, z)
            } else {
                Chunk::Proto(Box::new(ProtoChunk::new(x, z, world_gen)))
            });
        }
    }
    for (x, y, z, block) in (scene.blocks)() {
        let (x, z) = (x + 16 * ORIGIN.0, z + 16 * ORIGIN.1);
        let index = (((x >> 4) - cache.x) * 3 + ((z >> 4) - cache.z)) as usize;
        if !(0..3).contains(&((x >> 4) - cache.x)) || !(0..3).contains(&((z >> 4) - cache.z)) {
            continue;
        }
        let (lx, lz) = ((x & 15) as usize, (z & 15) as usize);
        match &mut cache.chunks[index] {
            Chunk::Proto(proto) => {
                proto.set_block_state(x & 15, y, z & 15, block.default_state);
            }
            Chunk::Level(data) => {
                data.set_block_absolute_y(lx, y, lz, block.default_state.id);
            }
        }
    }
    cache
}

fn sections(chunk: &Chunk) -> Vec<LightContainer> {
    match chunk {
        Chunk::Proto(proto) => proto.light.sky_light.to_vec(),
        Chunk::Level(data) => data
            .light_engine
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .sky_light
            .to_vec(),
    }
}

fn describe(cache: &Cache) -> String {
    let mut out = String::new();
    for (index, chunk) in cache.chunks.iter().enumerate() {
        let mut parts = Vec::new();
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for (number, section) in sections(chunk).iter().enumerate() {
            let mut sum = 0u32;
            for y in 0..16 {
                for z in 0..16 {
                    for x in 0..16 {
                        let value = section.get(x, y, z);
                        sum += u32::from(value);
                        hash = (hash ^ u64::from(value)).wrapping_mul(0x0100_0000_01b3);
                    }
                }
            }
            if sum != 0 {
                parts.push(format!("{number}:{sum}"));
            }
        }
        let _ = writeln!(
            out,
            "chunk {index} hash {hash:016x} sums {}",
            parts.join(" ")
        );
    }
    out
}

/// The light of a column of the centre chunk, written as runs from the top.
fn column(cache: &Cache, x: usize, z: usize) -> String {
    let center = sections(&cache.chunks[4]);
    let mut runs: Vec<(u8, i32, i32)> = Vec::new();
    for y in (MIN_Y..MIN_Y + 16 * SECTIONS as i32).rev() {
        let rel = (y - MIN_Y) as usize;
        let value = center[rel >> 4].get(x, rel & 15, z);
        match runs.last_mut() {
            Some((last, _, low)) if *last == value => *low = y,
            _ => runs.push((value, y, y)),
        }
    }
    runs.iter()
        .map(|(value, high, low)| format!("{value}@{high}..{low}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn sky_light() -> Result<(), Box<dyn std::error::Error>> {
    let world_gen = world_gen();
    let mut report = String::new();
    for scene in all_scenes() {
        let mut cache = build(&scene, &world_gen);
        SkyLightPropagator::new().convert_light(&mut cache);
        let _ = writeln!(report, "== {}", scene.name);
        report.push_str(&describe(&cache));
        for (x, z) in [(0, 0), (6, 6), (13, 13), (15, 3), (2, 9)] {
            let _ = writeln!(report, "column {x},{z}: {}", column(&cache, x, z));
        }
    }

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("sky_light.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
