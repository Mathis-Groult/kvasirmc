//! A record of what `TemplatePiece::place_blocks` does, made before it is split
//! (`tests/golden/template_piece_place.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! A small hand-made template (every palette kind the function treats on its own: stairs with and
//! without a waterlogged property, a log, chests with no / unseeded / seeded loot tables, a
//! spawner, structure blocks, structure voids, an unknown block, jigsaws with and without final
//! state and NBT, a block with NBT that tries to overwrite `x`/`y`/`z`/`id`, blocks outside the
//! template size) is placed with all twelve rotation x mirror pairs, two origins (one with a
//! rotation pivot), waterlogging on and off, over terrain with water, lava, dirt and air, with
//! five chunk boxes (the whole chunk, the far half, one layer, a corner far from the template, a
//! height range without the template). Each line has the settings, the chunk box, the bounding
//! boxes after the call, every block the call changed and every block entity as sorted SNBT.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::sync::Arc;

use pumpkin_data::{Block, Mirror, Rotation};
use pumpkin_nbt::{compound::NbtCompound, tag::NbtTag};
use pumpkin_util::{
    math::{block_box::BlockBox, vector3::Vector3},
    world_seed::Seed,
};

use super::super::TemplateBlock;
use super::{PaletteEntry, StructurePlaceSettings, StructureTemplate, TemplatePiece};
use crate::generation::{
    get_world_gen, proto_chunk::ProtoChunk, structure::piece::StructurePieceType,
};

const PALETTE: [&str; 12] = [
    "minecraft:stone",
    "minecraft:oak_stairs[facing=east,half=bottom,waterlogged=false]",
    "minecraft:oak_log[axis=x]",
    "minecraft:chest[facing=north,waterlogged=false]",
    "minecraft:structure_block",
    "minecraft:jigsaw",
    "minecraft:structure_void",
    "structure_void",
    "minecraft:no_such_block",
    "minecraft:oak_stairs[facing=north,half=top]",
    "minecraft:spawner",
    "minecraft:oak_fence",
];

fn compound(entries: &[(&str, NbtTag)]) -> NbtCompound {
    let mut nbt = NbtCompound::new();
    for (key, value) in entries {
        nbt.child_tags
            .insert((*key).to_owned().into(), value.clone());
    }
    nbt
}

fn text(value: &str) -> NbtTag {
    NbtTag::String(value.into())
}

fn jigsaw(final_state: &str) -> NbtCompound {
    compound(&[
        ("final_state", text(final_state)),
        ("pool", text("minecraft:empty")),
        ("name", text("minecraft:bottom")),
    ])
}

fn block(pos: (i32, i32, i32), state: u32, nbt: Option<NbtCompound>) -> TemplateBlock {
    TemplateBlock {
        pos: Vector3::new(pos.0, pos.1, pos.2),
        state,
        nbt,
    }
}

fn loot(seed: Option<i64>) -> NbtCompound {
    let mut nbt = compound(&[("LootTable", text("minecraft:chests/simple_dungeon"))]);
    if let Some(seed) = seed {
        nbt.put_long("LootTableSeed", seed);
    }
    nbt
}

/// Blocks with the structure the function skips or rewrites.
fn special_blocks() -> Vec<TemplateBlock> {
    vec![
        block((1, 1, 1), 4, Some(compound(&[("mode", text("DATA"))]))),
        block(
            (2, 1, 3),
            5,
            Some(jigsaw(
                "minecraft:oak_stairs[facing=south,half=top,waterlogged=false]",
            )),
        ),
        block((0, 1, 4), 5, Some(jigsaw("minecraft:chest[facing=west]"))),
        block((3, 1, 0), 5, None),
        block(
            (1, 0, 3),
            5,
            Some(compound(&[("pool", text("minecraft:empty"))])),
        ),
        block((1, 2, 2), 5, Some(jigsaw("minecraft:structure_void"))),
        block((0, 1, 0), 5, Some(jigsaw("minecraft:no_such_block"))),
        block(
            (2, 0, 0),
            5,
            Some(compound(&[("final_state", NbtTag::Int(7))])),
        ),
        block((2, 2, 0), 6, None),
        block((0, 2, 1), 7, None),
        block((3, 2, 3), 8, None),
    ]
}

/// Blocks that are placed, with and without block entities, inside and outside the template size.
fn placed_blocks() -> Vec<TemplateBlock> {
    let mut over_write = compound(&[
        ("x", NbtTag::Int(99)),
        ("y", NbtTag::Int(98)),
        ("z", NbtTag::Int(97)),
        ("id", text("minecraft:bogus")),
        ("CustomName", text("golden")),
        ("Count", NbtTag::Byte(3)),
    ]);
    over_write.put_compound(
        "Nested",
        compound(&[("b", NbtTag::Int(2)), ("a", NbtTag::Long(1))]),
    );
    vec![
        block((0, 0, 0), 0, None),
        block((1, 0, 0), 1, None),
        block((3, 0, 1), 2, None),
        block((0, 0, 2), 3, None),
        block((2, 0, 2), 3, Some(loot(None))),
        block((3, 0, 4), 3, Some(loot(Some(12345)))),
        block((1, 1, 3), 0, Some(over_write)),
        block((2, 1, 1), 9, None),
        block((3, 1, 2), 10, None),
        block(
            (2, 0, 4),
            0,
            Some(compound(&[("Items", NbtTag::List(vec![]))])),
        ),
        block((3, 2, 4), 11, None),
        block((6, 1, 6), 0, None),
        block((-2, 1, -1), 2, None),
    ]
}

