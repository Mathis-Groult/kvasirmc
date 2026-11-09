//! A record of what `ScreenHandler::on_slot_click` does, made before `internal_on_slot_click` was
//! split into one function per click action (`tests/golden/slot_clicks.txt`; rewrite it on
//! purpose with `KVASIR_BLESS=1`).
//!
//! Four scenarios (a 9x3 container and the player's own screen, each with a survival and a
//! creative player) start from a filled inventory and get thousands of clicks from a fixed
//! pseudo-random sequence: every action type, slots outside the window, below zero and in range,
//! drags in all three stages, bundles, armour. After every click the contents of every slot, the
//! cursor, what was dropped and what equipment changes were announced go into a running hash; a
//! line is written every 250 clicks, so a difference shows where it started.

use std::any::Any;
use std::fmt::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex};

use pumpkin_data::data_component_impl::{BundleContentsImpl, EquipmentSlot};
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::sound::Sound;
use pumpkin_data::statistic::StatisticCategory;
use pumpkin_protocol::java::client::play::{
    CSetContainerContent, CSetContainerProperty, CSetContainerSlot, CSetCursorItem,
    CSetPlayerInventory, CSetSelectedSlot,
};
use pumpkin_protocol::java::server::play::SlotActionType;

use crate::entity_equipment::EntityEquipment;
use crate::generic_container_screen_handler::create_generic_9x3;
use crate::inventory::{Inventory, SimpleInventory};
use crate::player::player_inventory::PlayerInventory;
use crate::player::player_screen_handler::PlayerScreenHandler;
use crate::screen_handler::{InventoryPlayer, ScreenHandler};

/// A player that remembers what the handler asked it to do.
struct RecordingPlayer {
    creative: bool,
    inventory: Arc<PlayerInventory>,
    log: Mutex<String>,
    level: AtomicI32,
}

impl RecordingPlayer {
    fn new(creative: bool) -> Self {
        Self {
            creative,
            inventory: Arc::new(PlayerInventory::new(
                Arc::new(Mutex::new(EntityEquipment::new())),
                Arc::new(rustc_hash::FxHashMap::default()),
            )),
            log: Mutex::new(String::new()),
            level: AtomicI32::new(0),
        }
    }

    fn note(&self, text: &str) {
        if let Ok(mut log) = self.log.lock() {
            log.push_str(text);
            log.push(';');
        }
    }

    /// What the player has been asked to do since the last call.
    fn take_log(&self) -> String {
        self.log
            .lock()
            .map(|mut log| std::mem::take(&mut *log))
            .unwrap_or_default()
    }
}

impl InventoryPlayer for RecordingPlayer {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn drop_item(&self, item: ItemStack, retain_ownership: bool) {
        self.note(&format!("drop {} {retain_ownership}", stack_text(&item)));
    }

    fn is_creative(&self) -> bool {
        self.creative
    }

    fn has_infinite_materials(&self) -> bool {
        self.creative
    }

    fn experience_level(&self) -> i32 {
        self.level.load(Ordering::Relaxed)
    }

    fn add_experience_levels(&self, levels: i32) {
        self.level.fetch_add(levels, Ordering::Relaxed);
    }

    fn enchantment_seed(&self) -> i32 {
        0
    }

    fn set_enchantment_seed(&self, _seed: i32) {}

    fn get_inventory(&self) -> Arc<PlayerInventory> {
        self.inventory.clone()
    }

    fn enqueue_inventory_packet(
        &self,
        _packet: &CSetContainerContent,
        _window_type: Option<pumpkin_data::screen::WindowType>,
    ) {
    }

    fn enqueue_slot_packet(
        &self,
        _packet: &CSetContainerSlot,
        _window_type: Option<pumpkin_data::screen::WindowType>,
        _total_slots: usize,
    ) {
    }

    fn enqueue_cursor_packet(&self, _packet: &CSetCursorItem) {}

    fn enqueue_property_packet(&self, _packet: &CSetContainerProperty) {}

