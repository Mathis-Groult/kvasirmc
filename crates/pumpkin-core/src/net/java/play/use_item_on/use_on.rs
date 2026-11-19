//! The use-item-on packet once the guards passed: the clicked block's own use, then the held
//! item's use on the block or its placement, the advancement trigger and the hand slot update.

#[allow(clippy::wildcard_imports)]
use super::super::*;
use crate::item::registry::should_try_block_placement;
use pumpkin_data::block_properties::BlockProperties;

/// What every step of one use-item-on packet works on.
pub(super) struct UseOn<'a> {
    pub player: &'a Arc<Player>,
    pub server: &'a Arc<Server>,
    pub use_item_on: &'a SUseItemOn,
    pub world: &'a Arc<World>,
    pub block: &'static Block,
    pub face: BlockDirection,
    pub hand: Hand,
}

impl UseOn<'_> {
    /// A spectator only opens the clicked block's screen, if it has one.
    pub(super) fn spectator_use(player: &Arc<Player>, server: &Arc<Server>, position: BlockPos) {
        let entity = &player.get_entity();
        let world = entity.world.load_full();
        let block = world.get_block(&position);

        let event = PlayerInteractEvent::new(
            player,
            InteractAction::RightClickBlock,
            block,
            Some(position),
        );

        send_cancellable_blocking! {{
            server;
            event;
            'cancelled: {
                let state_id = world.get_block_state_id(&position);
                player.try_send_client_packet(&CBlockUpdate::new(
                    position,
                    VarInt(i32::from(state_id.as_u16())),
                ));
                return;
            }
        }}

        if let Some(factory) = server
            .block_registry
            .get_screen_handler_factory(block, player, &position, server, &world)
        {
            player.open_handled_screen(factory.as_ref(), Some(position));
        }
    }

    /// The clicked block's own use (skipped while sneaking with something in either hand).
    /// Returns whether it consumed the action.
    pub(super) fn block_use(
        &self,
        item: &mut ItemStack,
        before: &ItemStack,
        held_item_empty: bool,
        off_hand_item_empty: bool,
    ) -> bool {
        let player = self.player;
        let hand = self.hand;
        let equipment_slot = if matches!(hand, Hand::Right) {
            EquipmentSlot::MAIN_HAND
        } else {
            EquipmentSlot::OFF_HAND
        };

        let sneaking = player.get_entity().is_sneaking();

        // Code based on the java class ServerPlayerInteractionManager
        if !(sneaking && (!held_item_empty || !off_hand_item_empty)) {
            let result = JavaClient::call_use_item_on(
                player,
                &self.use_item_on.position,
                &self.use_item_on.cursor_pos,
                self.face,
                item,
                &equipment_slot,
                self.world,
                self.block,
                self.server,
            );
            if result.consumes_action() {
                self.trigger_item_used_on_block(before);

                if matches!(result, BlockActionResult::SuccessServer) {
                    player.swing_hand(hand, true);
                }
                return true;
            }
        }
        false
    }

    /// The held item's use on the block, or its placement when it is a block item.
    pub(super) fn item_use(
        &self,
        mut item: ItemStack,
        before: &ItemStack,
    ) -> Result<(), BlockPlacingError> {
        let player = self.player;
        let server = self.server;
        let position = self.use_item_on.position;
        let face = self.face;
        let inventory = player.inventory();
        let mut should_try_decrement = false;

        let slot_index = if matches!(self.hand, Hand::Right) {
            inventory.get_selected_slot() as usize
        } else {
            PlayerInventory::OFF_HAND_SLOT
        };

        if item.is_empty() {
            // TODO item cool down
            // If the hand is empty we stop here
            return Ok(());
        }

        let item_result = server.item_registry.use_on_block(
            &mut item,
            player,
            position,
            face,
            self.use_item_on.cursor_pos,
            self.block,
            server,
        );

        if should_try_block_placement(&item_result) {
            // Check if the item is a block, because not every item can be placed :D
            let item_id = item.item.id;
            if let Some(block) = Block::from_item_id(item_id) {
                should_try_decrement = JavaClient::run_is_block_place(
                    player,
                    block,
                    server,
                    self.use_item_on,
                    position,
                    face,
                )?;
            }
        }

        if should_try_decrement {
            // TODO: Config
            // Decrease block count
            if player.gamemode.load() != GameMode::Creative {
                item.decrement(1);
            }
        }

        let after = item.clone();

        if item_result.consumes_action() || !item.are_equal(before) {
            self.trigger_item_used_on_block(before);
        }

        if matches!(item_result, BlockActionResult::SuccessServer) {
            player.swing_hand(self.hand, true);
        }

        self.update_hand(slot_index, before, after);
        Ok(())
    }

    /// Sends the break status of a used-up item, then syncs and stores the hand's new stack.
    fn update_hand(&self, slot_index: usize, before: &ItemStack, after: ItemStack) {
        let player = self.player;
        let inventory = player.inventory();
        // Broadcast the break entity status before the slot sync; the client
        // needs the old item texture in the slot for break particles.
        if !before.is_empty() && after.is_empty() {
            let slot = if slot_index == player.inventory.get_selected_slot() as usize {
                &EquipmentSlot::MAIN_HAND
            } else {
                &EquipmentSlot::OFF_HAND
            };
            if before.is_damageable() {
                player.increment_stat(StatisticCategory::Broken, before.item.id as i32, 1);
            }
            player.world().send_entity_status(
                player.get_entity(),
                equipment_break_status(slot),
                ActorEventID::None,
            );
        }

        if !after.are_equal(before) {
            player.sync_hand_slot(slot_index, after.clone());
            inventory.set_stack_in_hand(self.hand, after);
        }
    }

    /// The `ItemUsedOnBlock` advancement trigger: the clicked block, the item as it was before
    /// the use, a copper bulb's `lit` state, whether a campfire smokes up to the block, the biome.
    fn trigger_item_used_on_block(&self, before: &ItemStack) {
        let world = self.world;
        let block = self.block;
        let position = self.use_item_on.position;
        let state =
            pumpkin_data::block_properties::CopperBulbLikeProperties::handles_block_id(block.id)
                .then(|| {
                    let props =
                        pumpkin_data::block_properties::CopperBulbLikeProperties::from_state_id(
                            world.get_block_state(&position).id,
                        );
                    ("lit".to_string(), props.r#lit.to_string())
                });

        let mut is_smokey = false;
        for dy in 1..=5 {
            let check_pos = BlockPos::new(position.0.x, position.0.y - dy, position.0.z);
            let b = world.get_block(&check_pos);
            if b == &pumpkin_data::Block::CAMPFIRE || b == &pumpkin_data::Block::SOUL_CAMPFIRE {
                is_smokey = true;
                break;
            }
            if !world.get_block_state(&check_pos).is_air() {
                break;
            }
        }

        let biome = Some(world.get_biome(&position).registry_id.to_string());

        self.player.trigger_advancement(
            crate::entity::player::advancement::trigger::AdvancementTrigger::ItemUsedOnBlock {
                block_id: format!("minecraft:{}", block.name),
                item_id: before.item.registry_key.to_string(),
                state,
                smokey: Some(is_smokey),
                biome,
            },
        );
    }
}
