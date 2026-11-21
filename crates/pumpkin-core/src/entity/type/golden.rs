//! A record of what `from_type` builds for every entity type, made before the function was split
//! (`tests/golden/entity_from_type.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! The concrete struct behind the returned `dyn EntityBase` is told apart by its `TypeId`
//! (`EntityBase` has `Any` as a supertrait), so the record lists, per struct, the entity types
//! that got it, whether it is a living entity, and whether it keeps the type it was asked for. An
//! arm that builds another struct moves its type into another group.

use std::any::{Any, TypeId};
use std::fmt::Write;

use pumpkin_data::entity::EntityType;
use pumpkin_util::math::vector3::Vector3;
use uuid::Uuid;

use super::from_type;
use crate::entity::ai::test_support;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test(flavor = "multi_thread")]
async fn from_type_builds_the_same_entities() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load().first().cloned().ok_or("no world")?;
    let mut groups: Vec<(TypeId, bool, Vec<String>)> = Vec::new();
    for (index, kind) in EntityType::ALL.iter().enumerate() {
        let entity = from_type(
            kind,
            Vector3::new(0.5, 64.0, 0.5),
            &world,
            Uuid::from_u128(index as u128 + 1),
        );
        let any: &dyn Any = &*entity;
        let id = any.type_id();
        let living = entity.get_living_entity().is_some();
        let keeps = entity.get_entity().entity_type.id == kind.id;
        let name = format!(
            "{}{}",
            kind.resource_name,
            if keeps { "" } else { " (type changed)" }
        );
        match groups
            .iter_mut()
            .find(|group| group.0 == id && group.1 == living)
        {
            Some(group) => group.2.push(name),
            None => groups.push((id, living, vec![name])),
        }
    }
    server.stop();
    let mut now = String::new();
    for (_, living, names) in &groups {
        writeln!(
            now,
            "living={living} count={}\n  {}",
            names.len(),
            names.join(" ")
        )?;
    }
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/entity_from_type.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &now)?;
    } else {
        assert_eq!(std::fs::read_to_string(&path)?, now);
    }
    Ok(())
}
