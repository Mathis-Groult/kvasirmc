//! The steps of `Server::add_player`: which world and saved data a joining player gets, the
//! skin and advancement loading, and the registration once the player is admitted.

use std::sync::Arc;

use pumpkin_data::dimension::Dimension;
use pumpkin_nbt::compound::NbtCompound;
use tracing::warn;

use super::Server;
use crate::entity::player::Player;
use crate::net::PlayerConfig;
use crate::world::World;

impl Server {
    /// The world the player joins (the one in its saved data, else the first) and its saved data.
    pub(super) fn saved_player(
        &self,
        id: &uuid::Uuid,
        first_world: Arc<World>,
    ) -> (Arc<World>, Option<NbtCompound>) {
        let Ok(Some(data)) = self.player_data_storage.load_data(id) else {
            // No player data found or an error occurred, default to the Overworld.
            return (first_world, None);
        };
        let Some(dimension_key) = data.get_string("Dimension") else {
            // Player data exists but doesn't have a "Dimension" key.
            return (first_world, Some(data));
        };
        let Some(dimension) = Dimension::from_name(dimension_key) else {
            warn!("Invalid dimension key in player data: {dimension_key}");
            return (first_world, Some(data));
        };
        let world = self.get_world_from_dimension(dimension);
        (world, Some(data))
    }

    /// Fetches the player's skin in the background and keeps it for Bedrock clients.
    pub(super) fn spawn_skin_fetch(&self, player: &Arc<Player>) {
        let player = player.clone();
        self.spawn_task(async move {
            let properties = player.gameprofile.properties.load().clone();
            if let Some(skin) = Player::fetch_skin(&properties).await {
                player.bedrock_skin.store(Arc::new(skin));
            }
        });
    }

    /// Loads the player's advancements and hands them the player.
    pub(super) fn load_advancements(player: &Arc<Player>) {
        let mut advancements = player
            .advancements
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Err(e) = advancements.load() {
            warn!("Error loading player {}: {e}", player.gameprofile.id);
        }
        advancements.player = Arc::downgrade(player);
    }

    /// Records an admitted player: user cache, status list, management notification.
    pub(super) fn register_player(&self, player: &Player, config: Option<PlayerConfig>) {
        self.data
            .user_cache
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .upsert(player.gameprofile.id, player.gameprofile.name.clone());

        // TODO: Config if we want increase online
        // TODO: Config so we can also just ignore this hehe
        if config.is_some_and(|config| config.server_listing) {
            self.listing
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .add_player(player);
        }

        let player_dto = crate::net::management::dto::PlayerDto::new(
            Some(player.gameprofile.id),
            Some(player.gameprofile.name.clone()),
        );
        self.management_hub.broadcast_player_joined(&player_dto);
    }
}
