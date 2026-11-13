//! Cleanup of the events that carry more than one resource or nested resources.

use pumpkin_wasm_host_common::state::PluginHostState;

use super::{cleanup_item_stack, cleanup_player, cleanup_text_component, cleanup_world};
use crate::pumpkin::plugin::event::{
    AsyncPlayerChatEventData, BlockBreakEventData, BlockBrushEventData, BlockCookEventData,
    BlockDamageAbortEventData, BlockDispenseArmorEventData, BlockDispenseLootEventData,
    BlockDropItemEventData, BlockFertilizeEventData, BlockMultiPlaceEventData,
    BlockShearEntityEventData, CampfireStartEventData, CrafterCraftEventData,
    CreatureSpawnEventData, DialogShowEventData, EnchantItemEventData, EntityDyeEventData,
    HangingPlaceEventData, InventoryClickEventData, PiglinBarterEventData,
    PlayerChangeWorldEventData, PlayerChangedWorldEventData, PlayerChatEventData,
    PlayerDeathEventData, PlayerHarvestBlockEventData, PlayerJoinEventData, PlayerLeaveEventData,
    PlayerLoginEventData, PlayerNameEntityEventData, PlayerRespawnEventData,
    PlayerTakeLecternBookEventData, PrepareItemEnchantEventData, ServerBroadcastEventData,
    VaultDisplayItemEventData,
};

/// Releases the resources held by a `PlayerJoinEvent`.
pub(super) fn cleanup_player_join_event(state: &mut PluginHostState, data: &PlayerJoinEventData) {
    cleanup_player(state, &data.player);
    cleanup_text_component(state, &data.join_message);
}

/// Releases the resources held by a `PlayerLeaveEvent`.
pub(super) fn cleanup_player_leave_event(state: &mut PluginHostState, data: &PlayerLeaveEventData) {
    cleanup_player(state, &data.player);
    cleanup_text_component(state, &data.leave_message);
}

/// Releases the resources held by a `PlayerLoginEvent`.
pub(super) fn cleanup_player_login_event(state: &mut PluginHostState, data: &PlayerLoginEventData) {
    cleanup_player(state, &data.player);
    cleanup_text_component(state, &data.kick_message);
}

/// Releases the resources held by a `PlayerChatEvent`.
pub(super) fn cleanup_player_chat_event(state: &mut PluginHostState, data: &PlayerChatEventData) {
    cleanup_player(state, &data.player);
    for res in &data.recipients {
        cleanup_player(state, res);
    }
}

/// Releases the resources held by a `PlayerChangeWorldEvent`.
pub(super) fn cleanup_player_change_world_event(
    state: &mut PluginHostState,
    data: &PlayerChangeWorldEventData,
) {
    cleanup_player(state, &data.player);
    cleanup_world(state, &data.previous_world);
    cleanup_world(state, &data.new_world);
}

/// Releases the resources held by a `PlayerRespawnEvent`.
pub(super) fn cleanup_player_respawn_event(
    state: &mut PluginHostState,
    data: &PlayerRespawnEventData,
) {
    cleanup_player(state, &data.player);
    cleanup_world(state, &data.previous_world);
    cleanup_world(state, &data.respawned_world);
}

/// Releases the resources held by a `InventoryClickEvent`.
pub(super) fn cleanup_inventory_click_event(
    state: &mut PluginHostState,
    data: &InventoryClickEventData,
) {
    cleanup_player(state, &data.player);
    if let Some(res) = &data.clicked_item {
        cleanup_item_stack(state, res);
    }
    if let Some(res) = &data.cursor {
        cleanup_item_stack(state, res);
    }
}

/// Releases the resources held by a `BlockBreakEvent`.
pub(super) fn cleanup_block_break_event(state: &mut PluginHostState, data: &BlockBreakEventData) {
    if let Some(res) = &data.player {
        cleanup_player(state, res);
    }
}

