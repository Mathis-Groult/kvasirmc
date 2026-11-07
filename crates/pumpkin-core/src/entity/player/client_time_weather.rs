//! The time and weather this player sees.

use super::{Player, PlayerWeather};
use crate::world::World;
use pumpkin_protocol::bedrock::client::set_time::CSetTime;
use pumpkin_protocol::java::client::play::CUpdateTime;

impl Player {
    /// Sends the world time to only this player.
    pub fn send_time(&self, world: &World) {
        let advance_time = {
            let lock = world.level_info.load();
            lock.game_rules.advance_time
        };

        let (clock_packet, time_packet) = {
            let l_world = world
                .level_time
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some((custom_time, relative)) = self.per_player_time.load() {
                let time_of_day = if relative {
                    (l_world.time_of_day as u64 + custom_time) as i64
                } else {
                    custom_time as i64
                };
                let paused = l_world.paused || !advance_time;
                let rate = if paused { 0.0 } else { l_world.rate };
                (
                    CUpdateTime::new_clock(
                        l_world.world_age,
                        0,
                        time_of_day,
                        l_world.partial_tick,
                        rate,
                    ),
                    CSetTime::new(time_of_day as _),
                )
            } else {
                let (total_ticks, partial_tick, rate) = l_world.pack_network_state(advance_time);
                (
                    CUpdateTime::new_clock(l_world.world_age, 0, total_ticks, partial_tick, rate),
                    CSetTime::new(l_world.query_daytime() as _),
                )
            }
        };

        self.client
            .try_enqueue_packet_editioned(&clock_packet, &time_packet);
    }

    /// Day time for this client, from its own world's `time_of_day`. Bedrock has
    /// no game-time clock packet, so `CSetTime` carries this instead.
    #[must_use]
    pub fn client_time_of_day(&self, world_time_of_day: i64) -> i64 {
        match self.per_player_time.load() {
            Some((custom_time, true)) => (world_time_of_day as u64 + custom_time) as i64,
            Some((custom_time, false)) => custom_time as i64,
            None => world_time_of_day % 24000,
        }
    }

    pub fn set_player_time(&self, time: u64, relative: bool) {
        let world = self.world();
        self.per_player_time.store(Some((time, relative)));
        self.send_time(&world);
    }

    pub fn reset_player_time(&self) {
        let world = self.world();
        self.per_player_time.store(None);
        self.send_time(&world);
    }

    pub fn get_player_time(&self) -> Option<u64> {
        self.per_player_time.load().map(|(t, _)| t)
    }

    pub fn is_player_time_relative(&self) -> bool {
        self.per_player_time.load().is_none_or(|(_, r)| r)
    }

    pub fn set_player_weather(&self, weather: PlayerWeather) {
        self.per_player_weather.store(Some(weather));
    }

    pub fn reset_player_weather(&self) {
        self.per_player_weather.store(None);
    }

    pub fn get_player_weather(&self) -> Option<PlayerWeather> {
        self.per_player_weather.load()
    }
}
