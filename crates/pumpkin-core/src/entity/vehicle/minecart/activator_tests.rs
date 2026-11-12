//! A cart carrying passengers onto a powered activator rail (card #31): the tick used to call
//! `remove_passenger_sync` while it still held the cart's passenger lock, so the tick thread
//! waited on itself for good and the server froze. Vanilla ejects every passenger and wobbles
//! the cart (`MinecartRideable.activateMinecart`).
//!
//! Each tick runs on its own thread with a deadline, so a deadlock fails the test instead of
//! hanging it.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use pumpkin_data::Block;
use pumpkin_data::block_properties::{PoweredRailLikeProperties, RailShapeStraight};
use pumpkin_data::entity::EntityType;
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::world::BlockFlags;

use super::MinecartEntity;
use crate::entity::ai::test_support;
use crate::entity::living::LivingEntity;
use crate::entity::{Entity, EntityBase};
use crate::server::Server;
use crate::world::World;

type TestResult = Result<(), Box<dyn std::error::Error>>;
/// A spawned cart and its riders.
type Spawned = (Arc<MinecartEntity>, Vec<Arc<dyn EntityBase>>);

#[derive(Clone, Copy)]
enum Passenger {
    Zombie,
    Player,
}

/// A rideable cart at rest on an activator rail (`powered` or not) with `passengers`.
fn cart_on_activator(
    world: &Arc<World>,
    powered: bool,
    passengers: &[Passenger],
) -> Result<Spawned, Box<dyn std::error::Error>> {
    world.level.loaded_chunks.insert(
        Vector2::new(0, 0),
        pumpkin_world::chunk::ChunkData::empty_sync(0, 0),
    );
    let mut rail = PoweredRailLikeProperties::default(&Block::ACTIVATOR_RAIL);
    rail.shape = RailShapeStraight::EastWest;
    rail.powered = powered;
    world.set_block_state(
        &BlockPos(Vector3::new(8, 64, 8)),
        rail.to_state_id(&Block::ACTIVATOR_RAIL),
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
    let at = Vector3::new(8.5, 64.0, 8.5);
    let cart = Arc::new(MinecartEntity::new(Entity::new(
        Arc::clone(world),
        at,
        &EntityType::MINECART,
    )));
    let vehicle: Arc<dyn EntityBase> = Arc::clone(&cart) as Arc<dyn EntityBase>;
    if !world.spawn_entity(Arc::clone(&vehicle)) {
        return Err("the cart was not spawned".into());
    }
    let mut riders = Vec::new();
    for passenger in passengers {
        let seat = Vector3::new(at.x, at.y + 0.6, at.z);
        let rider: Arc<dyn EntityBase> = match passenger {
            Passenger::Zombie => Arc::new(LivingEntity::new(Entity::new(
                Arc::clone(world),
                seat,
                &EntityType::ZOMBIE,
            ))),
            Passenger::Player => test_support::java_player(world, GameMode::Survival),
        };
        cart.get_entity()
            .add_passenger_silently(Arc::clone(&vehicle), Arc::clone(&rider));
        riders.push(rider);
    }
    Ok((cart, riders))
}

/// One tick of `cart` on another thread; an error if it does not return within 10 s.
fn tick_with_deadline(server: &Arc<Server>, cart: &Arc<MinecartEntity>) -> TestResult {
    let (done, finished) = mpsc::channel();
    let runtime = tokio::runtime::Handle::current();
    let server = Arc::clone(server);
    let cart = Arc::clone(cart);
    std::thread::spawn(move || {
        let _runtime = runtime.enter();
        EntityBase::tick(cart.as_ref(), cart.as_ref(), &server);
        let _ = done.send(());
    });
    finished
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| "the minecart tick did not return within 10 s (deadlock)".into())
}

/// Who still rides, the cart's hurt time and damage.
fn state(cart: &MinecartEntity, riders: &[Arc<dyn EntityBase>]) -> String {
    let riding: Vec<bool> = riders
        .iter()
        .map(|rider| {
            cart.get_entity()
                .has_passenger(rider.get_entity().entity_id)
        })
        .collect();
    let vehicles: Vec<bool> = riders
        .iter()
        .map(|rider| rider.get_entity().has_vehicle())
        .collect();
    format!(
        "riding {riding:?} has vehicle {vehicles:?} hurt {} damage {}",
        cart.vehicle.get_hurt_time(),
        cart.vehicle.get_damage()
    )
}

async fn run(
    powered: bool,
    passengers: &[Passenger],
) -> Result<String, Box<dyn std::error::Error>> {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server
        .worlds
        .load()
        .first()
        .cloned()
        .ok_or("the server has no world")?;
    let (cart, riders) = cart_on_activator(&world, powered, passengers)?;
    // On a deadline miss the stuck tick still holds the passenger lock: return before reading
    // anything that takes it.
    tick_with_deadline(&server, &cart)?;
    let text = state(&cart, &riders);
    server.stop();
    Ok(text)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_powered_activator_rail_ejects_a_zombie() -> TestResult {
    assert_eq!(
        run(true, &[Passenger::Zombie]).await?,
        "riding [false] has vehicle [false] hurt 10 damage 50"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_powered_activator_rail_ejects_a_player_and_a_zombie() -> TestResult {
    assert_eq!(
        run(true, &[Passenger::Player, Passenger::Zombie]).await?,
        "riding [false, false] has vehicle [false, false] hurt 10 damage 50"
    );
    Ok(())
}

/// Guard: an unpowered activator rail leaves the passenger and the cart alone.
#[tokio::test(flavor = "multi_thread")]
async fn an_unpowered_activator_rail_keeps_the_passenger() -> TestResult {
    assert_eq!(
        run(false, &[Passenger::Zombie]).await?,
        "riding [true] has vehicle [true] hurt 0 damage 0"
    );
    Ok(())
}
