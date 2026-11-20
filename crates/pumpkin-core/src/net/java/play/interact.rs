#[allow(clippy::wildcard_imports)]
use super::*;

impl JavaClient {
    pub fn handle_interact(
        &self,
        player: &Arc<Player>,
        interact: &SInteract,
        server: &Arc<Server>,
    ) {
        if !player.has_client_loaded() {
            return;
        }
        player.update_last_action_time();
        let entity_id = interact.entity_id;

        let sneaking = interact.sneaking;
        let player_entity = &player.get_entity();
        if player_entity.is_sneaking() != sneaking {
            player_entity.set_sneaking(sneaking);
        }
        let Ok(action) = ActionType::try_from(interact.r#type.0) else {
            self.try_kick(&TextComponent::text("Invalid action type"));
            return;
        };

        // Resolve the target entity for the event
        let world = player_entity.world.load_full();
        let player_target = world.get_player_by_id(entity_id.0);
        let target: Option<Arc<dyn EntityBase>> = player_target
            .as_ref()
            .map(|p| Arc::clone(p) as Arc<dyn EntityBase>)
            .or_else(|| world.get_entity_by_id(entity_id.0));

        if let Some(target) = target {
            if !player.can_interact_with_entity(target.as_ref(), 1.0) {
                return;
            }
            if player.gamemode.load() == GameMode::Spectator {
                // Vanilla 26.3: an attack is dropped, and an interaction only opens the
                // target's menu (not ported); the camera comes from the spectate packet.
                return;
            }
            send_cancellable_blocking! {{
                server;
                PlayerInteractEntityEvent::new(
                    player,
                    Arc::clone(&target),
                    action,
                    interact.target_position,
                    sneaking,
                );

                'after: {
                    match event.action {
                        ActionType::Attack => self.attack_entity(
                            player,
                            server,
                            &world,
                            entity_id,
                            player_target.as_ref(),
                            &event.target,
                        ),
                        ActionType::Interact | ActionType::InteractAt => {
                            Self::interact_entity(player, server, interact, event.action, &event.target);
                        }
                    }
                }
            }}
        } else {
            // Entity not found
            send_cancellable_blocking! {{
                server;
                PlayerInteractUnknownEntityEvent::new(player, entity_id.0, action);

                'after: {
                    // Vanilla ignores an unknown target (no kick): it is usually one the server
                    // has just removed (card #61).
                    let _ = event;
                }
            }}
        }
    }
}

mod entity_use;
#[cfg(test)]
mod golden;
#[cfg(test)]
mod hand_tests;
