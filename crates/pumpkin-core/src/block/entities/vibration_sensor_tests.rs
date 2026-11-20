//! Sculk sensors hear vibrations (vanilla 26.3 `VibrationSystem` + `SculkSensorBlockEntity`),
//! driven the way a world drives them: game events through the world, block entities ticked.

use std::sync::Arc;

use pumpkin_data::Block;
use pumpkin_data::block_properties::{SculkSensorLikeProperties, SculkSensorPhase};
use pumpkin_util::math::{position::BlockPos, vector2::Vector2};
use pumpkin_world::world::BlockFlags;

use pumpkin_util::GameMode;
use uuid::Uuid;

use crate::block::blocks::sculk::sculk_shrieker::SculkShriekerBlock;
use crate::command::commands::script_golden::TestResult;
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::player::Player;
use crate::world::World;
use crate::world::game_event::GameEventContext;

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

fn break_at(world: &Arc<World>, block: &Block, dx: i32) {
    let at = BlockPos::new(X + dx, Y, 3);
    world.set_block_state(&at, block.default_state.id, BlockFlags::SKIP_DROPS);
    world.break_block(&at, None, BlockFlags::NOTIFY_ALL | BlockFlags::SKIP_DROPS);
}

#[tokio::test(flavor = "multi_thread")]
async fn breaking_a_block_nearby_is_heard() -> TestResult {
    let heard = with_world(|world| {
        break_at(world, &Block::STONE, 1);
        ticks_until_active(world, 30).is_some()
    })
    .await?;
    assert!(heard);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn breaking_wool_is_silent() -> TestResult {
    let heard = with_world(|world| {
        break_at(world, &Block::WHITE_WOOL, 1);
        ticks_until_active(world, 30).is_some()
    })
    .await?;
    assert!(!heard);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_vibration_in_flight_survives_a_save_and_reload() -> TestResult {
    use super::BlockEntity;
    use super::sculk_sensor::SculkSensorBlockEntity;
    let (before, after) = with_world(|world| {
        step_at(world, "step", 4);
        tick(world); // selected, three ticks of travel left
        let saved = {
            let entity = world.get_block_entity(&sensor()).expect("sensor");
            let mut nbt = pumpkin_nbt::compound::NbtCompound::new();
            entity.write_nbt(&mut nbt);
            nbt
        };
        let before = state(world);
        // The chunk unloads and loads again: a new block entity from the saved compound.
        world.add_block_entity(Arc::new(SculkSensorBlockEntity::from_nbt(&saved, sensor())));
        (before, ticks_until_active(world, 20))
    })
    .await?;
    assert_eq!(before.0, SculkSensorPhase::Inactive);
    // floor(4.0) = 4 travel ticks, one spent in the selecting tick.
    assert_eq!(after, Some(3));
    Ok(())
}

const WALKER: Identity = Identity {
    name: "Walker",
    uuid: Uuid::from_u128(0x0000_5c01),
    entity_id: 9_801,
};

fn shrieker() -> BlockPos {
    BlockPos::new(X + 5, Y, 0)
}

fn is_shrieking(world: &Arc<World>) -> bool {
    SculkShriekerBlock::is_shrieking(world, &shrieker())
}

/// A shrieker five blocks from the sensor, ticked with it every tick.
fn with_shrieker(world: &Arc<World>) {
    world.set_block_state(
        &shrieker(),
        Block::SCULK_SHRIEKER.default_state.id,
        BlockFlags::NOTIFY_ALL,
    );
}

fn tick_both(world: &Arc<World>) {
    tick(world);
    if let Some(entity) = world.get_block_entity(&shrieker()) {
        entity.tick(world);
    }
}

/// Ticks until the shrieker shrieks, at most `limit` ticks; returns the ticks used.
fn ticks_until_shriek(world: &Arc<World>, limit: u32) -> Option<u32> {
    (1..=limit).find(|_| {
        tick_both(world);
        is_shrieking(world)
    })
}

/// A survival player joined to the world, as the source entity of events.
fn walker(world: &Arc<World>) -> Result<Arc<Player>, String> {
    let (player, _tap) =
        test_support::java_player_as(world, GameMode::Survival, &WALKER).ok_or("no Java tap")?;
    world.add_player(&player)?;
    Ok(player)
}

fn player_event(world: &Arc<World>, player: &Player, event: &str, dx: i32) {
    world.emit_game_event_with_context(
        event,
        BlockPos::new(X + dx, Y, 0).to_centered_f64(),
        &GameEventContext {
            source_entity: Some(player),
            affected_state: None,
        },
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_player_who_sets_off_a_sensor_next_to_a_shrieker_makes_it_shriek() -> TestResult {
    let used = with_world(|world| -> Result<_, String> {
        with_shrieker(world);
        let player = walker(world)?;
        player_event(world, &player, "step", 3);
        // The sensor hears the step (3 ticks), clicks, and the click travels 5 blocks.
        Ok((ticks_until_active(world, 20), ticks_until_shriek(world, 20)))
    })
    .await??;
    assert_eq!(used, (Some(3), Some(5)));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sensor_set_off_by_no_player_leaves_the_shrieker_quiet() -> TestResult {
    let (clicked, shrieked) = with_world(|world| {
        with_shrieker(world);
        step_at(world, "step", 3);
        (ticks_until_active(world, 20), ticks_until_shriek(world, 20))
    })
    .await?;
    assert_eq!(clicked, Some(3));
    assert_eq!(shrieked, None);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_shrieker_does_not_hear_steps_itself() -> TestResult {
    let shrieked = with_world(|world| -> Result<_, String> {
        with_shrieker(world);
        let player = walker(world)?;
        // Out of the sensor's range (9 > 8) but beside the shrieker.
        player_event(world, &player, "step", 9);
        Ok(ticks_until_shriek(world, 30))
    })
    .await??;
    assert_eq!(shrieked, None);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_calibrated_sensor_saves_the_vibration_in_flight() -> TestResult {
    use super::BlockEntity;
    use super::calibrated_sculk_sensor::CalibratedSculkSensorBlockEntity;
    let after = with_world(|world| {
        world.set_block_state(
            &sensor(),
            Block::CALIBRATED_SCULK_SENSOR.default_state.id,
            BlockFlags::NOTIFY_ALL,
        );
        step_at(world, "step", 4);
        tick(world);
        let mut saved = pumpkin_nbt::compound::NbtCompound::new();
        world
            .get_block_entity(&sensor())
            .expect("calibrated sensor")
            .write_nbt(&mut saved);
        world.add_block_entity(Arc::new(CalibratedSculkSensorBlockEntity::from_nbt(
            &saved,
            sensor(),
        )));
        ticks_until_active(world, 20)
    })
    .await?;
    assert_eq!(after, Some(3));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_vibration_waits_until_the_neighbouring_chunks_are_loaded() -> TestResult {
    let (while_missing, after) = with_world(|world| {
        let neighbour = Vector2::new(X.div_euclid(16) + 1, 1);
        world.level.loaded_chunks.remove(&neighbour);
        step_at(world, "step", 3);
        let while_missing = ticks_until_active(world, 20);
        world.level.loaded_chunks.insert(
            neighbour,
            pumpkin_world::chunk::ChunkData::empty_sync(neighbour.x, neighbour.y),
        );
        (while_missing, ticks_until_active(world, 20))
    })
    .await?;
    assert_eq!(while_missing, None);
    // The travel time ran out while waiting, so it is delivered on the first tick back.
    assert_eq!(after, Some(1));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_vibration_loaded_in_flight_shows_its_particle_once() -> TestResult {
    use super::BlockEntity;
    use super::sculk_sensor::SculkSensorBlockEntity;
    use crate::net::java::PacketTap;
    use pumpkin_data::packet::clientbound::play::LEVEL_PARTICLES;
    use pumpkin_util::math::vector3::Vector3;

    fn particles(tap: &mut PacketTap) -> usize {
        tap.drain()
            .iter()
            .filter(|p| p.first().copied() == Some(LEVEL_PARTICLES.0 as u8))
            .count()
    }

    let counts = with_world(|world| -> Result<_, String> {
        let (player, mut tap) = test_support::java_player_as(world, GameMode::Survival, &WALKER)
            .ok_or("no Java tap")?;
        player
            .living_entity
            .entity
            .set_pos(Vector3::new(f64::from(X), f64::from(Y), 1.0));
        world.add_player(&player)?;
        step_at(world, "step", 4);
        tick(world); // selected: the particle is sent once, three ticks of travel left
        let at_selection = particles(&mut tap);
        let mut saved = pumpkin_nbt::compound::NbtCompound::new();
        world
            .get_block_entity(&sensor())
            .expect("sensor")
            .write_nbt(&mut saved);
        world.add_block_entity(Arc::new(SculkSensorBlockEntity::from_nbt(&saved, sensor())));
        tick(world);
        let on_reload = particles(&mut tap);
        tick(world);
        Ok((at_selection, on_reload, particles(&mut tap)))
    })
    .await??;
    assert_eq!(counts, (1, 1, 0));
    Ok(())
}
