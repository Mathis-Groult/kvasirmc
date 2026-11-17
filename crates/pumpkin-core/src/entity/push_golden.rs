//! A record of what `EntityBase::push_entities` does, made before the function was split
//! (`tests/golden/push_entities.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene spawns a subject (a zombie `LivingEntity`, a rideable minecart, a chest minecart)
//! and a few others around it (zombies, an iron golem, minecarts, an item, a player added to
//! the world) in its own loaded area of a real server's world, optionally seats one of them in
//! the subject or in another, and calls `push_entities` on the subject once. The line records
//! the returned value, every entity's velocity, and who rides what.

use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_util::GameMode;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;

use super::item::ItemEntity;
use super::living::LivingEntity;
use super::player::Player;
use super::vehicle::minecart::MinecartEntity;
use super::{Entity, EntityBase};
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;
use crate::world::World;

#[derive(Clone, Copy)]
enum Kind {
    Zombie,
    Golem,
    Cart,
    ChestCart,
    HopperCart,
    FurnaceCart,
    TntCart,
    SpawnerCart,
    CommandCart,
    DeadZombie,
    Item,
    Player,
    Creative,
}

/// One entity of a scene: kind and offset of its feet from the subject's.
type Other = (Kind, f64, f64, f64);

struct Scene {
    name: &'static str,
    subject: Kind,
    others: &'static [Other],
    /// Seats `others[i]` in the subject before the call.
    rider: Option<usize>,
    /// Seats `others[i]` in `others[j]` before the call.
    seated: Option<(usize, usize)>,
    /// Riding cooldown given to every other.
    cooldown: i32,
    /// The subject's velocity during the call (a minecart pushes another minecart only while
    /// moving along its facing, yaw 0 = +x).
    velocity: (f64, f64, f64),
}

const fn scene(name: &'static str, subject: Kind, others: &'static [Other]) -> Scene {
    Scene {
        name,
        subject,
        others,
        rider: None,
        seated: None,
        cooldown: 0,
        velocity: (0.0, 0.0, 0.0),
    }
}

const fn rolling(mut scene: Scene) -> Scene {
    scene.velocity = (0.1, 0.0, 0.0);
    scene
}

const fn ridden(mut scene: Scene, rider: usize) -> Scene {
    scene.rider = Some(rider);
    scene
}

const fn seated(mut scene: Scene, rider: usize, vehicle: usize) -> Scene {
    scene.seated = Some((rider, vehicle));
    scene
}

const fn cooling(mut scene: Scene, cooldown: i32) -> Scene {
    scene.cooldown = cooldown;
    scene
}

