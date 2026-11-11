//! The actions of a Bedrock item-stack request, one function per action kind, split out of
//! `BedrockClient::handle_item_stack_request` (statements in their original order): the request
//! flow and the item moves here, the crafting and screen actions in `crafts`. Each action returns
//! `false` to reject the request; the actions before it stay applied.

use std::sync::Arc;

use pumpkin_data::item_stack::ItemStack;
use pumpkin_inventory::screen_handler::ScreenHandler;
use pumpkin_protocol::bedrock::client::item_stack_response::{
    ItemStackResponseContainerInfo, ItemStackResponseInfo, ItemStackResponseSlotInfo,
};
use pumpkin_protocol::bedrock::network_item::{ContainerName, FullContainerName};
use pumpkin_protocol::bedrock::server::item_stack_request::{
    ItemStackRequest, ItemStackRequestAction, ItemStackRequestSlotInfo,
};
use pumpkin_protocol::codec::var_int::VarInt;

use super::{
    SlotUpdate, get_output_screen_slot, get_slot_stack, map_bedrock_container_slot, names_a_slot,
    record_specialized_container_input_updates, record_update, refresh_specialized_screen_results,
    update_slot_stack,
};
use crate::entity::player::Player;

mod crafts;

/// What a request has built so far: the item a craft created (what `CreatedOutput` holds),
/// whether that craft already used the crafting inputs, and the slots to report.
struct RequestState {
    created_item: Option<ItemStack>,
    crafting_inputs_consumed: bool,
    updates: Vec<SlotUpdate>,
}

/// The count and the two slots of a Take or Place.
#[derive(Clone, Copy)]
struct Transfer<'a> {
    count: u8,
    source: &'a ItemStackRequestSlotInfo,
    destination: &'a ItemStackRequestSlotInfo,
}

/// Applies one request's actions in order and answers it; a failed request reports no slots.
pub(super) fn handle_request(
    player: &Arc<Player>,
    screen_handler: &mut dyn ScreenHandler,
    request: ItemStackRequest,
) -> ItemStackResponseInfo {
    let mut state = RequestState {
        created_item: None,
        crafting_inputs_consumed: false,
        updates: Vec::new(),
    };
    let mut result = 0u8; // 0 = Success, 1 = Error

    for action in request.actions {
        if !apply_action(
            player,
            screen_handler,
            &mut state,
            action,
            &request.filter_strings,
        ) {
            result = 1;
            break;
        }
    }

    if result == 0 {
        refresh_specialized_screen_results(
            player.as_ref(),
            &mut *screen_handler,
            &request.filter_strings,
            &mut state.updates,
        );
    }

    let container_infos = response_containers(result, state.updates);

    ItemStackResponseInfo {
        result,
        client_request_id: request.request_id,
        containers: container_infos,
    }
}

/// The reported slots grouped by container, in the order they were first reported; a failed
/// request reports none.
fn response_containers(
    result: u8,
    updates: Vec<SlotUpdate>,
) -> Vec<ItemStackResponseContainerInfo> {
    let mut container_infos = Vec::new();
    if result == 0 {
        for update in updates {
            let container_info =
                container_infos
                    .iter_mut()
                    .find(|info: &&mut ItemStackResponseContainerInfo| {
                        info.full_container_name == update.container_name
                    });

            let slot_info = ItemStackResponseSlotInfo {
                requested_slot: update.slot_id,
                slot: update.slot_id,
                amount: update.count,
                item_stack_net_id: update.stack_id,
                custom_name: String::new(),
                filtered_custom_name: String::new(),
                durability_correction: VarInt(0),
            };

            if let Some(info) = container_info {
                info.slots.push(slot_info);
            } else {
                container_infos.push(ItemStackResponseContainerInfo {
                    full_container_name: update.container_name,
                    slots: vec![slot_info],
                });
            }
        }
    }
    container_infos
}

/// Whether a successful response reported a slot of the player's inventory or hotbar.
pub(super) fn touches_player_inventory(responses: &[ItemStackResponseInfo]) -> bool {
    // Collect inventory updates if we modified player inventory
    let mut inventory_updated = false;
    for response in responses {
        if response.result == 0 {
            for info in &response.containers {
                if info.full_container_name.container_name == ContainerName::Inventory
                    || info.full_container_name.container_name
                        == ContainerName::CombinedHotBarAndInventory
                    || info.full_container_name.container_name == ContainerName::HotBar
                {
                    inventory_updated = true;
                }
            }
        }
    }

    inventory_updated
}

