//! RPC methods of the players group.

use super::super::{
    dto::{KickPlayerDto, PlayerDto},
    rpc::RpcError,
};
use super::{check_no_params, parse_param};
use crate::{net::DisconnectReason, server::Server};
use pumpkin_data::translation;
use serde_json::Value;
use std::sync::Arc;

/// Handles the arm `"minecraft:players"`.
pub(super) fn list(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    check_no_params(params)?;
    let players: Vec<PlayerDto> = server
        .get_all_players()
        .into_iter()
        .map(|p| PlayerDto::new(Some(p.gameprofile.id), Some(p.gameprofile.name.clone())))
        .collect();
    Ok(serde_json::to_value(players).unwrap_or(Value::Null))
}

/// Handles the arm `"minecraft:players/kick"`.
pub(super) fn kick(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    let kick_requests: Vec<KickPlayerDto> = parse_param(params, "kick")?;
    let mut kicked = Vec::new();
    for req in kick_requests {
        let target = server.get_all_players().into_iter().find(|p| {
            req.player.id.is_some_and(|id| id == p.gameprofile.id)
                || req
                    .player
                    .name
                    .as_ref()
                    .is_some_and(|name| name == &p.gameprofile.name)
        });
        if let Some(player) = target {
            let message = req.message.as_ref().map_or_else(
                || {
                    pumpkin_macros::translate_cross!(
                        translation::java::MULTIPLAYER_DISCONNECT_KICKED,
                        translation::bedrock::DISCONNECT_KICKED
                    )
                },
                super::super::dto::MessageDto::to_text_component,
            );
            player.kick(DisconnectReason::Kicked, &message);
            kicked.push(req.player);
        }
    }
    Ok(serde_json::to_value(kicked).unwrap_or(Value::Null))
}
