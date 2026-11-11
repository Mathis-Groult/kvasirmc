#[allow(clippy::wildcard_imports)]
use super::*;
use pumpkin_data::screen::WindowType;
use pumpkin_inventory::{
    anvil::anvil_screen_handler::AnvilScreenHandler,
    cartography_table_screen_handler::CartographyTableScreenHandler,
    grindstone_screen_handler::GrindstoneScreenHandler, loom_screen_handler::LoomScreenHandler,
    smithing_table_screen_handler::SmithingTableScreenHandler,
    stonecutter_screen_handler::StonecutterScreenHandler,
};
use pumpkin_protocol::bedrock::server::item_stack_request::ItemStackRequestSlotInfo;

mod actions;

impl BedrockClient {
    pub fn handle_item_stack_request(
        &self,
        player: &Arc<Player>,
        packet: pumpkin_protocol::bedrock::server::item_stack_request::SItemStackRequest,
    ) {
        use pumpkin_protocol::bedrock::client::item_stack_response::CItemStackResponse;

        let (responses, inventory_updated) = {
            let current_screen_handler = player
                .current_screen_handler
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            let mut screen_handler = current_screen_handler
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);

            let mut responses = Vec::with_capacity(packet.requests.len());

            for request in packet.requests {
                responses.push(actions::handle_request(
                    player,
                    &mut *screen_handler,
                    request,
                ));
            }

            // Send updates to Java client
            screen_handler.send_content_updates();

            let inventory_updated = actions::touches_player_inventory(&responses);

            (responses, inventory_updated)
        };

        // Send Bedrock specific responses and updates
        self.try_enqueue_client_packet(&CItemStackResponse { responses });

