use pumpkin_data::data_component_impl::EquipmentSlot;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::translation;
use pumpkin_inventory::Inventory;
use pumpkin_inventory::player::player_inventory::PlayerInventory;
use pumpkin_inventory::screen_handler::{InventoryPlayer, ScreenHandler};
use pumpkin_protocol::codec::item_stack_seralizer::ItemStackSerializer;
use pumpkin_protocol::java::client::play::CSetContainerSlot;
use std::sync::Arc;

use pumpkin_util::PermissionLvl;
use pumpkin_util::permission::{Permission, PermissionDefault, PermissionRegistry};
use pumpkin_util::text::TextComponent;
use pumpkin_util::text::hover::HoverEvent;

use crate::command::argument_builder::{
    ArgumentBuilder, RequiredArgumentBuilder, argument, command, literal,
};
use crate::command::argument_types::argument_type::JavaClientArgumentType;
use crate::command::argument_types::client_parsed::ClientParsed;
use crate::command::argument_types::coordinates::block_pos::BlockPosArgumentType;
use crate::command::argument_types::core::integer::IntegerArgumentType;
use crate::command::argument_types::entity::EntityArgumentType;
use crate::command::argument_types::item::ItemStackArgumentType;
use crate::command::argument_types::slot::SlotArgumentType;
use crate::command::context::command_context::CommandContext;
use crate::command::errors::error_types::CommandErrorType;
use crate::command::node::dispatcher::CommandDispatcher;
use crate::command::node::{CommandExecutor, CommandExecutorResult};
use crate::entity::EntityBase;
use crate::entity::living::LivingEntity;
use crate::entity::player::Player;

const DESCRIPTION: &str = "Modifies items in block or entity inventories.";
const PERMISSION: &str = "minecraft:command.item";

const ERROR_NOT_CONTAINER: CommandErrorType<3> = CommandErrorType::new(
    translation::java::COMMANDS_ITEM_TARGET_NOT_A_CONTAINER,
    translation::java::COMMANDS_ITEM_TARGET_NOT_A_CONTAINER,
);

const ERROR_NO_SUCH_SLOT: CommandErrorType<1> = CommandErrorType::new(
    translation::java::COMMANDS_ITEM_TARGET_NO_SUCH_SLOT,
    translation::java::COMMANDS_ITEM_TARGET_NO_SUCH_SLOT,
);

struct BlockReplaceExecutor {
    has_count: bool,
}

impl CommandExecutor for BlockReplaceExecutor {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let world = context.source.world();
        let pos = BlockPosArgumentType::get_loaded_block_pos(context, "target")?;
        let slot = SlotArgumentType::get(context, "slots")?;
        let parsed_stack = ItemStackArgumentType::get(context, "item")?;
        let item = parsed_stack.item;
        let count = if self.has_count {
            IntegerArgumentType::get(context, "count")?
        } else {
            1
        };

        let block_entity = world.get_block_entity(&pos).ok_or_else(|| {
            ERROR_NOT_CONTAINER.create_without_context(
                TextComponent::text(pos.0.x.to_string()),
                TextComponent::text(pos.0.y.to_string()),
                TextComponent::text(pos.0.z.to_string()),
            )
        })?;

        let inventory = block_entity.get_inventory().ok_or_else(|| {
            ERROR_NOT_CONTAINER.create_without_context(
                TextComponent::text(pos.0.x.to_string()),
                TextComponent::text(pos.0.y.to_string()),
                TextComponent::text(pos.0.z.to_string()),
            )
        })?;

        if slot >= inventory.size() {
            return Err(
                ERROR_NO_SUCH_SLOT.create_without_context(TextComponent::text(slot.to_string()))
            );
        }

        let mut item_stack = parsed_stack.clone();
        item_stack.item_count = count as u8;
        inventory.set_stack(slot, item_stack);

        let item_name = item.registry_key;
        let msg = TextComponent::translate_cross(
            translation::java::COMMANDS_ITEM_BLOCK_SET_SUCCESS,
            translation::java::COMMANDS_ITEM_BLOCK_SET_SUCCESS,
            [
                TextComponent::text(pos.0.x.to_string()),
                TextComponent::text(pos.0.y.to_string()),
                TextComponent::text(pos.0.z.to_string()),
                TextComponent::text("[")
                    .add_child(item.translated_name())
                    .add_child(TextComponent::text("]"))
                    .hover_event(HoverEvent::ShowItem {
                        id: item_name.to_string().into(),
                        count: Some(count),
                        components: None,
                    }),
            ],
        );
        context.source.send_feedback(msg, true);

        Ok(1)
    }
}

struct EntityReplaceExecutor {
    has_count: bool,
}