fn template() -> Arc<StructureTemplate> {
    let mut template = StructureTemplate::default();
    template.set_author("golden".to_owned());
    template.size = Vector3::new(4, 3, 5);
    template.palette = PALETTE
        .iter()
        .map(|s| PaletteEntry::from_string(s))
        .collect();
    template.blocks = placed_blocks();
    template.blocks.extend(special_blocks());
    Arc::new(template)
}

const fn hash(x: i32, y: i32, z: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1)
        ^ (y as u32).wrapping_mul(0x85EB_CA77)
        ^ (z as u32).wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h
}

/// The untouched terrain: water, lava, dirt and air between y 62 and 70.
fn base(x: i32, y: i32, z: i32) -> &'static Block {
    if !(62..=70).contains(&y) {
        return &Block::AIR;
    }
    match hash(x, y, z) % 20 {
        0..=7 => &Block::WATER,
        8..=10 => &Block::LAVA,
        11..=13 => &Block::DIRT,
        _ => &Block::AIR,
    }
}

fn terrain(chunk: &mut ProtoChunk) {
    for x in 0..16 {
        for z in 0..16 {
            for y in 62..=70 {
                let block = base(x, y, z);
                if block.id != Block::AIR.id {
                    chunk.set_block_state(x, y, z, block.default_state);
                }
            }
        }
    }
}

fn changes(chunk: &ProtoChunk) -> String {
    let mut out = String::new();
    for x in 0..16 {
        for z in 0..16 {
            for y in 60..=72 {
                let now = chunk.get_block_state(&Vector3::new(x, y, z));
                if now.as_u16() != base(x, y, z).default_state.id.as_u16() {
                    let name = Block::from_state_id(now).name;
                    let _ = write!(out, " {x},{y},{z}={name}#{}", now.as_u16());
                }
            }
        }
    }
    out
}

fn snbt(nbt: &NbtCompound) -> String {
    let sorted: BTreeMap<&str, String> = nbt
        .child_tags
        .iter()
        .map(|(key, value)| {
            let value = match value {
                NbtTag::Compound(inner) => snbt(inner),
                other => other.to_string(),
            };
            (key.as_ref(), value)
        })
        .collect();
    let mut out = String::from("{");
    for (index, (key, value)) in sorted.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(out, "{key}:{value}");
    }
    out.push('}');
    out
}

fn boxed(b: &BlockBox) -> String {
    format!(
        "{},{},{}..{},{},{}",
        b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z
    )
}

struct Case {
    rotation: Rotation,
    mirror: Mirror,
    origin: Vector3<i32>,
    pivot: Vector3<i32>,
    waterlogging: bool,
    chunk_box: BlockBox,
}

fn run_case(world_gen: &crate::generation::generator::WorldGenerator, case: &Case) -> String {
    let mut chunk = ProtoChunk::new(0, 0, world_gen);
    terrain(&mut chunk);
    let settings = StructurePlaceSettings::new()
        .set_rotation(case.rotation)
        .set_mirror(case.mirror)
        .set_rotation_pivot(case.pivot)
        .set_apply_waterlogging(case.waterlogging)
        .set_bounding_box(Some(BlockBox::new(-5, -5, -5, -4, -4, -4)));
    let mut piece = TemplatePiece::from_settings(
        template(),
        "golden".to_owned(),
        settings,
        case.origin,
        StructurePieceType::MineshaftRoom,
    );
    piece.place_blocks(&mut chunk, &case.chunk_box);
    let settings_box = piece
        .place_settings
        .get_bounding_box()
        .map_or_else(|| "none".to_owned(), boxed);
    let mut line = format!(
        "{:?} {:?} origin {},{},{} pivot {},{},{} water {} chunk {} piece {} settings {} blocks",
        case.rotation,
        case.mirror,
        case.origin.x,
        case.origin.y,
        case.origin.z,
        case.pivot.x,
        case.pivot.y,
        case.pivot.z,
        case.waterlogging,
        boxed(&case.chunk_box),
        boxed(&piece.piece.bounding_box),
        settings_box,
    );
    line.push_str(&changes(&chunk));
    line.push_str(" entities");
    for entity in chunk.take_pending_block_entities() {
        let _ = write!(line, " {}", snbt(&entity));
    }
    line.push('\n');
    line
}

fn chunk_boxes() -> [BlockBox; 5] {
    [
        BlockBox::new(0, 60, 0, 15, 75, 15),
        BlockBox::new(8, 60, 8, 15, 75, 15),
        BlockBox::new(0, 65, 0, 15, 65, 15),
        BlockBox::new(0, 60, 0, 3, 75, 3),
        BlockBox::new(0, 100, 0, 15, 110, 15),
    ]
}

fn report() -> String {
    let world_gen = get_world_gen(
        Seed(1),
        pumpkin_data::dimension::Dimension::OVERWORLD,
        true,
        Vec::new(),
        "minecraft:plains".to_owned(),
    );
    let origins = [
        (Vector3::new(8, 64, 8), Vector3::new(0, 0, 0)),
        (Vector3::new(11, 64, 5), Vector3::new(1, 0, 2)),
    ];
    let mut report = String::new();
    for rotation in [
        Rotation::None,
        Rotation::Clockwise90,
        Rotation::Rotate180,
        Rotation::CounterClockwise90,
    ] {
        for mirror in [Mirror::None, Mirror::LeftRight, Mirror::FrontBack] {
            for (origin, pivot) in origins {
                for waterlogging in [false, true] {
                    for chunk_box in chunk_boxes() {
                        report.push_str(&run_case(
                            &world_gen,
                            &Case {
                                rotation,
                                mirror,
                                origin,
                                pivot,
                                waterlogging,
                                chunk_box,
                            },
                        ));
                    }
                }
            }
        }
    }
    report
}

#[test]
fn template_piece_place() -> Result<(), Box<dyn std::error::Error>> {
    let report = report();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("template_piece_place.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
