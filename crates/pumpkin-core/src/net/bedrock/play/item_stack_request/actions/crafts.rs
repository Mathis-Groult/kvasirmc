//! The crafting and screen actions of a Bedrock item-stack request: creative items, recipes
//! (trades, the stonecutter, enchanting, crafting grids), beacon payment, mining prediction, anvil
//! renaming, grindstone and loom.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use pumpkin_data::item_stack::ItemStack;
use pumpkin_inventory::Inventory;
use pumpkin_inventory::anvil::anvil_screen_handler::AnvilScreenHandler;
use pumpkin_inventory::beacon_screen_handler::BeaconScreenHandler;
use pumpkin_inventory::grindstone_screen_handler::GrindstoneScreenHandler;
use pumpkin_inventory::loom_screen_handler::LoomScreenHandler;
use pumpkin_inventory::screen_handler::{InventoryPlayer, ScreenHandler};
use pumpkin_inventory::stonecutter_screen_handler::StonecutterScreenHandler;
use pumpkin_protocol::bedrock::network_item::{ContainerName, FullContainerName};
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::codec::var_uint::VarUInt;

use super::super::{
    map_bedrock_container_slot, record_preview_update, record_specialized_container_input_updates,
    record_update,
};
use super::RequestState;
use crate::block::entities::beacon::BeaconBlockEntity;
use crate::entity::player::Player;

/// Creative inventory: the chosen item, as many stacks as `repetitions` (capped at 255), becomes
/// the created output.
pub(super) fn craft_creative(
    state: &mut RequestState,
    creative_item_id: VarUInt,
    repetitions: u8,
) -> bool {
    let index = (creative_item_id.0.saturating_sub(1)) as usize;
    if index < pumpkin_data::bedrock_creative::CREATIVE_ENTRIES.len() {
        let entry = pumpkin_data::bedrock_creative::CREATIVE_ENTRIES[index];
        if let Some(mapping) = pumpkin_data::item::JavaToBedrockItemMapping::from_bedrock(
            entry.item_id,
            entry.item_aux_value,
        ) {
            // Bedrock `repetitions` represents how many stacks to create; use the item's max stack size
            let max_stack = ItemStack::static_new_java(1, mapping.java_item).get_max_stack_size();
            let count = ((max_stack as u16) * (repetitions as u16)).min(u8::MAX as u16) as u8;
            state.created_item = Some(ItemStack::new(count, mapping.java_item));
        } else {
            tracing::warn!(
                "Failed to map bedrock item id {} and data {} to Java item",
                entry.item_id,
                entry.item_aux_value
            );
            return false;
        }
    } else {
        tracing::warn!(
            "Creative item index {} out of bounds (len: {})",
            index,
            pumpkin_data::bedrock_creative::CREATIVE_ENTRIES.len()
        );
        return false;
    }
    true
}

/// `CraftRecipe` and `CraftRecipeAuto`: a trade, a stonecutter recipe, an enchantment button or a
/// crafting-grid craft, by the open screen.
pub(super) fn craft_recipe(
    player: &Arc<Player>,
    screen_handler: &mut dyn ScreenHandler,
    state: &mut RequestState,
    recipe_id: VarUInt,
    repetitions: u8,
) -> bool {
    if screen_handler.window_type() == Some(pumpkin_data::screen::WindowType::Merchant) {
        return select_trade(screen_handler, state, recipe_id);
    }

    if screen_handler.window_type() == Some(pumpkin_data::screen::WindowType::Stonecutter) {
        return cut_stone(player, screen_handler, state, recipe_id, repetitions);
    }

    if screen_handler.window_type() == Some(pumpkin_data::screen::WindowType::Enchantment) {
        return enchant(player, screen_handler, state, recipe_id);
    }

    if repetitions > 0 {
        return craft_grid(player, screen_handler, state, repetitions);
    }
    true
}

