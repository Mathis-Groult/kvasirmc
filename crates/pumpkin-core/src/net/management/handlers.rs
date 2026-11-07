mod allowlist;
mod bans;
mod discovery;
mod gamerules;
mod ip_bans;
mod operators;
mod players;
mod server_control;
mod server_settings;
use std::{
    str::FromStr,
    sync::{Arc, LazyLock},
};

use pumpkin_data::game_rules::{GameRule, GameRuleValue};
use pumpkin_util::permission::PermissionLvl;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::server::Server;

use super::{
    dto::{IpBanDto, OperatorDto, PlayerDto, TypedGameRule, UserBanDto},
    rpc::{RpcError, RpcRequest, RpcResponse, is_valid_request_id},
};

#[expect(
    clippy::expect_used,
    reason = "assets/json-rpc-api-schema.json is validated at compile/init time"
)]
static OPENRPC_SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!(
        "../../../../../assets/json-rpc-api-schema.json"
    ))
    .expect("assets/json-rpc-api-schema.json must be valid JSON")
});

fn extract_param<'a>(params: Option<&'a Value>, param_name: &str) -> Result<&'a Value, RpcError> {
    match params {
        Some(Value::Object(map)) => map.get(param_name).ok_or_else(|| {
            RpcError::invalid_params(Some(format!(
                "Params passed by-name, but expected param [{param_name}] does not exist"
            )))
        }),
        Some(Value::Array(arr)) => {
            if arr.len() == 1 {
                Ok(&arr[0])
            } else {
                Err(RpcError::invalid_params(Some(
                    "Expected exactly one element in the params array".to_string(),
                )))
            }
        }
        _ => Err(RpcError::invalid_params(Some(
            "Expected params as array or named".to_string(),
        ))),
    }
}

fn check_no_params(params: Option<&Value>) -> Result<(), RpcError> {
    match params {
        None | Some(Value::Null) => Ok(()),
        Some(Value::Array(arr)) if arr.is_empty() => Ok(()),
        _ => Err(RpcError::invalid_params(Some(
            "Expected no params, or an empty array".to_string(),
        ))),
    }
}

fn parse_param<T: for<'de> Deserialize<'de>>(
    params: Option<&Value>,
    param_name: &str,
) -> Result<T, RpcError> {
    let val = extract_param(params, param_name)?;
    serde_json::from_value(val.clone())
        .map_err(|e| RpcError::invalid_params(Some(format!("Failed to parse parameter: {e}"))))
}

const fn u8_to_permission_lvl(lvl: u8) -> PermissionLvl {
    match lvl {
        0 => PermissionLvl::Zero,
        1 => PermissionLvl::One,
        2 => PermissionLvl::Two,
        3 => PermissionLvl::Three,
        _ => PermissionLvl::Four,
    }
}

pub async fn dispatch(request: RpcRequest, server: &Arc<Server>) -> Option<RpcResponse> {
    let id = request.id.clone();
    if let Some(ref req_id) = id
        && !is_valid_request_id(req_id)
    {
        return Some(RpcResponse::error(
            Value::Null,
            RpcError::invalid_request(Some(
                "Invalid request id - only String, Number and NULL supported".to_string(),
            )),
        ));
    }

    let Some(raw_method) = request.method else {
        return id.map(|req_id| {
            RpcResponse::error(
                req_id,
                RpcError::invalid_request(Some("Missing method name".to_string())),
            )
        });
    };

    let method = normalize_method(&raw_method);
    let result = execute_method(&method, request.params.as_ref(), server).await;

    id.map(|req_id| match result {
        Ok(val) => RpcResponse::success(req_id, val),
        Err(err) => RpcResponse::error(req_id, err),
    })
}

fn normalize_method(method: &str) -> String {
    if method == "rpc.discover" || method.contains(':') {
        method.to_string()
    } else {
        format!("minecraft:{method}")
    }
}

