//! Scoreboard and team display.

use super::{CustomScoreboard, Player};
use crate::net::ClientPlatform;

impl Player {
    pub fn reset_scoreboard(&self) {
        *self
            .custom_scoreboard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        self.send_scoreboard();
    }

    pub fn send_scoreboard(&self) {
        let guard = self
            .custom_scoreboard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match guard.as_ref() {
            Some(CustomScoreboard::Java(custom))
                if matches!(self.client.as_ref(), ClientPlatform::Java(_)) =>
            {
                custom.send_to_player(self);
            }
            Some(CustomScoreboard::Bedrock(custom))
                if matches!(self.client.as_ref(), ClientPlatform::Bedrock(_)) =>
            {
                custom.send_to_player(self);
            }
            _ => {
                drop(guard);
                self.world()
                    .scoreboard
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .send_to_player(self);
            }
        }
    }

    pub fn get_team(&self) -> Option<crate::world::scoreboard::Team> {
        let guard = self
            .custom_scoreboard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(CustomScoreboard::Java(sb)) = guard.as_ref()
            && let Some(team) = sb.get_entity_team(&self.gameprofile.name)
        {
            return Some(team.clone());
        }
        let world = self.world();
        let sb = world
            .scoreboard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sb.get_entity_team(&self.gameprofile.name).cloned()
    }

    pub fn get_team_name(&self) -> Option<String> {
        let guard = self
            .custom_scoreboard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(CustomScoreboard::Java(sb)) = guard.as_ref()
            && let Some(team) = sb.get_entity_team(&self.gameprofile.name)
        {
            return Some(team.name.clone());
        }
        let world = self.world();
        let sb = world
            .scoreboard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sb.get_entity_team(&self.gameprofile.name)
            .map(|team| team.name.clone())
    }
}
