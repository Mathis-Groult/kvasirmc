//! Light levels and brightness.

use super::{MAX_LIGHT_LEVEL, World, brightness};
use pumpkin_config::lighting::LightingEngineConfig;
use pumpkin_util::math::position::BlockPos;

impl World {
    pub fn get_lighting_config(&self) -> LightingEngineConfig {
        self.server
            .upgrade()
            .map(|s| s.advanced_config.world.lighting)
            .unwrap_or_default()
    }

    pub fn get_raw_brightness(&self, pos: &BlockPos, sky_darken: u8) -> u8 {
        let sky_light = self.get_sky_light_level(pos).saturating_sub(sky_darken);
        let block_light = self.get_block_light_level(pos).unwrap_or(0);
        sky_light.max(block_light)
    }

    pub fn get_max_local_raw_brightness(&self, pos: &BlockPos) -> u8 {
        self.get_raw_brightness(pos, self.get_sky_darken() as u8)
    }

    /// local brightness through the dimension curve.
    #[must_use]
    pub fn get_light_level_dependent_magic_value(&self, pos: &BlockPos) -> f32 {
        self.light_level_dependent_magic_value_with_sky(pos, self.get_sky_light_level(pos))
    }

    /// [`Self::get_light_level_dependent_magic_value`] for a sky light the caller already read.
    #[must_use]
    pub fn light_level_dependent_magic_value_with_sky(&self, pos: &BlockPos, sky_light: u8) -> f32 {
        let sky_light = sky_light.saturating_sub(self.get_sky_darken() as u8);
        let block_light = self.get_block_light_level(pos).unwrap_or(0);
        brightness::light_level_curve(sky_light.max(block_light), self.dimension.ambient_light)
    }

    pub fn get_block_light_level(&self, position: &BlockPos) -> Option<u8> {
        self.level
            .light_engine
            .get_block_light_level(&self.level, position)
    }

    pub fn get_sky_light_level(&self, position: &BlockPos) -> u8 {
        self.level
            .light_engine
            .get_sky_light_level(&self.level, position)
    }

    #[must_use]
    pub fn can_see_sky(&self, position: &BlockPos) -> bool {
        self.is_within_build_height(position)
            && self.get_sky_light_level(position) >= MAX_LIGHT_LEVEL
    }

    /// [`Self::can_see_sky`] for a sky light the caller already read.
    #[must_use]
    pub const fn can_see_sky_with_light(&self, position: &BlockPos, sky_light: u8) -> bool {
        self.is_within_build_height(position) && sky_light >= MAX_LIGHT_LEVEL
    }

    pub fn set_block_light_level(&self, position: &BlockPos, light_level: u8) {
        let _ = self
            .level
            .light_engine
            .set_block_light_level(&self.level, position, light_level);
    }

    pub fn set_sky_light_level(&self, position: &BlockPos, light_level: u8) {
        let _ = self
            .level
            .light_engine
            .set_sky_light_level(&self.level, position, light_level);
    }
}
