//! Vanilla 26.3 `ShelfBlock.useItemOn`: swap the held stack with the clicked slot, or, on a
//! powered shelf, swap the right end of the hotbar with the whole shelf chain.

use std::sync::Arc;

use pumpkin_data::HorizontalFacingExt;
use pumpkin_data::block_properties::AcaciaShelfLikeProperties;
use pumpkin_data::data_component_impl::{EquipmentSlot, UseEffectsImpl};
use pumpkin_data::game_event::GameEvent;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::sound::{Sound, SoundCategory};
use pumpkin_inventory::Inventory;
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;

use super::chain;
use crate::block::entities::shelf::ShelfBlockEntity;
use crate::block::{BlockActionResult, UseWithItemArgs};
use crate::entity::player::Player;
use crate::world::World;

const COLUMNS: f32 = 3.0;
const HOTBAR: usize = 9;

/// `SelectableSlotContainer.getHitSlot` for a one-row, three-column shelf: the column hit on the
/// front face, counted from the viewer's left. `None` when another face was clicked.
pub(super) fn hit_slot(
    facing: pumpkin_data::BlockDirection,
    face: pumpkin_data::BlockDirection,
    cursor: pumpkin_util::math::vector3::Vector3<f32>,
) -> Option<usize> {
    use pumpkin_data::BlockDirection::{East, North, South, West};
    if face != facing {
        return None;
    }
    let across = match face {
        North => 1.0 - cursor.x,
        South => cursor.x,
        West => cursor.z,
        East => 1.0 - cursor.z,
        _ => return None,
    };
    // `Mth.clamp(floor(x * 16 / (16 / 3)), 0, 2)`
    let column = (across * COLUMNS).floor().clamp(0.0, COLUMNS - 1.0);
    Some(column as usize)
}

pub(super) fn use_with_item(args: UseWithItemArgs<'_>) -> BlockActionResult {
    let UseWithItemArgs {
        world,
        position,
        player,
        hit,
        item_stack,
        equipment_slot,
        ..
    } = args;
    let Some(entity) = world.get_block_entity(position) else {
        return BlockActionResult::Pass;
    };
    let Some(shelf) = entity.as_any().downcast_ref::<ShelfBlockEntity>() else {
        return BlockActionResult::Pass;
    };
    if *equipment_slot != EquipmentSlot::MAIN_HAND {
        return BlockActionResult::Pass;
    }
    let state_id = world.get_block_state_id(position);
    let props = AcaciaShelfLikeProperties::from_state_id(state_id);
    let facing = props.facing.to_block_direction();
    let Some(slot) = hit_slot(facing, *hit.face, *hit.cursor_pos) else {
        return BlockActionResult::Pass;
    };

    if props.powered {
        return swap_hotbar(world, position, player, item_stack);
    }
    let held = item_stack.clone();
    let swapped = swap_single_item(&held, player, shelf, slot, world, position);
    if swapped {
        let sound = if held.is_empty() {
            Sound::BlockShelfTakeItem
        } else {
            Sound::BlockShelfSingleSwap
        };
        world.play_block_sound(sound, SoundCategory::Blocks, *position);
    } else if held.is_empty() {
        return BlockActionResult::Pass;
    } else {
        world.play_block_sound(Sound::BlockShelfPlaceItem, SoundCategory::Blocks, *position);
    }
    *item_stack = player.inventory().held_item();
    BlockActionResult::SuccessServer
}

/// `swapSingleItem`: the shelf slot takes the held stack and the hand takes what was there.
fn swap_single_item(
    held: &ItemStack,
    player: &Arc<Player>,
    shelf: &ShelfBlockEntity,
    slot: usize,
    world: &Arc<World>,
    position: &BlockPos,
) -> bool {
    let old = swap_item_no_update(shelf, slot, held.clone());
    let new_held = if has_infinite_materials(player) && old.is_empty() {
        held.clone()
    } else {
        old.clone()
    };
    let event = interact_event(&new_held);
    player.inventory().set_held_item(new_held);
    changed(world, position, event);
    !old.is_empty()
}

/// `swapHotbar`: the chain's shelves, leftmost first, trade slots with the right end of the hotbar.
fn swap_hotbar(
    world: &Arc<World>,
    position: &BlockPos,
    player: &Arc<Player>,
    item_stack: &mut ItemStack,
) -> BlockActionResult {
    let chain = chain::connected_positions(world, *position);
    if chain.is_empty() {
        return BlockActionResult::Pass;
    }
    let inventory = player.inventory();
    let held_before = inventory.held_item();
    let mut moved = false;
    for (index, pos) in chain.iter().enumerate() {
        let Some(entity) = world.get_block_entity(pos) else {
            continue;
        };
        let Some(shelf) = entity.as_any().downcast_ref::<ShelfBlockEntity>() else {
            continue;
        };
        for j in 0..ShelfBlockEntity::INVENTORY_SIZE {
            let first = HOTBAR - (chain.len() - index) * ShelfBlockEntity::INVENTORY_SIZE;
            let slot = first + j;
            if slot >= HOTBAR {
                continue;
            }
            let from_hotbar = inventory.remove_stack(slot);
            let from_shelf = swap_item_no_update(shelf, j, from_hotbar.clone());
            if !from_shelf.is_empty() || !from_hotbar.is_empty() {
                inventory.set_stack(slot, from_shelf);
                moved = true;
            }
        }
        changed(world, pos, Some(GameEvent::EntityInteract));
    }
    if !moved {
        return BlockActionResult::Consume;
    }
    world.play_block_sound(Sound::BlockShelfMultiSwap, SoundCategory::Blocks, *position);
    player.sync_inventory_to_client();
    let held_after = inventory.held_item();
    let unchanged = held_after.are_equal(&held_before);
    *item_stack = held_after;
    if unchanged {
        return BlockActionResult::Success;
    }
    BlockActionResult::SuccessServer
}

/// `ShelfBlockEntity.swapItemNoUpdate`: puts `stack` in the slot and returns what was there.
fn swap_item_no_update(shelf: &ShelfBlockEntity, slot: usize, stack: ItemStack) -> ItemStack {
    let old = shelf.get_stack(slot);
    shelf.set_stack(slot, stack);
    old
}

fn has_infinite_materials(player: &Player) -> bool {
    matches!(player.gamemode.load(), GameMode::Creative)
}

/// The vibration of a swap: `ITEM_INTERACT_FINISH`, unless the stack now in the hand carries
/// `USE_EFFECTS` with `interact_vibrations` off (spears), which swaps silently.
pub(super) fn interact_event(new_held: &ItemStack) -> Option<GameEvent> {
    let silent = new_held
        .get_data_component::<UseEffectsImpl>()
        .is_some_and(|effects| !effects.interact_vibrations);
    (!silent).then_some(GameEvent::ItemInteractFinish)
}

/// `setChanged(event)`: the shelf changed because of the player; no event, no vibration.
fn changed(world: &Arc<World>, position: &BlockPos, event: Option<GameEvent>) {
    if let Some(event) = event {
        world.emit_game_event(event.name(), position.to_centered_f64());
    }
}
