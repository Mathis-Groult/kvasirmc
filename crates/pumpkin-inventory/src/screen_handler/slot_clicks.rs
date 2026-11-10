//! What a click in a screen does, one function per kind of click.
//!
//! `dispatch` picks the function from the action the client sent. The functions are generic over
//! the screen handler so that every kind of screen shares them; they only use what the
//! [`ScreenHandler`] trait offers.

use std::cmp::max;
use std::sync::Arc;

use pumpkin_data::data_component_impl::EquipmentType;
use pumpkin_data::data_component_impl::{EquipmentSlot, EquippableImpl};
use pumpkin_data::item_stack::ItemStack;
use pumpkin_protocol::java::server::play::SlotActionType;
use tracing::warn;

use super::{InventoryPlayer, SLOT_INDEX_OUTSIDE, ScreenHandler};
use crate::container_click::MouseClick;
use crate::inventory::Inventory;
use crate::slot::Slot;

/// Does what one click on a screen asks for.
pub(super) fn dispatch<H: ScreenHandler + ?Sized>(
    handler: &mut H,
    slot_index: i32,
    button: i32,
    action_type: &SlotActionType,
    player: &dyn InventoryPlayer,
) {
    if *action_type == SlotActionType::PickupAll && button == 0 {
        pick_up_all(handler, player);
    } else if *action_type == SlotActionType::QuickCraft {
        quick_craft(handler, slot_index, button, player);
    } else if *action_type == SlotActionType::Throw {
        throw(handler, slot_index, button, player);
    } else if *action_type == SlotActionType::Clone {
        clone_stack(handler, slot_index, player);
    } else if (*action_type == SlotActionType::Pickup || *action_type == SlotActionType::QuickMove)
        && (button == 0 || button == 1)
    {
        click_slot(handler, slot_index, button, action_type, player);
    } else if *action_type == SlotActionType::Swap && ((0..9).contains(&button) || button == 40) {
        swap_with_hotbar(handler, slot_index, button, player);
    }
}

/// A double click: gathers the stacks equal to the cursor's onto the cursor.
fn pick_up_all<H: ScreenHandler + ?Sized>(handler: &mut H, player: &dyn InventoryPlayer) {
    let behavior = handler.get_behaviour_mut();
    let mut cursor_stack = behavior
        .cursor_stack
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut to_pick_up = cursor_stack.get_max_stack_size() - cursor_stack.item_count;

    for (index, slot) in behavior.slots.iter().enumerate() {
        if to_pick_up == 0 {
            break;
        }

        let item_stack = slot.get_cloned_stack();
        if !item_stack.are_items_and_components_equal(&cursor_stack) {
            continue;
        }

        if !behavior.can_grab_from_slot(index) || !slot.allow_modification(player) {
            continue;
        }

        let taken_stack = slot.safe_take(
            item_stack.item_count.min(to_pick_up),
            cursor_stack.get_max_stack_size() - cursor_stack.item_count,
            player,
        );
        to_pick_up -= taken_stack.item_count;
        cursor_stack.increment(taken_stack.item_count);
    }
}

/// The three stages of a drag across slots: start (0), add a slot (1), end (2).
fn quick_craft<H: ScreenHandler + ?Sized>(
    handler: &mut H,
    slot_index: i32,
    button: i32,
    player: &dyn InventoryPlayer,
) {
    let drag_type = button & 3;
    let drag_button = (button >> 2) & 3;
    if drag_type == 0 {
        handler.get_behaviour_mut().drag_slots.clear();
    } else if drag_type == 1 {
        add_drag_slot(handler, slot_index);
    } else if drag_type == 2 && !handler.get_behaviour().drag_slots.is_empty() {
        finish_drag(handler, drag_button, player);
    }
}

/// Adds a slot to the drag when the cursor's stack could go into it.
fn add_drag_slot<H: ScreenHandler + ?Sized>(handler: &mut H, slot_index: i32) {
    let behaviour = handler.get_behaviour_mut();
    if slot_index < 0 {
        warn!("Invalid slot index for drag action: {slot_index}. Must be >= 0");
        return;
    }
    let cursor_stack = behaviour
        .cursor_stack
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let slot = &behaviour.slots[slot_index as usize];
    let stack = slot.get_stack();
    if behaviour.can_put_in_slot(slot_index as usize)
        && !cursor_stack.is_empty()
        && slot.can_insert(&cursor_stack)
        && (stack.are_items_and_components_equal(&cursor_stack) || stack.is_empty())
        && slot.get_max_item_count_for_stack(&stack) > stack.item_count
    {
        behaviour.drag_slots.push(slot_index as u32);
    }
}