/// Releases the resources held by a `DialogShowEvent`.
pub(super) fn cleanup_dialog_show_event(state: &mut PluginHostState, data: &DialogShowEventData) {
    cleanup_player(state, &data.player);
    cleanup_text_component(state, &data.dialog.title);
    for body in &data.dialog.body {
        match body {
            crate::pumpkin::plugin::java_dialogs::DialogBody::PlainMessage(c) => {
                cleanup_text_component(state, c);
            }
            crate::pumpkin::plugin::java_dialogs::DialogBody::Item(item) => {
                cleanup_item_stack(state, item);
            }
        }
    }
    for input in &data.dialog.inputs {
        use crate::pumpkin::plugin::java_dialogs::DialogInput;
        match input {
            DialogInput::Bool(b) => cleanup_text_component(state, &b.label),
            DialogInput::Text(t) => {
                cleanup_text_component(state, &t.label);
                cleanup_text_component(state, &t.placeholder);
            }
            DialogInput::NumberRange(n) => cleanup_text_component(state, &n.label),
            DialogInput::SingleOption(s) => {
                cleanup_text_component(state, &s.label);
                for opt in &s.options {
                    cleanup_text_component(state, opt);
                }
            }
        }
    }
    for button in &data.dialog.buttons {
        cleanup_text_component(state, &button.text);
        if let Some(tooltip) = &button.tooltip {
            cleanup_text_component(state, tooltip);
        }
    }
    for link in &data.dialog.links {
        if let crate::pumpkin::plugin::java_dialogs::LinkLabel::Custom(c) = &link.label {
            cleanup_text_component(state, c);
        }
    }
    if let Some(ext_title) = &data.dialog.external_title {
        cleanup_text_component(state, ext_title);
    }
}

/// Releases the resources held by a `ServerBroadcastEvent`.
pub(super) fn cleanup_server_broadcast_event(
    state: &mut PluginHostState,
    data: &ServerBroadcastEventData,
) {
    cleanup_text_component(state, &data.message);
    cleanup_text_component(state, &data.sender);
}

/// Releases the resources held by a `PlayerDeathEvent`.
pub(super) fn cleanup_player_death_event(state: &mut PluginHostState, data: &PlayerDeathEventData) {
    cleanup_player(state, &data.player);
    cleanup_text_component(state, &data.death_message);
}

/// Releases the resources held by a `EntityDyeEvent`.
pub(super) fn cleanup_entity_dye_event(state: &mut PluginHostState, data: &EntityDyeEventData) {
    if let Some(res) = &data.player {
        cleanup_player(state, res);
    }
}

/// Releases the resources held by a `PrepareItemEnchantEvent`.
pub(super) fn cleanup_prepare_item_enchant_event(
    state: &mut PluginHostState,
    data: &PrepareItemEnchantEventData,
) {
    cleanup_player(state, &data.player);
    cleanup_item_stack(state, &data.item);
}

/// Releases the resources held by a `EnchantItemEvent`.
pub(super) fn cleanup_enchant_item_event(state: &mut PluginHostState, data: &EnchantItemEventData) {
    cleanup_player(state, &data.player);
    cleanup_item_stack(state, &data.item);
}

/// Releases the resources held by a `HangingPlaceEvent`.
pub(super) fn cleanup_hanging_place_event(
    state: &mut PluginHostState,
    data: &HangingPlaceEventData,
) {
    if let Some(res) = &data.player {
        cleanup_player(state, res);
    }
}

/// Releases the resources held by a `BlockBrushEvent`.
pub(super) fn cleanup_block_brush_event(state: &mut PluginHostState, data: &BlockBrushEventData) {
    cleanup_world(state, &data.target_world);
    cleanup_player(state, &data.player);
    cleanup_item_stack(state, &data.item);
}

/// Releases the resources held by a `BlockCookEvent`.
pub(super) fn cleanup_block_cook_event(state: &mut PluginHostState, data: &BlockCookEventData) {
    cleanup_world(state, &data.target_world);
    cleanup_item_stack(state, &data.source);
    cleanup_item_stack(state, &data.result);
}

/// Releases the resources held by a `BlockDamageAbortEvent`.
pub(super) fn cleanup_block_damage_abort_event(
    state: &mut PluginHostState,
    data: &BlockDamageAbortEventData,
) {
    cleanup_player(state, &data.player);
    cleanup_world(state, &data.target_world);
    cleanup_item_stack(state, &data.item_in_hand);
}

/// Releases the resources held by a `BlockDispenseArmorEvent`.
pub(super) fn cleanup_block_dispense_armor_event(
    state: &mut PluginHostState,
    data: &BlockDispenseArmorEventData,
) {
    cleanup_world(state, &data.target_world);
    cleanup_item_stack(state, &data.item);
}

/// Releases the resources held by a `BlockDispenseLootEvent`.
pub(super) fn cleanup_block_dispense_loot_event(
    state: &mut PluginHostState,
    data: &BlockDispenseLootEventData,
) {
    cleanup_world(state, &data.target_world);
    for res in &data.items {
        cleanup_item_stack(state, res);
    }
}

