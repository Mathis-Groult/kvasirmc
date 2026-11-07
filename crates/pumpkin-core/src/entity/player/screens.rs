//! Opening, clicking and closing container screens.

use super::Player;
use crate::plugin::player::inventory_interact::InventoryClickEvent;
use crate::server::Server;
use pumpkin_data::{screen::WindowType, translation};
use pumpkin_inventory::player::player_screen_handler::PlayerScreenHandler;
use pumpkin_inventory::screen_handler::{
    ClickType, InventoryPlayer, ScreenHandler, ScreenHandlerFactory,
};
use pumpkin_protocol::bedrock::client::container_open::CContainerOpen;
use pumpkin_protocol::codec::var_long::VarLong;
use pumpkin_protocol::java::client::play::{
    CCloseContainer, COpenBook, COpenScreen, COpenSignEditor,
};
use pumpkin_protocol::java::server::play::{
    SClickSlot, SContainerButtonClick, SRenameItem, SlotActionType,
};
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::text::TextComponent;
use pumpkin_util::text::color::NamedColor;
use pumpkin_util::{GameMode, Hand};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tracing::warn;

impl Player {
    pub fn open_book(&self, hand: Hand) {
        let hand_val = match hand {
            Hand::Right => 0,
            Hand::Left => 1,
        };
        let packet = COpenBook::new(hand_val.into());
        self.try_send_client_packet(&packet);
    }

    pub fn open_sign_editor(&self, location: BlockPos, is_front_text: bool) {
        if let Some(player_arc) = self.world().get_player_by_uuid(self.gameprofile.id)
            && let Some(server) = self.world().server.upgrade()
        {
            let mut event =
                crate::plugin::api::events::player::player_open_sign::PlayerOpenSignEvent {
                    player: player_arc,
                    block_pos: location,
                    is_front: is_front_text,
                    cancelled: false,
                };
            server.plugin_manager.fire_blocking(&server, &mut event);
            if event.cancelled {
                return;
            }
        }
        self.try_enqueue_packet_editioned(
            &COpenSignEditor::new(location, is_front_text),
            &pumpkin_protocol::bedrock::client::open_sign::COpenSign::new(location, is_front_text),
        );
    }

    pub fn increment_screen_handler_sync_id(&self) {
        let current_id = self.screen_handler_sync_id.load(Ordering::Relaxed);
        self.screen_handler_sync_id
            .store(current_id % 100 + 1, Ordering::Relaxed);
    }

    pub fn close_handled_screen(&self) {
        let (sync_id, bedrock_window_type) = {
            let current_handler_guard = self
                .current_screen_handler
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let handler = current_handler_guard
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let sync_id = handler.sync_id();
            let window_type = handler.window_type();
            let bedrock_window_type = match window_type {
                Some(WindowType::Crafting) => 1,
                Some(WindowType::Furnace) => 2,
                Some(WindowType::Enchantment) => 3,
                Some(WindowType::BrewingStand) => 4,
                Some(WindowType::Anvil) => 5,
                Some(WindowType::Hopper) => 8,
                Some(WindowType::Beacon) => 13,
                Some(WindowType::BlastFurnace) => 27,
                Some(WindowType::Smoker) => 28,
                Some(WindowType::Stonecutter) => 29,
                Some(WindowType::CartographyTable) => 30,
                Some(WindowType::Grindstone) => 26,
                Some(WindowType::Loom) => 24,
                Some(WindowType::Smithing) => 34,
                _ => 0,
            };
            (sync_id, bedrock_window_type)
        };

        self.try_enqueue_packet_editioned(
            &CCloseContainer::new(sync_id.into()),
            &pumpkin_protocol::bedrock::server::container_close::SContainerClose {
                container_id: sync_id,
                container_type: bedrock_window_type,
                server_initiated_close: true,
            },
        );
        self.on_handled_screen_closed();
    }

