#[allow(clippy::wildcard_imports)]
use super::*;
use crate::item::registry::should_try_block_placement;
use pumpkin_protocol::bedrock::server::inventory_transaction::HandSlot;
use std::ops::ControlFlow;

use pumpkin_data::Block;
use pumpkin_protocol::bedrock::server::inventory_transaction::{
    InventoryAction, LegacySetItemSlot, UseItemOnEntityTransactionData, UseItemTransactionData,
};

use super::item_stack_request::SlotUpdate;
use crate::world::World;

impl BedrockClient {
    fn correct_rejected_food_use(&self, player: &Player) {
        // Holding use can repeat rejected transactions. Limit prediction corrections
        // to every 20 ticks, without indefinitely trusting a previously sent snapshot.
        const CORRECTION_INTERVAL_TICKS: i32 = 20;
        let tick = player.tick_counter.load(Ordering::Relaxed);
        let recently_corrected = self.last_food_rejection_tick.load().is_some_and(|last| {
            tick >= last && tick.saturating_sub(last) < CORRECTION_INTERVAL_TICKS
        });
        let has_active_use = player
            .living_entity
            .item_in_use
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some();
        if recently_corrected && !has_active_use {
            return;
        }
        if player.has_client_loaded() {
            self.last_food_rejection_tick.store(Some(tick));
        }
        player.living_entity.clear_active_hand();
        player.send_health();
    }

    /// Applies a Bedrock inventory transaction: legacy slot drops and the slots the client wrote,
    /// the inventory resend, then the transaction itself (use an item on a block, in the air or
    /// on an entity, release it); a legacy request gets its response unless the transaction
    /// stopped early.
    pub fn handle_inventory_action(&self, player: &Arc<Player>, packet: SInventoryTransaction) {
        tracing::debug!("handle_inventory_action: packet={:?}", packet);
        let mut inventory_updated = false;
        let mut updates = Vec::new();
        let result = 0u8;

        if packet.actions.is_empty() && packet.legacy_request_id.0 != 0 {
            inventory_updated |=
                drop_legacy_slots(player, &packet.legacy_set_item_slots, &mut updates);
        }
        inventory_updated |= apply_inventory_actions(player, &packet.actions);
        if inventory_updated {
            self.send_main_inventory(player);
        }

        let flow = match packet.transaction_data {
            // Actions are already applied to the inventory screen handler above.
            TransactionData::Normal(_) | TransactionData::Mismatch(_) => ControlFlow::Continue(()),
            TransactionData::UseItem(data) => self.use_item(player, &data),
            TransactionData::UseItemOnEntity(data) => use_item_on_entity(player, &data),
            TransactionData::ReleaseItem(_data) => release_item(player),
        };
        if flow.is_break() {
            return;
        }

        if packet.legacy_request_id.0 != 0 {
            self.send_legacy_response(packet.legacy_request_id, result, updates);
        }
    }

    fn send_main_inventory(&self, player: &Player) {
        let slots = player
            .inventory()
            .main_inventory
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(NetworkItemStackDescriptor::from)
            .collect();
        self.try_enqueue_client_packet(&CInventoryContent {
            container_id: VarUInt(0),
            slots,
            full_container_name: FullContainerName {
                container_name: ContainerName::Inventory,
                dynamic_id: None,
            },
            storage_item: NetworkItemStackDescriptor::default(),
        });
    }

    fn use_item(&self, player: &Arc<Player>, data: &UseItemTransactionData) -> ControlFlow<()> {
        let face = match data.block_face {
            0 => BlockDirection::Down,
            2 => BlockDirection::North,
            3 => BlockDirection::South,
            4 => BlockDirection::West,
            5 => BlockDirection::East,
            _ => BlockDirection::Up,
        };
        let world = player.world();
        let block = world.get_block(&data.block_position);
        let Some(server) = world.server.upgrade() else {
            return ControlFlow::Break(());
        };

        if player.gamemode.load() == GameMode::Spectator {
            if let Some(factory) = server.block_registry.get_screen_handler_factory(
                block,
                player,
                &data.block_position,
                &server,
                &world,
            ) {
                player.open_handled_screen(factory.as_ref(), Some(data.block_position));
            }
            return ControlFlow::Break(());
        }

        if data.action_type.0 == 0 {
            use_on_block(player, data, block, face, &server, &world)
        } else {
            if data.action_type.0 == 1 {
                self.click_air(player, data, &server);
            }
            ControlFlow::Continue(())
        }
    }

