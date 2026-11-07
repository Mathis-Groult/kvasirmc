//! RPC methods of the ip bans group.

use super::super::{
    dto::{IncomingIpBanDto, IpBanDto},
    rpc::RpcError,
};
use super::{check_no_params, get_ip_bans, parse_param};
use crate::{
    data::{SaveJSONConfiguration, banlist_serializer::BannedIpEntry},
    net::DisconnectReason,
    server::Server,
};
use pumpkin_data::translation;
use serde_json::{Value, json};
use std::{net::IpAddr, sync::Arc};
use time::OffsetDateTime;

/// Handles the arm `"minecraft:ip_bans"`.
pub(super) fn list(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(serde_json::to_value(get_ip_bans(server)).unwrap_or(Value::Null))
}

/// Handles the arm `"minecraft:ip_bans/add"`.
pub(super) fn add(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    let add_bans: Vec<IncomingIpBanDto> = parse_param(params, "add")?;
    {
        let mut ip_ban_guard = server
            .data
            .banned_ip_list
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for ban in &add_bans {
            let target_ip: Option<IpAddr> = ban.ip.as_ref().map_or_else(
                || {
                    ban.player.as_ref().and_then(|player_dto| {
                        server
                            .get_all_players()
                            .into_iter()
                            .find(|p| {
                                player_dto.id.is_some_and(|id| id == p.gameprofile.id)
                                    || player_dto
                                        .name
                                        .as_ref()
                                        .is_some_and(|name| name == &p.gameprofile.name)
                            })
                            .map(|p| p.client.address().ip())
                    })
                },
                |ip_str| ip_str.parse().ok(),
            );

            if let Some(ip) = target_ip {
                let expires = ban.expires.as_deref().and_then(|exp| {
                    OffsetDateTime::parse(exp, &time::format_description::well_known::Rfc3339).ok()
                });
                let entry = BannedIpEntry::new(
                    ip,
                    ban.source
                        .clone()
                        .unwrap_or_else(|| "Management server".to_string()),
                    expires,
                    ban.reason.clone().unwrap_or_default(),
                );
                ip_ban_guard.banned_ips.retain(|b| b.ip != ip);
                ip_ban_guard.banned_ips.push(entry);

                let dto = IpBanDto {
                    ip: Some(ip.to_string()),
                    reason: ban.reason.clone(),
                    source: ban.source.clone(),
                    expires: ban.expires.clone(),
                };
                server.management_hub.broadcast_ip_banned(&dto);

                for player in server.get_all_players() {
                    if player.client.address().ip() == ip {
                        player.kick(
                            DisconnectReason::Kicked,
                            &pumpkin_macros::translate_cross!(
                                translation::java::MULTIPLAYER_DISCONNECT_BANNED_IP_REASON,
                                translation::bedrock::DISCONNECT_KICKED
                            ),
                        );
                    }
                }
            }
        }
        ip_ban_guard.save();
    };
    Ok(serde_json::to_value(get_ip_bans(server)).unwrap_or(Value::Null))
}

/// Handles the arm `"minecraft:ip_bans/remove"`.
pub(super) fn remove(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    let ips: Vec<String> = parse_param(params, "ip")?;
    {
        let mut ip_ban_guard = server
            .data
            .banned_ip_list
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for ip_str in &ips {
            if let Ok(ip) = ip_str.parse::<IpAddr>() {
                ip_ban_guard.banned_ips.retain(|b| b.ip != ip);
                server.management_hub.broadcast_ip_unbanned(ip_str);
            }
        }
        ip_ban_guard.save();
    };
    Ok(serde_json::to_value(get_ip_bans(server)).unwrap_or(Value::Null))
}

/// Handles the arm `"minecraft:ip_bans/clear"`.
pub(super) fn clear(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    check_no_params(params)?;
    {
        let mut ip_ban_guard = server
            .data
            .banned_ip_list
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for b in ip_ban_guard.banned_ips.drain(..) {
            server
                .management_hub
                .broadcast_ip_unbanned(&b.ip.to_string());
        }
        ip_ban_guard.save();
    };
    Ok(json!([]))
}

/// Handles the arm `"minecraft:ip_bans/set"`.
pub(super) fn set(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    let new_bans: Vec<IpBanDto> = parse_param(params, "banlist")?;
    {
        let mut ip_ban_guard = server
            .data
            .banned_ip_list
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        for old in &ip_ban_guard.banned_ips {
            let old_str = old.ip.to_string();
            if !new_bans
                .iter()
                .any(|n| n.ip.as_ref().is_some_and(|ip| ip == &old_str))
            {
                server.management_hub.broadcast_ip_unbanned(&old_str);
            }
        }

        let mut final_bans = Vec::new();
        for ban in &new_bans {
            if let Some(ref ip_str) = ban.ip
                && let Ok(ip) = ip_str.parse::<IpAddr>()
            {
                let expires = ban.expires.as_deref().and_then(|exp| {
                    OffsetDateTime::parse(exp, &time::format_description::well_known::Rfc3339).ok()
                });
                final_bans.push(BannedIpEntry::new(
                    ip,
                    ban.source
                        .clone()
                        .unwrap_or_else(|| "Management server".to_string()),
                    expires,
                    ban.reason.clone().unwrap_or_default(),
                ));
                if !ip_ban_guard.banned_ips.iter().any(|b| b.ip == ip) {
                    server.management_hub.broadcast_ip_banned(ban);
                    for player in server.get_all_players() {
                        if player.client.address().ip() == ip {
                            player.kick(
                                DisconnectReason::Kicked,
                                &pumpkin_macros::translate_cross!(
                                    translation::java::MULTIPLAYER_DISCONNECT_BANNED_IP_REASON,
                                    translation::bedrock::DISCONNECT_KICKED
                                ),
                            );
                        }
                    }
                }
            }
        }

        ip_ban_guard.banned_ips = final_bans;
        ip_ban_guard.save();
    };
    Ok(serde_json::to_value(get_ip_bans(server)).unwrap_or(Value::Null))
}