    pub fn on_handled_screen_closed(&self) {
        let current_screen_handler: Arc<std::sync::Mutex<dyn ScreenHandler>> = self
            .current_screen_handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();

        let window_type = {
            let mut handler = current_screen_handler
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let wt = handler.window_type();
            handler.on_closed(self);
            wt
        };

        let world = self.living_entity.entity.world.load();
        let server = world.server.upgrade();
        if let Some(server) = server
            && let Some(player_arc) = world.get_player_by_uuid(self.gameprofile.id)
        {
            let mut event =
                crate::plugin::api::events::player::inventory_close::InventoryCloseEvent::new(
                    &player_arc,
                    window_type,
                );
            server.plugin_manager.fire_blocking(&server, &mut event);
        }

        let player_screen_handler: Arc<std::sync::Mutex<dyn ScreenHandler>> =
            self.player_screen_handler.clone();

        if !Arc::ptr_eq(&player_screen_handler, &current_screen_handler) {
            player_screen_handler
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .copy_shared_slots(current_screen_handler);
        }

        *self
            .current_screen_handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            self.player_screen_handler.clone();
        self.open_container_pos.store(None);
    }

    pub fn on_screen_handler_opened<T: ScreenHandler + ?Sized>(
        &self,
        screen_handler: &std::sync::Mutex<T>,
    ) {
        let mut screen_handler = screen_handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        screen_handler.add_listener(self.screen_handler_listener.clone());
        screen_handler.update_sync_handler(self.screen_handler_sync_handler.clone());
    }

    pub fn on_rename_item(self: &Arc<Self>, packet: &SRenameItem<'_>) {
        self.update_last_action_time();

        let mut prepare_event =
            crate::plugin::api::events::inventory::prepare_anvil::PrepareAnvilEvent::new(
                self.clone(),
                packet.item_name.to_string(),
                1,
            );
        if let Some(server) = self.world().server.upgrade() {
            server
                .plugin_manager
                .fire_blocking(&server, &mut prepare_event);
        }

        let screen_handler_arc = self
            .current_screen_handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let mut screen_handler = screen_handler_arc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if let Some(anvil_handler) = screen_handler
            .as_any_mut()
            .downcast_mut::<pumpkin_inventory::anvil::AnvilScreenHandler>()
        {
            anvil_handler.set_item_name(packet.item_name, self.has_infinite_materials());
        }
    }

    pub fn open_handled_screen(
        &self,
        screen_handler_factory: &dyn ScreenHandlerFactory,
        block_pos: Option<BlockPos>,
    ) -> Option<u8> {
        if !self
            .current_screen_handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_any()
            .is::<PlayerScreenHandler>()
        {
            self.close_handled_screen();
        }

        let server = self.world().server.upgrade();
        if let Some(server) = server
            && let Some(player_arc) = self.world().get_player_by_uuid(self.gameprofile.id)
        {
            let mut event =
                crate::plugin::api::events::inventory::inventory_open::InventoryOpenEvent::new(
                    player_arc,
                );
            server.plugin_manager.fire_blocking(&server, &mut event);
            if event.cancelled {
                return None;
            }
        }

        self.increment_screen_handler_sync_id();

        if let Some(screen_handler) = screen_handler_factory.create_screen_handler(
            self.screen_handler_sync_id.load(Ordering::Relaxed),
            &self.inventory,
            self,
        ) {
            let screen_handler_temp = screen_handler
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let sync_id = screen_handler_temp.sync_id();
            let window_type = screen_handler_temp.window_type()?;

            let display_name = screen_handler_factory.get_display_name();
            let java_packet =
                COpenScreen::new(sync_id.into(), (window_type as i32).into(), &display_name);

            let bedrock_window_type = match window_type {
                WindowType::Crafting => 1,
                WindowType::Furnace => 2,
                WindowType::Enchantment => 3,
                WindowType::BrewingStand => 4,
                WindowType::Anvil => 5,
                WindowType::Hopper => 8,
                WindowType::Beacon => 13,
                WindowType::Merchant => 15,
                WindowType::BlastFurnace => 27,
                WindowType::Smoker => 28,
                WindowType::Stonecutter => 29,
                WindowType::CartographyTable => 30,
                WindowType::Grindstone => 26,
                WindowType::Loom => 24,
                WindowType::Smithing => 34,
                _ => 0,
            };

            let bedrock_packet = CContainerOpen {
                container_id: sync_id,
                container_type: bedrock_window_type,
                position: block_pos.unwrap_or(BlockPos::ZERO),
                target_entity_id: VarLong(-1),
            };

            self.try_enqueue_packet_editioned(&java_packet, &bedrock_packet);

            drop(screen_handler_temp);
            self.on_screen_handler_opened(&screen_handler);
            *self
                .current_screen_handler
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = screen_handler;
            self.open_container_pos.store(block_pos);
            Some(self.screen_handler_sync_id.load(Ordering::Relaxed))
        } else {
            if self.is_spectator() {
                self.send_system_message_raw(
                    &TextComponent::translate(translation::java::CONTAINER_SPECTATORCANTOPEN, [])
                        .color_named(NamedColor::Red),
                    true,
                );
            }
            None
        }
    }

