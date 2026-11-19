//! The hand an interact packet names is the hand whose item is used (card #96): vanilla's
//! `Player.interactOn(entity, hand)` works on `getItemInHand(hand)`, so an off-hand packet milks a
//! cow with the off-hand bucket and leaves the main hand alone.

use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::java::server::play::SInteract;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::{GameMode, Hand};
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use uuid::Uuid;

use crate::command::commands::script_golden::TestResult;
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::passive::cow::CowEntity;
use crate::entity::player::Player;
use crate::entity::{Entity, EntityBase};
use crate::net::ClientPlatform;
use crate::server::Server;
use crate::world::World;

const USER: Identity = Identity {
    name: "OffHand",
    uuid: Uuid::from_u128(0x0000_00fb),
    entity_id: 9_701,
};

const COW_ID: i32 = 9_702;
const INTERACT: i32 = 0;
const OFF_HAND: i32 = 1;

/// A survival user holding `main` and `off` next to an adult cow, after one off-hand interact
/// packet on the cow.
fn off_hand_interact(
    server: &Arc<Server>,
    world: &Arc<World>,
    main: ItemStack,
    off: ItemStack,
) -> Result<Arc<Player>, String> {
    let at = Vector3::new(200.5, 64.0, 8.5);
    let chunk = Vector2::new(12, 0);
    for dx in -1..=1 {
        for dz in -1..=1 {
            let near = Vector2::new(chunk.x + dx, chunk.y + dz);
            world.level.loaded_chunks.insert(
                near,
                pumpkin_world::chunk::ChunkData::empty_sync(near.x, near.y),
            );
        }
    }
    let cow = CowEntity::new(Entity::from_uuid_with_id(
        COW_ID,
        Uuid::from_u128(0x636f_7700),
        world.clone(),
        at,
        &EntityType::COW,
    ));
    if !world.spawn_entity(cow) {
        return Err("cow not spawned".into());
    }
    let (user, _tap) =
        test_support::java_player_as(world, GameMode::Survival, &USER).ok_or("no Java tap")?;
    user.get_entity()
        .set_pos(Vector3::new(at.x, at.y, at.z + 1.5));
    user.watched_section.store(Cylindrical::new(
        chunk,
        NonZero::<u8>::MIN.saturating_add(1),
    ));
    user.client_loaded.store(true, Ordering::Relaxed);
    user.inventory().set_stack_in_hand(Hand::Right, main);
    user.inventory().set_stack_in_hand(Hand::Left, off);
    world.add_player(&user)?;
    let ClientPlatform::Java(client) = user.client.as_ref() else {
        return Err("not a Java player".into());
    };
    let packet = SInteract {
        entity_id: VarInt(COW_ID),
        r#type: VarInt(INTERACT),
        target_position: None,
        hand: Some(VarInt(OFF_HAND)),
        sneaking: false,
    };
    client.handle_interact(&user, &packet, server);
    Ok(user)
}

fn hand(user: &Player, hand: Hand) -> String {
    let stack = user.inventory().get_stack_in_hand(hand);
    format!("{}x{}", stack.item.registry_key, stack.item_count)
}

#[tokio::test(flavor = "multi_thread")]
async fn an_off_hand_bucket_milks_a_cow() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let user = off_hand_interact(
        &server,
        &world,
        ItemStack::EMPTY.clone(),
        ItemStack::new(1, &Item::BUCKET),
    )?;
    let (main, off) = (hand(&user, Hand::Right), hand(&user, Hand::Left));
    server.stop();
    assert_eq!((main.as_str(), off.as_str()), ("airx0", "milk_bucketx1"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn an_off_hand_packet_leaves_the_main_hand_wheat() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let user = off_hand_interact(
        &server,
        &world,
        ItemStack::new(3, &Item::WHEAT),
        ItemStack::new(1, &Item::BUCKET),
    )?;
    let (main, off) = (hand(&user, Hand::Right), hand(&user, Hand::Left));
    server.stop();
    assert_eq!((main.as_str(), off.as_str()), ("wheatx3", "milk_bucketx1"));
    Ok(())
}