/// One action; `false` rejects the request.
fn apply_action(
    player: &Arc<Player>,
    screen_handler: &mut dyn ScreenHandler,
    state: &mut RequestState,
    action: ItemStackRequestAction,
    filter_strings: &[String],
) -> bool {
    match action {
        ItemStackRequestAction::CraftCreative {
            creative_item_id,
            repetitions,
        } => crafts::craft_creative(state, creative_item_id, repetitions),
        ItemStackRequestAction::Take {
            count,
            source,
            destination,
        }
        | ItemStackRequestAction::Place {
            count,
            source,
            destination,
        } => take_or_place(
            player,
            screen_handler,
            state,
            Transfer {
                count,
                source: &source,
                destination: &destination,
            },
        ),
        ItemStackRequestAction::Swap { slot1, slot2 } => {
            swap(player, screen_handler, state, &slot1, &slot2)
        }
        ItemStackRequestAction::Drop {
            count,
            source,
            randomly: _,
        } => drop_items(player, screen_handler, state, count, &source),
        ItemStackRequestAction::Destroy { count, source }
        | ItemStackRequestAction::Consume { count, source } => {
            destroy_or_consume(player, screen_handler, state, count, &source)
        }
        ItemStackRequestAction::CraftRecipe {
            recipe_id,
            repetitions,
        }
        | ItemStackRequestAction::CraftRecipeAuto {
            recipe_id,
            repetitions,
        } => crafts::craft_recipe(player, screen_handler, state, recipe_id, repetitions),
        ItemStackRequestAction::BeaconPayment {
            primary_effect_id,
            secondary_effect_id,
        } => crafts::beacon_payment(
            screen_handler,
            state,
            primary_effect_id,
            secondary_effect_id,
        ),
        ItemStackRequestAction::MineBlock {
            hotbar_slot,
            predicted_durability: _,
            stack_id: _,
        } => crafts::mine_block(&*screen_handler, state, hotbar_slot),
        ItemStackRequestAction::Optional {
            recipe_id: _,
            filter_string_index,
        } => crafts::rename_in_anvil(
            player,
            screen_handler,
            state,
            filter_string_index,
            filter_strings,
        ),
        ItemStackRequestAction::Grindstone {
            recipe_id: _,
            repair_cost: _,
            repetitions: _,
        } => crafts::grind(screen_handler, state),
        ItemStackRequestAction::Loom {
            pattern_id,
            repetitions: _,
        } => crafts::weave(screen_handler, state, &pattern_id),
        ItemStackRequestAction::CraftResultsDeprecated { .. }
        | ItemStackRequestAction::CraftReserved { .. }
        | ItemStackRequestAction::Create { .. }
        | ItemStackRequestAction::LabTableCombine
        | ItemStackRequestAction::CraftNonImplemented => {
            // Successful no-ops to prevent client-side transaction rollbacks
            true
        }
    }
}

