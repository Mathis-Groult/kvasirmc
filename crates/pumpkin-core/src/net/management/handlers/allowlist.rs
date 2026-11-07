//! RPC methods of the allowlist group.

use super::super::{dto::PlayerDto, rpc::RpcError};
use super::{check_no_params, get_allowlist, parse_param};
use crate::{
    command::commands::kick_non_whitelisted_players, data::SaveJSONConfiguration, server::Server,
};
use pumpkin_config::whitelist::WhitelistEntry;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

/// Handles the arm `"minecraft:allowlist"`.
pub(super) fn list(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    check_no_params(params)?;
    let list = get_allowlist(server);
    Ok(serde_json::to_value(list).unwrap_or(Value::Null))
}

/// Handles the arm `"minecraft:allowlist/add"`.
pub(super) fn add(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    let to_add: Vec<PlayerDto> = parse_param(params, "add")?;
    {
        let mut whitelist_guard = server
            .data
            .whitelist_config
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for p in &to_add {
            let uuid = p.id.unwrap_or_else(Uuid::new_v4);
            let name = p.name.clone().unwrap_or_default();
            if !whitelist_guard.whitelist.iter().any(|e| e.uuid == uuid) {
                whitelist_guard
                    .whitelist
                    .push(WhitelistEntry::new(uuid, name));
                server.management_hub.broadcast_player_added_to_allowlist(p);
            }
        }
        whitelist_guard.save();
    };
    Ok(serde_json::to_value(get_allowlist(server)).unwrap_or(Value::Null))
}

/// Handles the arm `"minecraft:allowlist/remove"`.
pub(super) fn remove(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    let to_remove: Vec<PlayerDto> = parse_param(params, "remove")?;
    {
        let mut whitelist_guard = server
            .data
            .whitelist_config
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for p in &to_remove {
            if let Some(pos) = whitelist_guard.whitelist.iter().position(|e| {
                p.id.is_some_and(|id| e.uuid == id)
                    || p.name.as_ref().is_some_and(|name| &e.name == name)
            }) {
                whitelist_guard.whitelist.remove(pos);
                server
                    .management_hub
                    .broadcast_player_removed_from_allowlist(p);
            }
        }
        whitelist_guard.save();
    };
    kick_non_whitelisted_players(server);
    Ok(serde_json::to_value(get_allowlist(server)).unwrap_or(Value::Null))
}

/// Handles the arm `"minecraft:allowlist/clear"`.
pub(super) fn clear(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    check_no_params(params)?;
    {
        let mut whitelist_guard = server
            .data
            .whitelist_config
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for entry in whitelist_guard.whitelist.drain(..) {
            let dto = PlayerDto::new(Some(entry.uuid), Some(entry.name));
            server
                .management_hub
                .broadcast_player_removed_from_allowlist(&dto);
        }
        whitelist_guard.save();
    };
    kick_non_whitelisted_players(server);
    Ok(json!([]))
}

/// Handles the arm `"minecraft:allowlist/set"`.
pub(super) fn set(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    let new_list: Vec<PlayerDto> = parse_param(params, "players")?;
    {
        let mut whitelist_guard = server
            .data
            .whitelist_config
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        for old in &whitelist_guard.whitelist {
            if !new_list
                .iter()
                .any(|n| n.id.is_some_and(|id| id == old.uuid))
            {
                let dto = PlayerDto::new(Some(old.uuid), Some(old.name.clone()));
                server
                    .management_hub
                    .broadcast_player_removed_from_allowlist(&dto);
            }
        }

        let mut final_entries = Vec::new();
        for p in &new_list {
            let uuid = p.id.unwrap_or_else(Uuid::new_v4);
            let name = p.name.clone().unwrap_or_default();
            if !whitelist_guard.whitelist.iter().any(|e| e.uuid == uuid) {
                server.management_hub.broadcast_player_added_to_allowlist(p);
            }
            final_entries.push(WhitelistEntry::new(uuid, name));
        }

        whitelist_guard.whitelist = final_entries;
        whitelist_guard.save();
    };
    kick_non_whitelisted_players(server);
    Ok(serde_json::to_value(get_allowlist(server)).unwrap_or(Value::Null))
}
