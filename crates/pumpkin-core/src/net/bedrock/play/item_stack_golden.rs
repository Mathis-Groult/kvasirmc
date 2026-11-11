//! A record of what `BedrockClient::handle_item_stack_request` does, made before the function
//! was split (`tests/golden/item_stack_requests.txt`; rewrite it on purpose with
//! `KVASIR_BLESS=1`).
//!
//! One real Bedrock player (see `test_support::bedrock_player_with_tap`) per scene gets 300
//! seeded requests in one screen: its own inventory (survival and creative), a crafting table,
//! a stonecutter, a merchant with three offers, an enchanting table (survival and creative), a
//! beacon, an anvil (survival and creative), a grindstone, a loom, a smithing table, a
//! cartography table, a chest, a furnace and a brewing stand. Half the requests follow the
//! shape a Bedrock client sends for that screen (craft, consume the inputs, take the created
//! output), the other half are 1-3 actions drawn from every action kind with every container
//! name, slot ids inside and past the ends and counts up to 255. The screen's inputs are
//! refilled with real recipe items before half of the requests.
//!
//! After each request the queued `ItemStackResponse` and `InventoryContent` packets are
//! decoded and their stack net ids renamed by first appearance (net ids come from a global
//! counter, so the raw bytes differ between runs); other packets count by id. Those texts,
//! every slot and the cursor (as NBT), the number of entities in the world (drops) and the
//! beacon's effects go into a running hash; a line is written every 50 requests, and per
//! scene the count of each action kind in requests that succeeded and failed. A request that
//! panics is recorded as `panic`. `KVASIR_DUMP=<file>` appends each request's decoded packets and
//! state to the file (to find what differs between two runs).

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use pumpkin_data::item::{Item, JavaToBedrockItemMapping};
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::screen::WindowType;
use pumpkin_inventory::anvil::anvil_screen_handler::AnvilScreenHandler;
use pumpkin_inventory::beacon_screen_handler::create_beacon_handler;
use pumpkin_inventory::brewing::brewing_screen_handler::BrewingScreenHandler;
use pumpkin_inventory::cartography_table_screen_handler::CartographyTableScreenHandler;
use pumpkin_inventory::crafting::crafting_screen_handler::CraftingTableScreenHandler;
use pumpkin_inventory::enchanting::enchanting_screen_handler::EnchantingTableScreenHandler;
use pumpkin_inventory::furnace_like::furnace_like_screen_handler::FurnaceLikeScreenHandler;
use pumpkin_inventory::generic_container_screen_handler::create_generic_9x3;
use pumpkin_inventory::grindstone_screen_handler::GrindstoneScreenHandler;
use pumpkin_inventory::inventory::{Inventory, SimpleInventory};
use pumpkin_inventory::loom_screen_handler::LoomScreenHandler;
use pumpkin_inventory::merchant::merchant_screen_handler::MerchantScreenHandler;
use pumpkin_inventory::screen_handler::ScreenHandler;
use pumpkin_inventory::smithing_table_screen_handler::SmithingTableScreenHandler;
use pumpkin_inventory::stonecutter_screen_handler::StonecutterScreenHandler;
use pumpkin_inventory::{ExperienceContainer, PropertyDelegate};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_nbt::tag::NbtTag;
use pumpkin_protocol::bedrock::client::BedrockRecipe;
use pumpkin_protocol::bedrock::network_item::{ContainerName, FullContainerName};
use pumpkin_protocol::bedrock::server::item_stack_request::{
    ItemStackRequest, ItemStackRequestAction, ItemStackRequestSlotInfo, SItemStackRequest,
};
use pumpkin_protocol::codec::item_stack_seralizer::ItemStackSerializer;
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::codec::var_uint::VarUInt;
use pumpkin_protocol::java::client::play::MerchantOffer;
use pumpkin_util::GameMode;
use pumpkin_util::text::TextComponent;

use crate::block::entities::beacon::BeaconBlockEntity;
use crate::entity::ai::test_support;
use crate::entity::player::Player;
use crate::net::ClientPlatform;
use crate::server::Server;

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Action = ItemStackRequestAction;

const REQUESTS: u32 = 300;

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

    fn chance(&mut self, one_in: u64) -> bool {
        self.next(one_in) == 0
    }
}

// ---------------------------------------------------------------------------------------
// Scenes

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Inventory,
    Crafting,
    Stonecutter,
    Merchant,
    Enchanting,
    Beacon,
    Anvil,
    Grindstone,
    Loom,
    Smithing,
    Cartography,
    Chest,
    Furnace,
    Brewing,
}

struct Scene {
    name: &'static str,
    screen: Screen,
    gamemode: GameMode,
}

const fn scene(name: &'static str, screen: Screen, gamemode: GameMode) -> Scene {
    Scene {
        name,
        screen,
        gamemode,
    }
}

const SCENES: [Scene; 17] = [
    scene("inventory", Screen::Inventory, GameMode::Survival),
    scene("inventory_creative", Screen::Inventory, GameMode::Creative),
    scene("crafting", Screen::Crafting, GameMode::Survival),
    scene("stonecutter", Screen::Stonecutter, GameMode::Survival),
    scene("merchant", Screen::Merchant, GameMode::Survival),
    scene("enchanting", Screen::Enchanting, GameMode::Survival),
    scene(
        "enchanting_creative",
        Screen::Enchanting,
        GameMode::Creative,
    ),
    scene("beacon", Screen::Beacon, GameMode::Survival),
    scene("anvil", Screen::Anvil, GameMode::Survival),
    scene("anvil_creative", Screen::Anvil, GameMode::Creative),
    scene("grindstone", Screen::Grindstone, GameMode::Survival),
    scene("loom", Screen::Loom, GameMode::Survival),
    scene("smithing", Screen::Smithing, GameMode::Survival),
    scene("cartography", Screen::Cartography, GameMode::Survival),
    scene("chest", Screen::Chest, GameMode::Survival),
    scene("furnace", Screen::Furnace, GameMode::Survival),
    scene("brewing", Screen::Brewing, GameMode::Survival),
];

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

