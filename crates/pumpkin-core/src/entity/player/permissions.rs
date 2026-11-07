//! Game mode and permission levels.

use super::super::EntityBase;
use super::Player;
use crate::command::context::command_source::CommandSource;
use crate::command::node::dispatcher::CommandDispatcher;
use crate::command::{CommandSender, client_suggestions};
use crate::net::ClientPlatform;
use crate::plugin::player::player_gamemode_change::PlayerGamemodeChangeEvent;
use crate::plugin::player::player_permission_check::PlayerPermissionCheckEvent;
use crate::server::Server;
use core::f32;
use pumpkin_data::entity::EntityStatus;
use pumpkin_protocol::java::client::play::{
    CChangeDifficulty, CGameEvent, CPlayerInfoUpdate, CSetCamera, GameEvent, PlayerAction,
    PlayerInfoFlags,
};
use pumpkin_util::GameMode;
use pumpkin_util::permission::PermissionLvl;
use std::sync::Arc;
use std::sync::atomic::Ordering;

impl Player {
    #[must_use]
    pub fn is_spectator(&self) -> bool {
        self.gamemode.load() == GameMode::Spectator
    }

    /// Updates the client of the player's current permission level.
    pub fn send_permission_lvl_update(&self) {
        let status = match self.permission_lvl.load() {
            PermissionLvl::Zero => EntityStatus::PermissionLevelAll,
            PermissionLvl::One => EntityStatus::PermissionLevelModerators,
            PermissionLvl::Two => EntityStatus::PermissionLevelGamemasters,
            PermissionLvl::Three => EntityStatus::PermissionLevelAdmins,
            PermissionLvl::Four => EntityStatus::PermissionLevelOwners,
        };
        // The player may not have a tracking entry yet after changing dimensions.
        // Permission levels belong to this connection, not to tracking clients.
        if let ClientPlatform::Java(java) = self.client.as_ref() {
            let packet = pumpkin_protocol::java::client::play::CEntityStatus::new(
                self.living_entity.entity.entity_id,
                status as i8,
            );
            if let Ok(data) = java.serialize_packet(&packet) {
                java.try_enqueue_packet(data);
            }
        }
    }

    /// Sets the player's difficulty level.
    pub fn send_difficulty_update(&self) {
        let world = self.world();
        let level_info = world.level_info.load();
        self.client.try_enqueue_packet_editioned(
            &CChangeDifficulty::new(level_info.difficulty as u8, level_info.difficulty_locked),
            &pumpkin_protocol::bedrock::client::CSetDifficulty {
                difficulty: (level_info.difficulty as u32).into(),
            },
        );
    }

    /// Sets the player's permission level and notifies the client.
    pub fn set_permission_lvl(
        self: &Arc<Self>,
        server: &Arc<Server>,
        lvl: PermissionLvl,
        command_dispatcher: &CommandDispatcher,
    ) {
        self.permission_lvl.store(lvl);
        self.send_permission_lvl_update();

        if let ClientPlatform::Bedrock(_) = self.client.as_ref() {
            client_suggestions::send_bedrock_commands_packet(self, server, command_dispatcher);
        } else {
            client_suggestions::send_c_commands_packet(self, server, command_dispatcher);
        }
    }

    pub fn can_use_game_master_blocks(&self) -> bool {
        self.gamemode.load() == GameMode::Creative
            && self.permission_lvl.load() >= PermissionLvl::Two
    }

    #[must_use]
    pub fn may_build(&self) -> bool {
        self.abilities.lock().is_ok_and(|a| a.allow_modify_world)
    }

