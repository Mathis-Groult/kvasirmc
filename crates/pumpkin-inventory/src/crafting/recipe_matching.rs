//! Matching one crafting recipe against the items in the crafting grid.
//!
//! A recipe is matched against what the grid holds: where the items sit (their bounding box) and
//! how many there are. Shaped recipes also have to fit the pattern cell by cell, shapeless ones
//! use each ingredient once, transmute recipes want exactly their two ingredients, and the
//! decorated pot reads four sherds from fixed slots.

use pumpkin_data::data_component_impl::ProvidesPotteryPatternImpl;
use pumpkin_data::item::Item;
use pumpkin_data::recipes::{CraftingRecipeTypes, RecipeIngredientTypes};
use pumpkin_data::tag;
use pumpkin_data::tag::Taggable;
use pumpkin_protocol::codec::recipe::{OwnedCraftingRecipe, OwnedRecipeIngredient};

use super::crafting_screen_handler::RecipeResult;
use super::recipe_provider::GenericRecipe;
use super::recipes::RecipeInputInventory;

/// What the grid holds: the size and the top-left corner of the box around the items, and how
/// many items there are.
pub(super) struct Occupancy {
    pub height: usize,
    pub width: usize,
    pub top_x: usize,
    pub top_y: usize,
    pub count: usize,
}

/// Something that says whether an item fits a place in a recipe.
trait Accepts {
    fn accepts(&self, item: &Item) -> bool;
}

impl Accepts for RecipeIngredientTypes {
    fn accepts(&self, item: &Item) -> bool {
        self.match_item(item)
    }
}

impl Accepts for OwnedRecipeIngredient {
    fn accepts(&self, item: &Item) -> bool {
        self.match_item(item)
    }
}

/// Checks if a recipe pattern is symmetrical horizontally.
fn is_symmetrical_horizontally<S: AsRef<str>>(pattern: &[S]) -> bool {
    let width = pattern.first().map_or(0, |s| s.as_ref().len());
    for row in pattern {
        let row = row.as_ref();
        if row.len() != width {
            return false;
        }
        for j in 0..width / 2 {
            if row.chars().nth(j) != row.chars().nth(width - j - 1) {
                return false;
            }
        }
    }
    true
}

fn ingredient_for<I>(key: &[(char, I)], symbol: char) -> Option<&I> {
    key.iter().find(|(k, _)| *k == symbol).map(|(_, v)| v)
}

/// A result of a recipe that adds nothing to the item.
const fn plain_result(item_id: String, count: u8) -> RecipeResult {
    RecipeResult {
        item_id,
        count,
        pot_decorations: None,
    }
}

/// Whether the pattern has the size of the box around the items and as many items as the grid
/// holds.
fn pattern_fits<S: AsRef<str>>(pattern: &[S], grid: &Occupancy) -> bool {
    if pattern.len() != grid.height || pattern.first().map_or(0, |f| f.as_ref().len()) != grid.width
    {
        return false;
    }
    grid.count
        == pattern
            .iter()
            .map(|l| l.as_ref().chars().filter(|c| *c != ' ').count())
            .sum::<usize>()
}

/// One cell of a pattern against the grid slot: a space needs an empty slot, a symbol an item its
/// ingredient accepts.
fn cell_matches<I: Accepts>(
    key: &[(char, I)],
    symbol: char,
    slot: &pumpkin_data::item_stack::ItemStack,
) -> bool {
    if symbol == ' ' {
        return slot.is_empty();
    }
    ingredient_for(key, symbol).is_some_and(|ingredient| ingredient.accepts(slot.item))
}

/// The pattern laid on the grid at the top-left corner of the items.
fn pattern_matches<I: Accepts, S: AsRef<str>>(
    key: &[(char, I)],
    pattern: &[S],
    grid: &Occupancy,
    inventory: &dyn RecipeInputInventory,
) -> bool {
    for (y, row_str) in pattern.iter().enumerate() {
        for (x, symbol) in row_str.as_ref().chars().enumerate() {
            let slot =
                inventory.get_stack((y + grid.top_y) * inventory.get_width() + (x + grid.top_x));
            if !cell_matches(key, symbol, &slot) {
                return false;
            }
        }
    }
    true
}

/// The pattern laid on the grid from the right, as a mirror image.
fn mirrored_pattern_matches<I: Accepts, S: AsRef<str>>(
    key: &[(char, I)],
    pattern: &[S],
    grid: &Occupancy,
    inventory: &dyn RecipeInputInventory,
) -> bool {
    for (y, row) in pattern.iter().enumerate() {
        let row = row.as_ref();
        for x in 0..row.len() {
            let Some(symbol) = row.chars().nth(x) else {
                return false;
            };
            let slot = inventory.get_stack(
                (y + grid.top_y) * inventory.get_height() + (grid.top_x + grid.width - 1 - x),
            );
            if !cell_matches(key, symbol, &slot) {
                return false;
            }
        }
    }
    true
}