/// Spreads the cursor's stack over the dragged slots.
fn finish_drag<H: ScreenHandler + ?Sized>(
    handler: &mut H,
    drag_button: i32,
    player: &dyn InventoryPlayer,
) {
    let behaviour = handler.get_behaviour_mut();
    if behaviour.drag_slots.len() == 1 {
        let slot = behaviour.drag_slots[0] as i32;
        behaviour.drag_slots.clear();
        handler.internal_on_slot_click(slot, drag_button, SlotActionType::Pickup, player);

        return;
    }
    if drag_button == 2 && !player.has_infinite_materials() {
        return; // Only creative
    }

    let mut cursor_stack = behaviour
        .cursor_stack
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let initial_count = cursor_stack.item_count;
    let slots_count = behaviour.drag_slots.len();
    for slot_index in &behaviour.drag_slots {
        let Some(slot) = behaviour.slots.get(*slot_index as usize).cloned() else {
            continue;
        };
        let stack = slot.get_stack();

        if behaviour.can_put_in_slot(*slot_index as usize)
            && (stack.are_items_and_components_equal(&cursor_stack) || stack.is_empty())
            && slot.can_insert(&cursor_stack)
        {
            let mut inserting_count = match drag_button {
                0 => (initial_count as usize)
                    .checked_div(slots_count)
                    .map_or(0, |c| c as u8),
                1 => 1,
                2 => {
                    cursor_stack.item_count = cursor_stack.get_max_stack_size();
                    cursor_stack.item_count
                }
                _ => 0,
            };
            inserting_count = inserting_count
                .min(max(
                    0,
                    slot.get_max_item_count_for_stack(&stack) - stack.item_count,
                ))
                .min(cursor_stack.item_count);
            if inserting_count > 0 {
                let mut new_stack = stack.clone();
                if new_stack.is_empty() {
                    new_stack = cursor_stack.copy_with_count(0);
                }
                new_stack.increment(inserting_count);
                slot.set_stack(new_stack);
                if drag_button != 2 {
                    cursor_stack.decrement(inserting_count);
                }
                if cursor_stack.is_empty() {
                    *cursor_stack = ItemStack::EMPTY.clone();
                    break;
                }
            }
        }
    }

    if drag_button == 2 {
        *cursor_stack = ItemStack::EMPTY.clone();
    }
    behaviour.drag_slots.clear();
}

/// Throws one item, or the whole stack, out of a slot.
fn throw<H: ScreenHandler + ?Sized>(
    handler: &H,
    slot_index: i32,
    button: i32,
    player: &dyn InventoryPlayer,
) {
    if slot_index >= 0
        && handler
            .get_behaviour()
            .can_grab_from_slot(slot_index as usize)
        && handler
            .get_behaviour()
            .cursor_stack
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
    {
        let slot = handler.get_behaviour().slots[slot_index as usize].clone();
        let prev_stack = slot.get_cloned_stack();
        if !prev_stack.is_empty() {
            if button == 1 {
                // Throw all
                while slot
                    .get_cloned_stack()
                    .are_items_and_components_equal(&prev_stack)
                {
                    let drop_stack = slot.safe_take(prev_stack.item_count, u8::MAX, player);
                    // Vanilla stops once a take gives nothing; a slot that refuses to give
                    // (a smithing output without a recipe) would otherwise loop forever.
                    if drop_stack.is_empty() {
                        break;
                    }
                    player.drop_item(drop_stack, true);
                    // player.handleCreativeModeItemDrop(itemStack);
                }
            } else {
                let drop_stack = slot.safe_take(1, u8::MAX, player);
                if !drop_stack.is_empty() {
                    slot.on_take_item(player, &drop_stack);
                    player.drop_item(drop_stack, true);
                }
            }
        }
    }
}

