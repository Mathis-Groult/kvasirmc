//! The hand's stack after a use-item-on packet, as vanilla's `ServerPlayerGameMode.useItemOn`
//! leaves it: a block's own use works on the live stack in the hand (cards #91), and only a
//! damageable item that breaks sends the item-break status (card #92).

use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use pumpkin_data::block_properties::{CampfireLikeProperties, RespawnAnchorLikeProperties};
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::packet::CURRENT_MC_VERSION;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::java::client::play::CEntityStatus;
use pumpkin_protocol::java::server::play::SUseItemOn;
use pumpkin_protocol::packet::MultiVersionJavaPacket;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::{GameMode, Hand};
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use crate::block::entities::campfire::CampfireBlockEntity;
use crate::command::commands::script_golden::TestResult;
use crate::entity::EntityBase;
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::player::Player;
use crate::net::ClientPlatform;
use crate::net::java::PacketTap;
use crate::server::Server;
use crate::world::World;

const USER: Identity = Identity {
    name: "User",
    uuid: Uuid::from_u128(0x0000_00f9),
    entity_id: 9_351,
};

/// Loads the chunks around `pos` and lays `state` there on a stone floor.
fn lay(world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
    let chunk = pos.chunk_position();
    for dx in -1..=1 {
        for dz in -1..=1 {
            let at = Vector2::new(chunk.x + dx, chunk.y + dz);
            world
                .level
                .loaded_chunks
                .insert(at, pumpkin_world::chunk::ChunkData::empty_sync(at.x, at.y));
        }
    }
    let flags = BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK;
    world.set_block_state(&pos.down(), Block::STONE.default_state.id, flags);
    world.set_block_state(&pos, state, flags);
}

/// Puts a fresh survival user two blocks south of `pos` holding `held`, and hands its client one
/// use-item-on packet on the block's top face. Returns the user's main hand, the ids of the
/// packets it was sent and the user.
fn use_on(
    server: &Arc<Server>,
    world: &Arc<World>,
    pos: BlockPos,
    held: ItemStack,
) -> Result<(ItemStack, Vec<u8>, Arc<Player>), String> {
    let chunk = pos.chunk_position();
    let (user, mut tap): (Arc<Player>, PacketTap) =
        test_support::java_player_as(world, GameMode::Survival, &USER).ok_or("no Java tap")?;
    user.get_entity().set_pos(Vector3::new(
        f64::from(pos.0.x) + 0.5,
        f64::from(pos.0.y),
        f64::from(pos.0.z) + 2.5,
    ));
    user.watched_section.store(Cylindrical::new(
        chunk,
        NonZero::<u8>::MIN.saturating_add(1),
    ));
    user.client_loaded.store(true, Ordering::Relaxed);
    user.inventory().set_stack_in_hand(Hand::Right, held);
    world.add_player(&user)?;
    let ClientPlatform::Java(client) = user.client.as_ref() else {
        return Err("not a Java player".into());
    };
    drop(tap.drain());
    let packet = SUseItemOn {
        hand: VarInt(0),
        position: pos,
        face: VarInt(1),
        cursor_pos: Vector3::new(0.5, 1.0, 0.5),
        inside_block: false,
        is_against_world_border: false,
        sequence: VarInt(3),
    };
    client
        .handle_use_item_on(&user, &packet, server)
        .map_err(|e| e.to_string())?;
    let ids = tap
        .drain()
        .iter()
        .filter_map(|p| p.first().copied())
        .collect();
    Ok((user.inventory().held_item(), ids, user))
}

fn entity_status_id() -> u8 {
    u8::try_from(CEntityStatus::to_id(CURRENT_MC_VERSION)).unwrap_or(u8::MAX)
}