        if inventory_updated {
            player.update_bedrock_shield_sneaking();
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
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) fn map_bedrock_container_slot(
    screen_handler: &dyn ScreenHandler,
    container_name: ContainerName,
    slot_id: u8,
) -> Option<usize> {
    let is_player_screen = screen_handler.window_type().is_none();
    let screen_slots = screen_handler.get_behaviour().slots.len();
    let container_slots = screen_slots.saturating_sub(PlayerInventory::MAIN_SIZE);

    let screen_slot = match container_name {
        ContainerName::HotBar => {
            if is_player_screen {
                Some(36 + slot_id as usize)
            } else {
                Some(container_slots + 27 + slot_id as usize)
            }
        }
        ContainerName::Inventory | ContainerName::CombinedHotBarAndInventory => {
            if slot_id < 9 {
                if is_player_screen {
                    Some(36 + slot_id as usize)
                } else {
                    Some(container_slots + 27 + slot_id as usize)
                }
            } else if slot_id < 36 {
                if is_player_screen {
                    Some(slot_id as usize)
                } else {
                    Some(container_slots + (slot_id - 9) as usize)
                }
            } else {
                None
            }
        }
        ContainerName::Armor => (slot_id < 4).then(|| 5 + slot_id as usize),
        ContainerName::Offhand => (slot_id == 0).then_some(45),
        ContainerName::Cursor => None,
        ContainerName::CraftingInput => {
            if is_player_screen {
                if slot_id < 4 {
                    Some(1 + slot_id as usize)
                } else if (28..32).contains(&slot_id) {
                    Some(1 + (slot_id - 28) as usize)
                } else {
                    None
                }
            } else if screen_handler.window_type()
                == Some(pumpkin_data::screen::WindowType::Crafting)
            {
                if slot_id < 9 {
                    Some(1 + slot_id as usize)
                } else if (32..41).contains(&slot_id) {
                    Some(1 + (slot_id - 32) as usize)
                } else {
                    None
                }
            } else {
                None
            }
        }
        ContainerName::CraftingOutputPreview | ContainerName::CreatedOutput => {
            if is_player_screen {
                Some(0)
            } else if let Some(window_type) = screen_handler.window_type() {
                match window_type {
                    pumpkin_data::screen::WindowType::Crafting => Some(0),
                    pumpkin_data::screen::WindowType::Stonecutter => Some(1),
                    pumpkin_data::screen::WindowType::Anvil
                    | pumpkin_data::screen::WindowType::Furnace
                    | pumpkin_data::screen::WindowType::BlastFurnace
                    | pumpkin_data::screen::WindowType::Smoker
                    | pumpkin_data::screen::WindowType::Grindstone
                    | pumpkin_data::screen::WindowType::Merchant
                    | pumpkin_data::screen::WindowType::CartographyTable => Some(2),
                    pumpkin_data::screen::WindowType::Loom
                    | pumpkin_data::screen::WindowType::Smithing => Some(3),
                    _ => None,
                }
            } else {
                None
            }
        }
        ContainerName::AnvilInput => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::Anvil)
        )
        .then_some(0),
        ContainerName::AnvilMaterial => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::Anvil)
        )
        .then_some(1),
        ContainerName::AnvilResultPreview => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::Anvil)
        )
        .then_some(2),
        ContainerName::BeaconPayment => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::Beacon)
        )
        .then_some(0),
        ContainerName::BrewingStandResult => (matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::BrewingStand)
        ) && slot_id < 3)
            .then_some(slot_id as usize),
        ContainerName::BrewingStandInput => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::BrewingStand)
        )
        .then_some(3),
        ContainerName::BrewingStandFuel => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::BrewingStand)
        )
        .then_some(4),
        ContainerName::FurnaceIngredient
        | ContainerName::BlastFurnaceIngredient
        | ContainerName::SmokerIngredient => matches!(
            screen_handler.window_type(),
            Some(
                pumpkin_data::screen::WindowType::Furnace
                    | pumpkin_data::screen::WindowType::BlastFurnace
                    | pumpkin_data::screen::WindowType::Smoker
            )
        )
        .then_some(0),
        ContainerName::FurnaceFuel => matches!(
            screen_handler.window_type(),
            Some(
                pumpkin_data::screen::WindowType::Furnace
                    | pumpkin_data::screen::WindowType::BlastFurnace
                    | pumpkin_data::screen::WindowType::Smoker
            )
        )
        .then_some(1),
        ContainerName::FurnaceResult => matches!(
            screen_handler.window_type(),
            Some(
                pumpkin_data::screen::WindowType::Furnace
                    | pumpkin_data::screen::WindowType::BlastFurnace
                    | pumpkin_data::screen::WindowType::Smoker
            )
        )
        .then_some(2),
        ContainerName::EnchantingInput => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::Enchantment)
        )
        .then_some(0),
        ContainerName::EnchantingMaterial => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::Enchantment)
        )
        .then_some(1),
        ContainerName::GrindstoneInput => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::Grindstone)
        )
        .then_some(0),
        ContainerName::GrindstoneAdditional => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::Grindstone)
        )
        .then_some(1),
        ContainerName::GrindstoneResultPreview => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::Grindstone)
        )
        .then_some(2),
        ContainerName::LoomInput => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::Loom)
        )
        .then_some(0),
        ContainerName::LoomDye => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::Loom)
        )
        .then_some(1),
        ContainerName::LoomMaterial => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::Loom)
        )
        .then_some(2),
        ContainerName::LoomResultPreview => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::Loom)
        )
        .then_some(3),
        ContainerName::StonecutterInput => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::Stonecutter)
        )
        .then_some(0),
        ContainerName::StonecutterResultPreview => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::Stonecutter)
        )
        .then_some(1),
        ContainerName::CartographyInput => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::CartographyTable)
        )
        .then_some(0),
        ContainerName::CartographyAdditional => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::CartographyTable)
        )
        .then_some(1),
        ContainerName::CartographyResultPreview => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::CartographyTable)
        )
        .then_some(2),
        ContainerName::SmithingTableTemplate => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::Smithing)
        )
        .then_some(0),
        ContainerName::SmithingTableInput => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::Smithing)
        )
        .then_some(1),
        ContainerName::SmithingTableMaterial => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::Smithing)
        )
        .then_some(2),
        ContainerName::SmithingTableResultPreview => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::Smithing)
        )
        .then_some(3),
        ContainerName::TradeIngredient1 | ContainerName::Trade2Ingredient1 => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::Merchant)
        )
        .then_some(0),
        ContainerName::TradeIngredient2 | ContainerName::Trade2Ingredient2 => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::Merchant)
        )
        .then_some(1),
        ContainerName::TradeResultPreview | ContainerName::Trade2ResultPreview => matches!(
            screen_handler.window_type(),
            Some(pumpkin_data::screen::WindowType::Merchant)
        )
        .then_some(2),
        _ => ((slot_id as usize) < container_slots).then_some(slot_id as usize),
    };
    // A client can name any slot id: an index past this screen's slots maps nowhere, or every
    // caller would index past the slot list and the panic would stop the server.
    screen_slot.filter(|&index| index < screen_slots)
}