    pub fn open_handled_screen_direct(
        &self,
        screen_handler: Arc<std::sync::Mutex<dyn ScreenHandler>>,
        title: &TextComponent,
    ) {
        if !self
            .current_screen_handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_any()
            .is::<PlayerScreenHandler>()
        {
            self.close_handled_screen();
        }

        let screen_handler_temp = screen_handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let sync_id = screen_handler_temp.sync_id();
        let Some(window_type) = screen_handler_temp.window_type() else {
            return;
        };

        let java_packet = COpenScreen::new(sync_id.into(), (window_type as i32).into(), title);

        let bedrock_window_type = match window_type {
            WindowType::Crafting => 1,
            WindowType::Furnace => 2,
            WindowType::Enchantment => 3,
            WindowType::BrewingStand => 4,
            WindowType::Anvil => 5,
            WindowType::Hopper => 8,
            WindowType::Beacon => 13,
            WindowType::BlastFurnace => 27,
            WindowType::Smoker => 28,
            WindowType::Stonecutter => 29,
            WindowType::CartographyTable => 30,
            WindowType::Grindstone => 26,
            WindowType::Loom => 24,
            WindowType::Smithing => 34,
            _ => 0,
        };

        let bedrock_packet = CContainerOpen {
            container_id: sync_id,
            container_type: bedrock_window_type,
            position: BlockPos::ZERO,
            target_entity_id: VarLong(-1),
        };

        self.try_enqueue_packet_editioned(&java_packet, &bedrock_packet);

        drop(screen_handler_temp);
        self.on_screen_handler_opened(&screen_handler);
        *self
            .current_screen_handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = screen_handler;
        self.open_container_pos.store(None);
    }

    #[allow(clippy::too_many_lines)]
    pub fn on_slot_click(self: &Arc<Self>, packet: SClickSlot, server: &Arc<Server>) {
        self.update_last_action_time();

        let (
            sync_id,
            can_use,
            is_slot_valid,
            available_slots,
            clicked_item,
            cursor_item,
            window_type,
        ) = {
            let screen_handler_arc = self
                .current_screen_handler
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            let screen_handler = screen_handler_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);

            let b = screen_handler.get_behaviour();
            let sync_id = b.sync_id;
            let can_use = screen_handler.can_use(self.as_ref());
            let is_slot_valid = screen_handler.is_slot_valid(i32::from(packet.slot));
            let available_slots = b.slots.len();

            let clicked_item = (packet.slot >= 0 && (packet.slot as usize) < b.slots.len())
                .then(|| b.slots[packet.slot as usize].get_cloned_stack());

            let cursor_item = Some(
                b.cursor_stack
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone(),
            );

            let window_type = screen_handler.window_type();

            (
                sync_id,
                can_use,
                is_slot_valid,
                available_slots,
                clicked_item,
                cursor_item,
                window_type,
            )
        };

        let slot = packet.slot;
        let raw_slot = slot; // For now raw_slot == slot, as we don't have separate view/inventory indexing yet
        let hotbar_button = if matches!(packet.mode, SlotActionType::Swap) {
            packet.button
        } else {
            -1
        };

