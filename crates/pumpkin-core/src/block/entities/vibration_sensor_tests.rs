//! Sculk sensors hear vibrations (vanilla 26.3 `VibrationSystem` + `SculkSensorBlockEntity`),
//! driven the way a world drives them: game events through the world, block entities ticked.

use std::sync::Arc;

use pumpkin_data::Block;
use pumpkin_data::block_properties::{SculkSensorLikeProperties, SculkSensorPhase};
use pumpkin_util::math::{position::BlockPos, vector2::Vector2};
use pumpkin_world::world::BlockFlags;

use crate::command::commands::script_golden::TestResult;
use crate::entity::ai::test_support;
use crate::world::World;

const X: i32 = 300;
const Y: i32 = 100;

fn sensor() -> BlockPos {
    BlockPos::new(X, Y, 0)
}

async fn with_world<T>(run: impl FnOnce(&Arc<World>) -> T) -> Result<T, String> {
    let folder = tempfile::tempdir().map_err(|e| e.to_string())?;
    let server = test_support::server(&folder)
        .await
        .map_err(|e| e.to_string())?;
    let world = server.worlds.load()[0].clone();
    for dx in -2..=2 {
        for dz in -2..=2 {
            let at = Vector2::new(X.div_euclid(16) + dx, dz);
            world
                .level
                .loaded_chunks
                .insert(at, pumpkin_world::chunk::ChunkData::empty_sync(at.x, at.y));
        }
    }
    world.set_block_state(
        &sensor(),
        Block::SCULK_SENSOR.default_state.id,
        BlockFlags::NOTIFY_ALL,
    );
    let out = run(&world);
    server.stop();
    Ok(out)
}

/// One server tick: the world age moves on and the block entities tick.
fn tick(world: &Arc<World>) {
    world
        .level_time
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .world_age += 1;
    if let Some(entity) = world.get_block_entity(&sensor()) {
        entity.tick(world);
    }
}

fn step_at(world: &Arc<World>, event: &str, dx: i32) {
    world.emit_game_event(event, BlockPos::new(X + dx, Y, 0).to_centered_f64());
}

fn state(world: &Arc<World>) -> (SculkSensorPhase, u8) {
    let id = world.get_block_state(&sensor()).id;
    let props = SculkSensorLikeProperties::from_state_id(id);
    (props.sculk_sensor_phase, props.power)
}

/// Ticks until the sensor is no longer inactive, at most `limit` ticks; returns the ticks used.
fn ticks_until_active(world: &Arc<World>, limit: u32) -> Option<u32> {
    (1..=limit).find(|_| {
        tick(world);
        state(world).0 != SculkSensorPhase::Inactive
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_step_nearby_activates_the_sensor_after_its_travel_time() -> TestResult {
    let used = with_world(|world| {
        step_at(world, "step", 3);
        let used = ticks_until_active(world, 20);
        (used, state(world))
    })
    .await?;
    // Selected on the first tick, which already counts one of floor(3.0) travel ticks.
    assert_eq!(used, (Some(3), (SculkSensorPhase::Active, 10)));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_step_out_of_range_is_not_heard() -> TestResult {
    let used = with_world(|world| {
        step_at(world, "step", 9);
        ticks_until_active(world, 30)
    })
    .await?;
    assert_eq!(used, None);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn events_that_make_no_vibration_are_not_heard() -> TestResult {
    let used = with_world(|world| {
        step_at(world, "sculk_sensor_tendrils_clicking", 2);
        ticks_until_active(world, 30)
    })
    .await?;
    assert_eq!(used, None);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn the_closest_vibration_of_a_tick_wins() -> TestResult {
    let (used, state) = with_world(|world| {
        step_at(world, "step", 6);
        step_at(world, "block_place", 2);
        let used = ticks_until_active(world, 30);
        let frequency = world
            .get_block_entity(&sensor())
            .and_then(|be| {
                be.as_any()
                    .downcast_ref::<super::sculk_sensor::SculkSensorBlockEntity>()
                    .map(|sensor| *sensor.last_vibration_frequency.lock().unwrap())
            })
            .unwrap_or(-1);
        (used, (state(world), frequency))
    })
    .await?;
    // floor(2.0) travel ticks, the first spent in the selecting tick; strength 15 - floor(2/8*15).
    assert_eq!(used, Some(2));
    assert_eq!(state, ((SculkSensorPhase::Active, 12), 13));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn wool_all_around_the_sensor_blocks_the_vibration() -> TestResult {
    let used = with_world(|world| {
        let around = [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ];
        for (dx, dy, dz) in around {
            world.set_block_state(
                &BlockPos::new(X + dx, Y + dy, dz),
                Block::WHITE_WOOL.default_state.id,
                BlockFlags::NOTIFY_ALL,
            );
        }
        step_at(world, "step", 4);
        ticks_until_active(world, 30)
    })
    .await?;
    assert_eq!(used, None);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_vibration_waits_while_the_first_travels() -> TestResult {
    let power = with_world(|world| {
        step_at(world, "step", 6);
        tick(world);
        tick(world); // the first is selected and travelling
        step_at(world, "block_place", 1);
        ticks_until_active(world, 30);
        state(world).1
    })
    .await?;
    // Only the step at distance 6 is delivered: 15 - floor(6/8*15).
    assert_eq!(power, 4);
    Ok(())
}