impl CommandExecutor for EntityReplaceExecutor {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let targets = EntityArgumentType::get_entities(context, "target")?;
        let mojang_slot = SlotArgumentType::get(context, "slots")?;
        let parsed_stack = ItemStackArgumentType::get(context, "item")?;
        let item = parsed_stack.item;
        let count = if self.has_count {
            IntegerArgumentType::get(context, "count")?
        } else {
            1
        };

        let mut modified_count = 0;
        let mut item_stack = parsed_stack.clone();
        item_stack.item_count = count as u8;

        for target in &targets {
            if let Some(player) = target.get_player() {
                replace_player_slot(player, mojang_slot, &item_stack, &mut modified_count);
            } else if let Some(living) = target.get_living_entity() {
                replace_living_slot(living, mojang_slot, &item_stack, &mut modified_count);
            }
        }

        if modified_count == 0 {
            return Err(ERROR_NO_SUCH_SLOT
                .create_without_context(TextComponent::text(mojang_slot.to_string())));
        }

        let msg = success_message(&targets, modified_count, item, count);
        context.source.send_feedback(msg, true);

        Ok(modified_count)
    }
}

/// Puts `item_stack` into a player's slot (ender chest for 200-226) and tells the clients.
fn replace_player_slot(
    player: &Player,
    mojang_slot: usize,
    item_stack: &ItemStack,
    modified_count: &mut i32,
) {
    if let Some(player_arc) = player.world().get_player_by_uuid(player.gameprofile.id) {
        if mojang_slot == CURSOR_SLOT {
            replace_cursor(player, item_stack);
            *modified_count += 1;
        } else if CRAFTING_SLOTS.contains(&mojang_slot) {
            replace_crafting_slot(player, mojang_slot - CRAFTING_SLOTS.start, item_stack);
            *modified_count += 1;
        } else if (200..=226).contains(&mojang_slot) {
            let ender_slot = mojang_slot - 200;
            if ender_slot < player.ender_chest_inventory.size() {
                player_arc
                    .ender_chest_inventory
                    .set_stack(ender_slot, item_stack.clone());
                *modified_count += 1;
            }
        } else {
            let inventory = player.inventory();
            let mapped_slot = player_inventory_slot(mojang_slot, inventory);

            if let Some(slot) = mapped_slot
                && slot < inventory.size()
            {
                player_arc.inventory().set_stack(slot, item_stack.clone());

                let stack_serializer = ItemStackSerializer::from(item_stack.clone());
                let packet = CSetContainerSlot::new(0, 0, slot as i16, &stack_serializer);
                player_arc.enqueue_slot_packet(&packet, None, 0);

                let eq_slot = player_equipment_slot(slot, inventory);

                if let Some(eq) = eq_slot {
                    player
                        .living_entity
                        .send_equipment_changes(&[(eq, item_stack.clone())]);
                }

                *modified_count += 1;
            }
        }
    }
}

/// Command slot of the item the player carries on the cursor (`player.cursor`).
const CURSOR_SLOT: usize = 499;
/// Command slots of the 2x2 crafting grid (`player.crafting.0` to `player.crafting.3`).
const CRAFTING_SLOTS: std::ops::Range<usize> = 500..504;

/// Vanilla `Player.getSlot(499)`: the item carried in the open menu, which the client is told of.
fn replace_cursor(player: &Player, item_stack: &ItemStack) {
    let handler = player
        .current_screen_handler
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let mut handler = handler
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *handler
        .get_behaviour()
        .cursor_stack
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = item_stack.clone();
    handler.send_content_updates();
}

/// Vanilla `Player.getSlot(500 + n)`: a cell of the 2x2 crafting grid of the inventory menu
/// (menu slots 1 to 4; slot 0 is the result).
fn replace_crafting_slot(player: &Player, cell: usize, item_stack: &ItemStack) {
    let mut handler = player
        .player_screen_handler
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    handler.get_slot(1 + cell).set_stack(item_stack.clone());
    handler.send_content_updates();
}

/// The player inventory slot a command slot names: hands, armour, hotbar and main inventory.
fn player_inventory_slot(mojang_slot: usize, inventory: &PlayerInventory) -> Option<usize> {
    if mojang_slot == 98 {
        Some(inventory.get_selected_slot() as usize)
    } else if mojang_slot == 99 {
        Some(40)
    } else if mojang_slot == 100 {
        Some(36)
    } else if mojang_slot == 101 {
        Some(37)
    } else if mojang_slot == 102 {
        Some(38)
    } else if mojang_slot == 103 {
        Some(39)
    } else if mojang_slot <= 35 {
        Some(mojang_slot)
    } else {
        None
    }
}

/// The equipment slot a player inventory slot is worn in, if any.
fn player_equipment_slot(slot: usize, inventory: &PlayerInventory) -> Option<EquipmentSlot> {
    if slot == 36 {
        Some(EquipmentSlot::FEET)
    } else if slot == 37 {
        Some(EquipmentSlot::LEGS)
    } else if slot == 38 {
        Some(EquipmentSlot::CHEST)
    } else if slot == 39 {
        Some(EquipmentSlot::HEAD)
    } else if slot == 40 {
        Some(EquipmentSlot::OFF_HAND)
    } else if slot == inventory.get_selected_slot() as usize {
        Some(EquipmentSlot::MAIN_HAND)
    } else {
        None
    }
}