#[tokio::test(flavor = "multi_thread")]
async fn charging_a_respawn_anchor_uses_up_one_glowstone() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let pos = BlockPos::new(200, 64, 8);
    let anchor = RespawnAnchorLikeProperties { charges: 0 }.to_state_id(&Block::RESPAWN_ANCHOR);
    lay(&world, pos, anchor);
    let (hand, _, user) = use_on(&server, &world, pos, ItemStack::new(3, &Item::GLOWSTONE))?;
    let charges =
        RespawnAnchorLikeProperties::from_state_id(world.get_block_state_id(&pos)).charges;
    world.remove_player(&user, false).await;
    server.stop();
    assert_eq!(charges, 1, "the anchor was not charged");
    assert_eq!(hand.item_count, 2, "glowstone left in the hand (vanilla 2)");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn raw_food_put_on_a_campfire_leaves_the_hand() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let pos = BlockPos::new(216, 64, 8);
    let campfire = CampfireLikeProperties::default(&Block::CAMPFIRE).to_state_id(&Block::CAMPFIRE);
    lay(&world, pos, campfire);
    world.add_block_entity(Arc::new(CampfireBlockEntity::new(pos)));
    let (hand, _, user) = use_on(&server, &world, pos, ItemStack::new(2, &Item::BEEF))?;
    let cooking = world
        .get_block_entity(&pos)
        .and_then(|entity| {
            entity
                .as_any()
                .downcast_ref::<CampfireBlockEntity>()
                .map(|campfire| {
                    campfire
                        .items
                        .iter()
                        .map(|slot| {
                            slot.lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .item_count
                        })
                        .sum::<u8>()
                })
        })
        .unwrap_or_default();
    world.remove_player(&user, false).await;
    server.stop();
    assert_eq!(cooking, 1, "beef on the campfire");
    assert_eq!(hand.item_count, 1, "beef left in the hand (vanilla 1)");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn placing_the_last_block_sends_no_item_break_status() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let pos = BlockPos::new(232, 64, 8);
    lay(&world, pos, Block::STONE.default_state.id);
    let (hand, ids, user) = use_on(&server, &world, pos, ItemStack::new(1, &Item::STONE))?;
    let placed = world.get_block(&pos.up()) == &Block::STONE;
    world.remove_player(&user, false).await;
    server.stop();
    assert!(placed, "the stone was not placed");
    assert!(hand.is_empty(), "the last stone stayed in the hand");
    assert!(
        !ids.contains(&entity_status_id()),
        "an entity status (item break) was sent: {ids:?}"
    );
    Ok(())
}

/// Places `item` on a floor block with a sculk sensor three blocks away, ticks the world and
/// says whether the sensor heard it (vanilla `BlockItem.place` emits `BLOCK_PLACE`).
async fn sensor_hears_placing(item: &'static Item) -> Result<bool, Box<dyn std::error::Error>> {
    use pumpkin_data::block_properties::{SculkSensorLikeProperties, SculkSensorPhase};
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let pos = BlockPos::new(232, 64, 8);
    lay(&world, pos, Block::STONE.default_state.id);
    let sensor = BlockPos::new(235, 65, 8);
    world.set_block_state(
        &sensor,
        Block::SCULK_SENSOR.default_state.id,
        BlockFlags::NOTIFY_ALL,
    );
    let (_, _, user) = use_on(&server, &world, pos, ItemStack::new(1, item))?;
    let mut heard = false;
    for _ in 0..10 {
        world
            .level_time
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .world_age += 1;
        if let Some(entity) = world.get_block_entity(&sensor) {
            entity.tick(&world);
        }
        let id = world.get_block_state(&sensor).id;
        heard |= SculkSensorLikeProperties::from_state_id(id).sculk_sensor_phase
            != SculkSensorPhase::Inactive;
    }
    world.remove_player(&user, false).await;
    server.stop();
    Ok(heard)
}

#[tokio::test(flavor = "multi_thread")]
async fn placing_a_block_is_heard_by_a_sculk_sensor() -> TestResult {
    assert!(sensor_hears_placing(&Item::STONE).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn placing_wool_is_not_heard_by_a_sculk_sensor() -> TestResult {
    assert!(!sensor_hears_placing(&Item::WHITE_WOOL).await?);
    Ok(())
}