const NEAR_ZOMBIE: &[Other] = &[(Kind::Zombie, 0.3, 0.0, 0.1)];
const NEAR_GOLEM: &[Other] = &[(Kind::Golem, 0.3, 0.0, 0.1)];
const NEAR_CART: &[Other] = &[(Kind::Cart, 0.3, 0.0, 0.1)];
const NEAR_HOPPER_CART: &[Other] = &[(Kind::HopperCart, 0.3, 0.0, 0.1)];
const NEAR_ITEM: &[Other] = &[(Kind::Item, 0.3, 0.0, 0.1)];
const NEAR_PLAYER: &[Other] = &[(Kind::Player, 0.3, 0.0, 0.1)];
const NEAR_CREATIVE: &[Other] = &[(Kind::Creative, 0.3, 0.0, 0.1)];
const CROWD: &[Other] = &[
    (Kind::Zombie, 0.3, 0.0, 0.0),
    (Kind::Zombie, -0.3, 0.0, 0.2),
    (Kind::Player, 0.0, 0.0, -0.3),
    (Kind::Zombie, 2.0, 0.0, 0.0),
];
const ON_TOP: &[Other] = &[(Kind::Zombie, 0.0, 0.0, 0.0)];
const PICKUP_EDGE: &[Other] = &[(Kind::Zombie, 0.98, 0.0, 0.0)];
const PICKUP_OUT: &[Other] = &[(Kind::Zombie, 1.0, 0.0, 0.0)];
const ZOMBIE_AND_CART: &[Other] = &[(Kind::Zombie, 0.3, 0.0, 0.1), (Kind::Cart, -0.3, 0.0, 0.1)];
const EVERY_CART: &[Other] = &[
    (Kind::Cart, 0.3, 0.0, 0.1),
    (Kind::FurnaceCart, -0.3, 0.0, 0.1),
    (Kind::TntCart, 0.3, 0.0, -0.1),
    (Kind::SpawnerCart, -0.3, 0.0, -0.1),
    (Kind::CommandCart, 0.1, 0.0, 0.3),
    (Kind::HopperCart, 0.1, 0.0, -0.3),
    (Kind::ChestCart, 0.2, 0.0, 0.2),
];
const PICKUP_EDGE_SOUTH: &[Other] = &[(Kind::Zombie, 0.0, 0.0, 0.98)];
const ZOMBIE_ABOVE: &[Other] = &[(Kind::Zombie, 0.0, 0.8, 0.0)];
const TOUCHING_EAST: &[Other] = &[(Kind::Zombie, 0.790_000_021_457_672_1, 0.0, 0.0)];
const TOUCHING_SOUTH: &[Other] = &[(Kind::Zombie, 0.0, 0.0, 0.790_000_021_457_672_1)];
const TOUCHING_ABOVE: &[Other] = &[(Kind::Zombie, 0.1, 0.699_999_988_079_071, 0.0)];
/// Zombie 0 is picked up; 1 rides 2, far away, so only its kind decides whether it is pushed.
const PICKED_AND_RIDING_CART: &[Other] = &[
    (Kind::Zombie, 0.3, 0.0, 0.1),
    (Kind::Cart, -0.3, 0.0, 0.1),
    (Kind::Zombie, 3.0, 0.0, 0.0),
];
const PICKED_AND_RIDING_GOLEM: &[Other] = &[
    (Kind::Zombie, 0.3, 0.0, 0.1),
    (Kind::Golem, -0.3, 0.0, 0.1),
    (Kind::Zombie, 3.0, 0.0, 0.0),
];
const RIDER_AND_RIDING_ZOMBIE: &[Other] = &[
    (Kind::Zombie, 0.0, 0.0, 0.0),
    (Kind::Zombie, -0.3, 0.0, 0.1),
    (Kind::Zombie, 3.0, 0.0, 0.0),
];
const PASSENGER_CART: &[Other] = &[(Kind::Cart, 0.3, 0.0, 0.1), (Kind::Cart, -0.3, 0.0, 0.1)];
const TWO_ZOMBIES: &[Other] = &[
    (Kind::Zombie, 0.3, 0.0, 0.1),
    (Kind::Zombie, -0.3, 0.0, 0.1),
];