    fn click_air(&self, player: &Arc<Player>, data: &UseItemTransactionData, server: &Arc<Server>) {
        // Click air / Use item
        let client_stack = descriptor_to_stack(&data.item_in_hand);

        let hand = match data.hand {
            HandSlot::Mainhand => Hand::Right,
            HandSlot::Offhand => Hand::Left,
        };
        let mut held = player.inventory().get_stack_in_hand(hand);
        if !client_stack.is_empty() && (held.is_empty() || held.item.id != client_stack.item.id) {
            held = client_stack;
            player.inventory().set_stack_in_hand(hand, held.clone());
        }
        if !held.is_empty() {
            player.increment_stat(
                pumpkin_data::statistic::StatisticCategory::Used,
                held.item.id as i32,
                1,
            );
        }

        let event = PlayerInteractEvent::new(
            player,
            InteractAction::RightClickAir,
            &pumpkin_data::Block::AIR,
            None,
        );

        let stack_for_use = held.clone();

        self.start_using(player, hand, &mut held);

        send_cancellable_blocking! {{
            server;
            event;
            'after: {
                server.item_registry.on_use(&stack_for_use, player, hand);
            }
        }}
    }

    /// Starts using the held item (eating, an equippable), unless its cooldown runs.
    fn start_using(&self, player: &Player, hand: Hand, held: &mut ItemStack) {
        let mut cooldown_active = false;
        if let Some(cooldown) = held.get_use_cooldown() {
            let group = cooldown
                .cooldown_group
                .clone()
                .unwrap_or_else(|| held.item.registry_key.to_string());
            if player.is_on_cooldown(&group) {
                cooldown_active = true;
            }
        }

        if !cooldown_active {
            // Bedrock can repeat click-air while using an item. Do not
            // restart its server-side use timer on these timed inputs.
            let already_using = player.living_entity.item_use_time.load(Ordering::Relaxed) > 0
                && player
                    .living_entity
                    .item_in_use
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .as_ref()
                    .is_some_and(|item| item.are_items_and_components_equal(held));
            if !already_using && held.get_data_component::<ConsumableImpl>().is_some() {
                if held
                    .get_data_component::<FoodImpl>()
                    .is_none_or(|food| player.can_eat(food.can_always_eat))
                {
                    player.living_entity.set_active_hand(
                        hand,
                        held.clone(),
                        held.get_max_use_time(),
                    );
                } else {
                    // Correct predicted eating when the server's food
                    // level is already full, even if it has not changed.
                    self.correct_rejected_food_use(player);
                }
            }
            equip_from_hand(player, hand, held);
        }
    }

    fn send_legacy_response(
        &self,
        legacy_request_id: VarInt,
        result: u8,
        updates: Vec<SlotUpdate>,
    ) {
        use pumpkin_protocol::bedrock::client::item_stack_response::{
            CItemStackResponse, ItemStackResponseContainerInfo, ItemStackResponseInfo,
            ItemStackResponseSlotInfo,
        };

        let mut container_infos = Vec::new();
        if result == 0 {
            for update in updates {
                let container_info = container_infos.iter_mut().find(
                    |info: &&mut ItemStackResponseContainerInfo| {
                        info.full_container_name == update.container_name
                    },
                );

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
                        full_container_name: update.container_name.clone(),
                        slots: vec![slot_info],
                    });
                }
            }
        }

        self.try_enqueue_client_packet(&CItemStackResponse {
            responses: vec![ItemStackResponseInfo {
                result,
                client_request_id: legacy_request_id,
                containers: container_infos,
            }],
        });
    }
}

