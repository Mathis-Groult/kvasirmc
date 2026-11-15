//! `/clone` must not drop the contents of a container it moves or overwrites (card #43).
//!
//! Vanilla 26.3 `CloneCommands.clone` first sets every destination position, and for `move` every
//! source position, to a barrier with flags 816 (skip on-place, skip block-entity side effects,
//! suppress drops, known shape): a moved chest keeps its items only at the destination and an
//! overwritten container loses its contents without dropping them.

use std::sync::Arc;

use pumpkin_data::Block;
use pumpkin_util::math::position::BlockPos;
use pumpkin_world::world::BlockFlags;

use super::golden::{CHEST, at, build_area, describe_block_entity, run};
use crate::entity::ai::test_support;
use crate::world::World;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn entity_count(world: &Arc<World>) -> usize {
    world.entities.load().len()
}

fn chest_at(world: &Arc<World>, pos: &BlockPos) -> String {
    describe_block_entity(world, pos, (0, 0, 0))
}

#[tokio::test(flavor = "multi_thread")]
async fn moving_a_chest_keeps_its_items_at_the_destination_only() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let x0 = build_area(&world, 1);
    let source = at(x0, CHEST);
    let dest = BlockPos::new(x0 + 30, CHEST.1, CHEST.2);

    let command = format!(
        "clone {sx} {y} {z} {sx} {y} {z} {dx} {y} {z} replace move",
        sx = source.0.x,
        dx = dest.0.x,
        y = CHEST.1,
        z = CHEST.2
    );
    let result = run(&server, &world, x0, &command);

    let outcome = format!(
        "{result}; source {:?} [{}]; dest [{}]; entities {}",
        world.get_block_state_id(&source) == Block::AIR.default_state.id,
        chest_at(&world, &source),
        chest_at(&world, &dest),
        entity_count(&world)
    );
    server.stop();
    assert_eq!(
        outcome,
        "ok 1; source true []; dest [0,0,0:minecraft:chest/3=5x1012]; entities 0"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn cloning_over_a_container_deletes_its_contents_without_dropping_them() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let x0 = build_area(&world, 1);
    let chest = at(x0, CHEST);
    // A stone block two blocks below the chest is cloned onto the chest.
    let stone = BlockPos::new(chest.0.x, 60, chest.0.z);
    world.set_block_state(
        &stone,
        Block::STONE.default_state.id,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );

    let command = format!(
        "clone {x} 60 {z} {x} 60 {z} {x} {y} {z}",
        x = chest.0.x,
        y = chest.0.y,
        z = chest.0.z
    );
    let result = run(&server, &world, x0, &command);

    let outcome = format!(
        "{result}; stone {}; chest [{}]; entities {}",
        world.get_block_state_id(&chest) == Block::STONE.default_state.id,
        chest_at(&world, &chest),
        entity_count(&world)
    );
    server.stop();
    assert_eq!(outcome, "ok 1; stone true; chest []; entities 0");
    Ok(())
}

/// Guard: a plain clone leaves the source chest full and makes a full copy.
#[tokio::test(flavor = "multi_thread")]
async fn a_plain_clone_copies_the_chest_and_keeps_the_source() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let x0 = build_area(&world, 1);
    let source = at(x0, CHEST);
    let dest = BlockPos::new(x0 + 30, CHEST.1, CHEST.2);

    let command = format!(
        "clone {sx} {y} {z} {sx} {y} {z} {dx} {y} {z}",
        sx = source.0.x,
        dx = dest.0.x,
        y = CHEST.1,
        z = CHEST.2
    );
    let result = run(&server, &world, x0, &command);

    let outcome = format!(
        "{result}; source [{}]; dest [{}]; entities {}",
        chest_at(&world, &source),
        chest_at(&world, &dest),
        entity_count(&world)
    );
    server.stop();
    assert_eq!(
        outcome,
        "ok 1; source [0,0,0:minecraft:chest/3=5x1012]; dest [0,0,0:minecraft:chest/3=5x1012]; entities 0"
    );
    Ok(())
}