    fn enqueue_slot_set_packet(&self, _packet: &CSetPlayerInventory) {}

    fn enqueue_set_held_item_packet(&self, _packet: &CSetSelectedSlot) {}

    fn enqueue_equipment_change(&self, slot: &EquipmentSlot, stack: &ItemStack) {
        self.note(&format!(
            "equip {} {}",
            slot.get_slot_index(),
            stack_text(stack)
        ));
    }

    fn award_experience(&self, _amount: i32) {}

    fn increment_stat(&self, _category: StatisticCategory, _stat_id: i32, _amount: i32) {}

    fn play_block_sound(&self, _sound: Sound, _pitch: f32) {}
}

/// A stack as `item_id x count`, with the contents of a bundle in brackets.
fn stack_text(stack: &ItemStack) -> String {
    let mut text = format!("{}x{}", stack.item.id, stack.item_count);
    if let Some(bundle) = stack.get_data_component::<BundleContentsImpl>() {
        text.push('[');
        for inner in &bundle.items {
            text.push_str(&stack_text(inner));
            text.push(',');
        }
        text.push(']');
    }
    text
}

/// FNV-1a over a running state: a hash that does not change with the Rust version.
struct Fnv(u64);

impl Fnv {
    fn text(&mut self, text: &str) {
        for byte in text.bytes() {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
}

/// A fixed pseudo-random sequence (64-bit linear congruential generator).
struct Sequence(u64);

impl Sequence {
    fn next(&mut self, below: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % below
    }
}

/// The stack that goes into slot number `n` of a freshly filled window.
fn filler(sequence: &mut Sequence) -> ItemStack {
    let count = u8::try_from(sequence.next(64) + 1).unwrap_or(1);
    match sequence.next(14) {
        0..=4 => ItemStack::new(count, &Item::STONE),
        5 => ItemStack::new(count.min(40), &Item::DIRT),
        6 => ItemStack::new(1, &Item::DIAMOND_SWORD),
        7 => ItemStack::new(1, &Item::IRON_HELMET),
        8 => ItemStack::new(1, &Item::DIAMOND_CHESTPLATE),
        9 => ItemStack::new(count.min(64), &Item::APPLE),
        10 => ItemStack::new(count.min(16), &Item::ENDER_PEARL),
        11 => ItemStack::new(count, &Item::OAK_PLANKS),
        12 => {
            let mut bundle = ItemStack::new(1, &Item::BUNDLE);
            let mut items = Vec::new();
            for _ in 0..sequence.next(3) {
                items.push(ItemStack::new(count.min(20), &Item::STONE));
            }
            bundle.set_data_component(BundleContentsImpl { items });
            bundle
        }
        _ => ItemStack::EMPTY.clone(),
    }
}

/// One click of the sequence: slot, button and action.
fn next_click(sequence: &mut Sequence, slots: usize) -> Vec<(i32, i32, SlotActionType)> {
    let slot = match sequence.next(25) {
        0 | 1 => -999,
        2 => -1,
        _ => i32::try_from(sequence.next(slots as u64)).unwrap_or(0),
    };
    let button = i32::try_from(sequence.next(2)).unwrap_or(0);
    match sequence.next(100) {
        0..=29 => vec![(slot, button, SlotActionType::Pickup)],
        30..=41 => vec![(slot, button, SlotActionType::QuickMove)],
        42..=51 => {
            let hotbar = match sequence.next(10) {
                9 => 40,
                n => i32::try_from(n).unwrap_or(0),
            };
            vec![(slot, hotbar, SlotActionType::Swap)]
        }
        52..=59 => vec![(slot, button, SlotActionType::Throw)],
        60..=64 => vec![(slot, 2, SlotActionType::Clone)],
        65..=71 => vec![(slot, 0, SlotActionType::PickupAll)],
        // A double click: pick a stack up, then gather every equal stack onto the cursor.
        96..=99 => vec![
            (slot, 0, SlotActionType::Pickup),
            (slot, 0, SlotActionType::PickupAll),
        ],
        72..=77 => vec![(slot, 7, SlotActionType::PickupAll)],
        78..=79 => vec![(slot, 3, SlotActionType::Pickup)],
        _ => drag(sequence, slots),
    }
}

/// A drag: start, a few slots, end. The mouse button is 0 (spread), 1 (one each) or 2 (creative).
fn drag(sequence: &mut Sequence, slots: usize) -> Vec<(i32, i32, SlotActionType)> {
    let mouse = i32::try_from(sequence.next(3)).unwrap_or(0);
    let mut clicks = vec![(-999, mouse << 2, SlotActionType::QuickCraft)];
    for _ in 0..=sequence.next(4) {
        let slot = i32::try_from(sequence.next(slots as u64)).unwrap_or(0);
        clicks.push((slot, (mouse << 2) | 1, SlotActionType::QuickCraft));
    }
    if sequence.next(12) == 0 {
        clicks.push((-1, (mouse << 2) | 1, SlotActionType::QuickCraft));
    }
    clicks.push((-999, (mouse << 2) | 2, SlotActionType::QuickCraft));
    clicks
}

/// Everything a click can change, as one line of text.
fn state_text(handler: &dyn ScreenHandler, player: &RecordingPlayer) -> String {
    let behaviour = handler.get_behaviour();
    let mut text = String::new();
    for slot in &behaviour.slots {
        text.push_str(&stack_text(&slot.get_cloned_stack()));
        text.push(' ');
    }
    let cursor = behaviour
        .cursor_stack
        .lock()
        .map(|stack| stack_text(&stack))
        .unwrap_or_default();
    let _ = write!(text, "| {cursor} | {}", player.take_log());
    for index in 0..player.inventory.size() {
        let _ = write!(text, " {}", stack_text(&player.inventory.get_stack(index)));
    }
    text
}

/// Runs the clicks of one scenario and returns its lines.
fn scenario(name: &str, creative: bool, player_window: bool, seed: u64) -> String {
    let mut sequence = Sequence(seed);
    let player = RecordingPlayer::new(creative);
    for index in 0..player.inventory.size() {
        player.inventory.set_stack(index, filler(&mut sequence));
    }
    let container = Arc::new(SimpleInventory::new(27));
    for index in 0..27 {
        container.set_stack(index, filler(&mut sequence));
    }
    let mut handler: Box<dyn ScreenHandler> = if player_window {
        Box::new(PlayerScreenHandler::new(&player.inventory, None, 0, None))
    } else {
        Box::new(create_generic_9x3(1, &player.inventory, container, &player))
    };
    let slots = handler.get_behaviour().slots.len();

    let mut hash = Fnv(0xcbf2_9ce4_8422_2325);
    let mut out = String::new();
    let mut panics = 0;
    for step in 1..=4000 {
        for (slot, button, action) in next_click(&mut sequence, slots) {
            let label = format!("{slot} {button} {action:?}");
            let clicked = catch_unwind(AssertUnwindSafe(|| {
                handler.on_slot_click(slot, button, action, &player);
            }));
            if clicked.is_err() {
                panics += 1;
                hash.text("panic");
            }
            hash.text(&label);
            hash.text(&state_text(handler.as_ref(), &player));
        }
        if step % 250 == 0 {
            let _ = writeln!(
                out,
                "{name} step={step} hash={:016x} panics={panics}",
                hash.0
            );
        }
    }
    out
}

fn record() -> String {
    let mut out = String::new();
    out.push_str(&scenario("container survival", false, false, 11));
    out.push_str(&scenario("container creative", true, false, 12));
    out.push_str(&scenario("player screen survival", false, true, 13));
    out.push_str(&scenario("player screen creative", true, true, 14));
    out
}

#[test]
fn slot_clicks_do_what_they_did() -> Result<(), Box<dyn std::error::Error>> {
    let now = record();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("slot_clicks.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, now);
    Ok(())
}
