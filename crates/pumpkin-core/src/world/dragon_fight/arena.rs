//! Keeping the End arena loaded for the fight (vanilla 26.3 `EndDragonFight`).

use std::sync::Arc;

use pumpkin_util::math::vector2::Vector2;

use super::{ARENA_TICKET_LEVEL, DragonFight};
use crate::world::World;

impl DragonFight {
    /// Vanilla `addTicketWithRadius(TicketType.DRAGON, ChunkPos(0, 0), 9)` while players are
    /// in the arena, removed when none are. Pumpkin's ticket levels are vanilla's plus 10
    /// (`ChunkLoading::FULL_CHUNK_LEVEL` is 43 where vanilla's full-chunk level is 33).
    pub(super) fn hold_arena_ticket(&mut self, world: &World, wanted: bool) {
        if wanted == self.arena_ticket {
            return;
        }
        let level = pumpkin_world::chunk_system::ChunkLoading::FULL_CHUNK_LEVEL - 33
            + ARENA_TICKET_LEVEL as i8;
        let center = Vector2::new(self.origin.0.x >> 4, self.origin.0.z >> 4);
        let mut loading = world
            .level
            .chunk_loading
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if wanted {
            loading.add_ticket(center, level);
        } else {
            loading.remove_ticket(center, level);
        }
        loading.send_change();
        self.arena_ticket = wanted;
    }

    #[must_use]
    pub fn is_arena_loaded(&self, world: &Arc<World>) -> bool {
        if self.skip_arena_loaded_check {
            return true;
        }
        let center_chunk_x = self.origin.0.x >> 4;
        let center_chunk_z = self.origin.0.z >> 4;
        for dx in -3..=3 {
            for dz in -3..=3 {
                let cp = Vector2::new(center_chunk_x + dx, center_chunk_z + dz);
                if !world.level.is_chunk_loaded(&cp) {
                    return false;
                }
            }
        }
        true
    }
}