fn offer(
    cost: (&'static Item, u8),
    cost_b: Option<(&'static Item, u8)>,
    output: &'static Item,
    uses: (i32, i32),
) -> MerchantOffer {
    MerchantOffer {
        base_cost_a: ItemStackSerializer(Cow::Owned(ItemStack::new(cost.1, cost.0))),
        output: ItemStackSerializer(Cow::Owned(ItemStack::new(1, output))),
        cost_b: cost_b
            .map(|(item, count)| ItemStackSerializer(Cow::Owned(ItemStack::new(count, item)))),
        reward_exp: true,
        uses: uses.0,
        max_uses: uses.1,
        xp: 2,
        special_price: 0,
        price_multiplier: 0.05,
        demand: 0,
    }
}

fn merchant_offers() -> Vec<MerchantOffer> {
    vec![
        offer((&Item::EMERALD, 9), None, &Item::BOOKSHELF, (0, 12)),
        offer(
            (&Item::EMERALD, 5),
            Some((&Item::BOOK, 1)),
            &Item::LANTERN,
            (0, 4),
        ),
        offer((&Item::EMERALD, 1), None, &Item::BREAD, (3, 3)),
    ]
}

/// Opens the scene's screen (the inventory scenes open nothing); returns the beacon when the
/// scene has one.
fn open(player: &Arc<Player>, server: &Server, screen: Screen) -> Option<Arc<BeaconBlockEntity>> {
    let inventory = &player.inventory;
    let recipes = server.recipe_manager.clone();
    let mut beacon = None;
    let handler: Arc<Mutex<dyn ScreenHandler>> = match screen {
        Screen::Inventory => return None,
        Screen::Crafting => Arc::new(Mutex::new(CraftingTableScreenHandler::new(
            1,
            inventory,
            Some(recipes),
        ))),
        Screen::Stonecutter => Arc::new(Mutex::new(StonecutterScreenHandler::new(
            1,
            inventory,
            Some(recipes),
        ))),
        Screen::Merchant => Arc::new(Mutex::new(MerchantScreenHandler::new(
            1,
            inventory,
            Arc::new(SimpleInventory::new(3)),
            merchant_offers(),
        ))),
        Screen::Enchanting => {
            let table: Arc<dyn Inventory> = Arc::new(SimpleInventory::new(2));
            Arc::new(Mutex::new(EnchantingTableScreenHandler::new(
                1, inventory, &table, 0x5eed, 15,
            )))
        }
        Screen::Beacon => {
            let entity = Arc::new(BeaconBlockEntity::new(
                pumpkin_util::math::position::BlockPos::new(0, 64, 0),
            ));
            entity.levels.store(4, Ordering::Relaxed);
            beacon = Some(Arc::clone(&entity));
            Arc::new(Mutex::new(create_beacon_handler(1, inventory, entity)))
        }
        Screen::Anvil => Arc::new(Mutex::new(AnvilScreenHandler::new(
            1,
            inventory,
            Arc::new(SimpleInventory::new(3)),
        ))),
        Screen::Grindstone => Arc::new(Mutex::new(GrindstoneScreenHandler::new(
            1,
            inventory,
            Arc::new(SimpleInventory::new(3)),
        ))),
        Screen::Loom => Arc::new(Mutex::new(LoomScreenHandler::new(1, inventory))),
        Screen::Smithing => Arc::new(Mutex::new(SmithingTableScreenHandler::new(
            1,
            inventory,
            Some(recipes),
        ))),
        Screen::Cartography => {
            Arc::new(Mutex::new(CartographyTableScreenHandler::new(1, inventory)))
        }
        Screen::Chest => Arc::new(Mutex::new(create_generic_9x3(
            1,
            inventory,
            Arc::new(SimpleInventory::new(27)),
            player.as_ref(),
        ))),
        Screen::Furnace => {
            let delegate: Arc<dyn PropertyDelegate> = Arc::new(NoProperties);
            Arc::new(Mutex::new(FurnaceLikeScreenHandler::new(
                1,
                inventory,
                Arc::new(SimpleInventory::new(3)),
                &delegate,
                Arc::new(NoExperience),
                WindowType::Furnace,
            )))
        }
        Screen::Brewing => {
            let delegate: Arc<dyn PropertyDelegate> = Arc::new(NoProperties);
            Arc::new(Mutex::new(BrewingScreenHandler::new(
                1,
                inventory,
                Arc::new(SimpleInventory::new(5)),
                &delegate,
            )))
        }
    };
    player.open_handled_screen_direct(handler, &TextComponent::text("golden"));
    beacon
}

fn current_handler(player: &Player) -> Arc<Mutex<dyn ScreenHandler>> {
    player
        .current_screen_handler
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

// ---------------------------------------------------------------------------------------
// Items and refills

const ITEMS: [&Item; 18] = [
    &Item::STONE,
    &Item::OAK_LOG,
    &Item::OAK_PLANKS,
    &Item::STICK,
    &Item::EMERALD,
    &Item::LAPIS_LAZULI,
    &Item::DIAMOND_SWORD,
    &Item::DIAMOND,
    &Item::NETHERITE_INGOT,
    &Item::NETHERITE_UPGRADE_SMITHING_TEMPLATE,
    &Item::WHITE_BANNER,
    &Item::RED_DYE,
    &Item::PAPER,
    &Item::MAP,
    &Item::DIAMOND_CHESTPLATE,
    &Item::IRON_INGOT,
    &Item::BOOK,
    &Item::COBBLESTONE,
];

fn stack(count: u64, item: &'static Item) -> ItemStack {
    if count == 0 {
        return ItemStack::EMPTY.clone();
    }
    let limit = u64::from(ItemStack::new(1, item).get_max_stack_size());
    ItemStack::new(count.min(limit) as u8, item)
}

fn random_stack(sequence: &mut Sequence) -> ItemStack {
    let item = sequence.pick(&ITEMS);
    let count = 1 + sequence.next(64);
    stack(count, item)
}

fn fill_inventory(player: &Player, sequence: &mut Sequence) {
    let mut main = player
        .inventory
        .main_inventory
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for slot in main.iter_mut() {
        *slot = if sequence.chance(3) {
            ItemStack::EMPTY.clone()
        } else {
            random_stack(sequence)
        };
    }
}

/// Puts `stacks` into the first screen slots (`None` leaves a slot as it is).
fn set_slots(player: &Player, stacks: &[Option<ItemStack>]) {
    let handler = current_handler(player);
    let handler = handler
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let slots = &handler.get_behaviour().slots;
    for (index, stack) in stacks.iter().enumerate() {
        if let (Some(stack), Some(slot)) = (stack, slots.get(index)) {
            slot.set_stack(stack.clone());
        }
    }
}

/// Logs in the first cell, or planks in the first cell and the one below it (sticks); `below`
/// is 3 in the 2x2 grid and 4 in a crafting table.
fn grid_refill(sequence: &mut Sequence, below: usize) -> Vec<Option<ItemStack>> {
    let mut cells = vec![None; below + 1];
    if sequence.chance(2) {
        cells[1] = Some(stack(1 + sequence.next(8), &Item::OAK_LOG));
        cells[below] = Some(ItemStack::EMPTY.clone());
    } else {
        let planks = stack(1 + sequence.next(8), &Item::OAK_PLANKS);
        cells[1] = Some(planks.clone());
        cells[below] = Some(planks);
    }
    cells
}

fn merchant_refill(player: &Player, sequence: &mut Sequence) -> Vec<Option<ItemStack>> {
    let mut main = player
        .inventory
        .main_inventory
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let emeralds = sequence.next(36) as usize;
    main[emeralds] = stack(1 + sequence.next(64), &Item::EMERALD);
    let book = sequence.next(36) as usize;
    main[book] = stack(1 + sequence.next(3), &Item::BOOK);
    drop(main);
    if sequence.chance(3) {
        vec![
            Some(stack(sequence.next(16), &Item::EMERALD)),
            Some(stack(sequence.next(2), &Item::BOOK)),
        ]
    } else {
        Vec::new()
    }
}

fn pick_stack(sequence: &mut Sequence, choices: &[(&'static Item, u64)]) -> ItemStack {
    let (item, most) = sequence.pick(choices);
    stack(sequence.next(most + 1), item)
}

/// The two or three inputs of an anvil, grindstone, loom, smithing or cartography table.
fn workstation_refill(screen: Screen, sequence: &mut Sequence) -> Vec<Option<ItemStack>> {
    match screen {
        Screen::Anvil => vec![
            Some(pick_stack(
                sequence,
                &[(&Item::DIAMOND_SWORD, 1), (&Item::STONE, 2)],
            )),
            Some(pick_stack(
                sequence,
                &[
                    (&Item::DIAMOND, 3),
                    (&Item::DIAMOND_SWORD, 1),
                    (&Item::BOOK, 1),
                ],
            )),
        ],
        Screen::Grindstone => vec![
            Some(pick_stack(
                sequence,
                &[(&Item::DIAMOND_SWORD, 1), (&Item::STONE, 1)],
            )),
            Some(pick_stack(
                sequence,
                &[(&Item::DIAMOND_SWORD, 1), (&Item::BOOK, 1)],
            )),
        ],
        Screen::Loom => vec![
            Some(pick_stack(
                sequence,
                &[(&Item::WHITE_BANNER, 2), (&Item::STONE, 1)],
            )),
            Some(pick_stack(
                sequence,
                &[(&Item::RED_DYE, 4), (&Item::PAPER, 1)],
            )),
            Some(ItemStack::EMPTY.clone()),
        ],
        Screen::Smithing => vec![
            Some(pick_stack(
                sequence,
                &[(&Item::NETHERITE_UPGRADE_SMITHING_TEMPLATE, 2)],
            )),
            Some(pick_stack(
                sequence,
                &[
                    (&Item::DIAMOND_SWORD, 1),
                    (&Item::DIAMOND_CHESTPLATE, 1),
                    (&Item::STONE, 1),
                ],
            )),
            Some(pick_stack(sequence, &[(&Item::NETHERITE_INGOT, 3)])),
        ],
        Screen::Cartography => vec![
            Some(pick_stack(
                sequence,
                &[(&Item::FILLED_MAP, 1), (&Item::MAP, 2)],
            )),
            Some(pick_stack(sequence, &[(&Item::PAPER, 2), (&Item::MAP, 1)])),
        ],
        _ => Vec::new(),
    }
}

fn refill(
    player: &Player,
    screen: Screen,
    beacon: Option<&Arc<BeaconBlockEntity>>,
    sequence: &mut Sequence,
) {
    let stacks = match screen {
        Screen::Inventory => grid_refill(sequence, 3),
        Screen::Crafting => grid_refill(sequence, 4),
        Screen::Stonecutter => vec![Some(pick_stack(
            sequence,
            &[
                (&Item::STONE, 64),
                (&Item::COBBLESTONE, 64),
                (&Item::OAK_LOG, 4),
            ],
        ))],
        Screen::Merchant => merchant_refill(player, sequence),
        Screen::Enchanting => vec![
            Some(pick_stack(
                sequence,
                &[
                    (&Item::DIAMOND_SWORD, 1),
                    (&Item::BOOK, 1),
                    (&Item::STONE, 3),
                ],
            )),
            Some(stack(sequence.next(12), &Item::LAPIS_LAZULI)),
        ],
        Screen::Beacon => {
            if let Some(beacon) = beacon {
                beacon
                    .levels
                    .store(sequence.next(5) as i32, Ordering::Relaxed);
            }
            vec![Some(pick_stack(
                sequence,
                &[
                    (&Item::IRON_INGOT, 3),
                    (&Item::DIAMOND, 2),
                    (&Item::STONE, 1),
                ],
            ))]
        }
        Screen::Anvil
        | Screen::Grindstone
        | Screen::Loom
        | Screen::Smithing
        | Screen::Cartography => workstation_refill(screen, sequence),
        Screen::Chest => {
            let mut cells = vec![None; 27];
            cells[sequence.next(27) as usize] = Some(random_stack(sequence));
            cells
        }
        Screen::Furnace => vec![
            Some(pick_stack(sequence, &[(&Item::RAW_IRON, 8)])),
            Some(pick_stack(sequence, &[(&Item::COAL, 4)])),
            Some(pick_stack(sequence, &[(&Item::IRON_INGOT, 8)])),
        ],
        Screen::Brewing => vec![
            Some(stack(1, &Item::GLASS_BOTTLE)),
            Some(stack(sequence.next(2), &Item::GLASS_BOTTLE)),
            Some(stack(1, &Item::GLASS_BOTTLE)),
            Some(pick_stack(sequence, &[(&Item::NETHER_WART, 2)])),
            Some(pick_stack(sequence, &[(&Item::BLAZE_POWDER, 2)])),
        ],
    };
    set_slots(player, &stacks);
}

/// The enchanting table takes a new random seed after each enchantment; the golden puts a
/// fixed one back before each request so that the offered enchantments repeat.
fn fix_enchanting_seed(player: &Arc<Player>) {
    let handler = current_handler(player);
    let mut handler = handler
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(table) = handler
        .as_any_mut()
        .downcast_mut::<EnchantingTableScreenHandler>()
    {
        table.enchantment_seed = 0x5eed;
        table.update_enchantments(player.as_ref());
    }
}

// ---------------------------------------------------------------------------------------
// Requests

fn slot(container: ContainerName, slot_id: u8) -> ItemStackRequestSlotInfo {
    ItemStackRequestSlotInfo {
        container_name: FullContainerName {
            container_name: container,
            dynamic_id: None,
        },
        slot_id,
        stack_id: 0,
    }
}

fn screen_containers(screen: Screen) -> &'static [ContainerName] {
    use ContainerName as C;
    match screen {
        Screen::Inventory | Screen::Crafting => &[C::CraftingInput, C::CraftingOutputPreview],
        Screen::Stonecutter => &[C::StonecutterInput, C::StonecutterResultPreview],
        Screen::Merchant => &[
            C::TradeIngredient1,
            C::TradeIngredient2,
            C::Trade2Ingredient1,
            C::Trade2Ingredient2,
            C::TradeResultPreview,
            C::Trade2ResultPreview,
        ],
        Screen::Enchanting => &[C::EnchantingInput, C::EnchantingMaterial],
        Screen::Beacon => &[C::BeaconPayment],
        Screen::Anvil => &[C::AnvilInput, C::AnvilMaterial, C::AnvilResultPreview],
        Screen::Grindstone => &[
            C::GrindstoneInput,
            C::GrindstoneAdditional,
            C::GrindstoneResultPreview,
        ],
        Screen::Loom => &[
            C::LoomInput,
            C::LoomDye,
            C::LoomMaterial,
            C::LoomResultPreview,
        ],
        Screen::Smithing => &[
            C::SmithingTableTemplate,
            C::SmithingTableInput,
            C::SmithingTableMaterial,
            C::SmithingTableResultPreview,
        ],
        Screen::Cartography => &[
            C::CartographyInput,
            C::CartographyAdditional,
            C::CartographyResultPreview,
        ],
        Screen::Chest => &[C::LevelEntity],
        Screen::Furnace => &[C::FurnaceIngredient, C::FurnaceFuel, C::FurnaceResult],
        Screen::Brewing => &[
            C::BrewingStandInput,
            C::BrewingStandFuel,
            C::BrewingStandResult,
        ],
    }
}

const COMMON: [ContainerName; 9] = [
    ContainerName::Cursor,
    ContainerName::HotBar,
    ContainerName::HotBar,
    ContainerName::Inventory,
    ContainerName::CombinedHotBarAndInventory,
    ContainerName::Armor,
    ContainerName::Offhand,
    ContainerName::CreatedOutput,
    ContainerName::CraftingInput,
];

const SLOT_IDS: [u8; 20] = [
    0, 0, 1, 2, 3, 4, 5, 8, 9, 10, 20, 27, 28, 31, 32, 35, 36, 40, 50, 200,
];

const COUNTS: [u8; 9] = [0, 1, 1, 2, 4, 16, 32, 64, 255];

const EFFECTS: [i32; 10] = [0, 1, 2, 3, 5, 8, 10, 11, 25, -1];

const PATTERNS: [&str; 7] = [
    "bordure",
    "minecraft:stripe_bottom",
    "cross",
    "globe",
    "",
    "no_such_pattern",
    "flower",
];

fn random_slot(sequence: &mut Sequence, screen: Screen) -> ItemStackRequestSlotInfo {
    let name = if sequence.chance(2) {
        sequence.pick(screen_containers(screen))
    } else {
        sequence.pick(&COMMON)
    };
    let slot_id = if sequence.chance(3) {
        sequence.pick(&SLOT_IDS)
    } else {
        sequence.next(9) as u8
    };
    slot(name, slot_id)
}

fn creative_id(sequence: &mut Sequence) -> u32 {
    let entries = pumpkin_data::bedrock_creative::CREATIVE_ENTRIES.len() as u32;
    if sequence.chance(3) {
        sequence.pick(&[0, 1, 2, 3, 100, entries, entries + 1, 1_000_000])
    } else {
        1 + sequence.next(u64::from(entries)) as u32
    }
}

fn no_op(sequence: &mut Sequence) -> Action {
    match sequence.next(5) {
        0 => Action::Create { result_index: 0 },
        1 => Action::LabTableCombine,
        2 => Action::CraftNonImplemented,
        3 => Action::CraftReserved {
            reserved_id: String::new(),
            repetitions: 1,
        },
        _ => Action::CraftResultsDeprecated {
            result_items: Vec::new(),
            times_crafted: 1,
        },
    }
}

fn random_action(sequence: &mut Sequence, screen: Screen) -> Action {
    let count = sequence.pick(&COUNTS);
    match sequence.next(20) {
        0..=4 => Action::Take {
            count,
            source: random_slot(sequence, screen),
            destination: random_slot(sequence, screen),
        },
        5..=7 => Action::Place {
            count,
            source: random_slot(sequence, screen),
            destination: random_slot(sequence, screen),
        },
        8 => Action::Swap {
            slot1: random_slot(sequence, screen),
            slot2: random_slot(sequence, screen),
        },
        9 => Action::Drop {
            count,
            source: random_slot(sequence, screen),
            randomly: sequence.chance(2),
        },
        10 => Action::Destroy {
            count,
            source: random_slot(sequence, screen),
        },
        11 => Action::Consume {
            count,
            source: random_slot(sequence, screen),
        },
        12 | 13 => {
            let recipe_id = VarUInt(sequence.next(6) as u32);
            let repetitions = sequence.pick(&[0, 1, 2, 64, 255]);
            if sequence.chance(2) {
                Action::CraftRecipe {
                    recipe_id,
                    repetitions,
                }
            } else {
                Action::CraftRecipeAuto {
                    recipe_id,
                    repetitions,
                }
            }
        }
        14 => Action::CraftCreative {
            creative_item_id: VarUInt(creative_id(sequence)),
            repetitions: sequence.pick(&[0, 1, 2, 255]),
        },
        15 => Action::BeaconPayment {
            primary_effect_id: VarInt(sequence.pick(&EFFECTS)),
            secondary_effect_id: VarInt(sequence.pick(&EFFECTS)),
        },
        16 => Action::MineBlock {
            hotbar_slot: VarInt(sequence.next(12) as i32 - 1),
            predicted_durability: VarInt(0),
            stack_id: VarInt(0),
        },
        17 => Action::Optional {
            recipe_id: VarUInt(0),
            filter_string_index: sequence.next(4) as i32 - 1,
        },
        18 => {
            if sequence.chance(2) {
                Action::Grindstone {
                    recipe_id: VarUInt(0),
                    repair_cost: VarInt(0),
                    repetitions: 1,
                }
            } else {
                Action::Loom {
                    pattern_id: sequence.pick(&PATTERNS).to_string(),
                    repetitions: 1,
                }
            }
        }
        _ => no_op(sequence),
    }
}

/// Moves the created output (or a result preview) to the cursor or a hotbar slot.
fn take_result(sequence: &mut Sequence, source: ItemStackRequestSlotInfo, count: u8) -> Action {
    let destination = if sequence.chance(2) {
        slot(ContainerName::Cursor, 0)
    } else {
        slot(ContainerName::HotBar, sequence.next(9) as u8)
    };
    if sequence.chance(2) {
        Action::Take {
            count,
            source,
            destination,
        }
    } else {
        Action::Place {
            count,
            source,
            destination,
        }
    }
}

fn created(sequence: &mut Sequence, preview: ItemStackRequestSlotInfo) -> ItemStackRequestSlotInfo {
    if sequence.chance(2) {
        slot(ContainerName::CreatedOutput, 50)
    } else {
        preview
    }
}

fn consume(count: u8, container: ContainerName, slot_id: u8) -> Action {
    Action::Consume {
        count,
        source: slot(container, slot_id),
    }
}

fn craft_flow(sequence: &mut Sequence, grid_start: u8) -> Vec<Action> {
    let mut actions = vec![Action::CraftRecipe {
        recipe_id: VarUInt(sequence.next(4) as u32),
        repetitions: sequence.pick(&[1, 1, 2, 64]),
    }];
    for cell in 0..sequence.next(3) as u8 {
        actions.push(consume(1, ContainerName::CraftingInput, grid_start + cell));
    }
    let count = sequence.pick(&[1, 4, 8, 64]);
    actions.push(take_result(
        sequence,
        slot(ContainerName::CreatedOutput, 50),
        count,
    ));
    actions
}

fn creative_flow(sequence: &mut Sequence) -> Vec<Action> {
    let create = Action::CraftCreative {
        creative_item_id: VarUInt(creative_id(sequence)),
        repetitions: sequence.pick(&[1, 1, 2]),
    };
    let count = sequence.pick(&[1, 16, 64, 128]);
    vec![
        create,
        take_result(sequence, slot(ContainerName::CreatedOutput, 50), count),
    ]
}

fn stonecutter_flow(sequence: &mut Sequence, cutter_ids: &[u32]) -> Vec<Action> {
    let recipe_id = if cutter_ids.is_empty() || sequence.chance(3) {
        sequence.next(5) as u32
    } else {
        sequence.pick(cutter_ids)
    };
    let craft = Action::CraftRecipe {
        recipe_id: VarUInt(recipe_id),
        repetitions: sequence.pick(&[1, 1, 2]),
    };
    let count = sequence.pick(&[1, 2, 4]);
    let source = created(sequence, slot(ContainerName::StonecutterResultPreview, 1));
    vec![
        craft,
        consume(1, ContainerName::StonecutterInput, 3),
        take_result(sequence, source, count),
    ]
}

fn merchant_flow(sequence: &mut Sequence) -> Vec<Action> {
    let mut actions = vec![Action::CraftRecipe {
        recipe_id: VarUInt(sequence.next(5) as u32),
        repetitions: 1,
    }];
    let count = sequence.pick(&[1, 5, 9, 64]);
    actions.push(consume(count, ContainerName::Trade2Ingredient1, 4));
    if sequence.chance(2) {
        actions.push(consume(1, ContainerName::Trade2Ingredient2, 5));
    }
    let source = created(sequence, slot(ContainerName::Trade2ResultPreview, 50));
    let count = sequence.pick(&[1, 1, 2]);
    actions.push(take_result(sequence, source, count));
    actions
}

/// Optional (rename), consume both inputs, take the result.
fn anvil_flow(sequence: &mut Sequence) -> Vec<Action> {
    let source = created(sequence, slot(ContainerName::AnvilResultPreview, 2));
    vec![
        Action::Optional {
            recipe_id: VarUInt(0),
            filter_string_index: sequence.next(3) as i32 - 1,
        },
        consume(1, ContainerName::AnvilInput, 1),
        consume(1, ContainerName::AnvilMaterial, 2),
        take_result(sequence, source, 1),
    ]
}

fn two_input_flow(
    sequence: &mut Sequence,
    first: Action,
    inputs: (ContainerName, u8, ContainerName, u8),
    preview: ItemStackRequestSlotInfo,
) -> Vec<Action> {
    let source = created(sequence, preview);
    let take = take_result(sequence, source, 1);
    let consumes = [
        consume(1, inputs.0, inputs.1),
        consume(1, inputs.2, inputs.3),
    ];
    // Clients send the consumes before the take; the other order reaches the output-slot
    // branch of take with the inputs still in place.
    let [a, b] = consumes;
    if sequence.chance(2) {
        vec![first, a, b, take]
    } else {
        vec![first, take, a, b]
    }
}

fn scripted(sequence: &mut Sequence, scene: &Scene, cutter_ids: &[u32]) -> Vec<Action> {
    use ContainerName as C;
    match scene.screen {
        Screen::Inventory if scene.gamemode == GameMode::Creative && sequence.chance(2) => {
            creative_flow(sequence)
        }
        Screen::Inventory => craft_flow(sequence, 28),
        Screen::Crafting => craft_flow(sequence, 32),
        Screen::Stonecutter => stonecutter_flow(sequence, cutter_ids),
        Screen::Merchant => merchant_flow(sequence),
        Screen::Enchanting => {
            let mut actions = vec![Action::CraftRecipe {
                recipe_id: VarUInt(sequence.next(6) as u32),
                repetitions: 1,
            }];
            if sequence.chance(2) {
                actions.push(take_result(sequence, slot(C::EnchantingInput, 0), 1));
            }
            actions
        }
        Screen::Beacon => {
            let pay = Action::BeaconPayment {
                primary_effect_id: VarInt(sequence.pick(&EFFECTS)),
                secondary_effect_id: VarInt(sequence.pick(&EFFECTS)),
            };
            match sequence.next(3) {
                0 => vec![pay],
                1 => vec![pay, consume(1, C::BeaconPayment, 0)],
                _ => vec![consume(1, C::BeaconPayment, 0), pay],
            }
        }
        Screen::Anvil => anvil_flow(sequence),
        Screen::Grindstone => two_input_flow(
            sequence,
            Action::Grindstone {
                recipe_id: VarUInt(0),
                repair_cost: VarInt(0),
                repetitions: 1,
            },
            (C::GrindstoneInput, 16, C::GrindstoneAdditional, 17),
            slot(C::GrindstoneResultPreview, 2),
        ),
        Screen::Loom => {
            let pattern = sequence.pick(&PATTERNS).to_string();
            two_input_flow(
                sequence,
                Action::Loom {
                    pattern_id: pattern,
                    repetitions: 1,
                },
                (C::LoomInput, 9, C::LoomDye, 10),
                slot(C::LoomResultPreview, 3),
            )
        }
        Screen::Smithing => two_input_flow(
            sequence,
            consume(1, C::SmithingTableTemplate, 51),
            (C::SmithingTableInput, 52, C::SmithingTableMaterial, 53),
            slot(C::SmithingTableResultPreview, 3),
        ),
        Screen::Cartography => {
            let source = created(sequence, slot(C::CartographyResultPreview, 2));
            vec![take_result(sequence, source, 1)]
        }
        Screen::Chest => {
            let chest = slot(C::LevelEntity, sequence.next(27) as u8);
            let hotbar = slot(C::HotBar, sequence.next(9) as u8);
            let count = sequence.pick(&COUNTS);
            if sequence.chance(2) {
                vec![Action::Take {
                    count,
                    source: chest,
                    destination: hotbar,
                }]
            } else {
                vec![Action::Place {
                    count,
                    source: hotbar,
                    destination: chest,
                }]
            }
        }
        Screen::Furnace => {
            let count = sequence.pick(&COUNTS);
            if sequence.chance(3) {
                vec![Action::Drop {
                    count,
                    source: slot(C::FurnaceResult, 2),
                    randomly: false,
                }]
            } else {
                vec![take_result(sequence, slot(C::FurnaceResult, 2), count)]
            }
        }
        Screen::Brewing => {
            let source = slot(C::BrewingStandResult, sequence.next(4) as u8);
            vec![take_result(sequence, source, 1)]
        }
    }
}

fn request(
    sequence: &mut Sequence,
    scene: &Scene,
    cutter_ids: &[u32],
    request_id: i32,
) -> ItemStackRequest {
    let actions = if sequence.chance(2) {
        scripted(sequence, scene, cutter_ids)
    } else {
        let count = 1 + sequence.next(3);
        (0..count)
            .map(|_| random_action(sequence, scene.screen))
            .collect()
    };
    let filter_strings = match sequence.next(4) {
        0 => vec!["Named".to_string()],
        1 => vec![String::new(), "Other".to_string()],
        _ => Vec::new(),
    };
    ItemStackRequest {
        request_id: VarInt(request_id),
        actions,
        filter_strings,
        filter_cause: 0,
    }
}

const fn kind(action: &Action) -> &'static str {
    match action {
        Action::Take { .. } => "take",
        Action::Place { .. } => "place",
        Action::Swap { .. } => "swap",
        Action::Drop { .. } => "drop",
        Action::Destroy { .. } => "destroy",
        Action::Consume { .. } => "consume",
        Action::Create { .. } => "create",
        Action::LabTableCombine => "lab_table",
        Action::BeaconPayment { .. } => "beacon",
        Action::MineBlock { .. } => "mine_block",
        Action::CraftRecipe { .. } => "craft",
        Action::CraftRecipeAuto { .. } => "craft_auto",
        Action::CraftCreative { .. } => "creative",
        Action::Optional { .. } => "optional",
        Action::Grindstone { .. } => "grindstone",
        Action::Loom { .. } => "loom",
        Action::CraftReserved { .. } => "reserved",
        Action::CraftNonImplemented => "non_implemented",
        Action::CraftResultsDeprecated { .. } => "deprecated",
    }
}

/// The stonecutter recipe network ids whose output can be cut from stone or cobblestone, so
/// that some requests take the recipe-table branch instead of the index fallback.
fn stonecutter_ids(player: &Player) -> Vec<u32> {
    let handler = current_handler(player);
    let handler = handler
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(cutter) = handler.as_any().downcast_ref::<StonecutterScreenHandler>() else {
        return Vec::new();
    };
    let mut outputs = Vec::new();
    for input in [&Item::STONE, &Item::COBBLESTONE] {
        for choice in cutter.get_available_recipes(&ItemStack::new(1, input)) {
            let key = choice
                .result_id
                .strip_prefix("minecraft:")
                .unwrap_or(&choice.result_id);
            if let Some(mapping) = Item::from_registry_key(key)
                .and_then(|item| JavaToBedrockItemMapping::from_java_item_id(item.id))
            {
                outputs.push(i32::from(mapping.bedrock_item.id));
            }
        }
    }
    let mut ids = Vec::new();
    for recipe in crate::net::bedrock::recipe::static_recipes() {
        if let BedrockRecipe::Shapeless(recipe) = recipe
            && recipe.block == "stonecutter"
            && recipe
                .output
                .first()
                .is_some_and(|out| outputs.contains(&out.id.0))
        {
            ids.push(recipe.recipe_network_id.0);
        }
    }
    ids
}

// ---------------------------------------------------------------------------------------
// Reading the queued packets

/// Gives each stack net id a number in order of first appearance.
#[derive(Default)]
struct NetIds(HashMap<i32, usize>);

impl NetIds {
    fn name(&mut self, id: i32) -> usize {
        let next = self.0.len() + 1;
        *self.0.entry(id).or_insert(next)
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u8(&mut self) -> Option<u8> {
        let byte = *self.bytes.get(self.at)?;
        self.at += 1;
        Some(byte)
    }

    fn skip(&mut self, count: usize) -> Option<()> {
        if self.at + count > self.bytes.len() {
            return None;
        }
        self.at += count;
        Some(())
    }

    fn var_uint(&mut self) -> Option<u32> {
        let mut value = 0u32;
        for index in 0..5 {
            let byte = self.u8()?;
            value |= u32::from(byte & 0x7f) << (7 * index);
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }

    fn var_int(&mut self) -> Option<i32> {
        let raw = self.var_uint()?;
        Some(((raw >> 1) as i32) ^ -((raw & 1) as i32))
    }

    fn string(&mut self) -> Option<String> {
        let length = self.var_uint()? as usize;
        let start = self.at;
        self.skip(length)?;
        Some(String::from_utf8_lossy(&self.bytes[start..self.at]).into_owned())
    }

    fn container(&mut self) -> Option<String> {
        let name = self.u8()?;
        if self.u8()? == 1 {
            self.skip(4)?;
            return Some(format!("{name}+dyn"));
        }
        Some(name.to_string())
    }
}

/// The packet id and the payload of a queued Bedrock packet.
fn split_packet(packet: &[u8]) -> Option<(u32, &[u8])> {
    let mut reader = Reader {
        bytes: packet,
        at: 0,
    };
    reader.var_uint()?;
    let header = reader.var_uint()?;
    Some((header & 0x3ff, &packet[reader.at..]))
}

/// `ItemStackResponse` as text, and the result code of each response.
fn read_response(payload: &[u8], ids: &mut NetIds) -> Option<(String, Vec<u8>)> {
    let mut reader = Reader {
        bytes: payload,
        at: 0,
    };
    let mut text = String::new();
    let mut results = Vec::new();
    for _ in 0..reader.var_uint()? {
        let result = reader.u8()?;
        results.push(result);
        let _ = write!(text, "r{result} #{}", reader.var_int()?);
        if reader.u8()? == 1 {
            for _ in 0..reader.var_uint()? {
                let _ = write!(text, " [c{}", reader.container()?);
                for _ in 0..reader.var_uint()? {
                    let (requested, slot, amount) = (reader.u8()?, reader.u8()?, reader.u8()?);
                    let id = if reader.u8()? == 1 {
                        ids.name(reader.var_int()?)
                    } else {
                        0
                    };
                    let names = (reader.string()?, reader.string()?);
                    let durability = reader.var_int()?;
                    let _ = write!(
                        text,
                        " {requested}/{slot}x{amount}#{id}{}{}d{durability}",
                        names.0, names.1
                    );
                }
                text.push(']');
            }
        }
        text.push(';');
    }
    if reader.at != payload.len() {
        let _ = write!(text, " trailing {}", payload.len() - reader.at);
    }
    Some((text, results))
}

fn read_descriptor(reader: &mut Reader<'_>, ids: &mut NetIds, text: &mut String) -> Option<()> {
    let id = i16::from_le_bytes([reader.u8()?, reader.u8()?]);
    let count = u16::from_le_bytes([reader.u8()?, reader.u8()?]);
    let aux = reader.var_uint()?;
    let net = if reader.u8()? == 1 {
        ids.name(reader.var_int()?)
    } else {
        0
    };
    let block = reader.var_uint()?;
    let extra = reader.var_uint()? as usize;
    reader.skip(extra)?;
    if id != 0 {
        let _ = write!(text, "{id}:{count}:{aux}:{block}#{net}+{extra};");
    } else {
        text.push('.');
    }
    Some(())
}

/// `InventoryContent` as text.
fn read_inventory_content(payload: &[u8], ids: &mut NetIds) -> Option<String> {
    let mut reader = Reader {
        bytes: payload,
        at: 0,
    };
    let mut text = format!("w{} ", reader.var_uint()?);
    for _ in 0..reader.var_uint()? {
        read_descriptor(&mut reader, ids, &mut text)?;
    }
    let _ = write!(text, " c{} ", reader.container()?);
    read_descriptor(&mut reader, ids, &mut text)?;
    if reader.at != payload.len() {
        let _ = write!(text, " trailing {}", payload.len() - reader.at);
    }
    Some(text)
}

// ---------------------------------------------------------------------------------------
// State

/// NBT as text with the compound keys sorted (a compound is a `HashMap`, so its `Debug`
/// order changes between runs).
fn nbt_text(tag: &NbtTag, text: &mut String) {
    match tag {
        NbtTag::Compound(compound) => compound_text(compound, text),
        NbtTag::List(items) => {
            text.push('[');
            for item in items {
                nbt_text(item, text);
                text.push(',');
            }
            text.push(']');
        }
        other => {
            let _ = write!(text, "{other:?}");
        }
    }
}

fn compound_text(compound: &NbtCompound, text: &mut String) {
    let mut keys: Vec<&str> = compound.child_tags.keys().map(AsRef::as_ref).collect();
    keys.sort_unstable();
    text.push('{');
    for key in keys {
        let _ = write!(text, "{key}:");
        if let Some(tag) = compound.child_tags.get(key) {
            nbt_text(tag, text);
        }
        text.push(',');
    }
    text.push('}');
}

fn stack_text(stack: &ItemStack) -> String {
    if stack.is_empty() {
        return "-".to_string();
    }
    let mut compound = NbtCompound::new();
    stack.write_item_stack(&mut compound);
    let mut text = String::new();
    compound_text(&compound, &mut text);
    text
}

/// Every slot of the open screen, the cursor, the player's level and the beacon.
fn state_text(player: &Player, beacon: Option<&Arc<BeaconBlockEntity>>) -> String {
    let handler = current_handler(player);
    let handler = handler
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let behaviour = handler.get_behaviour();
    let mut text = String::new();
    for slot in &behaviour.slots {
        text.push_str(&stack_text(&slot.get_cloned_stack()));
        text.push(';');
    }
    let cursor = behaviour
        .cursor_stack
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let _ = write!(
        text,
        " cursor {} level {}",
        stack_text(&cursor),
        player.experience_level.load(Ordering::Relaxed)
    );
    if let Some(beacon) = beacon {
        let _ = write!(
            text,
            " beacon {} {} {}",
            beacon.primary_effect.load(Ordering::Relaxed),
            beacon.secondary_effect.load(Ordering::Relaxed),
            stack_text(&beacon.get_stack(0))
        );
    }
    text
}

// ---------------------------------------------------------------------------------------
// The run

#[derive(Default)]
struct Tally {
    actions: BTreeMap<&'static str, (u32, u32)>,
    packets: BTreeMap<u32, u32>,
    ok: u32,
    errors: u32,
    panics: u32,
}

impl Tally {
    fn count(&mut self, actions: &[&'static str], results: &[u8]) {
        let ok = results.first() == Some(&0);
        if ok {
            self.ok += 1;
        } else {
            self.errors += 1;
        }
        for action in actions {
            let counts = self.actions.entry(action).or_default();
            if ok {
                counts.0 += 1;
            } else {
                counts.1 += 1;
            }
        }
    }
}

/// Decodes what the request queued: the text that goes into the hash, and the result codes.
fn read_packets(
    tap: &mut crate::net::bedrock::BedrockPacketTap,
    ids: &mut NetIds,
    tally: &mut Tally,
) -> (String, Vec<u8>) {
    let mut text = String::new();
    let mut results = Vec::new();
    for packet in tap.drain() {
        let Some((id, payload)) = split_packet(&packet) else {
            text.push_str("unreadable;");
            continue;
        };
        *tally.packets.entry(id).or_default() += 1;
        match id {
            148 => match read_response(payload, ids) {
                Some((response, codes)) => {
                    text.push_str(&response);
                    results.extend(codes);
                }
                None => text.push_str("bad response;"),
            },
            49 => {
                let content = read_inventory_content(payload, ids);
                text.push_str(content.as_deref().unwrap_or("bad content"));
            }
            _ => {
                let _ = write!(text, "p{id};");
            }
        }
    }
    (text, results)
}

async fn run_scene(server: &Arc<Server>, scene: &Scene, seed: u64) -> Result<String, String> {
    let world = server
        .worlds
        .load()
        .first()
        .cloned()
        .ok_or("the server has no world")?;
    let (player, mut tap) = test_support::bedrock_player_with_tap(&world, scene.gamemode).await?;
    let ClientPlatform::Bedrock(client) = player.client.as_ref() else {
        return Err("not a Bedrock player".to_string());
    };
    let mut sequence = Sequence(seed);
    if scene.gamemode == GameMode::Survival {
        player.set_experience_level(30, false);
    }
    fill_inventory(&player, &mut sequence);
    let beacon = open(&player, server, scene.screen);
    let cutter_ids = stonecutter_ids(&player);
    tap.drain();

    let mut report = String::new();
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut ids = NetIds::default();
    let mut tally = Tally::default();
    for number in 1..=REQUESTS {
        if sequence.chance(2) {
            refill(&player, scene.screen, beacon.as_ref(), &mut sequence);
        }
        fix_enchanting_seed(&player);
        let request = request(&mut sequence, scene, &cutter_ids, -(number as i32));
        let kinds: Vec<&'static str> = request.actions.iter().map(kind).collect();
        let asked = if std::env::var_os("KVASIR_DUMP").is_some() {
            format!("{:?}", request.actions)
        } else {
            String::new()
        };
        let entities = world.entities.load().len();
        let packet = SItemStackRequest {
            requests: vec![request],
        };
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
            client.handle_item_stack_request(&player, packet);
        }));
        let (text, results) = if outcome.is_err() {
            tally.panics += 1;
            ("panic".to_string(), vec![2])
        } else {
            read_packets(&mut tap, &mut ids, &mut tally)
        };
        tally.count(&kinds, &results);
        let drops = world.entities.load().len() as i64 - entities as i64;
        hash = fnv(hash, text.as_bytes());
        let state = state_text(&player, beacon.as_ref());
        if let Some(dump) = std::env::var_os("KVASIR_DUMP") {
            use std::io::Write as _;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(dump)
            {
                let _ = writeln!(
                    file,
                    "{} {number} {text}\n  {state} drops {drops}\n  asked {asked}",
                    scene.name
                );
            }
        }
        hash = fnv(hash, state.as_bytes());
        hash = fnv(hash, &drops.to_le_bytes());
        if number % 50 == 0 {
            let _ = writeln!(
                report,
                "{} requests {}-{number}: {hash:016x} ok {} error {} panic {}",
                scene.name,
                number - 49,
                tally.ok,
                tally.errors,
                tally.panics
            );
        }
    }
    let _ = writeln!(
        report,
        "{} actions (ok, error): {:?}",
        scene.name, tally.actions
    );
    let _ = writeln!(report, "{} packets: {:?}", scene.name, tally.packets);
    Ok(report)
}

#[tokio::test(flavor = "multi_thread")]
async fn item_stack_requests() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let mut now = String::new();
    for (index, scene) in SCENES.iter().enumerate() {
        now.push_str(&run_scene(&server, scene, 0x1717 + index as u64).await?);
    }
    server.stop();

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("item_stack_requests.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, now);
    Ok(())
}