/// Selects trade `recipe_id - 1`, which moves the payment into the trade slots, and records those
/// slots.
fn select_trade(
    screen_handler: &mut dyn ScreenHandler,
    state: &mut RequestState,
    recipe_id: VarUInt,
) -> bool {
    let Some(handler) = screen_handler
    .as_any_mut()
    .downcast_mut::<pumpkin_inventory::merchant::merchant_screen_handler::MerchantScreenHandler>()
    else {
    return false;
    };
    let trade = recipe_id.0.saturating_sub(1) as usize;
    if trade >= handler.offers.len() {
        return false;
    }
    handler.set_selected_offer(trade);
    for (container_name, slot_id, screen_slot) in [
        (ContainerName::Trade2Ingredient1, 4, 0),
        (ContainerName::Trade2Ingredient2, 5, 1),
        (ContainerName::Trade2ResultPreview, 50, 2),
    ] {
        let stack = handler.get_behaviour().slots[screen_slot].get_cloned_stack();
        record_update(
            &mut state.updates,
            FullContainerName {
                container_name,
                dynamic_id: None,
            },
            slot_id,
            &stack,
        );
    }
    true
}

/// Stonecutter: picks the recipe by its network id (or, failing that, by index), takes the output
/// once per repetition and makes the total the created output.
fn cut_stone(
    player: &Arc<Player>,
    screen_handler: &mut dyn ScreenHandler,
    state: &mut RequestState,
    recipe_id: VarUInt,
    repetitions: u8,
) -> bool {
    let Some(handler) = screen_handler
        .as_any_mut()
        .downcast_mut::<StonecutterScreenHandler>()
    else {
        return false;
    };
    let input_stack = handler.input_inventory.get_stack(0);
    if input_stack.is_empty() {
        return false;
    }
    let available = handler.get_available_recipes(&input_stack);
    let recipe_idx = crate::net::bedrock::recipe::static_recipes()
        .iter()
        .find_map(|r| match r {
            pumpkin_protocol::bedrock::client::BedrockRecipe::Shapeless(s)
                if s.block == "stonecutter" && s.recipe_network_id == recipe_id =>
            {
                s.output.first().and_then(|out| {
                    let out_id = out.id.0;
                    available.iter().position(|r| {
                        let item_id = r
                            .result_id
                            .strip_prefix("minecraft:")
                            .unwrap_or(&r.result_id);
                        pumpkin_data::item::Item::from_registry_key(item_id)
                            .and_then(|item| {
                                pumpkin_data::item::JavaToBedrockItemMapping::from_java_item_id(
                                    item.id,
                                )
                            })
                            .is_some_and(|m| (m.bedrock_item.id as i32) == out_id)
                    })
                })
            }
            pumpkin_protocol::bedrock::client::BedrockRecipe::Shapeless(_)
            | pumpkin_protocol::bedrock::client::BedrockRecipe::Shaped(_) => None,
        })
        .or_else(|| ((recipe_id.0 as usize) < available.len()).then_some(recipe_id.0 as usize));

    let Some(idx) = recipe_idx else {
        return false;
    };

    handler.set_selected_recipe(idx as u8);
    handler.update_output();

    let output_slot = handler.get_behaviour().slots[1].clone();
    let output_stack = output_slot.get_cloned_stack();
    if output_stack.is_empty() {
        return false;
    }

    let reps = repetitions.max(1);
    let mut total_crafted = output_stack.clone();
    total_crafted.item_count = total_crafted.item_count.saturating_mul(reps);
    state.created_item = Some(total_crafted);

    for _ in 0..reps {
        output_slot.on_take_item(player.as_ref(), &output_stack);
    }
    state.crafting_inputs_consumed = true;

    record_specialized_container_input_updates(&*screen_handler, &mut state.updates);
    record_preview_update(
        &mut state.updates,
        ContainerName::StonecutterResultPreview,
        1,
        &output_stack,
    );
    true
}