/// Puts `item_stack` into a mob's equipment slot and tells the clients.
fn replace_living_slot(
    living: &LivingEntity,
    mojang_slot: usize,
    item_stack: &ItemStack,
    modified_count: &mut i32,
) {
    let mapped_eq = living_equipment_slot(mojang_slot);

    if let Some(eq) = mapped_eq {
        living
            .entity_equipment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .put(&eq, item_stack.clone());
        living.send_equipment_changes(&[(eq, item_stack.clone())]);
        *modified_count += 1;
    }
}

/// The equipment slot a command slot names on a mob.
const fn living_equipment_slot(mojang_slot: usize) -> Option<EquipmentSlot> {
    if mojang_slot == 98 {
        Some(EquipmentSlot::MAIN_HAND)
    } else if mojang_slot == 99 {
        Some(EquipmentSlot::OFF_HAND)
    } else if mojang_slot == 100 {
        Some(EquipmentSlot::FEET)
    } else if mojang_slot == 101 {
        Some(EquipmentSlot::LEGS)
    } else if mojang_slot == 102 {
        Some(EquipmentSlot::CHEST)
    } else if mojang_slot == 103 {
        Some(EquipmentSlot::HEAD)
    } else if mojang_slot == 105 {
        Some(EquipmentSlot::BODY)
    } else if mojang_slot == 106 {
        Some(EquipmentSlot::SADDLE)
    } else {
        None
    }
}

/// The feedback: the one target by name, or the number of targets changed.
fn success_message(
    targets: &[Arc<dyn EntityBase>],
    modified_count: i32,
    item: &Item,
    count: i32,
) -> TextComponent {
    let item_name = item.registry_key;
    if targets.len() == 1 {
        TextComponent::translate_cross(
            translation::java::COMMANDS_ITEM_ENTITY_SET_SUCCESS_SINGLE,
            translation::java::COMMANDS_ITEM_ENTITY_SET_SUCCESS_SINGLE,
            [
                targets[0].as_ref().get_display_name(),
                TextComponent::text("[")
                    .add_child(item.translated_name())
                    .add_child(TextComponent::text("]"))
                    .hover_event(HoverEvent::ShowItem {
                        id: item_name.to_string().into(),
                        count: Some(count),
                        components: None,
                    }),
            ],
        )
    } else {
        TextComponent::translate_cross(
            translation::java::COMMANDS_ITEM_ENTITY_SET_SUCCESS_MULTIPLE,
            translation::java::COMMANDS_ITEM_ENTITY_SET_SUCCESS_MULTIPLE,
            [
                TextComponent::text(modified_count.to_string()),
                TextComponent::text("[")
                    .add_child(item.translated_name())
                    .add_child(TextComponent::text("]"))
                    .hover_event(HoverEvent::ShowItem {
                        id: item_name.to_string().into(),
                        count: Some(count),
                        components: None,
                    }),
            ],
        )
    }
}

fn block_item_node() -> RequiredArgumentBuilder {
    argument("item", ItemStackArgumentType)
        .executes(BlockReplaceExecutor { has_count: false })
        .then(
            argument("count", IntegerArgumentType::new(1, 99))
                .executes(BlockReplaceExecutor { has_count: true }),
        )
}

fn entity_item_node() -> RequiredArgumentBuilder {
    argument("item", ItemStackArgumentType)
        .executes(EntityReplaceExecutor { has_count: false })
        .then(
            argument("count", IntegerArgumentType::new(1, 99))
                .executes(EntityReplaceExecutor { has_count: true }),
        )
}

pub fn register(dispatcher: &mut CommandDispatcher, registry: &PermissionRegistry) {
    registry.register_permission_or_panic(Permission::new(
        PERMISSION,
        DESCRIPTION,
        PermissionDefault::Op(PermissionLvl::Two),
    ));

    let block_node = literal("block").then(
        argument("target", BlockPosArgumentType).then(
            argument(
                "slots",
                ClientParsed::new(SlotArgumentType, JavaClientArgumentType::SlotSource),
            )
            .then(literal("with").then(block_item_node())),
        ),
    );

    let entity_node = literal("entity").then(
        argument("target", EntityArgumentType::Entities).then(
            argument(
                "slots",
                ClientParsed::new(SlotArgumentType, JavaClientArgumentType::SlotSource),
            )
            .then(literal("with").then(entity_item_node())),
        ),
    );

    dispatcher.register(
        command("item", DESCRIPTION)
            .requires(PERMISSION)
            .then(literal("replace").then(block_node).then(entity_node)),
    );
}

#[cfg(test)]
mod golden;
