//! Taking from result slots: a throw-all (Ctrl+Q) must end, and drop what can really be made.
//!
//! Each click runs on its own thread with a deadline, because the failure being tested is a
//! loop that never ends; a stuck thread is left behind and dies with the test process.

use std::sync::mpsc;
use std::time::Duration;

use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_protocol::java::server::play::SlotActionType;

use crate::click_golden::RecordingPlayer;
use crate::crafting::crafting_screen_handler::CraftingTableScreenHandler;
use crate::screen_handler::ScreenHandler;
use crate::smithing_table_screen_handler::SmithingTableScreenHandler;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn table_with_logs(player: &RecordingPlayer, logs: u8) -> CraftingTableScreenHandler {
    let mut handler = CraftingTableScreenHandler::new(0, &player.inventory, None);
    handler.get_behaviour().slots[1].set_stack(ItemStack::new(logs, &Item::OAK_LOG));
    handler.send_content_updates();
    handler
}

fn stack_text(stack: &ItemStack) -> String {
    if stack.is_empty() {
        return "empty".to_string();
    }
    format!("{} x{}", stack.item.registry_key, stack.item_count)
}

/// Result slot, one other cell, cursor, and what the player dropped (items by numeric id).
fn outcome(
    handler: &dyn ScreenHandler,
    player: &RecordingPlayer,
    result: usize,
    cell: usize,
) -> String {
    let behaviour = handler.get_behaviour();
    let cursor = behaviour
        .cursor_stack
        .lock()
        .map_or_else(|_| ItemStack::EMPTY.clone(), |cursor| cursor.clone());
    format!(
        "result {} cell {} cursor {} log {}",
        stack_text(&behaviour.slots[result].get_cloned_stack()),
        stack_text(&behaviour.slots[cell].get_cloned_stack()),
        stack_text(&cursor),
        player.take_log()
    )
}

/// Runs `click` on a thread and waits at most ten seconds for its outcome.
fn within_deadline(click: impl FnOnce() -> String + Send + 'static) -> Result<String, String> {
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(click());
    });
    finished
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| "the click did not finish within 10 s".to_string())
}

#[test]
fn throw_all_from_a_crafting_result_drops_every_craft_and_stops() -> TestResult {
    let outcome = within_deadline(|| {
        let player = RecordingPlayer::new(false);
        let mut handler = table_with_logs(&player, 2);
        handler.on_slot_click(0, 1, SlotActionType::Throw, &player);
        outcome(&handler, &player, 0, 1)
    })?;
    assert_eq!(
        outcome,
        "result empty cell empty cursor empty log drop 63x4 true;drop 63x4 true;"
    );
    Ok(())
}

#[test]
fn throw_all_from_a_creative_crafting_result_stops() -> TestResult {
    let outcome = within_deadline(|| {
        let player = RecordingPlayer::new(true);
        let mut handler = table_with_logs(&player, 1);
        handler.on_slot_click(0, 1, SlotActionType::Throw, &player);
        outcome(&handler, &player, 0, 1)
    })?;
    assert_eq!(
        outcome,
        "result empty cell empty cursor empty log drop 63x4 true;"
    );
    Ok(())
}

#[test]
fn throw_all_from_a_result_that_gives_nothing_stops() -> TestResult {
    let outcome = within_deadline(|| {
        let player = RecordingPlayer::new(false);
        let mut handler = SmithingTableScreenHandler::new(0, &player.inventory, None);
        handler.get_behaviour().slots[3]
            .set_stack_no_callbacks(ItemStack::new(1, &Item::DIAMOND_SWORD));
        handler.on_slot_click(3, 1, SlotActionType::Throw, &player);
        outcome(&handler, &player, 3, 0)
    })?;
    // The test placed the sword without a recipe: it is taken once, then the slot gives nothing.
    assert_eq!(
        outcome,
        "result empty cell empty cursor empty log drop 1050x1 true;"
    );
    Ok(())
}

#[test]
fn picking_up_a_crafting_result_uses_the_ingredients_once() -> TestResult {
    let outcome = within_deadline(|| {
        let player = RecordingPlayer::new(false);
        let mut handler = table_with_logs(&player, 1);
        handler.on_slot_click(0, 0, SlotActionType::Pickup, &player);
        handler.send_content_updates();
        handler.on_slot_click(0, 0, SlotActionType::Pickup, &player);
        handler.send_content_updates();
        outcome(&handler, &player, 0, 1)
    })?;
    assert_eq!(outcome, "result empty cell empty cursor oak_planks x4 log ");
    Ok(())
}
