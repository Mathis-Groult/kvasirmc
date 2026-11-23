//! A record of what `LightPropagator::propagate` does, made before it is split
//! (`tests/golden/light_propagate.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene builds a three by three square of chunks (proto and finished ones), puts blocks in
//! it, spreads the light through `LightEngine::initialize_light`, applies block edits through
//! `update_block_light` and `run_light_updates` (the decrease queue, then the increase queue), and
//! writes down the light of every section after each step. Two scenes drive the sky propagator by
//! hand, because the engine only edits block light.

use std::fmt::Write;
use std::sync::Arc;

use pumpkin_config::lighting::LightingEngineConfig;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::world_seed::Seed;

use super::engine::{LightEngine, PropagationEntry, SkyLightPropagator};
use super::storage::{get_sky_light, set_sky_light};
use crate::chunk::ChunkData;
use crate::chunk::format::LightContainer;
use crate::chunk_system::chunk_state::Chunk;
use crate::chunk_system::generation_cache::Cache;
use crate::generation::generator::WorldGenerator;
use crate::generation::get_world_gen;
use crate::generation::proto_chunk::{GenerationCache, ProtoChunk};

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Light {
    Block,
    Sky,
    /// Sky light, with the finished chunks lit like loaded ground: 15 from y 96 up.
    SkyLit,
}

struct Scene {
    name: &'static str,
    kinds: Kinds,
    light: Light,
    blocks: fn() -> Vec<Place>,
    edits: fn() -> Vec<Place>,
}

const fn scene(
    name: &'static str,
    kinds: Kinds,
    light: Light,
    blocks: fn() -> Vec<Place>,
    edits: fn() -> Vec<Place>,
) -> Scene {
    Scene {
        name,
        kinds,
        light,
        blocks,
        edits,
    }
}

fn none() -> Vec<Place> {
    Vec::new()
}

