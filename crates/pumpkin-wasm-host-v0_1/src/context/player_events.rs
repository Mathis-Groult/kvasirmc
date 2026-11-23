use std::sync::Arc;

use super::register_typed_event;
use crate::{events::WasmPluginEventHandler, pumpkin::plugin::event::EventType};
use pumpkin_core::plugin::Context;

pub fn register_player_event(
    ctx: &Context,
    handler: &Arc<WasmPluginEventHandler>,
    priority: pumpkin_core::plugin::EventPriority,
    blocking: bool,
    event_type: EventType,
) {
    use pumpkin_core::plugin::player::{
        bedrock_form_response::BedrockFormResponseEvent,
        changed_main_hand::PlayerChangedMainHandEvent, egg_throw::PlayerEggThrowEvent,
        exp_change::PlayerExpChangeEvent, fish::PlayerFishEvent,
        inventory_close::InventoryCloseEvent, inventory_interact::InventoryClickEvent,
        item_held::PlayerItemHeldEvent, player_change_world::PlayerChangeWorldEvent,
        player_chat::PlayerChatEvent, player_command_send::PlayerCommandSendEvent,
        player_custom_payload::PlayerCustomPayloadEvent,
        player_gamemode_change::PlayerGamemodeChangeEvent,
        player_interact_entity_event::PlayerInteractEntityEvent,
        player_interact_event::PlayerInteractEvent,
        player_interact_unknown_entity_event::PlayerInteractUnknownEntityEvent,
        player_join::PlayerJoinEvent, player_leave::PlayerLeaveEvent,
        player_login::PlayerLoginEvent, player_move::PlayerMoveEvent,
        player_permission_check::PlayerPermissionCheckEvent, player_respawn::PlayerRespawnEvent,
        player_teleport::PlayerTeleportEvent, player_toggle_flight_event::PlayerToggleFlightEvent,
        player_toggle_sneak_event::PlayerToggleSneakEvent,
        player_toggle_sprint_event::PlayerToggleSprintEvent,
    };

    macro_rules! register {
        ($event:ty) => {
            register_typed_event::<$event>(ctx, handler, priority, blocking)
        };
    }

    match event_type {
        EventType::PlayerJoinEvent => register!(PlayerJoinEvent),
        EventType::PlayerLeaveEvent => register!(PlayerLeaveEvent),
        EventType::PlayerLoginEvent => register!(PlayerLoginEvent),
        EventType::PlayerChatEvent => register!(PlayerChatEvent),
        EventType::PlayerCommandSendEvent => register!(PlayerCommandSendEvent),
        EventType::PlayerPermissionCheckEvent => register!(PlayerPermissionCheckEvent),
        EventType::PlayerMoveEvent => register!(PlayerMoveEvent),
        EventType::PlayerTeleportEvent => register!(PlayerTeleportEvent),
        EventType::PlayerChangeWorldEvent => register!(PlayerChangeWorldEvent),
        EventType::PlayerRespawnEvent => register!(PlayerRespawnEvent),
        EventType::PlayerExpChangeEvent => register!(PlayerExpChangeEvent),
        EventType::PlayerItemHeldEvent => register!(PlayerItemHeldEvent),
        EventType::PlayerChangedMainHandEvent => register!(PlayerChangedMainHandEvent),
        EventType::PlayerGamemodeChangeEvent => register!(PlayerGamemodeChangeEvent),
        EventType::PlayerCustomPayloadEvent => register!(PlayerCustomPayloadEvent),
        EventType::PlayerFishEvent => register!(PlayerFishEvent),
        EventType::PlayerEggThrowEvent => register!(PlayerEggThrowEvent),
        EventType::PlayerInteractUnknownEntityEvent => {
            register_typed_event::<PlayerInteractUnknownEntityEvent>(
                ctx, handler, priority, blocking,
            );
        }
        EventType::PlayerInteractEntityEvent => register!(PlayerInteractEntityEvent),
        EventType::PlayerInteractEvent => register!(PlayerInteractEvent),
        EventType::PlayerToggleSneakEvent => register!(PlayerToggleSneakEvent),
        EventType::PlayerToggleFlightEvent => register!(PlayerToggleFlightEvent),
        EventType::PlayerToggleSprintEvent => register!(PlayerToggleSprintEvent),
        EventType::InventoryClickEvent => register!(InventoryClickEvent),
        EventType::InventoryCloseEvent => register!(InventoryCloseEvent),
        EventType::BedrockFormResponseEvent => register!(BedrockFormResponseEvent),
        _ => register_player_api_event(ctx, handler, priority, blocking, event_type),
    }
}

