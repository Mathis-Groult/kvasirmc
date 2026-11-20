//! Falling anvils (vanilla 26.3 `FallingBlockEntity.causeFallDamage`, card #62): an anvil that
//! falls onto a mob hurts it by 2 per block fallen beyond the first.

use std::sync::Arc;

use pumpkin_data::Block;
use pumpkin_data::entity::EntityType;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::world::BlockFlags;

use super::FallingEntity;
use crate::entity::ai::test_support;
use crate::entity::living::LivingEntity;
use crate::entity::{Entity, EntityBase};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test(flavor = "multi_thread")]
async fn a_falling_anvil_hurts_the_mob_it_lands_on() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server
        .worlds
        .load()
        .first()
        .cloned()
        .ok_or("the server has no world")?;
    world.level.loaded_chunks.insert(
        Vector2::new(0, 0),
        pumpkin_world::chunk::ChunkData::empty_sync(0, 0),
    );
    world.set_block_state(
        &BlockPos(Vector3::new(8, 63, 8)),
        Block::STONE.default_state.id,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
    let zombie = Arc::new(LivingEntity::new(Entity::new(
        Arc::clone(&world),
        Vector3::new(8.5, 64.0, 8.5),
        &EntityType::ZOMBIE,
    )));
    if !world.spawn_entity(Arc::clone(&zombie) as Arc<dyn EntityBase>) {
        return Err("the zombie was not spawned".into());
    }
    let health = zombie.health.load();

    let anvil_state = Block::ANVIL.default_state.id;
    let entity = Entity::new(
        Arc::clone(&world),
        Vector3::new(8.5, 74.0, 8.5),
        &EntityType::FALLING_BLOCK,
    );
    let anvil = Arc::new(FallingEntity::new(entity, anvil_state));
    world.spawn_entity_non_save(Arc::clone(&anvil) as Arc<dyn EntityBase>);
    for _ in 0..80 {
        if anvil.get_entity().is_removed() {
            break;
        }
        EntityBase::tick(anvil.as_ref(), anvil.as_ref(), &server);
    }
    assert!(anvil.get_entity().is_removed(), "the anvil never landed");
    // 10 blocks: ceil(10 - 1) = 9 blocks counted, 2 each.
    let lost = health - zombie.health.load();
    assert!(
        (lost - 18.0).abs() < 1.0e-3,
        "the zombie lost {lost} health (vanilla: 18)"
    );
    server.stop();
    Ok(())
}