#[expect(clippy::too_many_lines)]
async fn execute_method(
    method: &str,
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    match method {
        "rpc.discover" => discovery::discover(params),

        // ==========================================
        // Allowlist
        // ==========================================
        "minecraft:allowlist" => allowlist::list(params, server),
        "minecraft:allowlist/add" => allowlist::add(params, server),
        "minecraft:allowlist/remove" => allowlist::remove(params, server),
        "minecraft:allowlist/clear" => allowlist::clear(params, server),
        "minecraft:allowlist/set" => allowlist::set(params, server),

        // ==========================================
        // Bans
        // ==========================================
        "minecraft:bans" => bans::list(params, server),
        "minecraft:bans/add" => bans::add(params, server),
        "minecraft:bans/remove" => bans::remove(params, server),
        "minecraft:bans/clear" => bans::clear(params, server),
        "minecraft:bans/set" => bans::set(params, server),

        // ==========================================
        // IP Bans
        // ==========================================
        "minecraft:ip_bans" => ip_bans::list(params, server),
        "minecraft:ip_bans/add" => ip_bans::add(params, server),
        "minecraft:ip_bans/remove" => ip_bans::remove(params, server),
        "minecraft:ip_bans/clear" => ip_bans::clear(params, server),
        "minecraft:ip_bans/set" => ip_bans::set(params, server),

        // ==========================================
        // Players
        // ==========================================
        "minecraft:players" => players::list(params, server),
        "minecraft:players/kick" => players::kick(params, server),

        // ==========================================
        // Operators
        // ==========================================
        "minecraft:operators" => operators::list(params, server),
        "minecraft:operators/add" => operators::add(params, server),
        "minecraft:operators/remove" => operators::remove(params, server),
        "minecraft:operators/clear" => operators::clear(params, server),
        "minecraft:operators/set" => operators::set(params, server),

        // ==========================================
        // Server State
        // ==========================================
        "minecraft:server/status" => server_control::status(params, server),
        "minecraft:server/save" => server_control::save(params, server).await,
        "minecraft:server/stop" => server_control::stop(params, server),
        "minecraft:server/system_message" => server_control::system_message(params, server),

        // ==========================================
        // Server Settings
        // ==========================================
        "minecraft:serversettings/autosave" => server_settings::autosave(params, server),
        "minecraft:serversettings/autosave/set" => server_settings::autosave_set(params, server),

        "minecraft:serversettings/difficulty" => server_settings::difficulty(params, server),
        "minecraft:serversettings/difficulty/set" => {
            server_settings::difficulty_set(params, server)
        }

        "minecraft:serversettings/enforce_allowlist" => {
            server_settings::enforce_allowlist(params, server)
        }
        "minecraft:serversettings/enforce_allowlist/set" => {
            server_settings::enforce_allowlist_set(params, server)
        }

        "minecraft:serversettings/use_allowlist" => server_settings::use_allowlist(params, server),
        "minecraft:serversettings/use_allowlist/set" => {
            server_settings::use_allowlist_set(params, server)
        }

        "minecraft:serversettings/max_players" => server_settings::max_players(params, server),
        "minecraft:serversettings/max_players/set" => {
            server_settings::max_players_set(params, server)
        }

        "minecraft:serversettings/pause_when_empty_seconds" => {
            server_settings::pause_when_empty_seconds(params, server)
        }
        "minecraft:serversettings/pause_when_empty_seconds/set" => {
            server_settings::pause_when_empty_seconds_set(params, server)
        }

        "minecraft:serversettings/player_idle_timeout" => {
            server_settings::player_idle_timeout(params, server)
        }
        "minecraft:serversettings/player_idle_timeout/set" => {
            server_settings::player_idle_timeout_set(params, server)
        }

        "minecraft:serversettings/allow_flight" => server_settings::allow_flight(params, server),
        "minecraft:serversettings/allow_flight/set" => {
            server_settings::allow_flight_set(params, server)
        }

        "minecraft:serversettings/motd" => server_settings::motd(params, server),
        "minecraft:serversettings/motd/set" => server_settings::motd_set(params, server),

        "minecraft:serversettings/spawn_protection_radius" => {
            server_settings::spawn_protection_radius(params, server)
        }
        "minecraft:serversettings/spawn_protection_radius/set" => {
            server_settings::spawn_protection_radius_set(params, server)
        }

        "minecraft:serversettings/force_game_mode" => {
            server_settings::force_game_mode(params, server)
        }
        "minecraft:serversettings/force_game_mode/set" => {
            server_settings::force_game_mode_set(params, server)
        }

        "minecraft:serversettings/game_mode" => server_settings::game_mode(params, server),
        "minecraft:serversettings/game_mode/set" => server_settings::game_mode_set(params, server),

        "minecraft:serversettings/view_distance" => server_settings::view_distance(params, server),
        "minecraft:serversettings/view_distance/set" => {
            server_settings::view_distance_set(params, server)
        }

        "minecraft:serversettings/simulation_distance" => {
            server_settings::simulation_distance(params, server)
        }
        "minecraft:serversettings/simulation_distance/set" => {
            server_settings::simulation_distance_set(params, server)
        }

        "minecraft:serversettings/accept_transfers" => {
            server_settings::accept_transfers(params, server)
        }
        "minecraft:serversettings/accept_transfers/set" => {
            server_settings::accept_transfers_set(params, server)
        }

        "minecraft:serversettings/status_heartbeat_interval" => {
            server_settings::status_heartbeat_interval(params, server)
        }
        "minecraft:serversettings/status_heartbeat_interval/set" => {
            server_settings::status_heartbeat_interval_set(params, server)
        }

        "minecraft:serversettings/operator_user_permission_level" => {
            server_settings::operator_user_permission_level(params, server)
        }
        "minecraft:serversettings/operator_user_permission_level/set" => {
            server_settings::operator_user_permission_level_set(params, server)
        }

        "minecraft:serversettings/hide_online_players" => {
            server_settings::hide_online_players(params, server)
        }
        "minecraft:serversettings/hide_online_players/set" => {
            server_settings::hide_online_players_set(params, server)
        }

        "minecraft:serversettings/status_replies" => {
            server_settings::status_replies(params, server)
        }
        "minecraft:serversettings/status_replies/set" => {
            server_settings::status_replies_set(params, server)
        }

        "minecraft:serversettings/entity_broadcast_range" => {
            server_settings::entity_broadcast_range(params, server)
        }
        "minecraft:serversettings/entity_broadcast_range/set" => {
            server_settings::entity_broadcast_range_set(params, server)
        }

        // ==========================================
        // Gamerules
        // ==========================================
        "minecraft:gamerules" => gamerules::list(params, server),
        "minecraft:gamerules/update" => gamerules::update(params, server),

        unknown => Err(RpcError::method_not_found(Some(format!(
            "Method not found: {unknown}"
        )))),
    }
}