/// Take or Place: moves `count` items from `source` to `destination`; a merchant result completes
/// the trade, an output slot takes its inputs first.
fn take_or_place(
    player: &Arc<Player>,
    screen_handler: &mut dyn ScreenHandler,
    state: &mut RequestState,
    transfer: Transfer<'_>,
) -> bool {
    let Transfer {
        count,
        source,
        destination,
    } = transfer;
    let source_stack = get_slot_stack(&*screen_handler, source, state.created_item.as_ref());
    if source_stack.is_empty() && state.created_item.is_none() {
        tracing::debug!("Source stack is empty in Take/Place");
        return false;
    }
    let count = count.min(source_stack.item_count);
    if count > 0 {
        if !names_a_slot(&*screen_handler, destination) {
            tracing::debug!("Destination slot does not exist in Take/Place");
            return false;
        }
        let mut dest_stack =
            get_slot_stack(&*screen_handler, destination, state.created_item.as_ref());
        if dest_stack.is_empty() {
            dest_stack = source_stack.copy_with_count(count);
        } else if dest_stack.are_items_and_components_equal(&source_stack) {
            dest_stack.item_count = dest_stack.item_count.saturating_add(count);
        } else {
            tracing::debug!("Destination stack is not compatible with source stack");
            return false;
        }

        if let Some(dest_screen_slot) = map_bedrock_container_slot(
            &*screen_handler,
            destination.container_name.container_name,
            destination.slot_id,
        ) {
            let slot = &screen_handler.get_behaviour().slots[dest_screen_slot];
            if !slot.can_insert(&dest_stack) {
                return false;
            }
        }

        let merchant_result = screen_handler.window_type()
            == Some(pumpkin_data::screen::WindowType::Merchant)
            && matches!(
                source.container_name.container_name,
                ContainerName::CreatedOutput
                    | ContainerName::TradeResultPreview
                    | ContainerName::Trade2ResultPreview
            );
        if merchant_result {
            return take_merchant_result(
                player,
                screen_handler,
                state,
                Transfer {
                    count,
                    source,
                    destination,
                },
                &source_stack,
                &dest_stack,
            );
        }

        if let Some(out_slot) = get_output_screen_slot(&*screen_handler, source)
            && !state.crafting_inputs_consumed
        {
            let slot = screen_handler.get_behaviour().slots[out_slot].clone();
            if !slot.can_take_items(player.as_ref()) {
                return false;
            }
            slot.on_take_item(player.as_ref(), &source_stack);
            record_specialized_container_input_updates(&*screen_handler, &mut state.updates);
        }

        finish_transfer(
            player,
            screen_handler,
            state,
            Transfer {
                count,
                source,
                destination,
            },
            source_stack,
            &dest_stack,
        );
    }
    true
}

/// Taking a merchant's result: only the whole stack, and only if the trade completes; records the
/// trade slots, the emptied result and the destination.
fn take_merchant_result(
    player: &Arc<Player>,
    screen_handler: &mut dyn ScreenHandler,
    state: &mut RequestState,
    transfer: Transfer<'_>,
    source_stack: &ItemStack,
    dest_stack: &ItemStack,
) -> bool {
    let Transfer {
        count,
        source,
        destination,
    } = transfer;
    if count != source_stack.item_count {
        return false;
    }
    let Some(handler) = screen_handler
        .as_any_mut()
        .downcast_mut::<pumpkin_inventory::merchant::merchant_screen_handler::MerchantScreenHandler>()
    else {
        return false;
    };
    if !handler.complete_bedrock_trade(player.as_ref()) {
        return false;
    }

    update_slot_stack(player, handler, destination, dest_stack.clone());
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
    record_update(
        &mut state.updates,
        source.container_name.clone(),
        source.slot_id,
        ItemStack::EMPTY,
    );
    record_update(
        &mut state.updates,
        destination.container_name.clone(),
        destination.slot_id,
        dest_stack,
    );
    true
}

/// The end of a Take or Place: the source loses `count` items (and the created output with it),
/// both slots are written and reported.
fn finish_transfer(
    player: &Arc<Player>,
    screen_handler: &mut dyn ScreenHandler,
    state: &mut RequestState,
    transfer: Transfer<'_>,
    mut source_stack: ItemStack,
    dest_stack: &ItemStack,
) {
    let Transfer {
        count,
        source,
        destination,
    } = transfer;
    source_stack.decrement(count);
    if source.container_name.container_name == ContainerName::CreatedOutput
        && let Some(ref mut stack) = state.created_item
    {
        stack.decrement(count);
        if stack.is_empty() {
            state.created_item = None;
        }
    }
    let source_stack = if source_stack.is_empty() {
        ItemStack::EMPTY.clone()
    } else {
        source_stack
    };

    update_slot_stack(player, &mut *screen_handler, source, source_stack.clone());
    update_slot_stack(
        player,
        &mut *screen_handler,
        destination,
        dest_stack.clone(),
    );

    record_update(
        &mut state.updates,
        source.container_name.clone(),
        source.slot_id,
        &source_stack,
    );
    record_update(
        &mut state.updates,
        destination.container_name.clone(),
        destination.slot_id,
        dest_stack,
    );
}

