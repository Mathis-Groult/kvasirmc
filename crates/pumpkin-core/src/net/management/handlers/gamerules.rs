//! RPC methods of the gamerules group.

use super::super::{dto::UntypedGameRule, rpc::RpcError};
use super::{check_no_params, get_gamerules, parse_param, update_gamerule};
use crate::server::Server;
use pumpkin_data::game_rules::GameRule;
use serde_json::Value;
use std::sync::Arc;

/// Handles the arm `"minecraft:gamerules"`.
pub(super) fn list(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    check_no_params(params)?;
    let rules = get_gamerules(server);
    Ok(serde_json::to_value(rules).unwrap_or(Value::Null))
}

/// Handles the arm `"minecraft:gamerules/update"`.
pub(super) fn update(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    let update: UntypedGameRule = parse_param(params, "gamerule")?;
    let Some(rule) = GameRule::all().iter().find(|r| r.to_string() == update.key) else {
        return Err(RpcError::invalid_params(Some(format!(
            "Unknown gamerule: {}",
            update.key
        ))));
    };

    let typed = update_gamerule(server, rule, &update.value)?;
    server.management_hub.broadcast_gamerule_changed(&typed);
    Ok(serde_json::to_value(typed).unwrap_or(Value::Null))
}
