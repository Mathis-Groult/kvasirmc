//! RPC methods of the server control group.

use super::super::{
    dto::{PlayerDto, ServerState, SystemMessageDto, VersionDto},
    rpc::RpcError,
};
use super::{check_no_params, parse_param};
use crate::server::Server;
use pumpkin_data::packet::CURRENT_MC_VERSION;
use serde_json::{Value, json};
use std::sync::Arc;

/// Handles the arm `"minecraft:server/status"`.
pub(super) fn status(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    check_no_params(params)?;
    let state = ServerState {
        started: true,
        players: server
            .get_all_players()
            .into_iter()
            .map(|p| PlayerDto::new(Some(p.gameprofile.id), Some(p.gameprofile.name.clone())))
            .collect(),
        version: VersionDto {
            name: CURRENT_MC_VERSION.to_string(),
            protocol: CURRENT_MC_VERSION.protocol_version(),
        },
    };
    Ok(serde_json::to_value(state).unwrap_or(Value::Null))
}

/// Handles the arm `"minecraft:server/save"`.
pub(super) async fn save(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    let _flush: bool = parse_param(params, "flush")?;
    server.management_hub.broadcast_server_saving();
    let _ = server.save_all().await;
    server.management_hub.broadcast_server_saved();
    Ok(json!(true))
}

/// Handles the arm `"minecraft:server/stop"`.
pub(super) fn stop(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    check_no_params(params)?;
    server.management_hub.broadcast_server_stopping();
    let s = server.clone();
    tokio::spawn(async move {
        s.shutdown().await;
        std::process::exit(0);
    });
    Ok(json!(true))
}

/// Handles the arm `"minecraft:server/system_message"`.
pub(super) fn system_message(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    let sys_msg: SystemMessageDto = parse_param(params, "message")?;
    let component = sys_msg.message.to_text_component();
    let overlay = sys_msg.overlay;
    if let Some(ref receivers) = sys_msg.receiving_players {
        if receivers.is_empty() {
            return Ok(json!(false));
        }
        for player_dto in receivers {
            let target = server.get_all_players().into_iter().find(|p| {
                player_dto.id.is_some_and(|id| id == p.gameprofile.id)
                    || player_dto
                        .name
                        .as_ref()
                        .is_some_and(|name| name == &p.gameprofile.name)
            });
            if let Some(player) = target {
                player.send_system_message_raw(&component, overlay);
            }
        }
    } else {
        for player in server.get_all_players() {
            player.send_system_message_raw(&component, overlay);
        }
    }
    Ok(json!(true))
}
