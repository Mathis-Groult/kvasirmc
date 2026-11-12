//! Card #34: finishing an item in use. Vanilla 26.3 `LivingEntity.completeUsingItem` applies the
//! item only if it is still the stack in the hand it is used from (otherwise
//! `releaseUsingItem()` and nothing happens), and replaces that hand's stack with the result.

use std::sync::Arc;

use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_inventory::Inventory;
use pumpkin_inventory::player::player_inventory::PlayerInventory;
use pumpkin_util::{GameMode, Hand};

use crate::entity::EntityBase;
use crate::entity::ai::test_support;
use crate::entity::player::Player;
use crate::server::Server;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A survival player with food level 6 who has `stack` in use from `hand` (finishing on the
/// next tick), after `place` put the stacks into the inventory.
fn eating(
    server: &Arc<Server>,
    hand: Hand,
    stack: &ItemStack,
    place: impl FnOnce(&Player),
) -> Result<Arc<Player>, String> {
    let world = server
        .worlds
        .load()
        .first()
        .cloned()
        .ok_or("the server has no world")?;
    let player = test_support::java_player(&world, GameMode::Survival);
    player.hunger_manager.level.store(6);
    place(&player);
    player.living_entity.set_active_hand(hand, stack.clone(), 1);
    Ok(player)
}

fn tick(server: &Server, player: &Arc<Player>) {
    EntityBase::tick(&player.living_entity, player.as_ref(), server);
}

fn slot(player: &Player, slot: usize) -> String {
    let stack = player.inventory.get_slot(slot);
    if stack.is_empty() {
        "empty".to_owned()
    } else {
        format!("{} x{}", stack.item.registry_key, stack.item_count)
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn food_moved_out_of_the_hand_is_not_eaten() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let bread = ItemStack::new(1, &Item::BREAD);
    // The player started eating the held bread, then selected another slot (holding dirt).
    let player = eating(&server, Hand::Right, &bread, |player| {
        player.inventory.set_stack(1, bread.clone());
        player
            .inventory
            .set_stack(0, ItemStack::new(3, &Item::DIRT));
        player.inventory.set_selected_slot(0);
    })?;
    tick(&server, &player);
    let state = (
        player.hunger_manager.level.load(),
        slot(&player, 0),
        slot(&player, 1),
        player
            .living_entity
            .item_in_use
            .lock()
            .map_or(true, |item| item.is_some()),
    );
    server.stop();
    // Released: nothing eaten, nothing consumed, no item in use any more.
    assert_eq!(
        state,
        (6, "dirt x3".to_owned(), "bread x1".to_owned(), false)
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn food_in_the_other_hand_is_not_eaten() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let apple = ItemStack::new(1, &Item::ENCHANTED_GOLDEN_APPLE);
    // In use from the off hand, but the apple is in the main hand and the off hand is empty.
    let player = eating(&server, Hand::Left, &apple, |player| {
        player.inventory.set_held_item(apple.clone());
    })?;
    tick(&server, &player);
    let state = (
        player.hunger_manager.level.load(),
        slot(&player, 0),
        player
            .living_entity
            .active_effects
            .lock()
            .map_or(99, |effects| effects.len()),
    );
    server.stop();
    assert_eq!(state, (6, "enchanted_golden_apple x1".to_owned(), 0));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn eating_from_the_off_hand_consumes_the_off_hand_stack() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let bread = ItemStack::new(2, &Item::BREAD);
    let player = eating(&server, Hand::Left, &bread, |player| {
        player.inventory.set_held_item(bread.clone());
        player
            .inventory
            .set_stack(PlayerInventory::OFF_HAND_SLOT, bread.clone());
    })?;
    tick(&server, &player);
    let state = (
        player.hunger_manager.level.load(),
        slot(&player, 0),
        slot(&player, PlayerInventory::OFF_HAND_SLOT),
    );
    server.stop();
    assert_eq!(state, (11, "bread x2".to_owned(), "bread x1".to_owned()));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn held_food_is_eaten_and_consumed() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let stew = ItemStack::new(1, &Item::MUSHROOM_STEW);
    let player = eating(&server, Hand::Right, &stew, |player| {
        player.inventory.set_held_item(stew.clone());
    })?;
    tick(&server, &player);
    let state = (player.hunger_manager.level.load(), slot(&player, 0));
    server.stop();
    assert_eq!(state, (12, "bowl x1".to_owned()));
    Ok(())
}
