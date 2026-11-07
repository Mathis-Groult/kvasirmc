//! World age, time of day and weather.

use super::World;
use crate::server::Server;
use pumpkin_util::math::position::BlockPos;
use pumpkin_world::chunk::ChunkHeightmapType::MotionBlocking;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

impl World {
    pub fn tick_environment(self: &Arc<Self>) {
        let (is_night, time_of_day) = {
            let mut level_time = self
                .level_time
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let advance_time = self.level_info.load().game_rules.advance_time;
            // Vanilla `ServerLevel.tickTime`. Periodic `CUpdateTime` is
            // `forceGameTimeSynchronization` in `Server::tick_worlds`, *before*
            // this increment.
            level_time.tick(advance_time);

            // Auto-save logic
            if level_time.world_age % 100 == 0 {
                self.level.should_unload.store(true, Relaxed);
                let cleaned_chunks = self.level.clean_memory();
                if !cleaned_chunks.is_empty() {
                    let world_clone = self.clone();
                    if let Some(server) = self.server.upgrade() {
                        server.spawn_task(async move {
                            world_clone.remove_entities_in_chunks(&cleaned_chunks).await;
                            world_clone.level.clean_entity_chunks(&cleaned_chunks);
                        });
                    }
                }
                // If autosave is configured and this tick will trigger an autosave, don't double notify
                if self.level.autosave_ticks == 0 {
                    self.level.level_channel.notify();
                } else {
                    let autosave = self.level.autosave_ticks as i64;
                    if autosave == 0 || level_time.world_age % autosave != 0 {
                        self.level.level_channel.notify();
                    }
                }
            }
            if self.level.autosave_ticks > 0 && self.level.save_enabled.load(Relaxed) {
                let autosave = self.level.autosave_ticks as i64;
                if autosave > 0 && level_time.world_age % autosave == 0 {
                    self.level.should_save.store(true, Relaxed);
                    self.level.level_channel.notify();
                }
            }
            (level_time.is_night(), level_time.time_of_day)
        };

        let (should_reset_weather, weather_cycle_enabled) = {
            let mut weather = self
                .weather
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            weather.tick_weather(self);
            (
                weather.raining || weather.thundering,
                weather.weather_cycle_enabled,
            )
        };

        if self.should_skip_night() && is_night {
            let level_time = {
                let mut guard = self
                    .level_time
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let time = time_of_day + 24000;
                guard.set_time(time - time % 24000);
                guard.clone()
            };
            level_time.send_time(self);

            for player in self.players.load().iter() {
                player.wake_up();
            }

            if weather_cycle_enabled && should_reset_weather {
                let mut weather = self
                    .weather
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                weather.reset_weather_cycle(self);
            }
        }
    }

    pub fn get_world_age(&self) -> i64 {
        self.level_time
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .world_age
    }

    /// Vanilla `MinecraftServer.forceGameTimeSynchronization`.
    ///
    /// Broadcasts current overworld `getGameTime()` with an empty clock map.
    /// Must run before [`crate::world::time::LevelTime::tick`]: the client
    /// already advanced to this number in `ClientLevel.tickTime()`. Sending the
    /// post-increment value, or a clock snapshot, makes `getGameTime()` hold
    /// for two client ticks, so `Entity.limitPistonMovement` does not reset
    /// `pistonDeltas` and clips the second honey/piston step at ±0.51.
    pub fn force_game_time_synchronization(&self, server: &Server) {
        let level_time = self
            .level_time
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        level_time.send_game_time_sync(server);
    }

    pub fn get_time_of_day(&self) -> i64 {
        self.level_time
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .time_of_day
    }

    pub fn set_time_of_day(&self, time: i64) {
        let level_time = {
            let mut guard = self
                .level_time
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard.set_time(time);
            guard.clone()
        };
        level_time.send_time(self);
    }

    pub fn is_raining(&self) -> bool {
        self.weather
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .raining
    }

    pub fn is_raining_at(&self, pos: &BlockPos) -> bool {
        if !self.is_raining() {
            return false;
        }
        if self.get_heightmap_height(MotionBlocking, pos.0.x, pos.0.z) + 1 > pos.0.y {
            return false;
        }
        self.can_see_sky(pos)
            && self
                .get_biome(pos)
                .weather
                .is_rain_at(pos.0.x, pos.0.y, pos.0.z, self.sea_level)
    }

    pub fn set_raining(&self, raining: bool) {
        if let Some(server) = self.server.upgrade() {
            let world_arc = server.get_world_from_dimension(&self.dimension);
            let mut event =
                crate::plugin::api::events::world::weather_change::WeatherChangeEvent::new(
                    world_arc, raining,
                );
            server.plugin_manager.fire_blocking(&server, &mut event);
            if event.cancelled {
                return;
            }
        }
        let mut weather = self
            .weather
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if weather.raining != raining {
            let thunder = weather.thundering;
            weather.set_weather_parameters(self, 0, 0, raining, thunder);
        }
    }

    pub fn is_thundering(&self) -> bool {
        self.weather
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .thundering
    }

    pub fn set_thundering(&self, thundering: bool) {
        if let Some(server) = self.server.upgrade() {
            let world_arc = server.get_world_from_dimension(&self.dimension);
            let mut event =
                crate::plugin::api::events::world::weather_change::ThunderChangeEvent::new(
                    world_arc, thundering,
                );
            server.plugin_manager.fire_blocking(&server, &mut event);
            if event.cancelled {
                return;
            }
        }
        let mut weather = self
            .weather
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if weather.thundering != thundering {
            let raining = weather.raining;
            weather.set_weather_parameters(self, 0, 0, raining, thundering);
        }
    }

    pub fn skip_time(&self, skip_amount: i64) {
        let mut time_event =
            crate::plugin::api::events::world::time_skip::TimeSkipEvent::new(skip_amount);
        if let Some(server) = self.server.upgrade() {
            server
                .plugin_manager
                .fire_blocking(&server, &mut time_event);
        }
    }
}
