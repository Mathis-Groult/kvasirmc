//! A record of what `create_block_entity` builds for every block entity type id, made before the
//! function was split (`tests/golden/create_block_entity.txt`; rewrite it on purpose with
//! `KVASIR_BLESS=1`).
//!
//! Per id of `BLOCK_ENTITY_TYPES` (plus the ids just past the table and `u16::MAX`): the table
//! name, then either `none` or the resource location, the position, the concrete struct (its
//! `TypeId`, numbered in order of first appearance), `get_id` and the whole `write_internal`
//! NBT with every compound printed in key order. A type whose arm builds another struct, passes
//! another constructor argument or is missing moves or changes its line.

use std::any::{Any, TypeId};
use std::fmt::Write;

use pumpkin_data::block_properties::BLOCK_ENTITY_TYPES;
use pumpkin_nbt::{compound::NbtCompound, tag::NbtTag};
use pumpkin_util::math::position::BlockPos;

use super::create_block_entity;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A tag with every compound in key order (`child_tags` is a hash map).
fn tag_text(tag: &NbtTag) -> String {
    match tag {
        NbtTag::Compound(compound) => compound_text(compound),
        NbtTag::List(items) => {
            let items: Vec<String> = items.iter().map(tag_text).collect();
            format!("[{}]", items.join(", "))
        }
        other => format!("{other:?}"),
    }
}

fn compound_text(compound: &NbtCompound) -> String {
    let mut keys: Vec<&str> = compound.child_tags.keys().map(|key| &**key).collect();
    keys.sort_unstable();
    let fields: Vec<String> = keys
        .into_iter()
        .map(|key| format!("{key}: {}", tag_text(&compound.child_tags[key])))
        .collect();
    format!("{{{}}}", fields.join(", "))
}

fn line(id: u16, kinds: &mut Vec<TypeId>) -> Result<String, std::fmt::Error> {
    let position = BlockPos::new(3, -5, 7);
    let name = BLOCK_ENTITY_TYPES
        .get(usize::from(id))
        .copied()
        .unwrap_or("-");
    let mut out = format!("{id} {name} => ");
    let Some(entity) = create_block_entity(id, position) else {
        out.push_str("none");
        return Ok(out);
    };
    let any: &dyn Any = &*entity;
    let type_id = any.type_id();
    let kind = kinds
        .iter()
        .position(|seen| *seen == type_id)
        .unwrap_or_else(|| {
            kinds.push(type_id);
            kinds.len() - 1
        });
    let mut nbt = NbtCompound::new();
    entity.write_internal(&mut nbt);
    write!(
        out,
        "{} at {:?} kind={kind} get_id={} loot={} {}",
        entity.resource_location(),
        entity.get_position().0,
        entity.get_id(),
        entity.has_loot_table(),
        compound_text(&nbt)
    )?;
    Ok(out)
}

#[test]
fn create_block_entity_builds_the_same_entities() -> TestResult {
    let mut kinds: Vec<TypeId> = Vec::new();
    let mut now = String::new();
    let past = BLOCK_ENTITY_TYPES.len() as u16 + 2;
    for id in (0..past).chain([u16::MAX - 1, u16::MAX]) {
        writeln!(now, "{}", line(id, &mut kinds)?)?;
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/create_block_entity.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &now)?;
    } else {
        assert_eq!(std::fs::read_to_string(&path)?, now);
    }
    Ok(())
}