        let click_type = match packet.mode {
            SlotActionType::Pickup => {
                if packet.button == SClickSlot::BUTTON_LEFT {
                    ClickType::Left
                } else {
                    ClickType::Right
                }
            }
            SlotActionType::QuickMove => {
                if packet.button == SClickSlot::BUTTON_LEFT {
                    ClickType::ShiftLeft
                } else {
                    ClickType::ShiftRight
                }
            }
            SlotActionType::Swap => ClickType::NumberKey(packet.button as u8),
            SlotActionType::Clone => ClickType::Middle,
            SlotActionType::Throw => {
                if packet.button == SClickSlot::BUTTON_DROP_SINGLE {
                    ClickType::Drop
                } else {
                    ClickType::ControlDrop
                }
            }
            SlotActionType::QuickCraft => {
                if [0, 4, 8].contains(&packet.button) {
                    ClickType::Left
                } else if [1, 5, 9].contains(&packet.button) {
                    ClickType::Right
                } else {
                    ClickType::Middle
                }
            }
            SlotActionType::PickupAll => ClickType::DoubleClick,
        };

        if i32::from(sync_id) != packet.sync_id.0 {
            send_cancellable_blocking! {{
                server;
                InventoryClickEvent::new(
                    self,
                    window_type,
                    click_type,
                    slot,
                    raw_slot,
                    None,
                    cursor_item,
                    i32::from(hotbar_button),
                );
                'after: {}
                'cancelled: {}
            }}
            return;
        }

        if self.gamemode.load() == GameMode::Spectator {
            let screen_handler_arc = self
                .current_screen_handler
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            screen_handler_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .sync_state();
            return;
        }

        if !can_use {
            warn!(
                "Player {} interacted with invalid menu {:?}",
                self.gameprofile.name, window_type
            );
            return;
        }

        if !is_slot_valid {
            warn!(
                "Player {} clicked invalid slot index: {}, available slots: {}",
                self.gameprofile.name, slot, available_slots
            );
            return;
        }

        let cancel_screen = || {
            let screen_handler_arc = self
                .current_screen_handler
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            screen_handler_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .cancel();
        };