const SCENES: &[Scene] = &[
    scene("zombie alone", Kind::Zombie, &[]),
    scene("zombie near a zombie", Kind::Zombie, NEAR_ZOMBIE),
    scene("zombie on top of a zombie", Kind::Zombie, ON_TOP),
    scene("zombie near a cart", Kind::Zombie, NEAR_CART),
    scene("zombie near an item", Kind::Zombie, NEAR_ITEM),
    scene("zombie near a player", Kind::Zombie, NEAR_PLAYER),
    scene("zombie near a creative player", Kind::Zombie, NEAR_CREATIVE),
    scene("zombie in a crowd", Kind::Zombie, CROWD),
    scene("cart near a zombie", Kind::Cart, NEAR_ZOMBIE),
    scene("cart near a golem", Kind::Cart, NEAR_GOLEM),
    scene("cart near a cart", Kind::Cart, NEAR_CART),
    scene("cart near a hopper cart", Kind::Cart, NEAR_HOPPER_CART),
    scene("cart near an item", Kind::Cart, NEAR_ITEM),
    scene("cart near a player", Kind::Cart, NEAR_PLAYER),
    scene("cart near a creative player", Kind::Cart, NEAR_CREATIVE),
    scene("cart in a crowd", Kind::Cart, CROWD),
    scene(
        "cart with a zombie at the pickup edge",
        Kind::Cart,
        PICKUP_EDGE,
    ),
    scene(
        "cart with a zombie past the pickup edge",
        Kind::Cart,
        PICKUP_OUT,
    ),
    cooling(
        scene(
            "cart near a zombie on riding cooldown",
            Kind::Cart,
            NEAR_ZOMBIE,
        ),
        5,
    ),
    ridden(
        scene("ridden cart near its rider", Kind::Cart, NEAR_ZOMBIE),
        0,
    ),
    ridden(
        scene("ridden cart near a zombie", Kind::Cart, TWO_ZOMBIES),
        0,
    ),
    seated(
        scene(
            "cart near a zombie riding a cart",
            Kind::Cart,
            ZOMBIE_AND_CART,
        ),
        0,
        1,
    ),
    scene("cart near a zombie and a cart", Kind::Cart, ZOMBIE_AND_CART),
    scene("chest cart near a zombie", Kind::ChestCart, NEAR_ZOMBIE),
    scene("chest cart near a cart", Kind::ChestCart, NEAR_CART),
    scene("chest cart near a player", Kind::ChestCart, NEAR_PLAYER),
    ridden(
        scene("chest cart near its rider", Kind::ChestCart, NEAR_CART),
        0,
    ),
    scene("chest cart near an item", Kind::ChestCart, NEAR_ITEM),
    // Appended after the first mutation run: a subject that cannot be pushed, every minecart
    // kind as subject and as neighbour, moving minecarts (the only ones that push minecarts),
    // the pickup reach on z and above, boxes touching exactly, and neighbours riding something.
    scene("dead zombie near a zombie", Kind::DeadZombie, NEAR_ZOMBIE),
    rolling(scene(
        "rolling cart near every cart",
        Kind::Cart,
        EVERY_CART,
    )),
    rolling(scene(
        "rolling chest cart near every cart",
        Kind::ChestCart,
        EVERY_CART,
    )),
    rolling(scene(
        "rolling hopper cart near a cart",
        Kind::HopperCart,
        NEAR_CART,
    )),
    rolling(scene(
        "rolling furnace cart near a cart",
        Kind::FurnaceCart,
        NEAR_CART,
    )),
    rolling(scene(
        "rolling tnt cart near a cart",
        Kind::TntCart,
        NEAR_CART,
    )),
    rolling(scene(
        "rolling spawner cart near a cart",
        Kind::SpawnerCart,
        NEAR_CART,
    )),
    rolling(scene(
        "rolling command cart near a cart",
        Kind::CommandCart,
        NEAR_CART,
    )),
    rolling(scene(
        "rolling hopper cart near a zombie",
        Kind::HopperCart,
        NEAR_ZOMBIE,
    )),
    rolling(ridden(
        scene(
            "rolling chest cart near its cart passenger",
            Kind::ChestCart,
            PASSENGER_CART,
        ),
        0,
    )),
    scene(
        "cart with a zombie at the south pickup edge",
        Kind::Cart,
        PICKUP_EDGE_SOUTH,
    ),
    scene("cart with a zombie just above", Kind::Cart, ZOMBIE_ABOVE),
    cooling(
        scene("cart touching a zombie east", Kind::Cart, TOUCHING_EAST),
        5,
    ),
    cooling(
        scene("cart touching a zombie south", Kind::Cart, TOUCHING_SOUTH),
        5,
    ),
    cooling(
        scene("cart touching a zombie above", Kind::Cart, TOUCHING_ABOVE),
        5,
    ),
    seated(
        scene(
            "cart picking up, near a cart riding a zombie",
            Kind::Cart,
            PICKED_AND_RIDING_CART,
        ),
        1,
        2,
    ),
    seated(
        scene(
            "cart picking up, near a golem riding a zombie",
            Kind::Cart,
            PICKED_AND_RIDING_GOLEM,
        ),
        1,
        2,
    ),
    seated(
        ridden(
            scene(
                "ridden cart near a zombie riding a zombie",
                Kind::Cart,
                RIDER_AND_RIDING_ZOMBIE,
            ),
            0,
        ),
        1,
        2,
    ),
    rolling(scene("rolling cart near a player", Kind::Cart, NEAR_PLAYER)),
    rolling(scene(
        "rolling chest cart near a player",
        Kind::ChestCart,
        NEAR_PLAYER,
    )), // Appended after the second mutation run: the other minecart kinds next to a zombie (only
    // the minecart path keeps them from pushing non-minecarts).
    scene("furnace cart near a zombie", Kind::FurnaceCart, NEAR_ZOMBIE),
    scene("tnt cart near a zombie", Kind::TntCart, NEAR_ZOMBIE),
    scene("spawner cart near a zombie", Kind::SpawnerCart, NEAR_ZOMBIE),
    scene("command cart near a zombie", Kind::CommandCart, NEAR_ZOMBIE),
];

