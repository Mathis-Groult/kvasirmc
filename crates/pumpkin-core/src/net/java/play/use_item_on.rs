#[allow(clippy::wildcard_imports)]
use super::*;
use use_on::UseOn;

impl JavaClient {
    pub fn handle_use_item_on(
        &self,
        player: &Arc<Player>,
        use_item_on: &SUseItemOn,
        server: &Arc<Server>,
    ) -> Result<(), BlockPlacingError> {
        if !player.has_client_loaded() {
            return Ok(());
        }
        player.update_last_action_time();
        self.update_sequence(use_item_on.sequence.0);

        let position = use_item_on.position;

        if !player.can_interact_with_block_at(&position, 1.0) {
            // TODO: maybe log?
            return Err(BlockPlacingError::BlockOutOfReach);
        }

        let Ok(face) = BlockDirection::try_from(use_item_on.face.0) else {
            return Err(BlockPlacingError::InvalidBlockFace);
        };

        let Ok(hand) = Hand::from_packet_id(use_item_on.hand.0) else {
            return Err(BlockPlacingError::InvalidHand);
        };

        if player.gamemode.load() == GameMode::Spectator {
            UseOn::spectator_use(player, server, position);
            return Ok(());
        }

        let inventory = player.inventory();
        let held_item = inventory.held_item();
        let off_hand_item = inventory.off_hand_item();
        let held_item_empty = held_item.is_empty();
        let off_hand_item_empty = off_hand_item.is_empty();

        let mut item = inventory.get_stack_in_hand(hand);
        let before = item.clone();
        let item_id = item.item.id;
        player.increment_stat(StatisticCategory::Used, item_id as i32, 1);

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
                return Ok(());
            }
        }}

        let use_on = UseOn {
            player,
            server,
            use_item_on,
            world: &world,
            block,
            face,
            hand,
        };
        if use_on.block_use(&mut item, &before, held_item_empty, off_hand_item_empty) {
            return Ok(());
        }
        use_on.item_use(item, &before)
    }

    #[expect(clippy::too_many_arguments)]
    fn call_use_item_on(
        player: &Arc<Player>,
        position: &BlockPos,
        cursor_pos: &Vector3<f32>,
        face: BlockDirection,
        held_item: &mut ItemStack,
        equipment_slot: &EquipmentSlot,
        world: &Arc<World>,
        block: &Block,
        server: &Arc<Server>,
    ) -> BlockActionResult {
        let result = server.block_registry.use_with_item(
            block,
            player,
            position,
            &BlockHitResult {
                face: &face,
                cursor_pos,
            },
            held_item,
            equipment_slot,
            server,
            world,
        );

        if result.consumes_action() {
            // TODO: Trigger ITEM_USED_ON_BLOCK Criteria
            return result;
        }

        if matches!(result, BlockActionResult::PassToDefaultBlockAction) {
            let result = server.block_registry.on_use(
                block,
                player,
                position,
                &BlockHitResult {
                    face: &face,
                    cursor_pos,
                },
                server,
                world,
            );

            if result.consumes_action() {
                // TODO: Trigger DEFAULT_BLOCK_USE Criteria
                return result;
            }
        }

        BlockActionResult::Pass
    }

    fn run_is_block_place(
        player: &Arc<Player>,
        block: &'static Block,
        server: &Arc<Server>,
        use_item_on: &SUseItemOn,
        location: BlockPos,
        face: BlockDirection,
    ) -> Result<bool, BlockPlacingError> {
        match server
            .block_registry
            .place_block(player, block, server, use_item_on, location, face)
        {
            Ok(Some((final_block_pos, new_state))) => {
                player.try_send_client_packet(&CBlockUpdate::new(
                    final_block_pos,
                    VarInt(i32::from(new_state.as_u16())),
                ));
                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(crate::block::registry::BlockPlacingError::InvalidGamemode) => {
                Err(BlockPlacingError::InvalidGamemode)
            }
            Err(crate::block::registry::BlockPlacingError::BlockOutOfWorld) => {
                Err(BlockPlacingError::BlockOutOfWorld)
            }
        }
    }
}

#[cfg(test)]
mod golden;
#[cfg(test)]
mod hand_tests;
mod use_on;