        send_cancellable_blocking! {{
            server;
            InventoryClickEvent::new(
                self,
                window_type,
                click_type,
                slot,
                raw_slot,
                clicked_item.clone(),
                cursor_item,
                i32::from(hotbar_button),
            );
            'after: {}
            'cancelled: {
                cancel_screen();
                return;
            }
        }}

        let mut interact_event =
            crate::plugin::api::events::inventory::inventory_interact::InventoryInteractEvent::new(
                self.clone(),
            );
        if let Some(server) = self.world().server.upgrade() {
            server
                .plugin_manager
                .fire_blocking(&server, &mut interact_event);
        }
        if interact_event.cancelled {
            cancel_screen();
            return;
        }

        if slot == 0
            && let Some(ref stack) = clicked_item
            && !stack.is_empty()
        {
            let mut craft_event =
                crate::plugin::api::events::inventory::craft_item::CraftItemEvent::new(
                    self.clone(),
                    stack.item.registry_key.to_string(),
                );
            let mut prep_craft =
                crate::plugin::api::events::inventory::prepare_item_craft::PrepareItemCraftEvent::new(
                    self.clone(),
                    stack.item.registry_key.to_string(),
                );
            if let Some(server) = self.world().server.upgrade() {
                server
                    .plugin_manager
                    .fire_blocking(&server, &mut craft_event);
                server
                    .plugin_manager
                    .fire_blocking(&server, &mut prep_craft);
            }
            if craft_event.cancelled || prep_craft.cancelled {
                cancel_screen();
                return;
            }
        }

        if window_type == Some(WindowType::Smithing)
            && slot == 3
            && let Some(ref stack) = clicked_item
            && !stack.is_empty()
        {
            let mut smith_event =
                crate::plugin::api::events::inventory::smith_item::SmithItemEvent::new(
                    self.clone(),
                    stack.item.registry_key.to_string(),
                );
            let mut prep_smith =
                crate::plugin::api::events::inventory::prepare_smithing::PrepareSmithingEvent::new(
                    self.clone(),
                    Some(stack.item.registry_key.to_string()),
                );
            if let Some(server) = self.world().server.upgrade() {
                server
                    .plugin_manager
                    .fire_blocking(&server, &mut smith_event);
                server
                    .plugin_manager
                    .fire_blocking(&server, &mut prep_smith);
            }
            if smith_event.cancelled {
                cancel_screen();
                return;
            }
        }

        if (window_type == Some(WindowType::Furnace)
            || window_type == Some(WindowType::BlastFurnace)
            || window_type == Some(WindowType::Smoker))
            && slot == 2
            && let Some(ref stack) = clicked_item
            && !stack.is_empty()
        {
            let mut extract_event =
                crate::plugin::api::events::inventory::furnace_extract::FurnaceExtractEvent::new(
                    self.clone(),
                    pumpkin_util::math::position::BlockPos::new(0, 0, 0),
                    stack.item.registry_key.to_string(),
                    stack.item_count as u32,
                    0.0,
                );
            if let Some(server) = self.world().server.upgrade() {
                server
                    .plugin_manager
                    .fire_blocking(&server, &mut extract_event);
            }
        }

        if window_type == Some(WindowType::Grindstone)
            && let Some(ref stack) = clicked_item
        {
            let mut prep_grindstone =
                crate::plugin::api::events::inventory::prepare_grindstone::PrepareGrindstoneEvent::new(
                    self.clone(),
                    if stack.is_empty() { None } else { Some(stack.item.registry_key.to_string()) },
                );
            if let Some(server) = self.world().server.upgrade() {
                server
                    .plugin_manager
                    .fire_blocking(&server, &mut prep_grindstone);
            }
        }

        if let Some(ref stack) = clicked_item {
            let mut prep_result =
                crate::plugin::api::events::inventory::prepare_inventory_result::PrepareInventoryResultEvent::new(
                    self.clone(),
                    if stack.is_empty() { None } else { Some(stack.item.registry_key.to_string()) },
                );
            if let Some(server) = self.world().server.upgrade() {
                server
                    .plugin_manager
                    .fire_blocking(&server, &mut prep_result);
            }
        }

        if packet.mode == SlotActionType::QuickCraft {
            let mut drag_event =
                crate::plugin::api::events::inventory::inventory_drag::InventoryDragEvent::new(
                    self.clone(),
                );
            if let Some(server) = self.world().server.upgrade() {
                server
                    .plugin_manager
                    .fire_blocking(&server, &mut drag_event);
            }
            if drag_event.cancelled {
                cancel_screen();
                return;
            }
        }

        let screen_handler_arc = self
            .current_screen_handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let mut screen_handler = screen_handler_arc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let not_in_sync = packet.revision.0
            != (screen_handler
                .get_behaviour()
                .revision
                .load(Ordering::Relaxed) as i32);

        screen_handler.disable_sync();
        screen_handler.on_slot_click(
            i32::from(slot),
            i32::from(packet.button),
            packet.mode.clone(),
            self.as_ref(),
        );

        for (key, value) in packet.array_of_changed_slots {
            screen_handler.set_received_hash(key as usize, value);
        }

        screen_handler.set_received_cursor_hash(packet.carried_item);
        screen_handler.enable_sync();

        if not_in_sync {
            screen_handler.update_to_client();
        } else {
            screen_handler.send_content_updates();
        }
    }

    /// Handles when the player clicks a button in a container (e.g. Enchantment Table)
    pub fn on_container_button_click(&self, packet: &SContainerButtonClick) {
        let screen_handler = self
            .current_screen_handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let mut screen_handler = screen_handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if i32::from(screen_handler.sync_id()) != packet.window_id.0 {
            return;
        }

        screen_handler.on_button_click(self, packet.button_id.0);
    }
}
