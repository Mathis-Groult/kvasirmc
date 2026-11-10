//! A record of what `Player::on_slot_click` does, made before the function was split
//! (`tests/golden/slot_click_packets.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Most of the function fires plugin events, so the test registers a recording handler for
//! each of the ten events it can fire; a seeded schedule makes one of them cancel now and then.
//! A real player (over an in-memory connection whose outgoing queue the test reads) gets 400
//! seeded click packets in each of ten screens: its own inventory in survival and creative, in
//! spectator, a crafting table, a smithing table, a furnace, a blast furnace, a smoker, a
//! grindstone and a chest. Packets carry a wrong sync id now and then, a stale revision, slots
//! -999, -1, inside and past the end, every mode with its buttons, and changed-slot hashes; the
//! special result slots are refilled (the crafting grids get logs, so planks show as the result) before a quarter of the clicks, which then aim at them, and
//! a quarter of the others aim at them too, so that the craft, smithing and furnace events have an item to report. After each click the event log, every slot and the cursor,
//! the player's inventory and the bytes of the queued packets go into a running hash; a line is
//! written every 50 clicks, and per screen the count of each event fired and cancelled.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::screen::WindowType;
use pumpkin_inventory::crafting::crafting_screen_handler::CraftingTableScreenHandler;
use pumpkin_inventory::furnace_like::furnace_like_screen_handler::FurnaceLikeScreenHandler;
use pumpkin_inventory::generic_container_screen_handler::create_generic_9x3;
use pumpkin_inventory::grindstone_screen_handler::GrindstoneScreenHandler;
use pumpkin_inventory::inventory::SimpleInventory;
use pumpkin_inventory::screen_handler::ScreenHandler;
use pumpkin_inventory::smithing_table_screen_handler::SmithingTableScreenHandler;
use pumpkin_inventory::{ExperienceContainer, PropertyDelegate};
use pumpkin_protocol::codec::item_stack_seralizer::OptionalItemStackHash;
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::java::server::play::{SClickSlot, SlotActionType};
use pumpkin_util::GameMode;
use pumpkin_util::text::TextComponent;

use crate::entity::ai::test_support;
use crate::entity::player::Player;
use crate::plugin::api::events::inventory::craft_item::CraftItemEvent;
use crate::plugin::api::events::inventory::furnace_extract::FurnaceExtractEvent;
use crate::plugin::api::events::inventory::inventory_drag::InventoryDragEvent;
use crate::plugin::api::events::inventory::inventory_interact::InventoryInteractEvent;
use crate::plugin::api::events::inventory::prepare_grindstone::PrepareGrindstoneEvent;
use crate::plugin::api::events::inventory::prepare_inventory_result::PrepareInventoryResultEvent;
use crate::plugin::api::events::inventory::prepare_item_craft::PrepareItemCraftEvent;
use crate::plugin::api::events::inventory::prepare_smithing::PrepareSmithingEvent;
use crate::plugin::api::events::inventory::smith_item::SmithItemEvent;
use crate::plugin::api::events::{Cancellable, EventPriority, Payload};
use crate::plugin::player::inventory_interact::InventoryClickEvent;
use crate::plugin::{BoxFuture, EventHandler};
use crate::server::Server;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// FNV-1a: a hash that does not change with the Rust version.
fn fnv(hash: u64, bytes: &[u8]) -> u64 {
    let mut hash = hash;
    for &byte in bytes {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    hash
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

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.next(items.len() as u64) as usize]
    }
}

/// What the recorders saw, and which event the next click should cancel.
#[derive(Default)]
struct Journal {
    lines: Vec<String>,
    cancel: Option<&'static str>,
    fired: BTreeMap<&'static str, (u32, u32)>,
}

/// A blocking handler that writes each event into the journal and cancels it when told to.
struct Recorder<E> {
    name: &'static str,
    journal: Arc<Mutex<Journal>>,
    describe: fn(&E) -> String,
    cancel: fn(&mut E),
    _event: PhantomData<fn(E)>,
}

impl<E: Payload + 'static> EventHandler<E> for Recorder<E> {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut E,
    ) -> BoxFuture<'a, ()> {
        let mut journal = self
            .journal
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let cancel = journal.cancel == Some(self.name);
        journal.lines.push(format!(
            "{} {} {}",
            self.name,
            (self.describe)(event),
            cancel
        ));
        let counts = journal.fired.entry(self.name).or_default();
        counts.0 += 1;
        if cancel {
            counts.1 += 1;
            (self.cancel)(event);
        }
        Box::pin(async {})
    }
}

fn stack_text(stack: &ItemStack) -> String {
    format!("{} x{}", stack.item.registry_key, stack.item_count)
}

fn optional_stack(stack: Option<&ItemStack>) -> String {
    stack.map_or_else(|| "none".to_string(), stack_text)
}

