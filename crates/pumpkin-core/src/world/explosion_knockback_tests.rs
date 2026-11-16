//! Wind charges (vanilla 26.3 `ServerExplosion.hurtEntities` with the wind charge's damage
//! calculator): a Java player's knockback goes in its explosion packet and is never added to
//! the server's stale copy of its motion (card #60); mobs are pushed (card #63).

use std::sync::Arc;

use pumpkin_data::entity::EntityType;
use pumpkin_util::GameMode;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;

use crate::entity::ai::test_support;
use crate::entity::living::LivingEntity;
use crate::entity::projectile::wind_charge::WIND_CHARGE_EXPLOSION_DAMAGE_CALCULATOR;
use crate::entity::{Entity, EntityBase};
use crate::world::{ExplosionInteraction, World};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn wind_burst(world: &Arc<World>, at: Vector3<f64>) {
    world.explode_with_calculator(
        at,
        1.2,
        ExplosionInteraction::Trigger,
        Some(WIND_CHARGE_EXPLOSION_DAMAGE_CALCULATOR.clone()),
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn wind_charges_do_not_build_up_a_players_motion() -> TestResult {
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
    let player = test_support::java_player(&world, GameMode::Survival);
    player.get_entity().set_pos(Vector3::new(8.5, 64.0, 8.5));
    world.add_player(&player)?;
    for burst in 1..=3 {
        // Under the feet, as a player jumping on wind charges does.
        wind_burst(&world, Vector3::new(8.5, 63.9, 8.5));
        let velocity = player.get_entity().velocity.load();
        assert!(
            velocity.length_squared() == 0.0,
            "burst {burst}: the server added the knockback to the player's motion: {velocity:?}"
        );
    }
    server.stop();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn wind_charges_push_mobs() -> TestResult {
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
    let zombie: Arc<dyn EntityBase> = Arc::new(LivingEntity::new(Entity::new(
        Arc::clone(&world),
        Vector3::new(8.5, 64.0, 8.5),
        &EntityType::ZOMBIE,
    )));
    if !world.spawn_entity(Arc::clone(&zombie)) {
        return Err("the zombie was not spawned".into());
    }
    wind_burst(&world, Vector3::new(7.5, 64.0, 8.5));
    let velocity = zombie.get_entity().velocity.load();
    assert!(
        velocity.x > 0.1,
        "the zombie was not pushed away from the burst: {velocity:?}"
    );
    server.stop();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_real_zombie_flies_away_from_a_wind_charge() -> TestResult {
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
    for x in 0..16 {
        for z in 0..16 {
            world.set_block_state(
                &pumpkin_util::math::position::BlockPos(Vector3::new(x, 63, z)),
                pumpkin_data::Block::STONE.default_state.id,
                pumpkin_world::world::BlockFlags::FORCE_STATE
                    | pumpkin_world::world::BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
            );
        }
    }
    let start = Vector3::new(8.5, 64.0, 8.5);
    let zombie =
        crate::entity::r#type::from_type(&EntityType::ZOMBIE, start, &world, uuid::Uuid::new_v4());
    if !world.spawn_entity(Arc::clone(&zombie)) {
        return Err("the zombie was not spawned".into());
    }
    for _ in 0..3 {
        zombie.tick(zombie.as_ref(), &server);
    }
    // A wind charge shot at the ground one block from the zombie.
    let charge = crate::entity::r#type::from_type(
        &EntityType::WIND_CHARGE,
        Vector3::new(7.5, 64.2, 8.5),
        &world,
        uuid::Uuid::new_v4(),
    );
    charge.on_hit(crate::entity::projectile::ProjectileHit::Block {
        pos: pumpkin_util::math::position::BlockPos(Vector3::new(7, 63, 8)),
        face: pumpkin_data::BlockDirection::Up,
        hit_pos: Vector3::new(7.5, 64.0, 8.5),
        normal: Vector3::new(0.0, 1.0, 0.0),
    });
    let pushed = zombie.get_entity().velocity.load();
    for _ in 0..10 {
        zombie.tick(zombie.as_ref(), &server);
    }
    let moved = zombie.get_entity().pos.load().x - start.x;
    assert!(
        moved > 0.5,
        "the zombie moved {moved} blocks in 10 ticks after a velocity of {pushed:?}"
    );
    server.stop();
    Ok(())
}