/// A legacy request with no actions: the items in the named slots are dropped into the world.
fn drop_legacy_slots(
    player: &Player,
    legacy_set_item_slots: &[LegacySetItemSlot],
    updates: &mut Vec<SlotUpdate>,
) -> bool {
    let mut inventory_updated = false;

    let mut player_screen_handler = player
        .player_screen_handler
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for legacy_slot in legacy_set_item_slots {
        let mapped_window_id = match legacy_slot.container_id {
            28 | 29 => 0,    // HotBar or Inventory
            6 | 120 => 120,  // Armor
            34 | 119 => 119, // Offhand
            other => other as i32,
        };
        for &slot_id in &legacy_slot.slots {
            if let Some(screen_slot) =
                map_bedrock_slot_to_screen_handler(mapped_window_id, slot_id as u32)
            {
                let current_stack = player_screen_handler
                    .get_slot(screen_slot)
                    .get_cloned_stack();
                if !current_stack.is_empty() {
                    player.drop_item(current_stack.clone());

                    player_screen_handler
                        .get_slot(screen_slot)
                        .set_stack(ItemStack::EMPTY.clone());
                    player_screen_handler.set_received_stack(screen_slot, ItemStack::EMPTY.clone());

                    record_update(
                        updates,
                        FullContainerName {
                            container_name: match legacy_slot.container_id {
                                28 => ContainerName::HotBar,
                                _ => ContainerName::Inventory,
                            },
                            dynamic_id: None,
                        },
                        slot_id,
                        ItemStack::EMPTY,
                    );
                    inventory_updated = true;
                }
            }
        }
    }
    player_screen_handler.send_content_updates();
    drop(player_screen_handler);
    inventory_updated
}

/// The slots the client wrote and the items it threw into the world.
#[allow(clippy::collapsible_if, clippy::unreachable)]
fn apply_inventory_actions(player: &Player, actions: &[InventoryAction]) -> bool {
    let mut inventory_updated = false;

    for action in actions {
        use pumpkin_protocol::bedrock::server::inventory_transaction::InventoryActionSource;
        let source_type = InventoryActionSource::from(action.source_type);
        if source_type == InventoryActionSource::World {
            let old_stack = descriptor_to_stack(&action.old_item);
            let new_stack = descriptor_to_stack(&action.new_item);
            if old_stack.is_empty() && !new_stack.is_empty() {
                player.drop_item(new_stack);
            }
        } else if let Some(window_id) = action.window_id {
            if let Some(screen_slot) =
                map_bedrock_slot_to_screen_handler(window_id, action.inventory_slot)
            {
                let item_stack = descriptor_to_stack(&action.new_item);

                let mut player_screen_handler = player
                    .player_screen_handler
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);

                let is_armor_equipped = player_screen_handler
                    .get_slot(screen_slot)
                    .get_stack()
                    .are_equal(&item_stack);

                if !is_armor_equipped {
                    if (5..9).contains(&screen_slot) {
                        player.enqueue_equipment_change(
                            &match screen_slot {
                                5 => EquipmentSlot::HEAD,
                                6 => EquipmentSlot::CHEST,
                                7 => EquipmentSlot::LEGS,
                                8 => EquipmentSlot::FEET,
                                _ => unreachable!(),
                            },
                            &item_stack,
                        );
                    } else if (36..45).contains(&screen_slot) {
                        let hotbar_slot = screen_slot - 36;
                        if player.inventory().get_selected_slot() == hotbar_slot as u8 {
                            let equipment = &[(EquipmentSlot::MAIN_HAND, item_stack.clone())];
                            player.living_entity.send_equipment_changes(equipment);
                        }
                    }
                }

                player_screen_handler
                    .get_slot(screen_slot)
                    .set_stack(item_stack.clone());
                player_screen_handler.set_received_stack(screen_slot, item_stack);
                player_screen_handler.send_content_updates();
                drop(player_screen_handler);

                inventory_updated = true;
            }
        }
    }
    inventory_updated
}

