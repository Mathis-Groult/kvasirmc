//! Name, spawn, difficulty and game rules.

use super::World;
use crate::entity::player::Player;
use pumpkin_data::game_rules::{GameRule, GameRuleValue};
use pumpkin_util::{Difficulty, math::position::BlockPos};
use std::sync::Arc;

impl World {
    /// Get the world folder name (e.g., `world`, `world_nether`, `world_the_end`).
    /// Falls back to "world" if the name cannot be determined.
    pub fn get_world_name(&self) -> &str {
        self.level
            .level_folder
            .root_folder
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("world")
    }

    /// Returns the configured shared world spawn block position and rotation.
    #[must_use]
    pub fn get_spawn_location(&self) -> (BlockPos, f32, f32) {
        let level_info = self.level_info.load();
        (
            BlockPos::new(level_info.spawn_x, level_info.spawn_y, level_info.spawn_z),
            level_info.spawn_yaw,
            level_info.spawn_pitch,
        )
    }

    #[must_use]
    pub fn is_in_spawn_protection(&self, player: &Player, position: &BlockPos) -> bool {
        if player.permission_lvl.load() == pumpkin_util::permission::PermissionLvl::Four {
            return false;
        }

        let Some(server) = self.server.upgrade() else {
            return false;
        };

        let radius = server.basic_config.spawn_protection;
        if radius == 0 {
            return false;
        }

        let radius = i32::try_from(radius).unwrap_or(i32::MAX);
        let spawn = self.get_spawn_location().0;
        let dx = (spawn.0.x - position.0.x).abs();
        let dz = (spawn.0.z - position.0.z).abs();

        dx <= radius && dz <= radius
    }

    pub fn set_difficulty(&self, difficulty: Difficulty) {
        let current_info = self.level_info.load();
        let mut new_info = (**current_info).clone();
        new_info.difficulty = difficulty;
        self.level_info.store(Arc::new(new_info));
    }

    pub fn get_game_rule(&self, rule: &GameRule) -> GameRuleValue<i64, bool> {
        let level_info = self.level_info.load();
        match level_info.game_rules.get(rule) {
            GameRuleValue::Int(v) => GameRuleValue::Int(*v),
            GameRuleValue::Bool(v) => GameRuleValue::Bool(*v),
        }
    }

    pub fn set_game_rule(&self, rule: &GameRule, value: GameRuleValue<i64, bool>) {
        let current_info = self.level_info.load();
        let mut new_info = (**current_info).clone();
        match (new_info.game_rules.get_mut(rule), value) {
            (GameRuleValue::Int(target), GameRuleValue::Int(val)) => {
                *target = val;
            }
            (GameRuleValue::Bool(target), GameRuleValue::Bool(val)) => {
                *target = val;
            }
            _ => {}
        }
        self.level_info.store(Arc::new(new_info));
        if *rule == GameRule::AdvanceTime {
            let level_time = self
                .level_time
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            level_time.send_time(self);
        }
    }
}