fn register_player_api_event(
    ctx: &Context,
    handler: &Arc<WasmPluginEventHandler>,
    priority: pumpkin_core::plugin::EventPriority,
    blocking: bool,
    event_type: EventType,
) {
    macro_rules! register {
        ($event:ty) => {
            register_typed_event::<$event>(ctx, handler, priority, blocking)
        };
    }

    match event_type {
        EventType::PlayerItemConsumeEvent => register!(pumpkin_core::plugin::api::events::player::player_item_consume::PlayerItemConsumeEvent),
        EventType::PlayerItemDamageEvent => register!(pumpkin_core::plugin::api::events::player::player_item_damage::PlayerItemDamageEvent),
        EventType::PlayerDropItemEvent => register!(pumpkin_core::plugin::api::events::player::player_drop_item::PlayerDropItemEvent),
        EventType::PlayerBedEnterEvent => register!(pumpkin_core::plugin::api::events::player::player_bed::PlayerBedEnterEvent),
        EventType::PlayerBedLeaveEvent => register!(pumpkin_core::plugin::api::events::player::player_bed::PlayerBedLeaveEvent),
        EventType::PlayerBucketEmptyEvent => register!(pumpkin_core::plugin::api::events::player::player_bucket::PlayerBucketEmptyEvent),
        EventType::PlayerBucketFillEvent => register!(pumpkin_core::plugin::api::events::player::player_bucket::PlayerBucketFillEvent),
        EventType::AsyncPlayerChatEvent => register!(pumpkin_core::plugin::api::events::player::async_player_chat::AsyncPlayerChatEvent),
        EventType::AsyncPlayerPreLoginEvent => register!(pumpkin_core::plugin::api::events::player::async_player_pre_login::AsyncPlayerPreLoginEvent),
        EventType::PlayerAdvancementDoneEvent => register!(pumpkin_core::plugin::api::events::player::player_advancement_done::PlayerAdvancementDoneEvent),
        EventType::PlayerAnimationEvent => register!(pumpkin_core::plugin::api::events::player::player_animation::PlayerAnimationEvent),
        EventType::PlayerArmorStandManipulateEvent => register!(pumpkin_core::plugin::api::events::player::player_armor_stand_manipulate::PlayerArmorStandManipulateEvent),
        EventType::PlayerBucketEntityEvent => register!(pumpkin_core::plugin::api::events::player::player_bucket_entity::PlayerBucketEntityEvent),
        EventType::PlayerChangedWorldEvent => register!(pumpkin_core::plugin::api::events::player::player_changed_world::PlayerChangedWorldEvent),
        EventType::PlayerChannelEvent => register!(pumpkin_core::plugin::api::events::player::player_channel::PlayerChannelEvent),
        EventType::PlayerCommandPreprocessEvent => register!(pumpkin_core::plugin::api::events::player::player_command_preprocess::PlayerCommandPreprocessEvent),
        EventType::PlayerEditBookEvent => register!(pumpkin_core::plugin::api::events::player::player_edit_book::PlayerEditBookEvent),
        EventType::PlayerElytraBoostEvent => register!(pumpkin_core::plugin::api::events::player::player_elytra_boost::PlayerElytraBoostEvent),
        EventType::PlayerExpCooldownChangeEvent => register!(pumpkin_core::plugin::api::events::player::player_exp_cooldown_change::PlayerExpCooldownChangeEvent),
        EventType::PlayerHarvestBlockEvent => register!(pumpkin_core::plugin::api::events::player::player_harvest_block::PlayerHarvestBlockEvent),
        EventType::PlayerHideEntityEvent => register!(pumpkin_core::plugin::api::events::player::player_hide_entity::PlayerHideEntityEvent),
        EventType::PlayerItemBreakEvent => register!(pumpkin_core::plugin::api::events::player::player_item_break::PlayerItemBreakEvent),
        EventType::PlayerItemMendEvent => register!(pumpkin_core::plugin::api::events::player::player_item_mend::PlayerItemMendEvent),
        EventType::PlayerKickEvent => register!(pumpkin_core::plugin::api::events::player::player_kick::PlayerKickEvent),
        EventType::PlayerLeashEntityEvent => register!(pumpkin_core::plugin::api::events::player::player_leash_entity::PlayerLeashEntityEvent),
        EventType::PlayerLevelChangeEvent => register!(pumpkin_core::plugin::api::events::player::player_level_change::PlayerLevelChangeEvent),
        _ => register_player_api_event_more(ctx, handler, priority, blocking, event_type),
    }
}