pub(crate) struct SlotUpdate {
    pub(crate) container_name: FullContainerName,
    pub(crate) slot_id: u8,
    pub(crate) count: u8,
    pub(crate) stack_id: VarInt,
}

pub(crate) fn record_update(
    updates: &mut Vec<SlotUpdate>,
    container_name: FullContainerName,
    slot_id: u8,
    stack: &ItemStack,
) {
    let count = stack.item_count;
    let stack_id = if stack.is_empty() {
        VarInt(0)
    } else {
        VarInt(stack.uid.get())
    };
    if let Some(existing) = updates
        .iter_mut()
        .find(|u| u.container_name == container_name && u.slot_id == slot_id)
    {
        existing.count = count;
        existing.stack_id = stack_id;
    } else {
        updates.push(SlotUpdate {
            container_name,
            slot_id,
            count,
            stack_id,
        });
    }
}

/// Whether `slot_info` is the cursor or a slot of the open screen. Bedrock's server rejects a
/// request that moves items to any other place; writing to it would store nothing, so the
/// items would vanish while the response tells the client they arrived.
fn names_a_slot(screen_handler: &dyn ScreenHandler, slot_info: &ItemStackRequestSlotInfo) -> bool {
    slot_info.container_name.container_name == ContainerName::Cursor
        || map_bedrock_container_slot(
            screen_handler,
            slot_info.container_name.container_name,
            slot_info.slot_id,
        )
        .is_some()
}

fn get_slot_stack(
    screen_handler: &dyn ScreenHandler,
    slot_info: &pumpkin_protocol::bedrock::server::item_stack_request::ItemStackRequestSlotInfo,
    created_item: Option<&ItemStack>,
) -> ItemStack {
    if let (ContainerName::CreatedOutput, Some(stack)) =
        (slot_info.container_name.container_name, created_item)
    {
        return stack.clone();
    }
    if slot_info.container_name.container_name == ContainerName::Cursor {
        return screen_handler
            .get_behaviour()
            .cursor_stack
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
    }
    map_bedrock_container_slot(
        screen_handler,
        slot_info.container_name.container_name,
        slot_info.slot_id,
    )
    .map_or_else(
        || ItemStack::EMPTY.clone(),
        |screen_slot| screen_handler.get_behaviour().slots[screen_slot].get_cloned_stack(),
    )
}

#[allow(clippy::unreachable)]
fn update_slot_stack(
    player: &Player,
    screen_handler: &mut dyn ScreenHandler,
    slot_info: &pumpkin_protocol::bedrock::server::item_stack_request::ItemStackRequestSlotInfo,
    new_stack: ItemStack,
) {
    if slot_info.container_name.container_name == ContainerName::Cursor {
        let mut cursor_lock = screen_handler
            .get_behaviour()
            .cursor_stack
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *cursor_lock = new_stack;
        return;
    }
    if let Some(screen_slot) = map_bedrock_container_slot(
        screen_handler,
        slot_info.container_name.container_name,
        slot_info.slot_id,
    ) {
        let is_player_screen = screen_handler.window_type().is_none();
        if is_player_screen {
            let current_stack =
                screen_handler.get_behaviour().slots[screen_slot].get_cloned_stack();
            if !current_stack.are_items_and_components_equal(&new_stack) {
                if (5..9).contains(&screen_slot) {
                    player.enqueue_equipment_change(
                        &match screen_slot {
                            5 => EquipmentSlot::HEAD,
                            6 => EquipmentSlot::CHEST,
                            7 => EquipmentSlot::LEGS,
                            8 => EquipmentSlot::FEET,
                            _ => unreachable!(),
                        },
                        &new_stack,
                    );
                } else if (36..45).contains(&screen_slot) {
                    let hotbar_slot = screen_slot - 36;
                    if player.inventory().get_selected_slot() == hotbar_slot as u8 {
                        let equipment = &[(EquipmentSlot::MAIN_HAND, new_stack.clone())];
                        player.living_entity.send_equipment_changes(equipment);
                    }
                }
            }
        }

        screen_handler.get_behaviour().slots[screen_slot].set_stack(new_stack.clone());
        screen_handler.set_received_stack(screen_slot, new_stack);
    }
}