fn record<E>(
    server: &Server,
    journal: &Arc<Mutex<Journal>>,
    name: &'static str,
    describe: fn(&E) -> String,
    cancel: fn(&mut E),
) where
    E: Payload + Send + Sync + 'static,
{
    server.plugin_manager.register::<E, _>(
        Arc::new(Recorder {
            name,
            journal: Arc::clone(journal),
            describe,
            cancel,
            _event: PhantomData,
        }),
        EventPriority::Normal,
        true,
    );
}

fn cancel<E: Cancellable>(event: &mut E) {
    event.set_cancelled(true);
}

/// Events that cannot be cancelled are only recorded.
const fn keep<E>(_event: &mut E) {}

/// The events a click can cancel.
const CANCELLABLE: [&str; 6] = [
    "click",
    "interact",
    "craft",
    "prepare_craft",
    "smith",
    "drag",
];

/// The ten events `on_slot_click` can fire.
const EVENTS: [&str; 10] = [
    "click",
    "interact",
    "craft",
    "prepare_craft",
    "smith",
    "prepare_smithing",
    "furnace_extract",
    "prepare_grindstone",
    "prepare_result",
    "drag",
];

fn register_recorders(server: &Server, journal: &Arc<Mutex<Journal>>) {
    record::<InventoryClickEvent>(
        server,
        journal,
        EVENTS[0],
        |e| {
            format!(
                "{:?} {:?} {} {} [{}] [{}] {}",
                e.window_type,
                e.click_type,
                e.slot,
                e.raw_slot,
                optional_stack(e.clicked_item.as_ref()),
                optional_stack(e.cursor.as_ref()),
                e.hotbar_button
            )
        },
        cancel::<InventoryClickEvent>,
    );
    record::<InventoryInteractEvent>(
        server,
        journal,
        EVENTS[1],
        |_| String::new(),
        cancel::<InventoryInteractEvent>,
    );
    record::<CraftItemEvent>(
        server,
        journal,
        EVENTS[2],
        |e| e.recipe_id.clone(),
        cancel::<CraftItemEvent>,
    );
    record::<PrepareItemCraftEvent>(
        server,
        journal,
        EVENTS[3],
        |e| e.recipe_id.clone(),
        cancel::<PrepareItemCraftEvent>,
    );
    record::<SmithItemEvent>(
        server,
        journal,
        EVENTS[4],
        |e| e.recipe_id.clone(),
        cancel::<SmithItemEvent>,
    );
    record::<PrepareSmithingEvent>(
        server,
        journal,
        EVENTS[5],
        |e| format!("{:?}", e.result_item),
        keep::<PrepareSmithingEvent>,
    );
    record::<FurnaceExtractEvent>(
        server,
        journal,
        EVENTS[6],
        |e| {
            format!(
                "{:?} {} {} {}",
                e.block_pos, e.item_id, e.item_amount, e.exp_gained
            )
        },
        keep::<FurnaceExtractEvent>,
    );
    record::<PrepareGrindstoneEvent>(
        server,
        journal,
        EVENTS[7],
        |e| format!("{:?}", e.result_item),
        keep::<PrepareGrindstoneEvent>,
    );
    record::<PrepareInventoryResultEvent>(
        server,
        journal,
        EVENTS[8],
        |e| format!("{:?}", e.result_item),
        keep::<PrepareInventoryResultEvent>,
    );
    record::<InventoryDragEvent>(
        server,
        journal,
        EVENTS[9],
        |_| String::new(),
        cancel::<InventoryDragEvent>,
    );
}

struct NoProperties;

impl PropertyDelegate for NoProperties {
    fn get_property(&self, _index: i32) -> i32 {
        0
    }
    fn set_property(&self, _index: i32, _value: i32) {}
    fn get_properties_size(&self) -> i32 {
        4
    }
}

struct NoExperience;

impl ExperienceContainer for NoExperience {
    fn extract_experience(&self) -> i32 {
        0
    }
}

const SCREENS: [&str; 10] = [
    "inventory",
    "inventory_creative",
    "spectator",
    "crafting",
    "smithing",
    "furnace",
    "blast_furnace",
    "smoker",
    "grindstone",
    "chest",
];

const ITEMS: [&Item; 10] = [
    &Item::STONE,
    &Item::OAK_PLANKS,
    &Item::IRON_INGOT,
    &Item::COAL,
    &Item::RAW_IRON,
    &Item::DIAMOND_SWORD,
    &Item::BUCKET,
    &Item::NETHERITE_UPGRADE_SMITHING_TEMPLATE,
    &Item::DIAMOND_CHESTPLATE,
    &Item::STICK,
];