/// Releases the resources held by a `BlockDropItemEvent`.
pub(super) fn cleanup_block_drop_item_event(
    state: &mut PluginHostState,
    data: &BlockDropItemEventData,
) {
    cleanup_world(state, &data.target_world);
    if let Some(res) = &data.player {
        cleanup_player(state, res);
    }
    for res in &data.items {
        cleanup_item_stack(state, res);
    }
}

/// Releases the resources held by a `BlockFertilizeEvent`.
pub(super) fn cleanup_block_fertilize_event(
    state: &mut PluginHostState,
    data: &BlockFertilizeEventData,
) {
    cleanup_world(state, &data.target_world);
    if let Some(res) = &data.player {
        cleanup_player(state, res);
    }
}

/// Releases the resources held by a `BlockMultiPlaceEvent`.
pub(super) fn cleanup_block_multi_place_event(
    state: &mut PluginHostState,
    data: &BlockMultiPlaceEventData,
) {
    cleanup_player(state, &data.player);
    cleanup_world(state, &data.target_world);
}

/// Releases the resources held by a `BlockShearEntityEvent`.
pub(super) fn cleanup_block_shear_entity_event(
    state: &mut PluginHostState,
    data: &BlockShearEntityEventData,
) {
    cleanup_world(state, &data.target_world);
    cleanup_item_stack(state, &data.item);
}

/// Releases the resources held by a `CampfireStartEvent`.
pub(super) fn cleanup_campfire_start_event(
    state: &mut PluginHostState,
    data: &CampfireStartEventData,
) {
    cleanup_world(state, &data.target_world);
    cleanup_item_stack(state, &data.item);
}

/// Releases the resources held by a `CrafterCraftEvent`.
pub(super) fn cleanup_crafter_craft_event(
    state: &mut PluginHostState,
    data: &CrafterCraftEventData,
) {
    cleanup_world(state, &data.target_world);
    cleanup_item_stack(state, &data.result);
}

/// Releases the resources held by a `VaultDisplayItemEvent`.
pub(super) fn cleanup_vault_display_item_event(
    state: &mut PluginHostState,
    data: &VaultDisplayItemEventData,
) {
    cleanup_world(state, &data.target_world);
    cleanup_item_stack(state, &data.item);
}

/// Releases the resources held by a `CreatureSpawnEvent`.
pub(super) fn cleanup_creature_spawn_event(
    state: &mut PluginHostState,
    data: &CreatureSpawnEventData,
) {
    cleanup_world(state, &data.target_world);
    if let Some(res) = &data.player {
        cleanup_player(state, res);
    }
}

/// Releases the resources held by a `PiglinBarterEvent`.
pub(super) fn cleanup_piglin_barter_event(
    state: &mut PluginHostState,
    data: &PiglinBarterEventData,
) {
    cleanup_item_stack(state, &data.input_item);
    for res in &data.outcome {
        cleanup_item_stack(state, res);
    }
}

/// Releases the resources held by a `AsyncPlayerChatEvent`.
pub(super) fn cleanup_async_player_chat_event(
    state: &mut PluginHostState,
    data: &AsyncPlayerChatEventData,
) {
    cleanup_player(state, &data.player);
    cleanup_text_component(state, &data.format);
}

/// Releases the resources held by a `PlayerChangedWorldEvent`.
pub(super) fn cleanup_player_changed_world_event(
    state: &mut PluginHostState,
    data: &PlayerChangedWorldEventData,
) {
    cleanup_player(state, &data.player);
    cleanup_world(state, &data.from_world);
    cleanup_world(state, &data.to_world);
}

/// Releases the resources held by a `PlayerHarvestBlockEvent`.
pub(super) fn cleanup_player_harvest_block_event(
    state: &mut PluginHostState,
    data: &PlayerHarvestBlockEventData,
) {
    cleanup_player(state, &data.player);
    for res in &data.harvested_items {
        cleanup_item_stack(state, res);
    }
}

/// Releases the resources held by a `PlayerNameEntityEvent`.
pub(super) fn cleanup_player_name_entity_event(
    state: &mut PluginHostState,
    data: &PlayerNameEntityEventData,
) {
    cleanup_player(state, &data.player);
    cleanup_text_component(state, &data.name);
}

/// Releases the resources held by a `PlayerTakeLecternBookEvent`.
pub(super) fn cleanup_player_take_lectern_book_event(
    state: &mut PluginHostState,
    data: &PlayerTakeLecternBookEventData,
) {
    cleanup_player(state, &data.player);
    cleanup_item_stack(state, &data.book);
}