fn get_output_screen_slot(
    screen_handler: &dyn ScreenHandler,
    source: &ItemStackRequestSlotInfo,
) -> Option<usize> {
    match source.container_name.container_name {
        ContainerName::CreatedOutput | ContainerName::CraftingOutputPreview => {
            map_bedrock_container_slot(
                screen_handler,
                source.container_name.container_name,
                source.slot_id,
            )
        }
        ContainerName::AnvilResultPreview => {
            (screen_handler.window_type() == Some(WindowType::Anvil)).then_some(2)
        }
        ContainerName::GrindstoneResultPreview => {
            (screen_handler.window_type() == Some(WindowType::Grindstone)).then_some(2)
        }
        ContainerName::StonecutterResultPreview => {
            (screen_handler.window_type() == Some(WindowType::Stonecutter)).then_some(1)
        }
        ContainerName::LoomResultPreview => {
            (screen_handler.window_type() == Some(WindowType::Loom)).then_some(3)
        }
        ContainerName::SmithingTableResultPreview => {
            (screen_handler.window_type() == Some(WindowType::Smithing)).then_some(3)
        }
        ContainerName::CartographyResultPreview => {
            (screen_handler.window_type() == Some(WindowType::CartographyTable)).then_some(2)
        }
        ContainerName::FurnaceResult => matches!(
            screen_handler.window_type(),
            Some(WindowType::Furnace | WindowType::BlastFurnace | WindowType::Smoker)
        )
        .then_some(2),
        ContainerName::TradeResultPreview | ContainerName::Trade2ResultPreview => {
            (screen_handler.window_type() == Some(WindowType::Merchant)).then_some(2)
        }
        ContainerName::BrewingStandResult => {
            (matches!(screen_handler.window_type(), Some(WindowType::BrewingStand))
                && source.slot_id < 3)
                .then_some(source.slot_id as usize)
        }
        _ => None,
    }
}

fn record_preview_update(
    updates: &mut Vec<SlotUpdate>,
    container_name: ContainerName,
    screen_slot: u8,
    stack: &ItemStack,
) {
    for slot_id in [0, screen_slot, 50] {
        record_update(
            updates,
            FullContainerName {
                container_name,
                dynamic_id: None,
            },
            slot_id,
            stack,
        );
    }
}

