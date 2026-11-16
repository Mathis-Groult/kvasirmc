//! Rail movement against vanilla 26.3 `OldMinecartBehavior.moveAlongTrack` (card #50): a
//! cart pushed sideways stays on its rail line, a player's push is vanilla's 0.05, a stopped
//! cart on a powered rail starts away from a solid block (and so turns back after running
//! into one), and a rider's movement keys start the cart toward where they point.

use std::sync::Arc;

use pumpkin_data::block_properties::{
    PoweredRailLikeProperties, RailLikeProperties, RailShape, RailShapeStraight,
};
use pumpkin_data::entity::EntityType;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_protocol::java::server::play::SPlayerInput;
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::world::BlockFlags;

use super::MinecartEntity;
use crate::entity::ai::test_support;
use crate::entity::{Entity, EntityBase};
use crate::server::Server;
use crate::world::World;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The column the tracks run in, along z.
const X: i32 = 8;
const Y: i32 = 64;
/// Where `MinecartItem` puts a cart on a flat rail at `X`, `Y`, `z`.
fn placed_at(z: i32) -> Vector3<f64> {
    Vector3::new(
        f64::from(X) + 0.5,
        f64::from(Y) + 0.0625,
        f64::from(z) + 0.5,
    )
}

async fn test_world()
-> Result<(Arc<Server>, Arc<World>, tempfile::TempDir), Box<dyn std::error::Error>> {
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
    Ok((server, world, folder))
}

