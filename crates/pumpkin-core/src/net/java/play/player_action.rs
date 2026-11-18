#[allow(clippy::wildcard_imports)]
use super::*;

impl JavaClient {
    pub fn handle_player_action(
        &self,
        player: &Arc<Player>,
        player_action: &SPlayerAction,
        server: &Server,
    ) {
        if !player.has_client_loaded() {
            return;
        }
        player.update_last_action_time();
        match Status::try_from(player_action.status.0) {
            Ok(status) => match status {
                Status::StartedDigging => self.start_digging(player, player_action, server),
                Status::CancelledDigging => self.cancel_digging(player, player_action),
                Status::FinishedDigging => self.finish_digging(player, player_action, server),
                Status::DropItem => {
                    player.drop_held_item(false);
                }
                Status::DropItemStack => {
                    player.drop_held_item(true);
                }
                Status::ReleaseItemInUse => {
                    let item_in_use = player
                        .living_entity
                        .item_in_use
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clone();
                    if let Some(stack) = item_in_use {
                        server.item_registry.on_stopped_using(&stack, player);
                    }

                    player.living_entity.clear_active_hand();
                }
                Status::SwapItem => {
                    player.swap_item();
                }
                Status::SpearJab => {
                    if player.gamemode.load() == GameMode::Spectator {
                        return;
                    }

                    let stack = player.inventory().held_item();
                    server.item_registry.on_spear_jab(&stack, player);
                }
                // The block being mined does not change, so there is nothing to update
                Status::ChangeDestroyDirection => {}
            },
            Err(_) => self.try_kick(&TextComponent::text("Invalid status")),
        }
    }

    pub fn update_sequence(&self, sequence: i32) {
        if sequence < 0 {
            error!("Expected packet sequence >= 0");
        }
        self.packet_sequence.store(
            self.packet_sequence.load(Ordering::Relaxed).max(sequence),
            Ordering::Relaxed,
        );
    }

    fn sync_block_state_to_client(&self, world: &World, position: BlockPos) {
        let synced_state_id = world.get_block_state_id(&position);
        self.try_send_packet(&CBlockUpdate::new(
            position,
            VarInt(i32::from(synced_state_id.as_u16())),
        ));
    }
}

mod digging;

#[cfg(test)]
mod golden;