#[allow(clippy::too_many_lines)]
fn record_specialized_container_input_updates(
    screen_handler: &dyn ScreenHandler,
    updates: &mut Vec<SlotUpdate>,
) {
    let Some(window_type) = screen_handler.window_type() else {
        return;
    };
    match window_type {
        WindowType::Anvil => {
            let slot0 = screen_handler.get_behaviour().slots[0].get_cloned_stack();
            let slot1 = screen_handler.get_behaviour().slots[1].get_cloned_stack();
            for slot_id in [0, 1] {
                record_update(
                    updates,
                    FullContainerName {
                        container_name: ContainerName::AnvilInput,
                        dynamic_id: None,
                    },
                    slot_id,
                    &slot0,
                );
                record_update(
                    updates,
                    FullContainerName {
                        container_name: ContainerName::AnvilMaterial,
                        dynamic_id: None,
                    },
                    slot_id,
                    &slot1,
                );
            }
        }
        WindowType::Grindstone => {
            let slot0 = screen_handler.get_behaviour().slots[0].get_cloned_stack();
            let slot1 = screen_handler.get_behaviour().slots[1].get_cloned_stack();
            for slot_id in [0, 1] {
                record_update(
                    updates,
                    FullContainerName {
                        container_name: ContainerName::GrindstoneInput,
                        dynamic_id: None,
                    },
                    slot_id,
                    &slot0,
                );
                record_update(
                    updates,
                    FullContainerName {
                        container_name: ContainerName::GrindstoneAdditional,
                        dynamic_id: None,
                    },
                    slot_id,
                    &slot1,
                );
            }
        }
        WindowType::Stonecutter => {
            let slot0 = screen_handler.get_behaviour().slots[0].get_cloned_stack();
            record_update(
                updates,
                FullContainerName {
                    container_name: ContainerName::StonecutterInput,
                    dynamic_id: None,
                },
                0,
                &slot0,
            );
        }
        WindowType::Loom => {
            let slot0 = screen_handler.get_behaviour().slots[0].get_cloned_stack();
            let slot1 = screen_handler.get_behaviour().slots[1].get_cloned_stack();
            let slot2 = screen_handler.get_behaviour().slots[2].get_cloned_stack();
            for slot_id in [0, 1, 2] {
                record_update(
                    updates,
                    FullContainerName {
                        container_name: ContainerName::LoomInput,
                        dynamic_id: None,
                    },
                    slot_id,
                    &slot0,
                );
                record_update(
                    updates,
                    FullContainerName {
                        container_name: ContainerName::LoomDye,
                        dynamic_id: None,
                    },
                    slot_id,
                    &slot1,
                );
                record_update(
                    updates,
                    FullContainerName {
                        container_name: ContainerName::LoomMaterial,
                        dynamic_id: None,
                    },
                    slot_id,
                    &slot2,
                );
            }
        }
        WindowType::Smithing => {
            let slot0 = screen_handler.get_behaviour().slots[0].get_cloned_stack();
            let slot1 = screen_handler.get_behaviour().slots[1].get_cloned_stack();
            let slot2 = screen_handler.get_behaviour().slots[2].get_cloned_stack();
            for slot_id in [0, 1, 2] {
                record_update(
                    updates,
                    FullContainerName {
                        container_name: ContainerName::SmithingTableTemplate,
                        dynamic_id: None,
                    },
                    slot_id,
                    &slot0,
                );
                record_update(
                    updates,
                    FullContainerName {
                        container_name: ContainerName::SmithingTableInput,
                        dynamic_id: None,
                    },
                    slot_id,
                    &slot1,
                );
                record_update(
                    updates,
                    FullContainerName {
                        container_name: ContainerName::SmithingTableMaterial,
                        dynamic_id: None,
                    },
                    slot_id,
                    &slot2,
                );
            }
        }
        WindowType::CartographyTable => {
            let slot0 = screen_handler.get_behaviour().slots[0].get_cloned_stack();
            let slot1 = screen_handler.get_behaviour().slots[1].get_cloned_stack();
            for slot_id in [0, 1] {
                record_update(
                    updates,
                    FullContainerName {
                        container_name: ContainerName::CartographyInput,
                        dynamic_id: None,
                    },
                    slot_id,
                    &slot0,
                );
                record_update(
                    updates,
                    FullContainerName {
                        container_name: ContainerName::CartographyAdditional,
                        dynamic_id: None,
                    },
                    slot_id,
                    &slot1,
                );
            }
        }
        WindowType::Merchant => {
            let slot0 = screen_handler.get_behaviour().slots[0].get_cloned_stack();
            let slot1 = screen_handler.get_behaviour().slots[1].get_cloned_stack();
            record_update(
                updates,
                FullContainerName {
                    container_name: ContainerName::Trade2Ingredient1,
                    dynamic_id: None,
                },
                4,
                &slot0,
            );
            record_update(
                updates,
                FullContainerName {
                    container_name: ContainerName::Trade2Ingredient2,
                    dynamic_id: None,
                },
                5,
                &slot1,
            );
        }
        _ => {}
    }
}

