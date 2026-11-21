//! A record of what `transform_block_properties` makes of every state of every block under every
//! rotation and mirror, made before the function was split
//! (`tests/golden/block_rotation.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each block is run under its own name and under the `minecraft:` spelling (the function tests
//! names for rails). Blocks with the same result under every rotation and mirror share one entry: the
//! number of states changed and a hash of the whole result list per combination, so any state that
//! maps elsewhere shows as a changed entry.

use std::fmt::Write;

use super::{
    DoorHinge, Facing, Mirror, Property, Rotation, StairsShape, transform_block_properties,
};
use crate::Block;
use crate::block_properties::BlockProperties;

const ROTATIONS: [Rotation; 4] = [
    Rotation::None,
    Rotation::Clockwise90,
    Rotation::Rotate180,
    Rotation::CounterClockwise90,
];
const MIRRORS: [Mirror; 3] = [Mirror::None, Mirror::LeftRight, Mirror::FrontBack];

fn fnv(hash: &mut u64, text: &str) {
    for byte in text.bytes() {
        *hash ^= u64::from(byte);
        *hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
}

/// States with some of the properties a stair or door reads missing, and a trapdoor (a name that
/// ends in `door` but is not one) given a hinge.
fn partial(out: &mut String) -> Result<(), std::fmt::Error> {
    let mut sets: Vec<Vec<Property>> = vec![Vec::new(), vec![Property::Waterlogged(true)]];
    for facing in [Facing::North, Facing::East, Facing::Up] {
        sets.push(vec![Property::Facing(facing)]);
        sets.push(vec![Property::Waterlogged(true), Property::Facing(facing)]);
    }
    for shape in [
        StairsShape::Straight,
        StairsShape::InnerLeft,
        StairsShape::OuterRight,
    ] {
        sets.push(vec![Property::StairsShape(shape)]);
        sets.push(vec![
            Property::Facing(Facing::East),
            Property::StairsShape(shape),
        ]);
    }
    for hinge in [DoorHinge::Left, DoorHinge::Right] {
        sets.push(vec![Property::Hinge(hinge)]);
        sets.push(vec![Property::Facing(Facing::East), Property::Hinge(hinge)]);
    }
    for name in [
        "oak_stairs",
        "oak_door",
        "oak_trapdoor",
        "door",
        "stairs",
        "minecraft:iron_door",
    ] {
        for props in &sets {
            for rotation in ROTATIONS {
                for mirror in MIRRORS {
                    let now = transform_block_properties(name, props, rotation, mirror);
                    writeln!(
                        out,
                        "partial {name} {props:?} {rotation:?}/{mirror:?} => {now:?}"
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn record() -> Result<String, std::fmt::Error> {
    let mut out = String::new();
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    let mut id = 0u16;
    while let Some(block_id) = crate::BlockId::new(id) {
        id += 1;
        let block = Block::from_id(block_id);
        let states: Vec<_> = block
            .states
            .iter()
            .filter_map(|state| block.properties(state.id))
            .map(|props| props.to_props())
            .collect();
        if states.len() <= 1 {
            continue;
        }
        let prefixed = format!("minecraft:{}", block.name);
        for name in [block.name, prefixed.as_str()] {
            if name != block.name && !name.contains("rail") {
                continue;
            }
            let mut signature = format!("states={}", states.len());
            for rotation in ROTATIONS {
                for mirror in MIRRORS {
                    let mut changed = 0usize;
                    let mut hash = 0xcbf2_9ce4_8422_2325u64;
                    for props in &states {
                        let now = transform_block_properties(name, props, rotation, mirror);
                        changed += usize::from(now != *props);
                        fnv(&mut hash, &format!("{now:?};"));
                    }
                    write!(signature, " {rotation:?}/{mirror:?}={changed}:{hash:016x}")?;
                }
            }
            match groups.iter_mut().find(|(known, _)| *known == signature) {
                Some((_, names)) => names.push(name.to_string()),
                None => groups.push((signature, vec![name.to_string()])),
            }
        }
    }
    for (signature, names) in &groups {
        writeln!(out, "{signature}\n  {}", names.join(" "))?;
    }
    partial(&mut out)?;
    Ok(out)
}

#[test]
fn block_properties_transform_the_same() -> Result<(), Box<dyn std::error::Error>> {
    let now = record()?;
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/block_rotation.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("no parent")?)?;
        std::fs::write(&path, &now)?;
    } else {
        assert_eq!(std::fs::read_to_string(&path)?, now);
    }
    Ok(())
}