/// Enchanting table: recipe ids 0-2 are the buttons, higher ids are one more than the button.
fn enchant(
    player: &Arc<Player>,
    screen_handler: &mut dyn ScreenHandler,
    state: &mut RequestState,
    recipe_id: VarUInt,
) -> bool {
    let button_id = if recipe_id.0 < 3 {
        recipe_id.0 as i32
    } else {
        recipe_id.0.saturating_sub(1) as i32
    };
    if (0..3).contains(&button_id) && screen_handler.on_button_click(player.as_ref(), button_id) {
        let enchanted_item = screen_handler.get_behaviour().slots[0].get_cloned_stack();
        let lapis_item = screen_handler.get_behaviour().slots[1].get_cloned_stack();
        record_update(
            &mut state.updates,
            FullContainerName {
                container_name: ContainerName::EnchantingInput,
                dynamic_id: None,
            },
            0,
            &enchanted_item,
        );
        record_update(
            &mut state.updates,
            FullContainerName {
                container_name: ContainerName::EnchantingMaterial,
                dynamic_id: None,
            },
            1,
            &lapis_item,
        );
    } else {
        return false;
    }
    true
}

/// Crafting grid (2x2 of the player screen or 3x3 of a table): takes the output `repetitions`
/// times, makes the total the created output and reports every grid slot.
fn craft_grid(
    player: &Arc<Player>,
    screen_handler: &mut dyn ScreenHandler,
    state: &mut RequestState,
    repetitions: u8,
) -> bool {
    screen_handler.update_to_client();

    let is_player = screen_handler.window_type().is_none();
    let grid_size = if is_player { 4 } else { 9 };
    let bedrock_grid_start = if is_player { 28 } else { 32 };
    for i in 0..grid_size {
        let grid_slot_index = 1 + i;
        let grid_slot = screen_handler.get_behaviour().slots[grid_slot_index].clone();
        let grid_stack = grid_slot.get_cloned_stack();
        tracing::info!(
            "Crafting Grid slot {i} (slot index {grid_slot_index}): Item ID: {}, Count: {}",
            grid_stack.item.id,
            grid_stack.item_count
        );
    }

    let output_slot = screen_handler.get_behaviour().slots[0].clone();
    let output_stack = output_slot.get_cloned_stack();

    if output_stack.is_empty() || repetitions > output_slot.get_max_item_count() {
        tracing::warn!("Client sent an invalid crafting request");
        return false;
    }

    let mut total_crafted = output_stack.clone();
    total_crafted.item_count = total_crafted.item_count.saturating_mul(repetitions);
    state.created_item = Some(total_crafted);

    for _ in 0..repetitions {
        output_slot.on_take_item(player.as_ref(), &output_stack);
    }
    state.crafting_inputs_consumed = true;

    // Record updates for all grid slots so Bedrock client is notified of consumed ingredients!
    let is_player = screen_handler.window_type().is_none();
    let grid_size = if is_player { 4 } else { 9 };
    for i in 0..grid_size {
        let grid_slot_index = 1 + i;
        let grid_slot = screen_handler.get_behaviour().slots[grid_slot_index].clone();
        let grid_stack = grid_slot.get_cloned_stack();
        record_update(
            &mut state.updates,
            FullContainerName {
                container_name: ContainerName::CraftingInput,
                dynamic_id: None,
            },
            (bedrock_grid_start + i) as u8,
            &grid_stack,
        );
    }
    true
}

/// Beacon: pays with one item from the payment slot for valid effects.
pub(super) fn beacon_payment(
    screen_handler: &mut dyn ScreenHandler,
    state: &mut RequestState,
    primary_effect_id: VarInt,
    secondary_effect_id: VarInt,
) -> bool {
    let Some(beacon_handler) = screen_handler
        .as_any_mut()
        .downcast_mut::<BeaconScreenHandler>()
    else {
        return false;
    };
    let Some(beacon_entity) = beacon_handler
        .inventory
        .as_any()
        .downcast_ref::<BeaconBlockEntity>()
    else {
        return false;
    };
    if beacon_entity
        .payment
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_empty()
    {
        return false;
    }
    let levels = beacon_entity.levels.load(Ordering::Relaxed);
    let primary_id = (primary_effect_id.0 > 0).then_some(primary_effect_id.0);
    let secondary_id = (secondary_effect_id.0 > 0).then_some(secondary_effect_id.0);

    if BeaconBlockEntity::validate_effects(primary_id, secondary_id, levels) {
        beacon_entity
            .primary_effect
            .store(primary_id.unwrap_or(-1), Ordering::Relaxed);
        beacon_entity
            .secondary_effect
            .store(secondary_id.unwrap_or(-1), Ordering::Relaxed);

        beacon_entity.remove_stack_specific(0, 1);
        beacon_entity.mark_dirty();
        let payment_stack = beacon_entity.get_stack(0);
        beacon_handler.sync_state();
        record_update(
            &mut state.updates,
            FullContainerName {
                container_name: ContainerName::BeaconPayment,
                dynamic_id: None,
            },
            0,
            &payment_stack,
        );
    } else {
        return false;
    }
    true
}