fn floor(top: i32, block: &'static Block) -> Vec<Place> {
    cuboid((-16, MIN_Y, -16), (31, top, 31), block)
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

/// A hollow box: the shell of `block` around the inside of `from`..`to`.
fn shell(from: (i32, i32, i32), to: (i32, i32, i32), block: &'static Block) -> Vec<Place> {
    let mut out = cuboid(from, to, block);
    out.retain(|&(x, y, z, _)| {
        x == from.0 || x == to.0 || y == from.1 || y == to.1 || z == from.2 || z == to.2
    });
    out
}

fn lamps(spots: &[(i32, i32, i32)], block: &'static Block) -> Vec<Place> {
    spots.iter().map(|&(x, y, z)| (x, y, z, block)).collect()
}

fn single_scenes() -> Vec<Scene> {
    let torch = || lamps(&[(8, 70, 8)], &Block::TORCH);
    vec![
        scene("torch_proto", PROTO, Light::Block, torch, none),
        scene("torch_ring", RING, Light::Block, torch, none),
        scene(
            "glowstone_mixed",
            MIXED,
            Light::Block,
            || lamps(&[(8, 70, 8)], &Block::GLOWSTONE),
            none,
        ),
        scene(
            "several_sources",
            MIXED,
            Light::Block,
            || {
                let mut out = lamps(&[(3, 40, 3), (7, 44, 12)], &Block::GLOWSTONE);
                out.extend(lamps(&[(12, 41, 5)], &Block::TORCH));
                out.extend(lamps(&[(10, 40, 10), (11, 40, 10)], &Block::REDSTONE_TORCH));
                out.extend(lamps(&[(5, 52, 6)], &Block::ENDER_CHEST));
                out
            },
            none,
        ),
        scene(
            "lava_pool",
            RING,
            Light::Block,
            || {
                let mut out = floor(30, &Block::STONE);
                out.extend(cuboid((4, 31, 4), (9, 31, 8), &Block::LAVA));
                out
            },
            none,
        ),
    ]
}

fn border_scenes() -> Vec<Scene> {
    let chunk_borders = || {
        let mut out = lamps(&[(0, 70, 0), (15, 70, 15), (16, 50, 8)], &Block::GLOWSTONE);
        out.extend(lamps(&[(-1, 70, 8), (8, 60, 16)], &Block::TORCH));
        out.extend(lamps(&[(8, 60, -1)], &Block::GLOWSTONE));
        // Two cells outside the scanned square: never seeded.
        out.extend(lamps(&[(-2, 80, 8), (18, 80, 8)], &Block::GLOWSTONE));
        out
    };
    let section_borders = || {
        let mut out = lamps(&[(8, 15, 8), (4, 16, 4)], &Block::GLOWSTONE);
        out.extend(lamps(&[(12, 31, 12), (2, 32, 2)], &Block::TORCH));
        out.extend(lamps(&[(14, -49, 14), (1, -48, 1)], &Block::GLOWSTONE));
        out
    };
    vec![
        scene(
            "chunk_borders_proto",
            PROTO,
            Light::Block,
            chunk_borders,
            none,
        ),
        scene(
            "chunk_borders_ring",
            RING,
            Light::Block,
            chunk_borders,
            none,
        ),
        scene(
            "chunk_borders_mixed",
            MIXED,
            Light::Block,
            chunk_borders,
            none,
        ),
        scene(
            "section_borders",
            MIXED,
            Light::Block,
            section_borders,
            none,
        ),
        scene(
            "negative_y",
            PROTO,
            Light::Block,
            || {
                let mut out = lamps(&[(8, -64, 8), (3, -63, 3)], &Block::GLOWSTONE);
                out.extend(lamps(&[(10, -1, 10), (11, 0, 11)], &Block::TORCH));
                out
            },
            none,
        ),
        scene(
            "negative_y_ring",
            RING,
            Light::Block,
            || lamps(&[(8, -64, 8), (3, -63, 3), (12, -2, 12)], &Block::GLOWSTONE),
            none,
        ),
        scene(
            "world_top",
            PROTO,
            Light::Block,
            || lamps(&[(8, 319, 8), (3, 318, 3)], &Block::GLOWSTONE),
            none,
        ),
    ]
}

fn wall_scenes() -> Vec<Scene> {
    let walled = || {
        let mut out = shell((2, 60, 2), (13, 70, 13), &Block::STONE);
        out.extend(lamps(&[(7, 65, 7)], &Block::GLOWSTONE));
        out.retain(|&(x, y, z, _)| (x, y, z) != (13, 66, 7));
        out
    };
    let partition = || {
        let mut out = cuboid((8, 40, -16), (8, 90, 31), &Block::STONE);
        out.retain(|&(_, y, z, _)| !((60..=61).contains(&y) && z == 5));
        out.extend(lamps(&[(5, 61, 5)], &Block::GLOWSTONE));
        out
    };
    vec![
        scene("walled_lamp_proto", PROTO, Light::Block, walled, none),
        scene("walled_lamp_mixed", MIXED, Light::Block, walled, none),
        scene("partition_gap", RING, Light::Block, partition, none),
        scene(
            "water_and_glass",
            MIXED,
            Light::Block,
            || {
                let mut out = cuboid((2, 40, 2), (13, 50, 13), &Block::WATER);
                out.extend(cuboid((5, 51, 5), (9, 53, 9), &Block::OAK_LEAVES));
                out.extend(cuboid((14, 45, 2), (14, 55, 13), &Block::GLASS));
                out.extend(cuboid((3, 41, 0), (3, 44, 1), &Block::STICKY_PISTON));
                out.extend(cuboid((12, 41, 0), (12, 44, 1), &Block::WHITE_WOOL_STAIRS));
                out.extend(lamps(&[(8, 47, 8), (14, 56, 8)], &Block::GLOWSTONE));
                out.extend(lamps(&[(3, 45, 0), (12, 45, 0)], &Block::TORCH));
                out
            },
            none,
        ),
        scene(
            "water_column",
            PROTO,
            Light::Block,
            || {
                let mut out = cuboid((7, 20, 7), (7, 34, 7), &Block::WATER);
                out.extend(lamps(&[(7, 35, 7)], &Block::GLOWSTONE));
                out
            },
            none,
        ),
    ]
}

fn update_scenes() -> Vec<Scene> {
    vec![
        scene(
            "remove_torch",
            PROTO,
            Light::Block,
            || lamps(&[(8, 70, 8)], &Block::TORCH),
            || lamps(&[(8, 70, 8)], &Block::AIR),
        ),
        scene(
            "remove_one_of_two",
            MIXED,
            Light::Block,
            || lamps(&[(6, 70, 8), (11, 70, 8)], &Block::GLOWSTONE),
            || lamps(&[(6, 70, 8)], &Block::AIR),
        ),
        scene(
            "remove_one_of_two_ring",
            RING,
            Light::Block,
            || lamps(&[(6, 70, 8), (6, 70, 9), (14, 72, 8)], &Block::GLOWSTONE),
            || lamps(&[(6, 70, 8), (14, 72, 8)], &Block::AIR),
        ),
        scene(
            "swap_lamps",
            PROTO,
            Light::Block,
            || {
                let mut out = lamps(&[(8, 70, 8)], &Block::GLOWSTONE);
                out.extend(lamps(&[(4, 70, 8)], &Block::TORCH));
                out
            },
            || {
                let mut out = lamps(&[(8, 70, 8)], &Block::TORCH);
                out.extend(lamps(&[(4, 70, 8)], &Block::GLOWSTONE));
                out
            },
        ),
        scene(
            "wall_goes_up",
            MIXED,
            Light::Block,
            || lamps(&[(8, 70, 8)], &Block::GLOWSTONE),
            || cuboid((10, 69, 7), (10, 71, 9), &Block::STONE),
        ),
        scene(
            "wall_comes_down",
            RING,
            Light::Block,
            || {
                let mut out = cuboid((10, 66, 4), (10, 74, 12), &Block::STONE);
                out.extend(lamps(&[(8, 70, 8)], &Block::GLOWSTONE));
                out
            },
            || lamps(&[(10, 70, 8), (10, 71, 8), (10, 70, 9)], &Block::AIR),
        ),
        scene(
            "edits_at_borders",
            MIXED,
            Light::Block,
            || lamps(&[(1, 70, 1), (14, 70, 14)], &Block::GLOWSTONE),
            || {
                let mut out = lamps(&[(0, 70, 0), (-1, 70, 4), (16, 70, 10)], &Block::TORCH);
                out.extend(lamps(&[(1, 70, 1), (14, 70, 14)], &Block::AIR));
                out
            },
        ),
        scene(
            "edits_at_cache_edge",
            PROTO,
            Light::Block,
            || lamps(&[(8, 70, 8)], &Block::GLOWSTONE),
            || {
                let mut out = lamps(
                    &[(-16, 70, 8), (31, 70, 8), (8, 70, -16)],
                    &Block::GLOWSTONE,
                );
                out.extend(lamps(&[(8, 70, 31), (-16, 70, 8)], &Block::AIR));
                out
            },
        ),
        scene(
            "edits_low_and_high",
            RING,
            Light::Block,
            || lamps(&[(8, -62, 8)], &Block::TORCH),
            || {
                let mut out = lamps(&[(8, -64, 8), (5, 319, 5), (9, 319, 9)], &Block::GLOWSTONE);
                out.extend(lamps(&[(8, -62, 8), (5, 319, 5)], &Block::AIR));
                out
            },
        ),
    ]
}

fn sky_scenes() -> Vec<Scene> {
    let ground = || {
        let mut out = floor(60, &Block::STONE);
        out.extend(lamps(&[(5, 62, 5)], &Block::TORCH));
        out
    };
    vec![
        scene("torch_on_ground", PROTO, Light::Block, ground, none),
        scene(
            "sky_overhang",
            RING,
            Light::Sky,
            || {
                let mut out = floor(30, &Block::STONE);
                out.extend(cuboid((-16, 40, -16), (5, 42, 31), &Block::STONE));
                out.extend(cuboid((6, 40, 6), (9, 41, 9), &Block::OAK_LEAVES));
                out.extend(lamps(&[(10, 40, 10)], &Block::WATER));
                out
            },
            || {
                let mut out = lamps(&[(8, 100, 8)], &Block::STONE);
                out.extend(lamps(&[(7, 41, 7)], &Block::AIR));
                out
            },
        ),
        scene(
            "sky_cave_mixed",
            MIXED,
            Light::Sky,
            || {
                let mut out = floor(100, &Block::STONE);
                out.retain(|&(x, y, z, _)| {
                    let room =
                        (2..=12).contains(&x) && (2..=12).contains(&z) && (60..80).contains(&y);
                    let shaft = x == 7 && z == 7 && y >= 80;
                    !room && !shaft
                });
                out
            },
            || lamps(&[(7, 90, 7), (4, 61, 4)], &Block::STONE),
        ),
        scene(
            "sky_blocks_ring",
            RING,
            Light::SkyLit,
            || floor(30, &Block::STONE),
            || {
                let mut out = lamps(&[(8, 150, 8), (-5, 150, 8)], &Block::STICKY_PISTON);
                out.extend(lamps(
                    &[(9, 170, 9), (-6, 170, 9)],
                    &Block::WHITE_WOOL_STAIRS,
                ));
                out.extend(lamps(&[(-7, 190, 8)], &Block::STONE));
                out.extend(lamps(&[(-7, 190, 8)], &Block::AIR));
                out
            },
        ),
    ]
}

fn all_scenes() -> Vec<Scene> {
    let mut all = single_scenes();
    all.extend(border_scenes());
    all.extend(wall_scenes());
    all.extend(update_scenes());
    all.extend(sky_scenes());
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

fn place(cache: &mut Cache, (x, y, z, block): Place) {
    let (x, z) = (x + 16 * ORIGIN.0, z + 16 * ORIGIN.1);
    let (rel_x, rel_z) = ((x >> 4) - cache.x, (z >> 4) - cache.z);
    if !(0..3).contains(&rel_x) || !(0..3).contains(&rel_z) {
        return;
    }
    let (lx, lz) = ((x & 15) as usize, (z & 15) as usize);
    match &mut cache.chunks[(rel_x * 3 + rel_z) as usize] {
        Chunk::Proto(proto) => proto.set_block_state(x & 15, y, z & 15, block.default_state),
        Chunk::Level(data) => {
            data.set_block_absolute_y(lx, y, lz, block.default_state.id);
        }
    }
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
    for put in (scene.blocks)() {
        place(&mut cache, put);
    }
    cache
}

fn sections(chunk: &Chunk, light: Light) -> Vec<LightContainer> {
    match (chunk, light) {
        (Chunk::Proto(proto), Light::Block) => proto.light.block_light.to_vec(),
        (Chunk::Proto(proto), _) => proto.light.sky_light.to_vec(),
        (Chunk::Level(data), which) => {
            let engine = data
                .light_engine
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if which == Light::Block {
                engine.block_light.to_vec()
            } else {
                engine.sky_light.to_vec()
            }
        }
    }
}

fn describe(cache: &Cache, light: Light) -> String {
    let mut out = String::new();
    for (index, chunk) in cache.chunks.iter().enumerate() {
        let mut parts = Vec::new();
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for (number, section) in sections(chunk, light).iter().enumerate() {
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
fn column(cache: &Cache, light: Light, x: usize, z: usize) -> String {
    let center = sections(&cache.chunks[4], light);
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
        .filter(|(value, _, _)| *value != 0)
        .map(|(value, high, low)| format!("{value}@{high}..{low}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn state_at(cache: &Cache, (x, y, z, _): Place) -> BlockStateId {
    let (x, z) = (x + 16 * ORIGIN.0, z + 16 * ORIGIN.1);
    cache.get_block_state(&Vector3::new(x, y, z))
}

fn world_pos((x, y, z, _): Place) -> BlockPos {
    BlockPos(Vector3::new(x + 16 * ORIGIN.0, y, z + 16 * ORIGIN.1))
}

fn report_step(report: &mut String, cache: &Cache, light: Light) {
    report.push_str(&describe(cache, light));
    for (x, z) in [(0, 0), (6, 6), (8, 8), (13, 13), (15, 3), (2, 9)] {
        let _ = writeln!(report, "column {x},{z}: {}", column(cache, light, x, z));
    }
}

/// Block edits through the engine: remove (decrease queue) or add (increase queue) lamps.
fn run_block_edits(engine: &mut LightEngine, cache: &mut Cache, edits: &[Place], out: &mut String) {
    for &edit in edits {
        let old = state_at(cache, edit).to_state().luminance;
        place(cache, edit);
        let new = edit.3.default_state.luminance;
        engine.update_block_light(cache, world_pos(edit), old, new);
        engine.run_light_updates(cache);
        let _ = writeln!(
            out,
            "-- edit {},{},{} {} -> {}",
            edit.0, edit.1, edit.2, old, new
        );
        report_step(out, cache, Light::Block);
    }
}

/// Sky edits by hand: a block that goes up darkens its cell and the dark spreads out through the
/// decrease queue, a block that goes away lets the lit neighbours spread in again.
fn run_sky_edits(
    propagator: &mut SkyLightPropagator,
    cache: &mut Cache,
    edits: &[Place],
) -> String {
    let mut out = String::new();
    for &edit in edits {
        let pos = world_pos(edit);
        let old = get_sky_light(cache, pos);
        place(cache, edit);
        if edit.3.default_state.id == BlockStateId::AIR {
            for dir in pumpkin_data::BlockDirection::all() {
                let around = pos.offset(dir.to_offset());
                if get_sky_light(cache, around) > 1
                    && propagator
                        .visited
                        .test_and_set(around.0.x, around.0.y, around.0.z)
                {
                    propagator.queue.push_back(PropagationEntry {
                        pos: around,
                        skip_direction: None,
                    });
                }
            }
            propagator.propagate(cache);
        } else {
            set_sky_light(cache, pos, 0);
            propagator.decrease_queue.push_back((pos, old));
            propagator.process_decrease_queue(cache);
        }
        propagator.visited.clear();
        let _ = writeln!(out, "-- edit {},{},{} sky {}", edit.0, edit.1, edit.2, old);
        report_step(&mut out, cache, Light::Sky);
    }
    out
}

/// Sky light 15 in every section from y 96 up of the finished chunks.
fn light_loaded_ground(cache: &Cache) {
    for chunk in &cache.chunks {
        let Chunk::Level(data) = chunk else { continue };
        let mut engine = data
            .light_engine
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for section in engine.sky_light.iter_mut().skip(10) {
            *section = LightContainer::new_filled(15);
        }
    }
}

fn run(scene: &Scene, world_gen: &WorldGenerator) -> String {
    let mut cache = build(scene, world_gen);
    let mut engine = LightEngine::new();
    let sky = scene.light != Light::Block;
    engine.initialize_light(&mut cache, &LightingEngineConfig::Default, sky);
    if scene.light == Light::SkyLit {
        light_loaded_ground(&cache);
    }
    let light = if sky { Light::Sky } else { Light::Block };
    let mut out = String::new();
    report_step(&mut out, &cache, light);
    let edits = (scene.edits)();
    if sky {
        let mut propagator = SkyLightPropagator::new();
        propagator.visited.ensure_capacity(
            (ORIGIN.0 - 1) * 16,
            MIN_Y,
            (ORIGIN.1 - 1) * 16,
            48,
            384,
            48,
        );
        out.push_str(&run_sky_edits(&mut propagator, &mut cache, &edits));
    } else {
        run_block_edits(&mut engine, &mut cache, &edits, &mut out);
    }
    out
}

#[test]
fn light_propagate() -> Result<(), Box<dyn std::error::Error>> {
    let world_gen = world_gen();
    let mut report = String::new();
    for scene in all_scenes() {
        let _ = writeln!(report, "== {}", scene.name);
        report.push_str(&run(&scene, &world_gen));
    }

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("light_propagate.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