/// Swap: exchanges two slots; output slots and slots that do not exist refuse.
fn swap(
    player: &Arc<Player>,
    screen_handler: &mut dyn ScreenHandler,
    state: &mut RequestState,
    slot1: &ItemStackRequestSlotInfo,
    slot2: &ItemStackRequestSlotInfo,
) -> bool {
    if get_output_screen_slot(&*screen_handler, slot1).is_some()
        || get_output_screen_slot(&*screen_handler, slot2).is_some()
        || !names_a_slot(&*screen_handler, slot1)
        || !names_a_slot(&*screen_handler, slot2)
    {
        return false;
    }

    let stack1 = get_slot_stack(&*screen_handler, slot1, state.created_item.as_ref());
    let stack2 = get_slot_stack(&*screen_handler, slot2, state.created_item.as_ref());

    update_slot_stack(player, &mut *screen_handler, slot1, stack2.clone());
    update_slot_stack(player, &mut *screen_handler, slot2, stack1.clone());

    record_update(
        &mut state.updates,
        slot1.container_name.clone(),
        slot1.slot_id,
        &stack2,
    );
    record_update(
        &mut state.updates,
        slot2.container_name.clone(),
        slot2.slot_id,
        &stack1,
    );
    true
}

/// Drop: throws up to `count` items from `source`; an output slot takes its inputs first.
fn drop_items(
    player: &Arc<Player>,
    screen_handler: &mut dyn ScreenHandler,
    state: &mut RequestState,
    count: u8,
    source: &ItemStackRequestSlotInfo,
) -> bool {
    let mut source_stack = get_slot_stack(&*screen_handler, source, state.created_item.as_ref());
    if source_stack.is_empty() {
        return false;
    }
    if let Some(out_slot) = get_output_screen_slot(&*screen_handler, source)
        && !state.crafting_inputs_consumed
    {
        let slot = screen_handler.get_behaviour().slots[out_slot].clone();
        if !slot.can_take_items(player.as_ref()) {
            return false;
        }
        slot.on_take_item(player.as_ref(), &source_stack);
        record_specialized_container_input_updates(&*screen_handler, &mut state.updates);
    }
    let count = count.min(source_stack.item_count);
    if count > 0 {
        let dropped_stack = source_stack.copy_with_count(count);
        player.drop_item(dropped_stack);

        source_stack.decrement(count);
        let source_stack = if source_stack.is_empty() {
            ItemStack::EMPTY.clone()
        } else {
            source_stack
        };

        update_slot_stack(player, &mut *screen_handler, source, source_stack.clone());

        record_update(
            &mut state.updates,
            source.container_name.clone(),
            source.slot_id,
            &source_stack,
        );
    }
    true
}

/// Destroy or Consume: removes up to `count` items from `source`. Merchant payments and crafting
/// inputs a craft already used are only reported.
fn destroy_or_consume(
    player: &Arc<Player>,
    screen_handler: &mut dyn ScreenHandler,
    state: &mut RequestState,
    count: u8,
    source: &ItemStackRequestSlotInfo,
) -> bool {
    if screen_handler.window_type() == Some(pumpkin_data::screen::WindowType::Merchant)
        && matches!(
            source.container_name.container_name,
            ContainerName::TradeIngredient1
                | ContainerName::TradeIngredient2
                | ContainerName::Trade2Ingredient1
                | ContainerName::Trade2Ingredient2
        )
    {
        // MerchantScreenHandler consumes both payment slots atomically when
        // Bedrock subsequently takes the CreatedOutput result.
        return true;
    }
    if state.crafting_inputs_consumed
        && source.container_name.container_name == ContainerName::CraftingInput
    {
        let source_stack = get_slot_stack(&*screen_handler, source, state.created_item.as_ref());
        record_update(
            &mut state.updates,
            source.container_name.clone(),
            source.slot_id,
            &source_stack,
        );
        return true;
    }

    let mut source_stack = get_slot_stack(&*screen_handler, source, state.created_item.as_ref());
    if source_stack.is_empty() {
        return false;
    }
    let count = count.min(source_stack.item_count);
    if count > 0 {
        source_stack.decrement(count);
        let source_stack = if source_stack.is_empty() {
            ItemStack::EMPTY.clone()
        } else {
            source_stack
        };

        update_slot_stack(player, &mut *screen_handler, source, source_stack.clone());

        record_update(
            &mut state.updates,
            source.container_name.clone(),
            source.slot_id,
            &source_stack,
        );
    }
    true
}
