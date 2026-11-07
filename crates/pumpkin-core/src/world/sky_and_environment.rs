//! Daylight, moon and environment rules.

use super::World;
use super::environment::EnvironmentAttributes;
use pumpkin_data::environment_attribute::{Activity, MoonPhase};
use pumpkin_util::{Difficulty, math::position::BlockPos};

impl World {
    /// Returns true if enough players are sleeping and we should skip the night.
    pub fn should_skip_night(&self) -> bool {
        let players = self.players.load();

        let eligible_players: Vec<_> = players
            .iter()
            .filter(|player| {
                !player
                    .sleeping_ignored
                    .load(std::sync::atomic::Ordering::Relaxed)
            })
            .collect();
        let player_count = eligible_players.len();
        let sleeping_player_count = eligible_players
            .iter()
            .filter(|player| {
                player
                    .sleeping_since
                    .load()
                    .is_some_and(|since| since >= 100)
            })
            .count();
        drop(players);

        if player_count == 0 {
            return false;
        }

        let sleep_percentage = self
            .level_info
            .load()
            .game_rules
            .players_sleeping_percentage
            .clamp(0, 100);
        let required_sleeping =
            ((player_count as f64 * sleep_percentage as f64) / 100.0).ceil() as usize;
        let required_sleeping = required_sleeping.max(1);

        sleeping_player_count >= required_sleeping
    }

    #[must_use]
    pub const fn environment_attributes(&self) -> EnvironmentAttributes<'_> {
        EnvironmentAttributes::new(self)
    }

    #[must_use]
    pub fn get_sky_darken(&self) -> i32 {
        let sky_light_level = self.environment_attributes().get_dimension_value_f32(
            pumpkin_data::environment_attribute::EnvironmentAttribute::GameplaySkyLightLevel,
        );
        (15.0 - sky_light_level).clamp(0.0, 15.0) as i32
    }

    #[must_use]
    pub fn is_bright_outside(&self) -> bool {
        !self.dimension.has_fixed_time && self.get_sky_darken() < 4
    }

    #[must_use]
    pub fn is_dark_outside(&self) -> bool {
        !self.dimension.has_fixed_time && !self.is_bright_outside()
    }

    /// Checks if daylight burns undead monsters (`EnvironmentAttributes.MONSTERS_BURN`).
    #[must_use]
    pub fn monsters_burn(&self, pos: &BlockPos) -> bool {
        self.environment_attributes().get_value_bool(
            pumpkin_data::environment_attribute::EnvironmentAttribute::GameplayMonstersBurn,
            pos,
        )
    }

    /// Checks if bees should stay inside beehives/nests (`EnvironmentAttributes.BEES_STAY_IN_HIVE`).
    #[must_use]
    pub fn bees_stay_in_hive(&self, pos: &BlockPos) -> bool {
        self.environment_attributes().get_value_bool(
            pumpkin_data::environment_attribute::EnvironmentAttribute::GameplayBeesStayInHive,
            pos,
        )
    }

    /// Checks if a creaking heart is active (`EnvironmentAttributes.CREAKING_ACTIVE`).
    #[must_use]
    pub fn creaking_active(&self, pos: &BlockPos) -> bool {
        self.environment_attributes().get_value_bool(
            pumpkin_data::environment_attribute::EnvironmentAttribute::GameplayCreakingActive,
            pos,
        )
    }

    /// Checks if an eyeblossom flower should be open (`EnvironmentAttributes.EYEBLOSSOM_OPEN`).
    #[must_use]
    pub fn eyeblossom_open(&self, pos: &BlockPos) -> Option<bool> {
        self.environment_attributes().get_value_tri_state(
            pumpkin_data::environment_attribute::EnvironmentAttribute::GameplayEyeblossomOpen,
            pos,
        )
    }

    #[must_use]
    pub fn get_effective_sky_brightness(&self, pos: &BlockPos) -> i32 {
        self.effective_sky_brightness_from(self.get_sky_light_level(pos))
    }

    pub(super) fn effective_sky_brightness_from(&self, sky_light: u8) -> i32 {
        i32::from(sky_light) - self.get_sky_darken()
    }

    #[must_use]
    pub fn get_sun_angle(&self, pos: &BlockPos) -> f32 {
        let sun_angle_deg = self.environment_attributes().get_value_f32(
            pumpkin_data::environment_attribute::EnvironmentAttribute::VisualSunAngle,
            pos,
        );
        sun_angle_deg * (std::f32::consts::PI / 180.0)
    }

    #[must_use]
    pub fn get_moon_phase(&self) -> MoonPhase {
        self.environment_attributes()
            .get_dimension_value_moon_phase()
    }

    #[must_use]
    pub fn can_pillager_patrol_spawn(&self, pos: &BlockPos) -> bool {
        self.environment_attributes().get_value_bool(
            pumpkin_data::environment_attribute::EnvironmentAttribute::GameplayCanPillagerPatrolSpawn,
            pos,
        )
    }

    #[must_use]
    pub fn surface_slime_spawn_chance(&self, pos: &BlockPos) -> f32 {
        self.environment_attributes().get_value_f32(
            pumpkin_data::environment_attribute::EnvironmentAttribute::GameplaySurfaceSlimeSpawnChance,
            pos,
        )
    }

    #[must_use]
    pub fn villager_activity(&self, pos: &BlockPos, baby: bool) -> Activity {
        self.environment_attributes().get_value_activity(baby, pos)
    }

    /// Returns whether monsters can be spawned in the world
    pub fn should_spawn_monsters(&self) -> bool {
        let level_data = self.level_info.load();
        level_data.game_rules.spawn_mobs
            && level_data.game_rules.spawn_monsters
            && level_data.difficulty != Difficulty::Peaceful
    }
}
