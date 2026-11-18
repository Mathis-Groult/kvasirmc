//! Players arriving in, respawning in and leaving the world.

use super::World;
use crate::{
    entity::{EntityBase, player::Player},
    plugin::player::player_leave::PlayerLeaveEvent,
};
use pumpkin_data::dimension::Dimension;
use pumpkin_data::translation;
use pumpkin_protocol::{
    bedrock::client::{
        common::BuildPlatform,
        player_list::{CPlayerList, PlayerListEntry, Skin},
        remove_actor::CRemoveActor,
    },
    codec::{var_int::VarInt, var_long::VarLong},
    java::client::play::{CPlayerInfoUpdate, CRemoveEntities, CRemovePlayerInfo, PlayerAction},
};
use pumpkin_util::text::{TextComponent, color::NamedColor};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tracing::info;

impl World {
    pub async fn respawn_player(self: &Arc<Self>, player: &Arc<Player>, alive: bool) {
        let (death_dimension, death_location) = respawn::death_location_of(player);

        let data_kept = u8::from(alive);

        let server = self.server.upgrade();
        let default_world = server.as_ref().map_or_else(
            || self.clone(),
            |s| s.get_world_from_dimension(&Dimension::OVERWORLD),
        );

        // Copy spawn info from default world level_info to avoid holding lock across await
        let (spawn_x, spawn_y, spawn_z, spawn_yaw, spawn_pitch, keep_inventory) = {
            let info = default_world.level_info.load();
            (
                info.spawn_x,
                info.spawn_y,
                info.spawn_z,
                info.spawn_yaw,
                info.spawn_pitch,
                info.game_rules.keep_inventory,
            )
        };

        // Get respawn position and dimension
        let (position, yaw, pitch, respawn_dimension) =
            if let Some(respawn) = player.calculate_respawn_point().await {
                (
                    respawn.position,
                    respawn.yaw,
                    respawn.pitch,
                    respawn.dimension,
                )
            } else {
                // No valid respawn point - send notification if player had one set
                respawn::forget_unusable_respawn_point(player).await;
                respawn::level_spawn_position(
                    &default_world,
                    spawn_x,
                    spawn_y,
                    spawn_z,
                    spawn_yaw,
                    spawn_pitch,
                )
                .await
            };

        let position = respawn::fire_spawn_location_event(server.as_ref(), player, position).await;

        let (resolved_world, position, yaw, pitch) = self
            .resolve_respawn_world(
                server.as_ref(),
                player,
                &respawn_dimension,
                (position, yaw, pitch),
            )
            .await;

        // Cancelled or unresolved cross-dimension respawns fall back to the current
        // world's spawn below; otherwise the resolved values from the event apply.
        let (target_world, position, yaw, pitch) = resolved_world.as_ref().map_or_else(
            || (self.clone(), position, yaw, pitch),
            |new_world| (new_world.clone(), position, yaw, pitch),
        );

        self.fire_respawn_event(player, &target_world, (position, yaw, pitch), alive)
            .await;

        Self::send_respawn_packets(
            player,
            &target_world,
            (death_dimension, death_location),
            data_kept,
            (position, yaw, pitch),
        )
        .await;

        respawn::reset_respawned_player(player, keep_inventory, (position, yaw, pitch));

        self.place_respawned_player(player, &target_world, (position, yaw, pitch))
            .await;
    }

    /// Adds a player to the world and broadcasts a join message if enabled.
    ///
    /// This function takes a player's UUID and an `Arc<Player>` reference.
    /// It inserts the player into the world's `current_players` map using the UUID as the key.
    /// Additionally, it broadcasts a join message to all connected players in the world.
    ///
    /// # Arguments
    ///
    /// * `player`: An `Arc<Player>` reference to the player object.
    pub fn add_player(&self, player: &Arc<Player>) -> Result<(), String> {
        self.players.rcu(|current_list| {
            let mut new_list = (**current_list).clone();
            new_list.push(player.clone());
            new_list
        });
        self.entity_tracker
            .add_entity(&(player.clone() as Arc<dyn EntityBase>), self);
        Ok(())
    }