/// A shaped recipe; the vanilla ones also match their mirror image when the pattern is not
/// symmetrical.
fn shaped_matches<I: Accepts, S: AsRef<str>>(
    key: &[(char, I)],
    pattern: &[S],
    grid: &Occupancy,
    inventory: &dyn RecipeInputInventory,
    mirrored_too: bool,
) -> bool {
    pattern_fits(pattern, grid)
        && (pattern_matches(key, pattern, grid, inventory)
            || (mirrored_too
                && !is_symmetrical_horizontally(pattern)
                && mirrored_pattern_matches(key, pattern, grid, inventory)))
}

/// A shapeless recipe: as many items as ingredients, each ingredient used for one item.
fn shapeless_matches<I: Accepts>(
    ingredients: &[I],
    grid: &Occupancy,
    inventory: &dyn RecipeInputInventory,
) -> bool {
    if grid.count != ingredients.len() {
        return false;
    }
    let mut ingredient_used = vec![false; ingredients.len()];
    'next_slot: for i in 0..inventory.size() {
        let slot = inventory.get_stack(i);
        if slot.is_empty() {
            continue 'next_slot;
        }
        for i in 0..ingredients.len() {
            if !ingredient_used[i] && ingredients[i].accepts(slot.item) {
                ingredient_used[i] = true;
                continue 'next_slot;
            }
        }
        return false;
    }
    true
}

/// A transmute recipe: two items, each the input or the material.
fn transmute_matches(
    input: &RecipeIngredientTypes,
    material: &RecipeIngredientTypes,
    grid: &Occupancy,
    inventory: &dyn RecipeInputInventory,
) -> bool {
    if grid.count != 2 {
        return false;
    }
    'item_stack: for i in 0..inventory.size() {
        let slot = inventory.get_stack(i);
        if slot.is_empty() {
            continue 'item_stack;
        }
        if !material.match_item(slot.item) && !input.match_item(slot.item) {
            return false;
        }
    }
    true
}

/// The decorated pot: four sherds or bricks in the slots north, west, east and south of the
/// middle of a 3x3 grid; the pot shows them.
fn decorated_pot_result(
    grid: &Occupancy,
    inventory: &dyn RecipeInputInventory,
) -> Option<RecipeResult> {
    if grid.count != 4 || inventory.get_width() != 3 || inventory.get_height() != 3 {
        return None;
    }
    let mut sherds = [0u16; 4];
    for (idx, position) in (1..=7).step_by(2).enumerate() {
        let slot = inventory.get_stack(position);
        if slot.is_empty()
            || (!slot
                .item
                .has_tag(&tag::Item::MINECRAFT_DECORATED_POT_INGREDIENTS)
                && slot
                    .get_data_component::<ProvidesPotteryPatternImpl>()
                    .is_none())
        {
            return None;
        }
        sherds[idx] = slot.item.id;
    }
    Some(RecipeResult {
        item_id: "minecraft:decorated_pot".to_string(),
        count: 1,
        pot_decorations: Some(sherds),
    })
}

/// Checks if a crafting recipe matches the current inventory state.
pub(super) fn recipe_matches(
    recipe: GenericRecipe<'_>,
    grid: &Occupancy,
    inventory: &dyn RecipeInputInventory,
) -> Option<RecipeResult> {
    match recipe {
        GenericRecipe::Vanilla(CraftingRecipeTypes::CraftingShaped {
            key,
            pattern,
            result,
            ..
        }) => shaped_matches(key, pattern, grid, inventory, true)
            .then(|| plain_result(result.id.to_string(), result.count)),
        GenericRecipe::Vanilla(CraftingRecipeTypes::CraftingShapeless {
            ingredients,
            result,
            ..
        }) => shapeless_matches(ingredients, grid, inventory)
            .then(|| plain_result(result.id.to_string(), result.count)),
        GenericRecipe::Vanilla(CraftingRecipeTypes::CraftingTransmute {
            input,
            material,
            result,
            ..
        }) => transmute_matches(input, material, grid, inventory)
            .then(|| plain_result(result.id.to_string(), result.count)),
        GenericRecipe::Vanilla(CraftingRecipeTypes::CraftingDecoratedPot { .. }) => {
            decorated_pot_result(grid, inventory)
        }
        GenericRecipe::Dynamic(OwnedCraftingRecipe::Shaped {
            pattern,
            key,
            result,
            ..
        }) => shaped_matches(key, pattern, grid, inventory, false)
            .then(|| plain_result(result.item_id.clone(), result.count)),
        GenericRecipe::Dynamic(OwnedCraftingRecipe::Shapeless {
            ingredients,
            result,
            ..
        }) => shapeless_matches(ingredients, grid, inventory)
            .then(|| plain_result(result.item_id.clone(), result.count)),
        _ => None,
    }
}
