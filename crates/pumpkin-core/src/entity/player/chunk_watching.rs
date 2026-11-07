//! Which chunks the player watches and holds loaded.

use super::Player;
use crate::world::World;
use pumpkin_protocol::java::client::play::CUnloadChunk;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::Ordering;

impl Player {
    /// Pairs tracked entities in chunks that were just queued for a player.
    pub(super) fn pair_entities_in_chunks(
        &self,
        world: &crate::world::World,
        chunks: &[pumpkin_util::math::vector2::Vector2<i32>],
    ) {
        if chunks.is_empty() {
            return;
        }
        if let Some(player) = world.get_player_by_uuid(self.gameprofile.id) {
            world
                .entity_tracker
                .update_player_chunks(&player, world, chunks);
        }
    }

    pub fn clean_up_chunk_tickets(&self, level: &Arc<pumpkin_world::level::Level>) {
        let mut lock = level
            .chunk_loading
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let held = self
            .held_chunk_tickets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some((center, view_level, sim_level)) = held {
            if let Some(view) = view_level {
                lock.remove_ticket(center, view);
            }
            if let Some(sim) = sim_level {
                lock.remove_ticket(center, sim);
            }
        }
        lock.send_change();
        level.should_unload.store(true, Ordering::Relaxed);
        level.level_channel.notify();
    }

    pub fn update_chunk_tickets_for_gamemode(self: &Arc<Self>) {
        crate::world::chunker::update_position(self);
    }

    pub fn change_world_chunks(
        &self,
        old_level: &Arc<pumpkin_world::level::Level>,
        new_world: &Arc<crate::world::World>,
    ) {
        self.clean_up_chunk_tickets(old_level);
        if let Ok(mut listener) = self.chunk_listener.lock() {
            *listener = new_world.level.chunk_listener.add_global_chunk_listener();
        }
        self.chunk_send_epoch.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut sender) = self.chunk_sender.lock() {
            sender.reset();
        }
    }

    pub async fn unload_watched_chunks(&self, world: &World) {
        let radial_chunks = self.watched_section.load().all_chunks_within();
        let level = &world.level;
        let chunks_to_clean = level.mark_chunks_as_not_watched(radial_chunks);
        if !chunks_to_clean.is_empty() {
            world.remove_entities_in_chunks(&chunks_to_clean).await;
            level.clean_entity_chunks(&chunks_to_clean);
        }
        for chunk in &chunks_to_clean {
            self.send_client_packet(&CUnloadChunk::new(chunk.x, chunk.y))
                .await;
        }

        self.watched_section.store(Cylindrical::new(
            Vector2::new(0, 0),
            NonZero::new(1).unwrap_or(NonZero::<u8>::MIN),
        ));
    }
}