/// Copies a slot's stack onto an empty cursor (creative mode).
fn clone_stack<H: ScreenHandler + ?Sized>(
    handler: &mut H,
    slot_index: i32,
    player: &dyn InventoryPlayer,
) {
    if player.has_infinite_materials() && slot_index >= 0 {
        let behaviour = handler.get_behaviour_mut();
        let mut cursor_stack = behaviour
            .cursor_stack
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !cursor_stack.is_empty() {
            return;
        }
        let slot = behaviour.slots[slot_index as usize].clone();
        let stack = slot.get_stack();
        *cursor_stack = stack.copy_with_count(stack.get_max_stack_size());
    }
}

/// A left or right click, or a shift click.
fn click_slot<H: ScreenHandler + ?Sized>(
    handler: &mut H,
    slot_index: i32,
    button: i32,
    action_type: &SlotActionType,
    player: &dyn InventoryPlayer,
) {
    let click_type = if button == 0 {
        MouseClick::Left
    } else {
        MouseClick::Right
    };

    // Drop item if outside inventory
    if slot_index == SLOT_INDEX_OUTSIDE {
        drop_from_cursor(handler, &click_type, player);
    } else if *action_type == SlotActionType::QuickMove {
        quick_move_slot(handler, slot_index, player);
    } else {
        pick_up_or_put_down(handler, slot_index, &click_type, player);
    }
}

/// A click outside the window drops the cursor's stack, or one item of it.
fn drop_from_cursor<H: ScreenHandler + ?Sized>(
    handler: &H,
    click_type: &MouseClick,
    player: &dyn InventoryPlayer,
) {
    let mut cursor_stack = handler
        .get_behaviour()
        .cursor_stack
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !cursor_stack.is_empty() {
        if *click_type == MouseClick::Left {
            player.drop_item(cursor_stack.clone(), true);
            *cursor_stack = ItemStack::EMPTY.clone();
        } else {
            player.drop_item(cursor_stack.split(1), true);
        }
    }
}

/// A shift click: moves the slot's stack until it no longer moves.
fn quick_move_slot<H: ScreenHandler + ?Sized>(
    handler: &mut H,
    slot_index: i32,
    player: &dyn InventoryPlayer,
) {
    if slot_index < 0 {
        return;
    }

    let slot = handler.get_behaviour().slots[slot_index as usize].clone();

    if !handler
        .get_behaviour()
        .can_grab_from_slot(slot_index as usize)
        || !slot.can_take_items(player)
    {
        return;
    }

    let mut moved_stack = handler.quick_move(player, slot_index);

    while !moved_stack.is_empty()
        && ItemStack::are_items_and_components_equal(&slot.get_cloned_stack(), &moved_stack)
    {
        moved_stack = handler.quick_move(player, slot_index);
    }
}

/// A left or right click on a slot: puts the cursor's stack down, picks the slot's stack up, or
/// swaps the two.
fn pick_up_or_put_down<H: ScreenHandler + ?Sized>(
    handler: &H,
    slot_index: i32,
    click_type: &MouseClick,
    player: &dyn InventoryPlayer,
) {
    if slot_index < 0 {
        return;
    }

    let slot = handler.get_behaviour().slots[slot_index as usize].clone();

    if *click_type == MouseClick::Left {
        slot.on_click(player);
    }

    let slot_stack = slot.get_cloned_stack();
    let can_put = handler.get_behaviour().can_put_in_slot(slot_index as usize);
    let can_grab = handler
        .get_behaviour()
        .can_grab_from_slot(slot_index as usize);
    let mut cursor_stack = handler
        .get_behaviour()
        .cursor_stack
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if *click_type == MouseClick::Right
        && bundle_click(
            &slot,
            &slot_stack,
            &mut cursor_stack,
            can_put,
            can_grab,
            player,
        )
    {
        return;
    }

    let equipment_slot = cursor_stack
        .get_data_component::<EquippableImpl>()
        .map_or(&EquipmentSlot::MAIN_HAND, |equippable| equippable.slot);

    if handler.handle_slot_click(
        player,
        click_type.clone(),
        slot.clone(),
        slot_stack.clone(),
        cursor_stack.clone(),
    ) {
        return;
    }
    let pickup = Pickup {
        slot: &slot,
        slot_index,
        is_left: *click_type == MouseClick::Left,
        can_put,
        can_grab,
        equipment_slot,
        player,
    };
    if slot_stack.is_empty() {
        put_on_empty_slot(&pickup, &mut cursor_stack);
    } else if slot.can_take_items(player)
        && !exchange_with_slot(&pickup, slot_stack, &mut cursor_stack)
    {
        return;
    }

    slot.mark_dirty();
}

