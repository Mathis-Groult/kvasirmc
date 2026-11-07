//! RPC methods of the bans group.

use super::super::{
    dto::{PlayerDto, UserBanDto},
    rpc::RpcError,
};
use super::{check_no_params, get_bans, parse_param};
use crate::{
    data::{SaveJSONConfiguration, banlist_serializer::BannedPlayerEntry},
    net::{DisconnectReason, GameProfile},
    server::Server,
};
use pumpkin_data::translation;
use serde_json::{Value, json};
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

/// Handles the arm `"minecraft:bans"`.
pub(super) fn list(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(serde_json::to_value(get_bans(server)).unwrap_or(Value::Null))
}

/// Handles the arm `"minecraft:bans/add"`.
pub(super) fn add(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    let add_bans: Vec<UserBanDto> = parse_param(params, "add")?;
    {
        let mut ban_guard = server
            .data
            .banned_player_list
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for ban in &add_bans {
            let uuid = ban.player.id.unwrap_or_else(Uuid::new_v4);
            let name = ban.player.name.clone().unwrap_or_default();
            let profile = GameProfile {
                id: uuid,
                name: name.clone(),
                properties: arc_swap::ArcSwap::from_pointee(Vec::new()),
                profile_actions: None,
            };
            let expires = ban.expires.as_deref().and_then(|exp| {
                OffsetDateTime::parse(exp, &time::format_description::well_known::Rfc3339).ok()
            });
            let entry = BannedPlayerEntry::new(
                &profile,
                ban.source
                    .clone()
                    .unwrap_or_else(|| "Management server".to_string()),
                expires,
                ban.reason.clone().unwrap_or_default(),
            );
            ban_guard.banned_players.retain(|b| b.uuid != uuid);
            ban_guard.banned_players.push(entry);
            server.management_hub.broadcast_player_banned(ban);

            for player in server.get_all_players() {
                if player.gameprofile.id == uuid {
                    player.kick(
                        DisconnectReason::Kicked,
                        &pumpkin_macros::translate_cross!(
                            translation::java::MULTIPLAYER_DISCONNECT_BANNED,
                            translation::bedrock::DISCONNECT_KICKED
                        ),
                    );
                }
            }
        }
        ban_guard.save();
    };
    Ok(serde_json::to_value(get_bans(server)).unwrap_or(Value::Null))
}

/// Handles the arm `"minecraft:bans/remove"`.
pub(super) fn remove(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    let to_remove: Vec<PlayerDto> = parse_param(params, "remove")?;
    {
        let mut ban_guard = server
            .data
            .banned_player_list
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for p in &to_remove {
            if let Some(pos) = ban_guard.banned_players.iter().position(|b| {
                p.id.is_some_and(|id| b.uuid == id)
                    || p.name.as_ref().is_some_and(|name| &b.name == name)
            }) {
                ban_guard.banned_players.remove(pos);
                server.management_hub.broadcast_player_unbanned(p);
            }
        }
        ban_guard.save();
    };
    Ok(serde_json::to_value(get_bans(server)).unwrap_or(Value::Null))
}

/// Handles the arm `"minecraft:bans/clear"`.
pub(super) fn clear(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    check_no_params(params)?;
    {
        let mut ban_guard = server
            .data
            .banned_player_list
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for b in ban_guard.banned_players.drain(..) {
            let dto = PlayerDto::new(Some(b.uuid), Some(b.name));
            server.management_hub.broadcast_player_unbanned(&dto);
        }
        ban_guard.save();
    };
    Ok(json!([]))
}

/// Handles the arm `"minecraft:bans/set"`.
pub(super) fn set(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    let new_bans: Vec<UserBanDto> = parse_param(params, "bans")?;
    {
        let mut ban_guard = server
            .data
            .banned_player_list
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        for old in &ban_guard.banned_players {
            if !new_bans
                .iter()
                .any(|n| n.player.id.is_some_and(|id| id == old.uuid))
            {
                let dto = PlayerDto::new(Some(old.uuid), Some(old.name.clone()));
                server.management_hub.broadcast_player_unbanned(&dto);
            }
        }

        let mut final_bans = Vec::new();
        for ban in &new_bans {
            let uuid = ban.player.id.unwrap_or_else(Uuid::new_v4);
            let name = ban.player.name.clone().unwrap_or_default();
            let profile = GameProfile {
                id: uuid,
                name: name.clone(),
                properties: arc_swap::ArcSwap::from_pointee(Vec::new()),
                profile_actions: None,
            };
            let expires = ban.expires.as_deref().and_then(|exp| {
                OffsetDateTime::parse(exp, &time::format_description::well_known::Rfc3339).ok()
            });
            final_bans.push(BannedPlayerEntry::new(
                &profile,
                ban.source
                    .clone()
                    .unwrap_or_else(|| "Management server".to_string()),
                expires,
                ban.reason.clone().unwrap_or_default(),
            ));
            if !ban_guard.banned_players.iter().any(|b| b.uuid == uuid) {
                server.management_hub.broadcast_player_banned(ban);
                for player in server.get_all_players() {
                    if player.gameprofile.id == uuid {
                        player.kick(
                            DisconnectReason::Kicked,
                            &pumpkin_macros::translate_cross!(
                                translation::java::MULTIPLAYER_DISCONNECT_BANNED,
                                translation::bedrock::DISCONNECT_KICKED
                            ),
                        );
                    }
                }
            }
        }

        ban_guard.banned_players = final_bans;
        ban_guard.save();
    };
    Ok(serde_json::to_value(get_bans(server)).unwrap_or(Value::Null))
}