fn put(world: &Arc<World>, x: i32, z: i32, state: BlockStateId) {
    world.set_block_state(
        &BlockPos(Vector3::new(x, Y, z)),
        state,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

/// Plain north-south rails at `X` from `from` to `to`.
fn plain_track(world: &Arc<World>, from: i32, to: i32) {
    let mut rail = RailLikeProperties::default(&Block::RAIL);
    rail.shape = RailShape::NorthSouth;
    for z in from..=to {
        put(world, X, z, rail.to_state_id(&Block::RAIL));
    }
}

/// Powered north-south rails at `X` from `from` to `to`, and stone just north of them.
fn powered_track_against_wall(world: &Arc<World>, from: i32, to: i32) {
    powered_track(world, from, to);
    put(world, X, from - 1, Block::STONE.default_state.id);
}

/// Powered north-south rails at `X` from `from` to `to`.
fn powered_track(world: &Arc<World>, from: i32, to: i32) {
    let mut rail = PoweredRailLikeProperties::default(&Block::POWERED_RAIL);
    rail.shape = RailShapeStraight::NorthSouth;
    rail.powered = true;
    for z in from..=to {
        put(world, X, z, rail.to_state_id(&Block::POWERED_RAIL));
    }
}

fn spawn(
    world: &Arc<World>,
    at: Vector3<f64>,
) -> Result<Arc<MinecartEntity>, Box<dyn std::error::Error>> {
    let cart = Arc::new(MinecartEntity::new(Entity::new(
        Arc::clone(world),
        at,
        &EntityType::MINECART,
    )));
    if !world.spawn_entity(Arc::clone(&cart) as Arc<dyn EntityBase>) {
        return Err("the cart was not spawned".into());
    }
    Ok(cart)
}

fn tick(cart: &MinecartEntity, server: &Server) {
    EntityBase::tick(cart, cart, server);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cart_pushed_sideways_stays_on_the_rail_line() -> TestResult {
    let (server, world, _folder) = test_world().await?;
    plain_track(&world, 0, 15);
    let cart = spawn(&world, placed_at(8))?;
    cart.get_entity()
        .velocity
        .store(Vector3::new(0.05, 0.0, 0.02));
    for step in 1..=10 {
        tick(&cart, &server);
        let pos = cart.get_entity().pos.load();
        assert!(
            (pos.x - 8.5).abs() < 1.0e-9,
            "tick {step}: the cart left the rail line, x = {}",
            pos.x
        );
        assert!(
            pos.z > 8.5,
            "tick {step}: the cart did not move along the rail"
        );
    }
    server.stop();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_player_beside_a_cart_pushes_it_by_vanillas_amount() -> TestResult {
    let (server, world, _folder) = test_world().await?;
    plain_track(&world, 0, 15);
    let cart = spawn(&world, placed_at(8))?;
    let player = test_support::java_player(&world, GameMode::Survival);
    player.get_entity().set_pos(Vector3::new(8.5, 64.0, 8.0));
    // What one world tick does to a cart a player stands against: the player's own tick pushes
    // it (vanilla `LivingEntity.pushEntities` -> `AbstractMinecart.push`), then the world tells
    // the cart about the player touching it (vanilla `AbstractMinecart` has no `playerTouch`, so
    // that adds nothing; Pumpkin's added 0.1 a tick there).
    cart.push(player.as_ref());
    cart.on_player_collision(&player);
    let velocity = cart.get_entity().velocity.load();
    assert!(
        (velocity.z - 0.05).abs() < 1.0e-9 && velocity.x.abs() < 1.0e-9,
        "push: {velocity:?}"
    );
    let before = cart.get_entity().pos.load().z;
    tick(&cart, &server);
    let moved = cart.get_entity().pos.load().z - before;
    assert!((moved - 0.05).abs() < 1.0e-9, "moved {moved} in one tick");
    server.stop();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_cart_on_a_powered_rail_starts_away_from_a_wall() -> TestResult {
    // A wall at each end in turn: the cart starts away from it, whatever way it faces (the
    // old code started every cart along its yaw, south for a new cart).
    // Both walls inside the one loaded chunk (z 0-15).
    for (wall_z, sense) in [(7, 1.0), (15, -1.0)] {
        let (server, world, _folder) = test_world().await?;
        powered_track(&world, 8, 14);
        put(&world, X, wall_z, Block::STONE.default_state.id);
        let start = if sense > 0.0 { 8 } else { 14 };
        let cart = spawn(&world, placed_at(start))?;
        for _ in 0..5 {
            tick(&cart, &server);
        }
        let entity = cart.get_entity();
        let moved = entity.pos.load().z - placed_at(start).z;
        assert!(
            entity.velocity.load().z * sense > 0.0 && moved * sense > 0.0,
            "wall at z {wall_z}: moved {moved}, velocity {:?}",
            entity.velocity.load()
        );
        server.stop();
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cart_that_hits_a_wall_on_a_powered_rail_turns_back() -> TestResult {
    let (server, world, _folder) = test_world().await?;
    powered_track_against_wall(&world, 8, 15);
    let cart = spawn(&world, placed_at(12))?;
    cart.get_entity()
        .velocity
        .store(Vector3::new(0.0, 0.0, -0.3));
    let mut nearest = f64::MAX;
    for _ in 0..40 {
        tick(&cart, &server);
        nearest = nearest.min(cart.get_entity().pos.load().z);
    }
    let entity = cart.get_entity();
    assert!(nearest < 8.6, "never reached the wall: nearest z {nearest}");
    assert!(
        entity.velocity.load().z > 0.1,
        "did not turn back: velocity {:?}, pos {:?}",
        entity.velocity.load(),
        entity.pos.load()
    );
    server.stop();
    Ok(())
}

/// Where a stopped cart with a player holding `input` while looking at `yaw` gets in
/// 40 ticks, as a distance along z.
async fn ridden_distance(input: i8, yaw: f32) -> Result<f64, Box<dyn std::error::Error>> {
    let (server, world, _folder) = test_world().await?;
    plain_track(&world, 0, 15);
    let start = placed_at(8);
    let cart = spawn(&world, start)?;
    let player = test_support::java_player(&world, GameMode::Survival);
    player
        .get_entity()
        .set_pos(Vector3::new(start.x, start.y + 0.6, start.z));
    player.get_entity().yaw.store(yaw);
    player
        .last_input
        .store(input, std::sync::atomic::Ordering::Relaxed);
    let vehicle: Arc<dyn EntityBase> = Arc::clone(&cart) as Arc<dyn EntityBase>;
    cart.get_entity()
        .add_passenger_silently(vehicle, player as Arc<dyn EntityBase>);
    for _ in 0..40 {
        tick(&cart, &server);
    }
    let moved = cart.get_entity().pos.load().z - start.z;
    server.stop();
    Ok(moved)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rider_starts_the_cart_where_the_keys_point() -> TestResult {
    // Yaw 0 looks south (+z), 180 north.
    let cases = [
        ("forward looking south", SPlayerInput::FORWARD, 0.0, 1.0),
        ("forward looking north", SPlayerInput::FORWARD, 180.0, -1.0),
        ("backward looking south", SPlayerInput::BACKWARD, 0.0, -1.0),
        ("backward looking north", SPlayerInput::BACKWARD, 180.0, 1.0),
    ];
    for (name, input, yaw, sense) in cases {
        let moved = ridden_distance(input, yaw).await?;
        assert!(moved * sense > 0.0, "{name}: moved {moved} along z");
    }
    Ok(())
}

/// A track along z that rises one block at `slope_z` (an ascending-south rail) and goes on one
/// block higher; plain rails.
fn track_with_slope(world: &Arc<World>, slope_z: i32) {
    let mut rail = RailLikeProperties::default(&Block::RAIL);
    rail.shape = RailShape::NorthSouth;
    let flat = rail.to_state_id(&Block::RAIL);
    rail.shape = RailShape::AscendingSouth;
    let slope = rail.to_state_id(&Block::RAIL);
    for z in 0..slope_z {
        put(world, X, z, flat);
    }
    put(world, X, slope_z, slope);
    for z in slope_z + 1..=15 {
        world.set_block_state(
            &BlockPos(Vector3::new(X, Y + 1, z)),
            flat,
            BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
        );
        put(world, X, z, Block::STONE.default_state.id);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cart_climbs_a_slope_and_goes_on_above_it() -> TestResult {
    // Card #32: the old tick kept the cart's height on a slope and ran it into the block
    // under the raised rail.
    let (server, world, _folder) = test_world().await?;
    track_with_slope(&world, 8);
    let cart = spawn(&world, placed_at(3))?;
    cart.get_entity()
        .velocity
        .store(Vector3::new(0.0, 0.0, 0.4));
    for _ in 0..30 {
        tick(&cart, &server);
    }
    let pos = cart.get_entity().pos.load();
    assert!(
        pos.z > 9.0 && (pos.y - (f64::from(Y) + 1.0625)).abs() < 1.0e-6,
        "the cart did not get up the slope: {pos:?}, velocity {:?}",
        cart.get_entity().velocity.load()
    );
    server.stop();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cart_rolls_down_a_slope_by_itself() -> TestResult {
    let (server, world, _folder) = test_world().await?;
    track_with_slope(&world, 8);
    // At rest on the upper level, on the slope's top end.
    let cart = spawn(
        &world,
        Vector3::new(f64::from(X) + 0.5, f64::from(Y) + 1.0625, 8.9),
    )?;
    for _ in 0..40 {
        tick(&cart, &server);
    }
    let pos = cart.get_entity().pos.load();
    assert!(
        pos.z < 7.0 && (pos.y - (f64::from(Y) + 0.0625)).abs() < 1.0e-6,
        "the cart did not roll down: {pos:?}, velocity {:?}",
        cart.get_entity().velocity.load()
    );
    server.stop();
    Ok(())
}
