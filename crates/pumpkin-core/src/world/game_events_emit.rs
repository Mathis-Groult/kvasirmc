//! Emitting game events.

use super::{World, game_event};
use pumpkin_util::math::vector3::Vector3;

impl World {
    /// Vanilla `BlockItem.place`: sets the block, then `BLOCK_PLACE` from the player with the
    /// placed state.
    pub fn place_block_for(
        self: &std::sync::Arc<Self>,
        player: &crate::entity::player::Player,
        position: &pumpkin_util::math::position::BlockPos,
        state: pumpkin_data::BlockStateId,
    ) {
        self.set_block_state(
            position,
            state,
            pumpkin_world::world::BlockFlags::NOTIFY_ALL,
        );
        self.emit_game_event_with_context(
            pumpkin_data::game_event::GameEvent::BlockPlace.name(),
            position.to_centered_f64(),
            &game_event::GameEventContext {
                source_entity: Some(player),
                affected_state: Some(state),
            },
        );
    }

    pub fn emit_game_event(&self, event_key: impl Into<String>, position: Vector3<f64>) {
        self.emit_game_event_with_context(
            event_key,
            position,
            &game_event::GameEventContext {
                source_entity: None,
                affected_state: None,
            },
        );
    }

    pub fn emit_game_event_with_context(
        &self,
        event_key: impl Into<String>,
        position: Vector3<f64>,
        context: &game_event::GameEventContext<'_>,
    ) {
        let mut event = crate::plugin::api::events::world::generic_game::GenericGameEvent::new(
            event_key.into(),
            position,
        );
        if let Some(server) = self.server.upgrade() {
            server.plugin_manager.fire_blocking(&server, &mut event);
        }
        if event.cancelled {
            return;
        }
        if let Some(kind) = pumpkin_data::game_event::GameEvent::from_name(&event.event_key) {
            self.post_game_event(kind, event.position, context);
        }
    }
}
