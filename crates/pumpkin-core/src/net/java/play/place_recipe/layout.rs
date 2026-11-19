//! The steps of the place-recipe packet: the recipe's grid layout, whether it is already placed,
//! the count already placed and the screen update.

#[allow(clippy::wildcard_imports)]
use super::super::*;
use crate::net::java::recipe_helper::GenericIngredient;
use crate::server::recipe::DynamicRecipe;
use pumpkin_data::recipes::{CraftingRecipeTypes, RECIPES_COOKING, RECIPES_CRAFTING};
use pumpkin_data::screen::WindowType;
use pumpkin_inventory::Inventory;
use pumpkin_protocol::codec::recipe::OwnedCraftingRecipe;

/// The crafting grid of the player's open screen and its width: 3 for a crafting table, 2 for the
/// inventory; `None` for any other screen.
pub(super) fn crafting_grid(player: &Player) -> Option<(usize, Arc<dyn Inventory>)> {
    let screen_handler_arc = player
        .current_screen_handler
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let handler = screen_handler_arc
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let grid_width: usize = match handler.window_type() {
        Some(WindowType::Crafting) => 3,
        None => 2, // player inventory 2x2
        _ => return None,
    };
    Some((grid_width, handler.get_behaviour().slots[1].get_inventory()))
}

/// The ingredient each grid slot needs for the recipe with this display id (crafting recipes
/// first, then cooking ones, then the dynamic ones); `None` when it cannot be placed.
pub(super) fn ingredient_layout(
    target_id: usize,
    grid_width: usize,
    dynamic_recipes: &[DynamicRecipe],
) -> Option<Vec<Option<GenericIngredient<'_>>>> {
    // Count crafting display IDs.
    let crafting_display_count = RECIPES_CRAFTING
        .iter()
        .filter(|r| {
            !matches!(
                r,
                CraftingRecipeTypes::CraftingSpecial
                    | CraftingRecipeTypes::CraftingDecoratedPot { .. }
            )
        })
        .count();
    let cooking_display_count = RECIPES_COOKING.len();

    let grid_size = grid_width * grid_width;
    let mut ingredient_slots: Vec<Option<GenericIngredient<'_>>> = vec![None; grid_size];

    if target_id < crafting_display_count {
        crafting_layout(target_id, grid_width, &mut ingredient_slots)?;
    } else if target_id < crafting_display_count + cooking_display_count {
        // TODO: cooking recipes
        return None;
    } else {
        let dynamic_id = target_id - crafting_display_count - cooking_display_count;
        let Some(DynamicRecipe::Crafting(crafting)) = dynamic_recipes.get(dynamic_id) else {
            return None;
        };

        dynamic_layout(crafting, grid_width, &mut ingredient_slots);
    }
    Some(ingredient_slots)
}

/// Fills `ingredient_slots` for the crafting recipe with this display id; `None` when there is
/// none or it is not shaped, shapeless or a transmute.
fn crafting_layout(
    target_id: usize,
    grid_width: usize,
    ingredient_slots: &mut [Option<GenericIngredient<'_>>],
) -> Option<()> {
    let grid_size = grid_width * grid_width;
    // Crafting recipe
    let mut counter = 0usize;
    let recipe = RECIPES_CRAFTING.iter().find(|r| {
        if matches!(
            r,
            CraftingRecipeTypes::CraftingSpecial | CraftingRecipeTypes::CraftingDecoratedPot { .. }
        ) {
            return false;
        }
        let found = counter == target_id;
        counter += 1;
        found
    });
    let recipe = recipe?;

    match recipe {
        CraftingRecipeTypes::CraftingShaped { pattern, key, .. } => {
            for (row, row_str) in pattern.iter().enumerate() {
                for (col, ch) in row_str.chars().enumerate() {
                    if ch != ' '
                        && let Some(ing) = key.iter().find_map(|(k, v)| (*k == ch).then_some(v))
                        && row * grid_width + col < grid_size
                    {
                        ingredient_slots[row * grid_width + col] =
                            Some(GenericIngredient::Vanilla(ing));
                    }
                }
            }
        }
        CraftingRecipeTypes::CraftingShapeless { ingredients, .. } => {
            for (i, ing) in ingredients.iter().enumerate().take(grid_size) {
                ingredient_slots[i] = Some(GenericIngredient::Vanilla(ing));
            }
        }
        CraftingRecipeTypes::CraftingTransmute {
            input, material, ..
        } => {
            if grid_size >= 2 {
                ingredient_slots[0] = Some(GenericIngredient::Vanilla(input));
                ingredient_slots[1] = Some(GenericIngredient::Vanilla(material));
            }
        }
        _ => return None,
    }
    Some(())
}

/// Fills `ingredient_slots` for a dynamic (plugin) crafting recipe.
fn dynamic_layout<'a>(
    crafting: &'a OwnedCraftingRecipe,
    grid_width: usize,
    ingredient_slots: &mut [Option<GenericIngredient<'a>>],
) {
    let grid_size = grid_width * grid_width;
    match crafting {
        pumpkin_protocol::codec::recipe::OwnedCraftingRecipe::Shaped { pattern, key, .. } => {
            for (row, row_str) in pattern.iter().enumerate() {
                for (col, ch) in row_str.chars().enumerate() {
                    if ch != ' '
                        && let Some((_, ing)) = key.iter().find(|(k, _)| *k == ch)
                        && row * grid_width + col < grid_size
                    {
                        ingredient_slots[row * grid_width + col] =
                            Some(GenericIngredient::Dynamic(ing));
                    }
                }
            }
        }

        pumpkin_protocol::codec::recipe::OwnedCraftingRecipe::Shapeless { ingredients, .. } => {
            for (i, ing) in ingredients.iter().enumerate().take(grid_size) {
                ingredient_slots[i] = Some(GenericIngredient::Dynamic(ing));
            }
        }
    }
}

/// Whether the grid already holds this recipe (decides stacking vs a fresh fill).
pub(super) fn recipe_already_placed(
    ingredient_slots: &[Option<GenericIngredient<'_>>],
    crafting_inv: &Arc<dyn Inventory>,
) -> bool {
    let mut ok = true;
    for (idx, ing) in ingredient_slots.iter().enumerate() {
        let stack = crafting_inv.get_stack(idx);
        match ing {
            None => {
                if !stack.is_empty() {
                    ok = false;
                    break;
                }
            }
            Some(ingredient) => {
                if stack.is_empty() || !ingredient.match_item(stack.item) {
                    ok = false;
                    break;
                }
            }
        }
    }
    ok
}

/// The smallest count in the recipe's occupied grid slots (0 when none is occupied).
pub(super) fn placed_minimum(
    ingredient_slots: &[Option<GenericIngredient<'_>>],
    crafting_inv: &Arc<dyn Inventory>,
) -> u8 {
    let mut min = u8::MAX;
    for (idx, ing) in ingredient_slots.iter().enumerate() {
        if ing.is_some() {
            let stack = crafting_inv.get_stack(idx);
            if !stack.is_empty() {
                min = min.min(stack.item_count);
            }
        }
    }
    if min == u8::MAX { 0 } else { min }
}

/// Sends the open screen's content updates.
pub(super) fn send_screen_updates(player: &Player) {
    let screen_handler_arc = player
        .current_screen_handler
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    screen_handler_arc
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .send_content_updates();
}