fn register_player_api_event_more(
    ctx: &Context,
    handler: &Arc<WasmPluginEventHandler>,
    priority: pumpkin_core::plugin::EventPriority,
    blocking: bool,
    event_type: EventType,
) {
    macro_rules! register {
        ($event:ty) => {
            register_typed_event::<$event>(ctx, handler, priority, blocking)
        };
    }

    match event_type {
        EventType::PlayerLocaleChangeEvent => register!(pumpkin_core::plugin::api::events::player::player_locale_change::PlayerLocaleChangeEvent),
        EventType::PlayerNameEntityEvent => register!(pumpkin_core::plugin::api::events::player::player_name_entity::PlayerNameEntityEvent),
        EventType::PlayerOpenSignEvent => register!(pumpkin_core::plugin::api::events::player::player_open_sign::PlayerOpenSignEvent),
        EventType::PlayerPortalEvent => register!(pumpkin_core::plugin::api::events::player::player_portal::PlayerPortalEvent),
        EventType::PlayerPreLoginEvent => register!(pumpkin_core::plugin::api::events::player::player_pre_login::PlayerPreLoginEvent),
        EventType::PlayerRiptideEvent => register!(pumpkin_core::plugin::api::events::player::player_riptide::PlayerRiptideEvent),
        EventType::PlayerShearEntityEvent => register!(pumpkin_core::plugin::api::events::player::player_shear_entity::PlayerShearEntityEvent),
        EventType::PlayerShowEntityEvent => register!(pumpkin_core::plugin::api::events::player::player_show_entity::PlayerShowEntityEvent),
        EventType::PlayerSpawnChangeEvent => register!(pumpkin_core::plugin::api::events::player::player_spawn_change::PlayerSpawnChangeEvent),
        EventType::PlayerStatisticIncrementEvent => register!(pumpkin_core::plugin::api::events::player::player_statistic_increment::PlayerStatisticIncrementEvent),
        EventType::PlayerSwapHandsEvent => register!(pumpkin_core::plugin::api::events::player::player_swap_hands::PlayerSwapHandItemsEvent),
        EventType::PlayerTakeLecternBookEvent => register!(pumpkin_core::plugin::api::events::player::player_take_lectern_book::PlayerTakeLecternBookEvent),
        EventType::PlayerUnleashEntityEvent => register!(pumpkin_core::plugin::api::events::player::player_unleash_entity::PlayerUnleashEntityEvent),
        EventType::PlayerVelocityEvent => register!(pumpkin_core::plugin::api::events::player::player_velocity::PlayerVelocityEvent),
        EventType::PlayerInputEvent => register!(pumpkin_core::plugin::api::events::player::player_input::PlayerInputEvent),
        EventType::PlayerInteractAtEntityEvent => register!(pumpkin_core::plugin::api::events::player::player_interact_at_entity::PlayerInteractAtEntityEvent),
        EventType::PlayerLinksSendEvent => register!(pumpkin_core::plugin::api::events::player::player_links_send::PlayerLinksSendEvent),
        EventType::PlayerPickupArrowEvent => register!(pumpkin_core::plugin::api::events::player::player_pickup_arrow::PlayerPickupArrowEvent),
        EventType::PlayerRecipeBookClickEvent => register!(pumpkin_core::plugin::api::events::player::player_recipe_book_click::PlayerRecipeBookClickEvent),
        EventType::PlayerRecipeBookSettingsChangeEvent => register!(pumpkin_core::plugin::api::events::player::player_recipe_book_settings_change::PlayerRecipeBookSettingsChangeEvent),
        EventType::PlayerRecipeDiscoverEvent => register!(pumpkin_core::plugin::api::events::player::player_recipe_discover::PlayerRecipeDiscoverEvent),
        EventType::PlayerRegisterChannelEvent => register!(pumpkin_core::plugin::api::events::player::player_register_channel::PlayerRegisterChannelEvent),
        EventType::PlayerResourcePackStatusEvent => register!(pumpkin_core::plugin::api::events::player::player_resource_pack_status::PlayerResourcePackStatusEvent),
        EventType::PlayerSpawnLocationEvent => register!(pumpkin_core::plugin::api::events::player::player_spawn_location::PlayerSpawnLocationEvent),
        EventType::PlayerUnregisterChannelEvent => register!(pumpkin_core::plugin::api::events::player::player_unregister_channel::PlayerUnregisterChannelEvent),
        _ => {
            tracing::error!("non-player event should not be routed to register_player_event");
        }
    }
}