fn random_stack(sequence: &mut Sequence) -> ItemStack {
    let item = sequence.pick(&ITEMS);
    let count = 1 + sequence.next(16) as u8;
    ItemStack::new(count.min(stack_limit(item)), item)
}

fn stack_limit(item: &'static Item) -> u8 {
    ItemStack::new(1, item).get_max_stack_size()
}

/// Opens screen `name` for the player (the inventory screens open nothing) and returns its
/// sync id and the slot the special events look at.
fn open(player: &Arc<Player>, name: &str) -> (u8, Option<i16>) {
    let inventory = &player.inventory;
    let handler: Arc<Mutex<dyn ScreenHandler>> = match name {
        "crafting" => Arc::new(Mutex::new(CraftingTableScreenHandler::new(
            1, inventory, None,
        ))),
        "smithing" => Arc::new(Mutex::new(SmithingTableScreenHandler::new(
            1, inventory, None,
        ))),
        "furnace" | "blast_furnace" | "smoker" => {
            let window_type = match name {
                "furnace" => WindowType::Furnace,
                "blast_furnace" => WindowType::BlastFurnace,
                _ => WindowType::Smoker,
            };
            let delegate: Arc<dyn PropertyDelegate> = Arc::new(NoProperties);
            Arc::new(Mutex::new(FurnaceLikeScreenHandler::new(
                1,
                inventory,
                Arc::new(SimpleInventory::new(3)),
                &delegate,
                Arc::new(NoExperience),
                window_type,
            )))
        }
        "grindstone" => Arc::new(Mutex::new(GrindstoneScreenHandler::new(
            1,
            inventory,
            Arc::new(SimpleInventory::new(3)),
        ))),
        "chest" => Arc::new(Mutex::new(create_generic_9x3(
            1,
            inventory,
            Arc::new(SimpleInventory::new(27)),
            player.as_ref(),
        ))),
        _ => return (0, Some(0)),
    };
    player.open_handled_screen_direct(handler, &TextComponent::text("golden"));
    let special = match name {
        "crafting" => Some(0),
        "smithing" => Some(3),
        "furnace" | "blast_furnace" | "smoker" | "grindstone" => Some(2),
        _ => None,
    };
    (1, special)
}

fn current_handler(player: &Player) -> Arc<Mutex<dyn ScreenHandler>> {
    player
        .current_screen_handler
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Every slot of the open screen and the cursor.
fn screen_text(player: &Player) -> String {
    let handler = current_handler(player);
    let handler = handler
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let behaviour = handler.get_behaviour();
    let mut text = format!(
        "sync {} rev {} ",
        behaviour.sync_id,
        behaviour
            .revision
            .load(std::sync::atomic::Ordering::Relaxed)
    );
    for slot in &behaviour.slots {
        text.push_str(&stack_text(&slot.get_cloned_stack()));
        text.push(';');
    }
    let cursor = behaviour
        .cursor_stack
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let _ = write!(text, " cursor {}", stack_text(&cursor));
    text
}

fn slot_count(player: &Player) -> i16 {
    let handler = current_handler(player);
    let handler = handler
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    handler.get_behaviour().slots.len() as i16
}

fn revision(player: &Player) -> i32 {
    let handler = current_handler(player);
    let handler = handler
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    handler
        .get_behaviour()
        .revision
        .load(std::sync::atomic::Ordering::Relaxed) as i32
}

/// Puts an item where the special events will see it: logs in the first crafting cell (the
/// result slot shows planks), anything else straight into the result slot.
fn refill(player: &Player, name: &str, slot: i16, sequence: &mut Sequence) {
    let handler = current_handler(player);
    let handler = handler
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let slots = &handler.get_behaviour().slots;
    if matches!(name, "inventory" | "inventory_creative" | "crafting") {
        let logs = ItemStack::new(1 + sequence.next(8) as u8, &Item::OAK_LOG);
        if let Some(cell) = slots.get(1) {
            cell.set_stack(logs);
        }
    } else if let Some(target) = slots.get(slot as usize) {
        target.set_stack_no_callbacks(random_stack(sequence));
    }
}

const MODES: [SlotActionType; 7] = [
    SlotActionType::Pickup,
    SlotActionType::QuickMove,
    SlotActionType::Swap,
    SlotActionType::Clone,
    SlotActionType::Throw,
    SlotActionType::QuickCraft,
    SlotActionType::PickupAll,
];

fn button_for(mode: &SlotActionType, sequence: &mut Sequence) -> i8 {
    if sequence.next(20) == 0 {
        return sequence.next(14) as i8 - 1;
    }
    match mode {
        SlotActionType::Pickup | SlotActionType::QuickMove | SlotActionType::Throw => {
            sequence.next(2) as i8
        }
        SlotActionType::Swap => sequence.pick(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 40]),
        SlotActionType::Clone => 2,
        SlotActionType::QuickCraft => sequence.pick(&[0, 1, 2, 4, 5, 6, 8, 9, 10]),
        SlotActionType::PickupAll => 0,
    }
}

