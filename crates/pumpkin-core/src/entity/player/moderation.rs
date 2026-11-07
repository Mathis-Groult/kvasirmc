//! Kicks, bans and activity checks.

use super::{Player, SpamType};
use crate::data::SaveJSONConfiguration;
use crate::net::DisconnectReason;
use crate::server::Server;
use pumpkin_data::translation;
use pumpkin_util::permission::PermissionLvl;
use pumpkin_util::text::TextComponent;
use std::sync::atomic::Ordering;
use tracing::warn;

impl Player {
    pub fn kick(&self, reason: DisconnectReason, message: &TextComponent) {
        if let Some(server) = self.world().server.upgrade()
            && let Some(player_arc) = self.world().get_player_by_uuid(self.gameprofile.id)
        {
            let mut event = crate::plugin::api::events::player::player_kick::PlayerKickEvent::new(
                player_arc,
                message.clone().to_pretty_console(),
            );
            server.plugin_manager.fire_blocking(&server, &mut event);
            if event.cancelled {
                return;
            }
        }
        self.client.try_kick(reason, message);
    }

    /// Updates the last action time to now. Call this on player actions like movement, chat, etc.
    pub fn update_last_action_time(&self) {
        self.last_action_time.store(std::time::Instant::now());
    }

    /// Checks whether sending a chat message or command constitutes spam.
    pub fn check_chat_spam(&self, server: &Server, spam_type: SpamType) -> bool {
        let anti_spam = &server.advanced_config.chat.anti_spam;
        if !anti_spam.enabled {
            return false;
        }

        if anti_spam.ops_bypass && self.permission_lvl.load() > PermissionLvl::Zero {
            return false;
        }

        let threshold = match spam_type {
            SpamType::Chat => anti_spam.chat_threshold_ticks(),
            SpamType::Command => anti_spam.command_threshold_ticks(),
        };

        let new_count = self
            .chat_spam_tick_count
            .fetch_add(anti_spam.message_cost, Ordering::SeqCst)
            + anti_spam.message_cost;

        if new_count > threshold {
            warn!(
                "Player {} kicked for spamming (spam score: {}/{})",
                self.gameprofile.name, new_count, threshold
            );
            self.kick(
                DisconnectReason::Kicked,
                &TextComponent::translate_cross(
                    translation::java::DISCONNECT_SPAM,
                    translation::bedrock::DISCONNECT_SPAM,
                    [],
                ),
            );
            return true;
        }

        false
    }

    pub fn get_ip(&self) -> String {
        self.client.address().to_string()
    }

    pub fn ban(&self, server: &Server, reason: Option<TextComponent>) {
        self.ban_explicit(server, reason, None, None, true, true);
    }

    pub fn ban_explicit(
        &self,
        server: &Server,
        reason: Option<TextComponent>,
        source: Option<String>,
        expires: Option<time::OffsetDateTime>,
        kick_if_online: bool,
        log_to_console: bool,
    ) {
        let string_reason = reason.clone().map_or_else(
            || "Banned by an operator.".to_string(),
            pumpkin_util::text::TextComponent::get_text,
        );
        let source_str = source.unwrap_or_else(|| "Plugin".to_string());

        if log_to_console {
            tracing::info!(
                "Banning player {} ({}) by {}: {}",
                self.gameprofile.name,
                self.gameprofile.id,
                source_str,
                string_reason
            );
        }

        {
            let mut banned_players = server
                .data
                .banned_player_list
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            banned_players
                .banned_players
                .retain(|entry| entry.uuid != self.gameprofile.id);

            banned_players.banned_players.push(
                crate::data::banlist_serializer::BannedPlayerEntry::new(
                    &self.gameprofile,
                    source_str,
                    expires,
                    string_reason,
                ),
            );

            banned_players.save();
        };

        if kick_if_online {
            let kick_reason = reason.unwrap_or_else(|| {
                TextComponent::translate_cross(
                    translation::java::MULTIPLAYER_DISCONNECT_BANNED,
                    translation::bedrock::DISCONNECTIONSCREEN_TITLE_BANNEDBYHOST,
                    [],
                )
            });

            self.kick(DisconnectReason::Kicked, &kick_reason);
        }
    }

    pub fn ban_ip(&self, server: &Server, reason: Option<TextComponent>) {
        self.ban_ip_explicit(server, reason, None, None, true, true);
    }

    pub fn ban_ip_explicit(
        &self,
        server: &Server,
        reason: Option<TextComponent>,
        source: Option<String>,
        expires: Option<time::OffsetDateTime>,
        kick_matching_players: bool,
        log_to_console: bool,
    ) {
        let string_reason = reason.clone().map_or_else(
            || "Banned by an operator.".to_string(),
            pumpkin_util::text::TextComponent::get_text,
        );
        let source_str = source.unwrap_or_else(|| "Plugin".to_string());
        let target_ip = self.client.address().ip();

        if log_to_console {
            tracing::info!(
                "Banning IP {} by {}: {}",
                target_ip,
                source_str,
                string_reason
            );
        }

        {
            let mut banned_ips = server
                .data
                .banned_ip_list
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            banned_ips.banned_ips.retain(|entry| entry.ip != target_ip);

            banned_ips
                .banned_ips
                .push(crate::data::banlist_serializer::BannedIpEntry::new(
                    target_ip,
                    source_str,
                    expires,
                    string_reason,
                ));

            banned_ips.save();
        };

        if kick_matching_players {
            let kick_reason = reason.unwrap_or_else(|| {
                TextComponent::translate_cross(
                    translation::java::MULTIPLAYER_DISCONNECT_IP_BANNED,
                    translation::java::MULTIPLAYER_DISCONNECT_IP_BANNED,
                    [],
                )
            });

            let affected = server.get_players_by_ip(target_ip);
            for target in affected {
                target.kick(DisconnectReason::Kicked, &kick_reason);
            }
        }
    }
}