/// `MineBlock`: reports the hotbar slot the client predicted a tool change for.
pub(super) fn mine_block(
    screen_handler: &dyn ScreenHandler,
    state: &mut RequestState,
    hotbar_slot: VarInt,
) -> bool {
    if (0..9).contains(&hotbar_slot.0)
        && let Some(screen_slot) =
            map_bedrock_container_slot(screen_handler, ContainerName::HotBar, hotbar_slot.0 as u8)
    {
        let current_stack = screen_handler.get_behaviour().slots[screen_slot].get_cloned_stack();
        record_update(
            &mut state.updates,
            FullContainerName {
                container_name: ContainerName::HotBar,
                dynamic_id: None,
            },
            hotbar_slot.0 as u8,
            &current_stack,
        );
    }
    true
}

/// Optional in an anvil: renames to the filter string at `filter_string_index` (or the first one,
/// or none).
pub(super) fn rename_in_anvil(
    player: &Arc<Player>,
    screen_handler: &mut dyn ScreenHandler,
    state: &mut RequestState,
    filter_string_index: i32,
    filter_strings: &[String],
) -> bool {
    if screen_handler.window_type() == Some(pumpkin_data::screen::WindowType::Anvil)
        && let Some(anvil) = screen_handler
            .as_any_mut()
            .downcast_mut::<AnvilScreenHandler>()
    {
        let name =
            if filter_string_index >= 0 && (filter_string_index as usize) < filter_strings.len() {
                &filter_strings[filter_string_index as usize]
            } else if let Some(first) = filter_strings.first() {
                first
            } else {
                ""
            };
        anvil.set_item_name(name, player.has_infinite_materials());
        let result_stack = anvil.inventory.get_stack(2);
        record_preview_update(
            &mut state.updates,
            ContainerName::AnvilResultPreview,
            2,
            &result_stack,
        );
    }
    true
}

/// Grindstone: recomputes the result.
pub(super) fn grind(screen_handler: &mut dyn ScreenHandler, state: &mut RequestState) -> bool {
    if screen_handler.window_type() == Some(pumpkin_data::screen::WindowType::Grindstone)
        && let Some(grindstone) = screen_handler
            .as_any_mut()
            .downcast_mut::<GrindstoneScreenHandler>()
    {
        grindstone.create_result();
        let result_stack = grindstone.inventory.get_stack(2);
        record_preview_update(
            &mut state.updates,
            ContainerName::GrindstoneResultPreview,
            2,
            &result_stack,
        );
    }
    true
}

/// Loom: applies the chosen pattern to the result.
pub(super) fn weave(
    screen_handler: &mut dyn ScreenHandler,
    state: &mut RequestState,
    pattern_id: &str,
) -> bool {
    if screen_handler.window_type() == Some(pumpkin_data::screen::WindowType::Loom)
        && let Some(loom) = screen_handler
            .as_any_mut()
            .downcast_mut::<LoomScreenHandler>()
    {
        loom.select_pattern(pattern_id);
        let result_stack = loom.output_inventory.get_stack(0);
        record_preview_update(
            &mut state.updates,
            ContainerName::LoomResultPreview,
            3,
            &result_stack,
        );
    }
    true
}