#[allow(clippy::too_many_lines)]
fn refresh_specialized_screen_results(
    player: &Player,
    screen_handler: &mut dyn ScreenHandler,
    filter_strings: &[String],
    updates: &mut Vec<SlotUpdate>,
) {
    let Some(window_type) = screen_handler.window_type() else {
        return;
    };
    match window_type {
        WindowType::Anvil => {
            if let Some(anvil) = screen_handler
                .as_any_mut()
                .downcast_mut::<AnvilScreenHandler>()
            {
                let has_infinite = player.has_infinite_materials();
                if let Some(name) = filter_strings.first() {
                    anvil.set_item_name(name, has_infinite);
                } else {
                    anvil.create_result(has_infinite);
                }
                let result_stack = anvil.inventory.get_stack(2);
                record_preview_update(updates, ContainerName::AnvilResultPreview, 2, &result_stack);
            }
        }
        WindowType::Grindstone => {
            if let Some(grindstone) = screen_handler
                .as_any_mut()
                .downcast_mut::<GrindstoneScreenHandler>()
            {
                grindstone.create_result();
                let result_stack = grindstone.inventory.get_stack(2);
                record_preview_update(
                    updates,
                    ContainerName::GrindstoneResultPreview,
                    2,
                    &result_stack,
                );
            }
        }
        WindowType::Stonecutter => {
            if let Some(stonecutter) = screen_handler
                .as_any_mut()
                .downcast_mut::<StonecutterScreenHandler>()
            {
                stonecutter.update_output();
                let result_stack = stonecutter.output_inventory.get_stack(0);
                record_preview_update(
                    updates,
                    ContainerName::StonecutterResultPreview,
                    1,
                    &result_stack,
                );
            }
        }
        WindowType::Loom => {
            if let Some(loom) = screen_handler
                .as_any_mut()
                .downcast_mut::<LoomScreenHandler>()
            {
                loom.slots_changed();
                let result_stack = loom.output_inventory.get_stack(0);
                record_preview_update(updates, ContainerName::LoomResultPreview, 3, &result_stack);
            }
        }
        WindowType::Smithing => {
            if let Some(smithing) = screen_handler
                .as_any_mut()
                .downcast_mut::<SmithingTableScreenHandler>()
            {
                smithing.update_output();
                let result_stack = smithing.output_inventory.get_stack(0);
                record_preview_update(
                    updates,
                    ContainerName::SmithingTableResultPreview,
                    3,
                    &result_stack,
                );
            }
        }
        WindowType::CartographyTable => {
            if let Some(cartography) = screen_handler
                .as_any_mut()
                .downcast_mut::<CartographyTableScreenHandler>()
            {
                cartography.slots_changed();
                let result_stack = cartography.output_inventory.get_stack(0);
                record_preview_update(
                    updates,
                    ContainerName::CartographyResultPreview,
                    2,
                    &result_stack,
                );
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumpkin_data::item::Item;
    use pumpkin_inventory::{
        SimpleInventory, build_equipment_slots,
        crafting::crafting_screen_handler::CraftingTableScreenHandler,
        entity_equipment::EntityEquipment,
    };
    use std::sync::Mutex;

    #[tokio::test]
    async fn crafting_table_maps_bedrock_player_inventory_after_its_ten_slots() {
        let inventory = Arc::new(PlayerInventory::new(
            Arc::new(Mutex::new(EntityEquipment::new())),
            Arc::new(build_equipment_slots()),
        ));
        let handler = CraftingTableScreenHandler::new(1, &inventory, None);

        assert_eq!(
            map_bedrock_container_slot(&handler, ContainerName::Inventory, 26),
            Some(27)
        );
        assert_eq!(
            map_bedrock_container_slot(&handler, ContainerName::HotBar, 0),
            Some(37)
        );
        assert_eq!(
            map_bedrock_container_slot(&handler, ContainerName::CraftingInput, 32),
            Some(1)
        );
        assert_eq!(
            map_bedrock_container_slot(&handler, ContainerName::CraftingInput, 40),
            Some(9)
        );
    }

    #[test]
    fn item_stack_response_uses_the_authoritative_stack_id() {
        let stack = ItemStack::new(3, &Item::SPRUCE_DOOR);
        let container = FullContainerName {
            container_name: ContainerName::Cursor,
            dynamic_id: None,
        };
        let mut updates = Vec::new();

        record_update(&mut updates, container.clone(), 0, &stack);
        assert_eq!(updates[0].count, 3);
        assert_eq!(updates[0].stack_id, VarInt(stack.uid.get()));

        record_update(&mut updates, container, 0, ItemStack::EMPTY);
        assert_eq!(updates[0].count, 0);
        assert_eq!(updates[0].stack_id, VarInt(0));
    }

    #[tokio::test]
    async fn specialized_containers_map_output_slots_correctly() {
        let inventory = Arc::new(PlayerInventory::new(
            Arc::new(Mutex::new(EntityEquipment::new())),
            Arc::new(build_equipment_slots()),
        ));

        let make_slot = |container_name: ContainerName, slot_id: u8| ItemStackRequestSlotInfo {
            container_name: FullContainerName {
                container_name,
                dynamic_id: None,
            },
            slot_id,
            stack_id: 0,
        };

        // Stonecutter
        let stonecutter = StonecutterScreenHandler::new(1, &inventory, None);
        assert_eq!(
            get_output_screen_slot(
                &stonecutter,
                &make_slot(ContainerName::StonecutterResultPreview, 0)
            ),
            Some(1)
        );
        assert_eq!(
            get_output_screen_slot(&stonecutter, &make_slot(ContainerName::CreatedOutput, 0)),
            Some(1)
        );
        assert_eq!(
            get_output_screen_slot(&stonecutter, &make_slot(ContainerName::StonecutterInput, 0)),
            None
        );

        // Anvil
        let anvil_inv = Arc::new(SimpleInventory::new(3));
        let anvil = AnvilScreenHandler::new(1, &inventory, anvil_inv);
        assert_eq!(
            get_output_screen_slot(&anvil, &make_slot(ContainerName::AnvilResultPreview, 0)),
            Some(2)
        );
        assert_eq!(
            get_output_screen_slot(&anvil, &make_slot(ContainerName::CreatedOutput, 0)),
            Some(2)
        );
        assert_eq!(
            get_output_screen_slot(&anvil, &make_slot(ContainerName::AnvilInput, 0)),
            None
        );

        // Grindstone
        let grindstone_inv = Arc::new(SimpleInventory::new(3));
        let grindstone = GrindstoneScreenHandler::new(1, &inventory, grindstone_inv);
        assert_eq!(
            get_output_screen_slot(
                &grindstone,
                &make_slot(ContainerName::GrindstoneResultPreview, 0)
            ),
            Some(2)
        );
        assert_eq!(
            get_output_screen_slot(&grindstone, &make_slot(ContainerName::CreatedOutput, 0)),
            Some(2)
        );

        // Loom
        let loom = LoomScreenHandler::new(1, &inventory);
        assert_eq!(
            get_output_screen_slot(&loom, &make_slot(ContainerName::LoomResultPreview, 0)),
            Some(3)
        );
        assert_eq!(
            get_output_screen_slot(&loom, &make_slot(ContainerName::CreatedOutput, 0)),
            Some(3)
        );

        // Smithing Table
        let smithing = SmithingTableScreenHandler::new(1, &inventory, None);
        assert_eq!(
            get_output_screen_slot(
                &smithing,
                &make_slot(ContainerName::SmithingTableResultPreview, 0)
            ),
            Some(3)
        );
        assert_eq!(
            get_output_screen_slot(&smithing, &make_slot(ContainerName::CreatedOutput, 0)),
            Some(3)
        );

        // Cartography Table
        let cartography = CartographyTableScreenHandler::new(1, &inventory);
        assert_eq!(
            get_output_screen_slot(
                &cartography,
                &make_slot(ContainerName::CartographyResultPreview, 0)
            ),
            Some(2)
        );
        assert_eq!(
            get_output_screen_slot(&cartography, &make_slot(ContainerName::CreatedOutput, 0)),
            Some(2)
        );
    }
}
