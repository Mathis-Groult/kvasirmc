//! RPC methods of the operators group.

use super::super::{
    dto::{OperatorDto, PlayerDto},
    rpc::RpcError,
};
use super::{check_no_params, get_operators, parse_param, u8_to_permission_lvl};
use crate::{data::SaveJSONConfiguration, server::Server};
use pumpkin_config::op::Op;
use pumpkin_util::permission::PermissionLvl;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

/// Handles the arm `"minecraft:operators"`.
pub(super) fn list(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(serde_json::to_value(get_operators(server)).unwrap_or(Value::Null))
}

/// Handles the arm `"minecraft:operators/add"`.
pub(super) fn add(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    let add_ops: Vec<OperatorDto> = parse_param(params, "add")?;
    {
        let mut op_guard = server
            .data
            .operator_config
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for op in &add_ops {
            let uuid = op.player.id.unwrap_or_else(Uuid::new_v4);
            let name = op.player.name.clone().unwrap_or_default();
            let level = op
                .permission_level
                .map_or(PermissionLvl::Four, u8_to_permission_lvl);
            let bypass = op.bypasses_player_limit.unwrap_or(false);
            op_guard.ops.retain(|o| o.uuid != uuid);
            op_guard.ops.push(Op::new(uuid, name, level, bypass));
            server.management_hub.broadcast_player_oped(op);

            for player in server.get_all_players() {
                if player.gameprofile.id == uuid {
                    player.permission_lvl.store(level);
                }
            }
        }
        op_guard.save();
    };
    Ok(serde_json::to_value(get_operators(server)).unwrap_or(Value::Null))
}

/// Handles the arm `"minecraft:operators/remove"`.
pub(super) fn remove(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    let to_remove: Vec<PlayerDto> = parse_param(params, "remove")?;
    {
        let mut op_guard = server
            .data
            .operator_config
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for p in &to_remove {
            if let Some(pos) = op_guard.ops.iter().position(|o| {
                p.id.is_some_and(|id| o.uuid == id)
                    || p.name.as_ref().is_some_and(|name| &o.name == name)
            }) {
                let removed = op_guard.ops.remove(pos);
                let dto = OperatorDto {
                    player: PlayerDto::new(Some(removed.uuid), Some(removed.name)),
                    permission_level: Some(removed.level as u8),
                    bypasses_player_limit: Some(removed.bypasses_player_limit),
                };
                server.management_hub.broadcast_player_deoped(&dto);

                for player in server.get_all_players() {
                    if player.gameprofile.id == removed.uuid {
                        player.permission_lvl.store(PermissionLvl::Zero);
                    }
                }
            }
        }
        op_guard.save();
    };
    Ok(serde_json::to_value(get_operators(server)).unwrap_or(Value::Null))
}

/// Handles the arm `"minecraft:operators/clear"`.
pub(super) fn clear(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    check_no_params(params)?;
    {
        let mut op_guard = server
            .data
            .operator_config
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for removed in op_guard.ops.drain(..) {
            let dto = OperatorDto {
                player: PlayerDto::new(Some(removed.uuid), Some(removed.name)),
                permission_level: Some(removed.level as u8),
                bypasses_player_limit: Some(removed.bypasses_player_limit),
            };
            server.management_hub.broadcast_player_deoped(&dto);

            for player in server.get_all_players() {
                if player.gameprofile.id == removed.uuid {
                    player.permission_lvl.store(PermissionLvl::Zero);
                }
            }
        }
        op_guard.save();
    };
    Ok(json!([]))
}

/// Handles the arm `"minecraft:operators/set"`.
pub(super) fn set(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    let new_ops: Vec<OperatorDto> = parse_param(params, "operators")?;
    {
        let mut op_guard = server
            .data
            .operator_config
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        for old in &op_guard.ops {
            if !new_ops
                .iter()
                .any(|n| n.player.id.is_some_and(|id| id == old.uuid))
            {
                let dto = OperatorDto {
                    player: PlayerDto::new(Some(old.uuid), Some(old.name.clone())),
                    permission_level: Some(old.level as u8),
                    bypasses_player_limit: Some(old.bypasses_player_limit),
                };
                server.management_hub.broadcast_player_deoped(&dto);
                for player in server.get_all_players() {
                    if player.gameprofile.id == old.uuid {
                        player.permission_lvl.store(PermissionLvl::Zero);
                    }
                }
            }
        }

        let mut final_ops = Vec::new();
        for op in &new_ops {
            let uuid = op.player.id.unwrap_or_else(Uuid::new_v4);
            let name = op.player.name.clone().unwrap_or_default();
            let level = op
                .permission_level
                .map_or(PermissionLvl::Four, u8_to_permission_lvl);
            let bypass = op.bypasses_player_limit.unwrap_or(false);
            final_ops.push(Op::new(uuid, name, level, bypass));

            if !op_guard.ops.iter().any(|o| o.uuid == uuid) {
                server.management_hub.broadcast_player_oped(op);
            }
            for player in server.get_all_players() {
                if player.gameprofile.id == uuid {
                    player.permission_lvl.store(level);
                }
            }
        }

        op_guard.ops = final_ops;
        op_guard.save();
    };
    Ok(serde_json::to_value(get_operators(server)).unwrap_or(Value::Null))
}