/// A seeded click packet for the open screen.
fn packet(
    player: &Player,
    sync_id: u8,
    special: Option<i16>,
    sequence: &mut Sequence,
) -> SClickSlot {
    let slots = slot_count(player);
    let slot = match sequence.next(20) {
        _ if special.is_some() && sequence.next(4) == 0 => special.unwrap_or_default(),
        0 | 1 => -999,
        2 => -1,
        3..=5 => slots + sequence.next(3) as i16,
        _ => sequence.next(slots as u64) as i16,
    };
    let mode = MODES[sequence.next(7) as usize].clone();
    let button = button_for(&mode, sequence);
    let sync = if sequence.next(10) == 0 {
        7
    } else {
        i32::from(sync_id)
    };
    let revision = if sequence.next(2) == 0 {
        revision(player)
    } else {
        sequence.next(5) as i32
    };
    let changed = if sequence.next(5) == 0 {
        vec![(
            sequence.next(slots as u64) as i16,
            OptionalItemStackHash(None),
        )]
    } else {
        Vec::new()
    };
    SClickSlot {
        sync_id: VarInt(sync),
        revision: VarInt(revision),
        slot,
        button,
        mode,
        length_of_array: VarInt(changed.len() as i32),
        array_of_changed_slots: changed,
        carried_item: OptionalItemStackHash(None),
    }
}

fn fill_inventory(player: &Player, sequence: &mut Sequence) {
    let mut main = player
        .inventory
        .main_inventory
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for slot in main.iter_mut() {
        *slot = if sequence.next(3) == 0 {
            ItemStack::EMPTY.clone()
        } else {
            random_stack(sequence)
        };
    }
}

fn run_screen(
    server: &Arc<Server>,
    journal: &Arc<Mutex<Journal>>,
    name: &'static str,
    seed: u64,
) -> String {
    let world = server.worlds.load().first().cloned();
    let Some(world) = world else {
        return "no world\n".to_string();
    };
    let gamemode = match name {
        "inventory_creative" => GameMode::Creative,
        "spectator" => GameMode::Spectator,
        _ => GameMode::Survival,
    };
    let Some((player, mut tap)) = test_support::java_player_with_tap(&world, gamemode) else {
        return "no packet tap\n".to_string();
    };
    let mut sequence = Sequence(seed);
    fill_inventory(&player, &mut sequence);
    let (sync_id, special) = open(&player, name);
    tap.drain();
    journal
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .fired
        .clear();

    let mut report = String::new();
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut packets = 0;
    for click in 1..=400 {
        let refilled = special.filter(|_| sequence.next(4) == 0);
        if let Some(slot) = refilled {
            refill(&player, name, slot, &mut sequence);
        }
        let cancel = (sequence.next(4) == 0).then(|| sequence.pick(&CANCELLABLE));
        let mut armed = journal
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        armed.cancel = cancel;
        armed.lines.clear();
        drop(armed);

        let mut packet = packet(&player, sync_id, special, &mut sequence);
        if let Some(slot) = refilled {
            packet.slot = slot;
        }
        if Some(packet.slot) == special
            && packet.mode == SlotActionType::Throw
            && packet.button == 1
        {
            // Throwing the whole stack from a result slot loops forever when taking does not
            // change the slot (`slot_clicks::throw`): a creative player's crafting result, a
            // smithing output without a recipe. A finding (card #22), kept out of this record.
            packet.mode = SlotActionType::Pickup;
        }
        player.on_slot_click(packet, server);
        let lines = {
            let journal = journal
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            journal.lines.join("|")
        };
        hash = fnv(hash, lines.as_bytes());
        hash = fnv(hash, screen_text(&player).as_bytes());
        for sent in tap.drain() {
            hash = fnv(hash, &sent);
            packets += 1;
        }
        if click % 50 == 0 {
            let _ = writeln!(
                report,
                "{name} clicks {}-{click}: {hash:016x} packets {packets}",
                click - 49
            );
        }
    }
    let journal = journal
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = writeln!(
        report,
        "{name} events (fired, cancelled): {:?}",
        journal.fired
    );
    report
}

#[tokio::test(flavor = "multi_thread")]
async fn slot_click_packets() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let journal = Arc::new(Mutex::new(Journal::default()));
    register_recorders(&server, &journal);
    let mut now = String::new();
    for (index, name) in SCREENS.iter().enumerate() {
        now.push_str(&run_screen(&server, &journal, name, 0x5eed + index as u64));
    }
    server.stop();

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("slot_click_packets.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, now);
    Ok(())
}