    pub fn set_gamemode(self: &Arc<Self>, gamemode: GameMode) -> bool {
        // We could send the same gamemode without any problems. But why waste bandwidth?
        // assert_ne!(
        //    self.gamemode.load(),
        //    gamemode,
        //    "Attempt to set the gamemode to the already current gamemode"
        // );
        // Why are we panicking if the gamemodes are the same? Vanilla just exits early.
        if self.gamemode.load() == gamemode {
            return false;
        }
        let Some(server) = self.world().server.upgrade() else {
            return false;
        };

        let mut event = PlayerGamemodeChangeEvent {
            player: self.clone(),
            new_gamemode: gamemode,
            previous_gamemode: self.gamemode.load(),
            cancelled: false,
        };
        server.plugin_manager.fire_blocking(&server, &mut event);
        if event.cancelled {
            return false;
        }

        let gamemode = event.new_gamemode;
        self.gamemode.store(gamemode);
        // TODO: Fix this when mojang fixes it
        // This is intentional to keep the pure vanilla mojang experience
        // self.previous_gamemode.store(self.previous_gamemode.load());
        {
            // Use another scope so that we instantly unlock `abilities`.
            let mut abilities = self
                .abilities
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            abilities.set_for_gamemode(gamemode);
        };
        self.send_abilities_update();

        if gamemode == GameMode::Creative {
            self.get_entity().extinguish();
            self.get_entity().set_on_fire(false);
        }

        // Stop elytra flight and reset sneaking when switching to spectator mode
        if gamemode == GameMode::Spectator {
            let entity = self.get_entity();
            if entity.is_fall_flying() {
                entity.set_fall_flying(false);
            }
            if entity.is_sneaking() {
                entity.set_sneaking(false);
            }
            entity.on_ground.store(false, Ordering::Relaxed);
            self.living_entity.fall_distance.store(0.0);
        }

        if gamemode != GameMode::Spectator && self.camera_target_id.load().is_some() {
            self.camera_target_id.store(None);
            self.try_send_client_packet(&CSetCamera::new(self.entity_id().into()));
        }

        self.living_entity.entity.invulnerable.store(
            matches!(gamemode, GameMode::Creative | GameMode::Spectator),
            Ordering::Relaxed,
        );
        self.living_entity
            .entity
            .no_physics
            .store(gamemode == GameMode::Spectator, Ordering::Relaxed);
        self.living_entity
            .entity
            .world
            .load()
            .broadcast_packet_all(&CPlayerInfoUpdate::new(
                PlayerInfoFlags::UPDATE_GAME_MODE.bits(),
                &[pumpkin_protocol::java::client::play::Player {
                    uuid: self.gameprofile.id,
                    actions: &[PlayerAction::UpdateGameMode((gamemode as i32).into())],
                }],
            ));

        self.client.try_enqueue_packet_editioned(
            &CGameEvent::new(GameEvent::ChangeGameMode, gamemode as i32 as f32),
            &pumpkin_protocol::bedrock::client::set_player_gamemode::CSetPlayerGameType {
                player_game_type: gamemode.into(),
            },
        );

        true
    }

    pub fn has_permission(self: &Arc<Self>, server: &Server, node: &str) -> bool {
        self.has_permission_at_level(server, node, self.permission_lvl.load())
    }

    /// Like [`Self::has_permission`], but node defaults compare against `level`
    /// instead of the player's own permission level.
    pub fn has_permission_at_level(
        self: &Arc<Self>,
        server: &Server,
        node: &str,
        level: PermissionLvl,
    ) -> bool {
        let result = server
            .permission_manager
            .has_permission(&self.gameprofile.id, node, level);

        let mut event = PlayerPermissionCheckEvent::new(self.clone(), node.to_string(), result);
        let server_arc = self.world().server.upgrade();
        if let Some(server_arc) = server_arc {
            server_arc
                .plugin_manager
                .fire_blocking(&server_arc, &mut event);
        }
        event.result
    }

    pub fn is_creative(&self) -> bool {
        self.gamemode.load() == GameMode::Creative
    }

    pub fn get_command_source(self: &Arc<Self>, server: &Arc<Server>) -> CommandSource {
        CommandSender::Player(self.clone()).into_source(server)
    }
}