    /// Tab-list add entry for `player` from its stored state, as Java and Bedrock packets.
    pub(super) fn with_player_list_entry<R>(
        player: &Player,
        f: impl FnOnce(&CPlayerInfoUpdate<'_>, &CPlayerList) -> R,
    ) -> R {
        let profile = &player.gameprofile;
        let properties = profile.properties.load();
        let actions = [
            PlayerAction::AddPlayer {
                name: &profile.name,
                properties: &properties,
            },
            PlayerAction::UpdateGameMode(VarInt(player.gamemode.load() as i32)),
            PlayerAction::UpdateListed(player.tab_list_listed.load(Ordering::Relaxed)),
            PlayerAction::UpdateLatency(VarInt(player.tab_list_latency.load(Ordering::Relaxed))),
            PlayerAction::UpdateListOrder(VarInt(player.tab_list_order.load(Ordering::Relaxed))),
            PlayerAction::UpdateHat(true),
        ];
        let entry = [pumpkin_protocol::java::client::play::Player {
            uuid: profile.id,
            actions: &actions,
        }];
        f(
            &CPlayerInfoUpdate::new(Self::TAB_LIST_ADD_FLAGS, &entry),
            &player.bedrock_player_list(),
        )
    }

    /// Publishes a player from another world -> tab list first, Java drops spawns without it.
    pub fn add_arriving_player(&self, player: &Arc<Player>) {
        let id = player.gameprofile.id;
        Self::with_player_list_entry(player, |java, bedrock| {
            self.broadcast_packet_except_editioned(&[id], java, bedrock);
        });
        for existing in self
            .players
            .load()
            .iter()
            .filter(|p| p.gameprofile.id != id)
        {
            Self::with_player_list_entry(existing, |java, bedrock| {
                player.client.try_enqueue_packet_editioned(java, bedrock);
            });
        }

        self.entity_tracker
            .add_entity(&(player.clone() as Arc<dyn EntityBase>), self);
    }

    /// Removes a player from the world and broadcasts a disconnect message if enabled.
    ///
    /// This function removes a player from the world based on their `Player` reference.
    /// It performs the following actions:
    ///
    /// 1. Removes the player from the `current_players` map using their UUID.
    /// 2. Broadcasts a `CRemovePlayerInfo` packet to all connected players to inform them about the player leaving.
    /// 3. Removes the player's entity from the world using its entity ID.
    /// 4. Optionally sends a disconnect message to all other players notifying them about the player leaving.
    ///
    /// # Arguments
    ///
    /// * `player`: A reference to the `Player` object to be removed.
    /// * `fire_event`: A boolean flag indicating whether to fire a `PlayerLeaveEvent` event.
    ///
    /// # Notes
    ///
    /// - This function assumes `broadcast_packet_expect` and `remove_entity` are defined elsewhere.
    /// - The disconnect message sending is currently optional. Consider making it a configurable option.
    pub async fn remove_player(
        &self,
        player: &Arc<Player>,
        fire_event: bool,
    ) -> Option<Arc<Player>> {
        let mut removed_player: Option<Arc<Player>> = None;

        self.players.rcu(|current_list| {
            let mut new_list = (**current_list).clone();
            // Find the player before we filter them out
            let pos = new_list
                .iter()
                .position(|p| p.gameprofile.id == player.gameprofile.id);
            if let Some(pos) = pos {
                removed_player = Some(new_list.remove(pos));
            }
            new_list
        });
        if let Some(ref player) = removed_player {
            self.entity_tracker
                .remove_entity(player.as_ref() as &dyn EntityBase, self);
            let uuid = player.gameprofile.id;
            let entity_id = player.entity_id();

            let bedrock_remove_player = CPlayerList {
                action: CPlayerList::ACTION_REMOVE,
                entries: vec![PlayerListEntry {
                    uuid,
                    entity_unique_id: VarLong(entity_id as i64),
                    username: player.gameprofile.name.clone(),
                    xuid: String::new(),
                    platform_chat_id: String::new(),
                    build_platform: BuildPlatform::Unknown,
                    skin: Skin::steve(),
                    is_teacher: false,
                    is_host: false,
                    is_sub_client: false,
                    player_color: [0, 0, 0, 0],
                }],
            };

            self.broadcast_editioned(&CRemovePlayerInfo::new(&[uuid]), &bedrock_remove_player);

            self.broadcast_editioned(
                &CRemoveEntities::new(&[entity_id.into()]),
                &CRemoveActor::new(VarLong(entity_id as i64)),
            );

            if fire_event {
                let msg_comp = TextComponent::translate_cross(
                    translation::java::MULTIPLAYER_PLAYER_LEFT,
                    translation::bedrock::MULTIPLAYER_PLAYER_LEFT,
                    [TextComponent::text(player.gameprofile.name.clone())],
                )
                .color_named(NamedColor::Yellow);
                let mut event = PlayerLeaveEvent::new(player.clone(), msg_comp);

                if let Some(server) = self.server.upgrade() {
                    server.plugin_manager.fire(&server, &mut event).await;

                    if !event.cancelled {
                        for player in self.players.load().iter() {
                            player.send_system_message(&event.leave_message);
                        }
                        info!("{}", event.leave_message.to_pretty_console());
                    }
                }
            }
        }
        removed_player
    }
}

mod respawn;
#[cfg(test)]
mod respawn_golden;