fn get_allowlist(server: &Server) -> Vec<PlayerDto> {
    let guard = server
        .data
        .whitelist_config
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard
        .whitelist
        .iter()
        .map(|e| PlayerDto::new(Some(e.uuid), Some(e.name.clone())))
        .collect()
}

fn get_bans(server: &Server) -> Vec<UserBanDto> {
    let guard = server
        .data
        .banned_player_list
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard
        .banned_players
        .iter()
        .map(|b| UserBanDto {
            player: PlayerDto::new(Some(b.uuid), Some(b.name.clone())),
            reason: if b.reason.is_empty() {
                None
            } else {
                Some(b.reason.clone())
            },
            source: if b.source.is_empty() {
                None
            } else {
                Some(b.source.clone())
            },
            expires: b.expires.and_then(|exp| {
                exp.format(&time::format_description::well_known::Rfc3339)
                    .ok()
            }),
        })
        .collect()
}

fn get_ip_bans(server: &Server) -> Vec<IpBanDto> {
    let guard = server
        .data
        .banned_ip_list
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard
        .banned_ips
        .iter()
        .map(|b| IpBanDto {
            ip: Some(b.ip.to_string()),
            reason: if b.reason.is_empty() {
                None
            } else {
                Some(b.reason.clone())
            },
            source: if b.source.is_empty() {
                None
            } else {
                Some(b.source.clone())
            },
            expires: b.expires.and_then(|exp| {
                exp.format(&time::format_description::well_known::Rfc3339)
                    .ok()
            }),
        })
        .collect()
}

fn get_operators(server: &Server) -> Vec<OperatorDto> {
    let guard = server
        .data
        .operator_config
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard
        .ops
        .iter()
        .map(|o| OperatorDto {
            player: PlayerDto::new(Some(o.uuid), Some(o.name.clone())),
            permission_level: Some(o.level as u8),
            bypasses_player_limit: Some(o.bypasses_player_limit),
        })
        .collect()
}

fn get_gamerules(server: &Server) -> Vec<TypedGameRule> {
    let level_data = server.level_info.load();
    let mut rules = Vec::new();
    for rule in GameRule::all() {
        match level_data.game_rules.get(rule) {
            GameRuleValue::Int(v) => {
                rules.push(TypedGameRule {
                    key: rule.to_string(),
                    type_name: "integer".to_string(),
                    value: json!(v),
                });
            }
            GameRuleValue::Bool(v) => {
                rules.push(TypedGameRule {
                    key: rule.to_string(),
                    type_name: "boolean".to_string(),
                    value: json!(v),
                });
            }
        }
    }
    rules
}

fn update_gamerule(
    server: &Server,
    rule: &GameRule,
    value: &Value,
) -> Result<TypedGameRule, RpcError> {
    let level_data = server.level_info.load();
    match level_data.game_rules.get(rule) {
        GameRuleValue::Int(_) => {
            let int_val = value
                .as_i64()
                .or_else(|| value.as_str().and_then(|s| s.parse::<i64>().ok()))
                .ok_or_else(|| {
                    RpcError::invalid_params(Some(format!(
                        "Stated type mismatches with actual type of gamerule \"{rule}\""
                    )))
                })?;
            for world in server.worlds.load().iter() {
                world.set_game_rule(rule, GameRuleValue::Int(int_val));
            }
            Ok(TypedGameRule {
                key: rule.to_string(),
                type_name: "integer".to_string(),
                value: json!(int_val),
            })
        }
        GameRuleValue::Bool(_) => {
            let bool_val = value
                .as_bool()
                .or_else(|| value.as_str().and_then(|s| bool::from_str(s).ok()))
                .ok_or_else(|| {
                    RpcError::invalid_params(Some(format!(
                        "Stated type mismatches with actual type of gamerule \"{rule}\""
                    )))
                })?;
            for world in server.worlds.load().iter() {
                world.set_game_rule(rule, GameRuleValue::Bool(bool_val));
            }
            Ok(TypedGameRule {
                key: rule.to_string(),
                type_name: "boolean".to_string(),
                value: json!(bool_val),
            })
        }
    }
}
