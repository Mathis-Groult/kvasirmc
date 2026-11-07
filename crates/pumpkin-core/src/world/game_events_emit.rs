//! Emitting game events.

use super::{World, game_event};
use pumpkin_util::math::vector3::Vector3;

impl World {
    pub fn emit_game_event(&self, event_key: impl Into<String>, position: Vector3<f64>) {
        self.emit_game_event_with_context(
            event_key,
            position,
            &game_event::GameEventContext {
                source_entity: None,
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