/// Clicking a block: the block's use with the held item, its default use, then the item's.
fn use_on_block(
    player: &Arc<Player>,
    data: &UseItemTransactionData,
    block: &'static Block,
    face: BlockDirection,
    server: &Arc<Server>,
    world: &Arc<World>,
) -> ControlFlow<()> {
    // Click block
    let client_stack = descriptor_to_stack(&data.item_in_hand);

    let mut held_item = player.inventory().held_item();
    if !client_stack.is_empty()
        && (held_item.is_empty() || held_item.item.id != client_stack.item.id)
    {
        held_item = client_stack;
    }

    let result = server.block_registry.use_with_item(
        block,
        player,
        &data.block_position,
        &BlockHitResult {
            face: &face,
            cursor_pos: &data.click_position,
        },
        &mut held_item,
        &EquipmentSlot::MAIN_HAND,
        server,
        world,
    );

    if result.consumes_action() {
        return ControlFlow::Break(());
    }

    if matches!(result, BlockActionResult::PassToDefaultBlockAction) {
        let result = server.block_registry.on_use(
            block,
            player,
            &data.block_position,
            &BlockHitResult {
                face: &face,
                cursor_pos: &data.click_position,
            },
            server,
            world,
        );

        if result.consumes_action() {
            return ControlFlow::Break(());
        }
    }

    use_stack_on_block(player, data, block, face, server, held_item);
    ControlFlow::Continue(())
}

fn use_stack_on_block(
    player: &Arc<Player>,
    data: &UseItemTransactionData,
    block: &'static Block,
    face: BlockDirection,
    server: &Arc<Server>,
    mut stack: ItemStack,
) {
    if !stack.is_empty() {
        let item_id = stack.item.id;
        let before = stack.clone();
        player.increment_stat(
            pumpkin_data::statistic::StatisticCategory::Used,
            item_id as i32,
            1,
        );
        let item_result = server.item_registry.use_on_block(
            &mut stack,
            player,
            data.block_position,
            face,
            data.click_position,
            block,
            server,
        );

        if should_try_block_placement(&item_result) {
            let item_id = stack.item.id;
            if let Some(placed_block) = pumpkin_data::Block::from_item_id(item_id) {
                let dummy_use_item_on = pumpkin_protocol::java::server::play::SUseItemOn {
                    hand: VarInt(0),
                    position: data.block_position,
                    face: VarInt(i32::from(data.block_face)),
                    cursor_pos: data.click_position,
                    inside_block: false,
                    is_against_world_border: false,
                    sequence: VarInt(0),
                };

                if let Ok(Some(_)) = server.block_registry.place_block(
                    player,
                    placed_block,
                    server,
                    &dummy_use_item_on,
                    data.block_position,
                    face,
                ) && player.gamemode.load() != GameMode::Creative
                {
                    stack.decrement(1);
                }
            }
        }
        if before.is_damageable() && stack.is_empty() {
            player.increment_stat(
                pumpkin_data::statistic::StatisticCategory::Broken,
                item_id as i32,
                1,
            );
            player.world().send_entity_status(
                player.get_entity(),
                crate::entity::equipment_break_status(&EquipmentSlot::MAIN_HAND),
                ActorEventID::None,
            );
        }
        player.inventory().set_held_item(stack);
    }
}