fn spawn(
    world: &Arc<World>,
    kind: Kind,
    at: Vector3<f64>,
    players: &mut Vec<Arc<Player>>,
) -> Result<Arc<dyn EntityBase>, String> {
    let entity: Arc<dyn EntityBase> = match kind {
        Kind::Zombie => Arc::new(LivingEntity::new(Entity::new(
            world.clone(),
            at,
            &EntityType::ZOMBIE,
        ))),
        Kind::Golem => Arc::new(LivingEntity::new(Entity::new(
            world.clone(),
            at,
            &EntityType::IRON_GOLEM,
        ))),
        Kind::DeadZombie => {
            let zombie = LivingEntity::new(Entity::new(world.clone(), at, &EntityType::ZOMBIE));
            zombie.health.store(0.0);
            Arc::new(zombie)
        }
        Kind::Cart
        | Kind::ChestCart
        | Kind::HopperCart
        | Kind::FurnaceCart
        | Kind::TntCart
        | Kind::SpawnerCart
        | Kind::CommandCart => {
            let entity_type = match kind {
                Kind::Cart => &EntityType::MINECART,
                Kind::ChestCart => &EntityType::CHEST_MINECART,
                Kind::FurnaceCart => &EntityType::FURNACE_MINECART,
                Kind::TntCart => &EntityType::TNT_MINECART,
                Kind::SpawnerCart => &EntityType::SPAWNER_MINECART,
                Kind::CommandCart => &EntityType::COMMAND_BLOCK_MINECART,
                _ => &EntityType::HOPPER_MINECART,
            };
            Arc::new(MinecartEntity::new(Entity::new(
                world.clone(),
                at,
                entity_type,
            )))
        }
        Kind::Item => Arc::new(ItemEntity::new(
            Entity::new(world.clone(), at, &EntityType::ITEM),
            ItemStack::new(1, &Item::STICK),
        )),
        Kind::Player | Kind::Creative => {
            let mode = if matches!(kind, Kind::Creative) {
                GameMode::Creative
            } else {
                GameMode::Survival
            };
            let player = test_support::java_player(world, mode);
            player.get_entity().set_pos(at);
            world.add_player(&player)?;
            players.push(player.clone());
            return Ok(player);
        }
    };
    if !world.spawn_entity(entity.clone()) {
        return Err("not spawned".to_string());
    }
    Ok(entity)
}

fn entity_text(entity: &dyn EntityBase, all: &[Arc<dyn EntityBase>]) -> String {
    let base = entity.get_entity();
    let v = base.velocity.load();
    let label = |id: i32| {
        all.iter()
            .position(|other| other.get_entity().entity_id == id)
            .map_or_else(|| "?".to_string(), |i| i.to_string())
    };
    let riders: Vec<String> = all
        .iter()
        .filter(|other| base.has_passenger(other.get_entity().entity_id))
        .map(|other| label(other.get_entity().entity_id))
        .collect();
    format!("v ({:.6}, {:.6}, {:.6}) riders {riders:?}", v.x, v.y, v.z)
}

async fn run(out: &mut String, world: &Arc<World>, index: usize, scene: &Scene) -> TestResult {
    let at = Vector3::new(index as f64 * 16.0 + 8.5, 64.0, 8.5);
    let mut players = Vec::new();
    let subject = spawn(world, scene.subject, at, &mut players)?;
    let mut all = vec![subject.clone()];
    for (kind, x, y, z) in scene.others {
        let other = spawn(
            world,
            *kind,
            Vector3::new(at.x + x, at.y + y, at.z + z),
            &mut players,
        )?;
        other
            .get_entity()
            .riding_cooldown
            .store(scene.cooldown, Relaxed);
        all.push(other);
    }
    if let Some(rider) = scene.rider {
        subject
            .get_entity()
            .add_passenger_silently(subject.clone(), all[rider + 1].clone());
    }
    if let Some((rider, vehicle)) = scene.seated {
        let vehicle = all[vehicle + 1].clone();
        vehicle
            .get_entity()
            .add_passenger_silently(vehicle.clone(), all[rider + 1].clone());
    }
    for entity in &all {
        entity.get_entity().velocity.store(Vector3::default());
    }
    let (vx, vy, vz) = scene.velocity;
    subject
        .get_entity()
        .velocity
        .store(Vector3::new(vx, vy, vz));
    let result = subject.push_entities(subject.as_ref());
    let states: Vec<String> = all
        .iter()
        .enumerate()
        .map(|(i, entity)| format!("{i}: {}", entity_text(entity.as_ref(), &all)))
        .collect();
    let _ = writeln!(
        out,
        "{}: returned {result}; {}",
        scene.name,
        states.join("; ")
    );
    for player in &players {
        world.remove_player(player, false).await;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn push_entities_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    for cx in -1..=SCENES.len() as i32 {
        for cz in -1..=1 {
            world.level.loaded_chunks.insert(
                Vector2::new(cx, cz),
                pumpkin_world::chunk::ChunkData::empty_sync(cx, cz),
            );
        }
    }
    let mut out = String::new();
    for (index, scene) in SCENES.iter().enumerate() {
        run(&mut out, &world, index, scene).await?;
    }
    server.stop();
    script_golden::bless_or_compare("push_entities.txt", &out)
}