/// What a left or right click on a slot works with.
#[derive(Clone, Copy)]
struct Pickup<'a> {
    slot: &'a Arc<dyn Slot>,
    slot_index: i32,
    is_left: bool,
    can_put: bool,
    can_grab: bool,
    equipment_slot: &'static EquipmentSlot,
    player: &'a dyn InventoryPlayer,
}

/// A right click with a bundle on either side: moves items in or out of the bundle. Returns
/// whether it did.
fn bundle_click(
    slot: &Arc<dyn Slot>,
    slot_stack: &ItemStack,
    cursor_stack: &mut ItemStack,
    can_put: bool,
    can_grab: bool,
    player: &dyn InventoryPlayer,
) -> bool {
    let mut intercepted = false;

    if can_put && slot.can_insert(cursor_stack) && !cursor_stack.is_empty() {
        let mut inner_slot_stack = slot.get_stack();
        if let Some(bundle) = inner_slot_stack
            .get_data_component_mut::<pumpkin_data::data_component_impl::BundleContentsImpl>()
            && bundle.try_insert(cursor_stack)
        {
            slot.set_stack(inner_slot_stack);
            intercepted = true;
        }
    }

    if !intercepted
        && can_grab
        && slot.can_take_items(player)
        && !slot_stack.is_empty()
        && let Some(bundle) = cursor_stack
            .get_data_component_mut::<pumpkin_data::data_component_impl::BundleContentsImpl>()
    {
        let mut inner_slot_stack = slot.get_stack();
        if bundle.try_insert(&mut inner_slot_stack) {
            if inner_slot_stack.item_count == 0 {
                inner_slot_stack = ItemStack::EMPTY.clone();
            }
            slot.set_stack(inner_slot_stack);
            intercepted = true;
        }
    }

    if !intercepted && can_grab && slot.can_take_items(player) && cursor_stack.is_empty() {
        let mut inner_slot_stack = slot.get_stack();
        if let Some(bundle) = inner_slot_stack
            .get_data_component_mut::<pumpkin_data::data_component_impl::BundleContentsImpl>()
            && let Some(extracted) = bundle.try_extract()
        {
            *cursor_stack = extracted;
            slot.set_stack(inner_slot_stack);
            intercepted = true;
        }
    }

    if !intercepted
        && can_put
        && slot_stack.is_empty()
        && let Some(bundle) = cursor_stack
            .get_data_component_mut::<pumpkin_data::data_component_impl::BundleContentsImpl>()
        && let Some(extracted) = bundle.try_extract()
    {
        let remainder = slot.insert_stack(extracted);
        if !remainder.is_empty() {
            bundle.items.insert(0, remainder);
        }
        intercepted = true;
    }

    if intercepted {
        if cursor_stack.item_count == 0 {
            *cursor_stack = ItemStack::EMPTY.clone();
        }
        slot.mark_dirty();
        return true;
    }
    false
}

/// Puts the cursor's stack (all of it, or one item) into an empty slot.
fn put_on_empty_slot(pickup: &Pickup, cursor_stack: &mut ItemStack) {
    let Pickup {
        slot,
        slot_index,
        is_left,
        can_put,
        equipment_slot,
        player,
        ..
    } = *pickup;
    if can_put && !cursor_stack.is_empty() {
        if equipment_slot.slot_type() == EquipmentType::HumanoidArmor
            && (5..9).contains(&slot_index)
        {
            player.enqueue_equipment_change(equipment_slot, cursor_stack);
        }

        let transfer_count = if is_left { cursor_stack.item_count } else { 1 };
        *cursor_stack = slot.insert_stack_count(cursor_stack.clone(), transfer_count);
    }
}

