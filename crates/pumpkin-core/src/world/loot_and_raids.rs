//! Loot tables and raids.

use super::World;
use pumpkin_util::math::position::BlockPos;

impl World {
    #[must_use]
    pub fn get_loot_table(&self, key: &str) -> Option<crate::world::loot::LootTableHandle> {
        self.server.upgrade().map_or_else(
            || {
                let full_key = if key.contains(':') {
                    key.to_string()
                } else {
                    format!("minecraft:{key}")
                };
                pumpkin_data::loot_table::get_loot_table(key)
                    .or_else(|| pumpkin_data::loot_table::get_loot_table(&full_key))
                    .map(crate::world::loot::LootTableHandle::Static)
            },
            |server| server.datapack_manager.get_loot_table(key),
        )
    }

    pub fn generate_loot(&self, loot_table: String) {
        let mut loot_event =
            crate::plugin::api::events::world::loot_generate::LootGenerateEvent::new(loot_table);
        if let Some(server) = self.server.upgrade() {
            server
                .plugin_manager
                .fire_blocking(&server, &mut loot_event);
        }
    }

    pub fn trigger_raid(&self, pos: BlockPos) {
        let mut raid_event =
            crate::plugin::api::events::raid::raid_trigger::RaidTriggerEvent::new(pos);
        if let Some(server) = self.server.upgrade() {
            server
                .plugin_manager
                .fire_blocking(&server, &mut raid_event);
        }
    }

    pub fn spawn_raid_wave(&self, wave: u32, pos: BlockPos) {
        let mut wave_event =
            crate::plugin::api::events::raid::raid_spawn_wave::RaidSpawnWaveEvent::new(wave, pos);
        if let Some(server) = self.server.upgrade() {
            server
                .plugin_manager
                .fire_blocking(&server, &mut wave_event);
        }
    }

    pub fn finish_raid(&self, victory: bool) {
        let mut raid_event =
            crate::plugin::api::events::raid::raid_finish::RaidFinishEvent::new(victory);
        if let Some(server) = self.server.upgrade() {
            server
                .plugin_manager
                .fire_blocking(&server, &mut raid_event);
        }
    }

    pub fn stop_raid(&self, reason: String) {
        let mut raid_event =
            crate::plugin::api::events::raid::raid_stop::RaidStopEvent::new(reason);
        if let Some(server) = self.server.upgrade() {
            server
                .plugin_manager
                .fire_blocking(&server, &mut raid_event);
        }
    }
}
