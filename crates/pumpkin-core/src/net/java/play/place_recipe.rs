#[allow(clippy::wildcard_imports)]
use super::*;

mod layout;

impl JavaClient {
    pub fn handle_place_recipe(
        &self,
        server: &Arc<Server>,
        player: &Arc<Player>,
        packet: &SPlaceRecipe,
    ) {
        use crate::net::java::recipe_helper::{
            GenericIngredient, compute_biggest_craftable, take_n_ingredient,
        };
        use pumpkin_inventory::crafting::recipe_provider::RecipeProvider;

        let target_id = packet.recipe_display_id.0 as usize;
        let use_max = packet.use_max_items;

        let mut click_event = crate::plugin::api::events::player::player_recipe_book_click::PlayerRecipeBookClickEvent::new(
            player.clone(),
            format!("display_{}", packet.recipe_display_id.0),
            use_max,
        );
        server
            .plugin_manager
            .fire_blocking(server, &mut click_event);
        if click_event.cancelled {
            return;
        }

        let dynamic_recipes = server.recipe_manager.get_dynamic_recipes();

        let Some((grid_width, crafting_inv)) = layout::crafting_grid(player) else {
            return;
        };

        let grid_size = grid_width * grid_width;
        let Some(ingredient_slots) =
            layout::ingredient_layout(target_id, grid_width, &dynamic_recipes)
        else {
            return;
        };

        // Check if this exact recipe is already placed (determines stacking vs fresh fill).
        let recipe_matches = layout::recipe_already_placed(&ingredient_slots, &crafting_inv);

        // Read minimum count from occupied slots before clearing (needed for stacking).
        let current_min = if recipe_matches && !use_max {
            layout::placed_minimum(&ingredient_slots, &crafting_inv)
        } else {
            0
        };

        // Always clear the grid first, returning items to inventory.
        for i in 0..grid_size {
            let stack = crafting_inv.remove_stack(i);
            if !stack.is_empty() {
                player.inventory.offer(stack, false, player.as_ref());
            }
        }

        // Determine how many of each ingredient to place per slot.
        let active_ingredients: Vec<GenericIngredient<'_>> =
            ingredient_slots.iter().flatten().copied().collect();
        let amount_to_craft = if use_max {
            compute_biggest_craftable(&active_ingredients, &player.inventory)
        } else if recipe_matches {
            current_min.saturating_add(1)
        } else {
            1
        };

        if amount_to_craft == 0 {
            layout::send_screen_updates(player);
            return;
        }

        // Fill each grid slot with exactly `amount_to_craft` matching items.
        for (idx, ing) in ingredient_slots.iter().enumerate() {
            let Some(ingredient) = ing else { continue };
            let taken = take_n_ingredient(&player.inventory, ingredient, amount_to_craft);
            if !taken.is_empty() {
                crafting_inv.set_stack(idx, taken);
            }
        }

        layout::send_screen_updates(player);
    }
}

#[cfg(test)]
mod golden;