/// Takes the slot's stack onto an empty cursor, merges the cursor into it, or swaps the two.
/// Returns false when the click ends without marking the slot dirty.
fn exchange_with_slot(
    pickup: &Pickup,
    slot_stack: ItemStack,
    cursor_stack: &mut ItemStack,
) -> bool {
    let Pickup {
        slot,
        slot_index,
        is_left,
        can_put,
        can_grab,
        equipment_slot,
        player,
    } = *pickup;
    if cursor_stack.is_empty() && can_grab {
        let take_count = if is_left {
            slot_stack.item_count
        } else {
            slot_stack.item_count.div_ceil(2)
        };
        let taken = slot.try_take_stack_range(take_count, u8::MAX, player);
        if let Some(taken) = taken {
            // Reverse order of operations, shouldn't affect anything
            *cursor_stack = taken.clone();
            slot.on_take_item(player, &taken);

            if (5..9).contains(&slot_index) {
                let equipment_slot = cursor_stack
                    .get_data_component::<EquippableImpl>()
                    .map_or(&EquipmentSlot::MAIN_HAND, |equippable| equippable.slot);
                player.enqueue_equipment_change(equipment_slot, ItemStack::EMPTY);
            }
        }
    } else if can_put && slot.can_insert(cursor_stack) {
        let matching_stack = ItemStack::are_items_and_components_equal(&slot_stack, cursor_stack);
        if !matching_stack && !can_grab {
            return false;
        }
        if equipment_slot.slot_type() == EquipmentType::HumanoidArmor
            && (5..9).contains(&slot_index)
        {
            player.enqueue_equipment_change(equipment_slot, cursor_stack);
        }

        if matching_stack {
            let insert_count = if is_left { cursor_stack.item_count } else { 1 };
            *cursor_stack = slot.insert_stack_count(cursor_stack.clone(), insert_count);
        } else if cursor_stack.item_count <= slot.get_max_item_count_for_stack(cursor_stack) {
            let old_cursor_stack = cursor_stack.clone();
            *cursor_stack = slot_stack;
            slot.set_stack(old_cursor_stack);
        }
    } else if can_grab && ItemStack::are_items_and_components_equal(&slot_stack, cursor_stack) {
        let taken = slot.try_take_stack_range(
            slot_stack.item_count,
            cursor_stack
                .get_max_stack_size()
                .saturating_sub(cursor_stack.item_count),
            player,
        );

        if let Some(taken) = taken {
            cursor_stack.increment(taken.item_count);
            slot.on_take_item(player, &taken);
        }
    }
    true
}

/// A number key (or the off-hand key) over a slot: swaps it with a hotbar slot.
fn swap_with_hotbar<H: ScreenHandler + ?Sized>(
    handler: &H,
    slot_index: i32,
    button: i32,
    player: &dyn InventoryPlayer,
) {
    if slot_index < 0 {
        return;
    }
    let player_inventory = player.get_inventory();
    let mut button_stack = player_inventory.get_stack(button as usize);
    let source_slot = handler.get_behaviour().slots[slot_index as usize].clone();
    let source_stack = source_slot.get_cloned_stack();
    let can_put = handler.get_behaviour().can_put_in_slot(slot_index as usize);
    let can_grab = handler
        .get_behaviour()
        .can_grab_from_slot(slot_index as usize);

    if !button_stack.is_empty() || !source_stack.is_empty() {
        if button_stack.is_empty() {
            if can_grab && source_slot.can_take_items(player) {
                player_inventory.set_stack(button as usize, source_stack.clone());
                source_slot.set_stack(ItemStack::EMPTY.clone());
                source_slot.on_take_item(player, &source_stack);
            }
        } else if can_put && source_stack.is_empty() && source_slot.can_insert(&button_stack) {
            let max_count = source_slot.get_max_item_count_for_stack(&button_stack);
            if button_stack.item_count > max_count {
                source_slot.set_stack(button_stack.split(max_count));
                player_inventory.set_stack(button as usize, button_stack);
            } else {
                player_inventory.set_stack(button as usize, ItemStack::EMPTY.clone());
                source_slot.set_stack(button_stack);
            }
        } else if can_grab
            && can_put
            && source_slot.can_take_items(player)
            && source_slot.can_insert(&button_stack)
        {
            let max_count = source_slot.get_max_item_count_for_stack(&button_stack);
            if button_stack.item_count > max_count {
                source_slot.set_stack(button_stack.split(max_count));
                player_inventory.set_stack(button as usize, button_stack);
                source_slot.on_take_item(player, &source_stack);

                let mut displaced_stack = source_stack;
                player_inventory.insert_stack_anywhere(&mut displaced_stack);
                if !displaced_stack.is_empty() {
                    player.drop_item(displaced_stack, true);
                }
            } else {
                player_inventory.set_stack(button as usize, source_stack.clone());
                source_slot.set_stack(button_stack);
                source_slot.on_take_item(player, &source_stack);
            }
        }
    }
}