/// Equips the held item when it is equippable and differs from what that slot holds.
fn equip_from_hand(player: &Player, hand: Hand, held: &mut ItemStack) {
    if let Some(equippable) = held.get_data_component::<EquippableImpl>() {
        let should_change = {
            let inventory = player.inventory();
            let equipment_guard = inventory
                .entity_equipment
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let current_equipped = equipment_guard.get(equippable.slot);
            !current_equipped.are_items_and_components_equal(held)
        };
        if should_change {
            player.enqueue_equipment_change(equippable.slot, held);

            let inventory = player.inventory();
            let mut equipment_guard = inventory
                .entity_equipment
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let equip_item = equipment_guard
                .equipment
                .entry(equippable.slot.clone())
                .or_insert_with(|| ItemStack::EMPTY.clone());
            if equip_item.is_empty() {
                *equip_item = held.clone();
                held.decrement_unless_creative(player.gamemode.load(), 1);
            } else {
                let old_held = held.clone();
                *held = equip_item.clone();
                *equip_item = old_held;
            }
            drop(equipment_guard);
            player.inventory().set_stack_in_hand(hand, held.clone());
        }
    }
}

fn use_item_on_entity(
    player: &Arc<Player>,
    data: &UseItemOnEntityTransactionData,
) -> ControlFlow<()> {
    let action = match data.action_type.0 {
        // Bedrock does not distinguish an entity hit position here. ItemInteract is
        // therefore exposed as the general Interact action rather than InteractAt.
        0 | 2 => ActionType::Interact,
        1 => ActionType::Attack,
        action => {
            tracing::warn!("invalid UseItemOnEntity action type {action}");
            return ControlFlow::Break(());
        }
    };
    let Ok(target_runtime_id) = i32::try_from(data.target_entity_runtime_id.0) else {
        tracing::warn!(
            "invalid UseItemOnEntity target runtime ID {}",
            data.target_entity_runtime_id.0
        );
        return ControlFlow::Break(());
    };

    let world = player.world();
    let Some(target) = world.get_entity_by_id(target_runtime_id) else {
        return ControlFlow::Break(());
    };
    if !player.can_interact_with_entity(target.as_ref(), 1.0) {
        return ControlFlow::Break(());
    }
    let Some(server) = world.server.upgrade() else {
        return ControlFlow::Break(());
    };

    let mut event = PlayerInteractEntityEvent::new(
        player,
        target.clone(),
        action,
        None,
        player.get_entity().is_sneaking(),
    );
    server.plugin_manager.fire_blocking(&server, &mut event);
    if event.cancelled {
        return ControlFlow::Break(());
    }

    match event.action {
        ActionType::Interact | ActionType::InteractAt => {
            let mut stack = player.inventory().held_item();
            let item_id = stack.item.id;
            let before = stack.clone();
            let native_interaction =
                event
                    .target
                    .interact_with_hand(player, &mut stack, pumpkin_util::Hand::Right);
            if !native_interaction.unwrap_or_else(|| event.target.interact(player, &mut stack)) {
                server
                    .item_registry
                    .use_on_entity(&mut stack, player, event.target);
            }
            if native_interaction.is_none() && !stack.are_equal(&before) {
                player.increment_stat(
                    pumpkin_data::statistic::StatisticCategory::Used,
                    item_id as i32,
                    1,
                );
                if before.is_damageable() && stack.is_empty() {
                    player.increment_stat(
                        pumpkin_data::statistic::StatisticCategory::Broken,
                        item_id as i32,
                        1,
                    );
                    player.world().send_entity_status(
                        player.get_entity(),
                        crate::entity::equipment_break_status(&EquipmentSlot::MAIN_HAND),
                        ActorEventID::None,
                    );
                }
            }
            player.inventory().set_held_item(stack);
        }
        ActionType::Attack => player.attack(&event.target),
    }
    ControlFlow::Continue(())
}

fn release_item(player: &Player) -> ControlFlow<()> {
    let item_in_use = player
        .living_entity
        .item_in_use
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    if let Some(stack) = item_in_use {
        let Some(server) = player.world().server.upgrade() else {
            return ControlFlow::Break(());
        };
        server.item_registry.on_stopped_using(&stack, player);
    }
    player.living_entity.clear_active_hand();
    ControlFlow::Continue(())
}
