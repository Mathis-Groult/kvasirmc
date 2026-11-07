//! RPC methods of the server settings group.

use super::super::rpc::RpcError;
use super::{check_no_params, parse_param};
use crate::{command::commands::kick_non_whitelisted_players, server::Server};
use serde_json::{Value, json};
use std::sync::{Arc, atomic::Ordering};

/// Handles the arm `"minecraft:serversettings/autosave"`.
pub(super) fn autosave(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(json!(
        server
            .management_hub
            .settings
            .autosave
            .load(Ordering::Relaxed)
    ))
}

/// Handles the arm `"minecraft:serversettings/autosave/set"`.
pub(super) fn autosave_set(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    let enable: bool = parse_param(params, "enable")?;
    server
        .management_hub
        .settings
        .autosave
        .store(enable, Ordering::Relaxed);
    Ok(json!(enable))
}

/// Handles the arm `"minecraft:serversettings/difficulty"`.
pub(super) fn difficulty(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(json!(
        server.management_hub.settings.difficulty.load().as_str()
    ))
}

/// Handles the arm `"minecraft:serversettings/difficulty/set"`.
pub(super) fn difficulty_set(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    let diff: String = parse_param(params, "difficulty")?;
    let lower = diff.to_lowercase();
    if matches!(lower.as_str(), "peaceful" | "easy" | "normal" | "hard") {
        server
            .management_hub
            .settings
            .difficulty
            .store(Arc::new(lower.clone()));
        Ok(json!(lower))
    } else {
        Err(RpcError::invalid_params(Some(format!(
            "Invalid difficulty: {diff}"
        ))))
    }
}

/// Handles the arm `"minecraft:serversettings/enforce_allowlist"`.
pub(super) fn enforce_allowlist(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(json!(
        server
            .management_hub
            .settings
            .enforce_allowlist
            .load(Ordering::Relaxed)
    ))
}

/// Handles the arm `"minecraft:serversettings/enforce_allowlist/set"`.
pub(super) fn enforce_allowlist_set(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    let enforce: bool = parse_param(params, "enforce")?;
    server
        .management_hub
        .settings
        .enforce_allowlist
        .store(enforce, Ordering::Relaxed);
    if enforce {
        kick_non_whitelisted_players(server);
    }
    Ok(json!(enforce))
}

/// Handles the arm `"minecraft:serversettings/use_allowlist"`.
pub(super) fn use_allowlist(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(json!(server.white_list.load(Ordering::Relaxed)))
}

/// Handles the arm `"minecraft:serversettings/use_allowlist/set"`.
pub(super) fn use_allowlist_set(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    let use_list: bool = parse_param(params, "use")?;
    server.white_list.store(use_list, Ordering::Relaxed);
    if use_list
        && server
            .management_hub
            .settings
            .enforce_allowlist
            .load(Ordering::Relaxed)
    {
        kick_non_whitelisted_players(server);
    }
    Ok(json!(use_list))
}

/// Handles the arm `"minecraft:serversettings/max_players"`.
pub(super) fn max_players(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(json!(
        server
            .management_hub
            .settings
            .max_players
            .load(Ordering::Relaxed)
    ))
}

/// Handles the arm `"minecraft:serversettings/max_players/set"`.
pub(super) fn max_players_set(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    let max: u32 = parse_param(params, "max")?;
    server
        .management_hub
        .settings
        .max_players
        .store(max, Ordering::Relaxed);
    Ok(json!(max))
}

/// Handles the arm `"minecraft:serversettings/pause_when_empty_seconds"`.
pub(super) fn pause_when_empty_seconds(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(json!(
        server
            .management_hub
            .settings
            .pause_when_empty_seconds
            .load(Ordering::Relaxed)
    ))
}

/// Handles the arm `"minecraft:serversettings/pause_when_empty_seconds/set"`.
pub(super) fn pause_when_empty_seconds_set(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    let sec: i32 = parse_param(params, "seconds")?;
    server
        .management_hub
        .settings
        .pause_when_empty_seconds
        .store(sec, Ordering::Relaxed);
    Ok(json!(sec))
}

/// Handles the arm `"minecraft:serversettings/player_idle_timeout"`.
pub(super) fn player_idle_timeout(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(json!(server.player_idle_timeout.load(Ordering::Relaxed)))
}

/// Handles the arm `"minecraft:serversettings/player_idle_timeout/set"`.
pub(super) fn player_idle_timeout_set(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    let sec: i32 = parse_param(params, "seconds")?;
    server.player_idle_timeout.store(sec, Ordering::Relaxed);
    Ok(json!(sec))
}

/// Handles the arm `"minecraft:serversettings/allow_flight"`.
pub(super) fn allow_flight(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(json!(
        server
            .management_hub
            .settings
            .allow_flight
            .load(Ordering::Relaxed)
    ))
}

/// Handles the arm `"minecraft:serversettings/allow_flight/set"`.
pub(super) fn allow_flight_set(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    let allow: bool = parse_param(params, "allow")?;
    server
        .management_hub
        .settings
        .allow_flight
        .store(allow, Ordering::Relaxed);
    Ok(json!(allow))
}

/// Handles the arm `"minecraft:serversettings/motd"`.
pub(super) fn motd(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(json!(server.management_hub.settings.motd.load().as_str()))
}

/// Handles the arm `"minecraft:serversettings/motd/set"`.
pub(super) fn motd_set(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    let motd: String = parse_param(params, "message")?;
    server
        .management_hub
        .settings
        .motd
        .store(Arc::new(motd.clone()));
    Ok(json!(motd))
}

/// Handles the arm `"minecraft:serversettings/spawn_protection_radius"`.
pub(super) fn spawn_protection_radius(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(json!(
        server
            .management_hub
            .settings
            .spawn_protection_radius
            .load(Ordering::Relaxed)
    ))
}

/// Handles the arm `"minecraft:serversettings/spawn_protection_radius/set"`.
pub(super) fn spawn_protection_radius_set(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    let radius: u32 = parse_param(params, "radius")?;
    server
        .management_hub
        .settings
        .spawn_protection_radius
        .store(radius, Ordering::Relaxed);
    Ok(json!(radius))
}

/// Handles the arm `"minecraft:serversettings/force_game_mode"`.
pub(super) fn force_game_mode(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(json!(
        server
            .management_hub
            .settings
            .force_game_mode
            .load(Ordering::Relaxed)
    ))
}

/// Handles the arm `"minecraft:serversettings/force_game_mode/set"`.
pub(super) fn force_game_mode_set(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    let force: bool = parse_param(params, "force")?;
    server
        .management_hub
        .settings
        .force_game_mode
        .store(force, Ordering::Relaxed);
    Ok(json!(force))
}

/// Handles the arm `"minecraft:serversettings/game_mode"`.
pub(super) fn game_mode(params: Option<&Value>, server: &Arc<Server>) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(json!(
        server.management_hub.settings.game_mode.load().as_str()
    ))
}

/// Handles the arm `"minecraft:serversettings/game_mode/set"`.
pub(super) fn game_mode_set(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    let mode: String = parse_param(params, "mode")?;
    let lower = mode.to_lowercase();
    if matches!(
        lower.as_str(),
        "survival" | "creative" | "adventure" | "spectator"
    ) {
        server
            .management_hub
            .settings
            .game_mode
            .store(Arc::new(lower.clone()));
        Ok(json!(lower))
    } else {
        Err(RpcError::invalid_params(Some(format!(
            "Invalid game mode: {mode}"
        ))))
    }
}

/// Handles the arm `"minecraft:serversettings/view_distance"`.
pub(super) fn view_distance(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(json!(
        server
            .management_hub
            .settings
            .view_distance
            .load(Ordering::Relaxed)
    ))
}

/// Handles the arm `"minecraft:serversettings/view_distance/set"`.
pub(super) fn view_distance_set(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    let dist: u32 = parse_param(params, "distance")?;
    server
        .management_hub
        .settings
        .view_distance
        .store(dist, Ordering::Relaxed);
    Ok(json!(dist))
}

/// Handles the arm `"minecraft:serversettings/simulation_distance"`.
pub(super) fn simulation_distance(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(json!(
        server
            .management_hub
            .settings
            .simulation_distance
            .load(Ordering::Relaxed)
    ))
}

/// Handles the arm `"minecraft:serversettings/simulation_distance/set"`.
pub(super) fn simulation_distance_set(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    let dist: u32 = parse_param(params, "distance")?;
    server
        .management_hub
        .settings
        .simulation_distance
        .store(dist, Ordering::Relaxed);
    Ok(json!(dist))
}

/// Handles the arm `"minecraft:serversettings/accept_transfers"`.
pub(super) fn accept_transfers(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(json!(
        server
            .management_hub
            .settings
            .accept_transfers
            .load(Ordering::Relaxed)
    ))
}

/// Handles the arm `"minecraft:serversettings/accept_transfers/set"`.
pub(super) fn accept_transfers_set(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    let accept: bool = parse_param(params, "accept")?;
    server
        .management_hub
        .settings
        .accept_transfers
        .store(accept, Ordering::Relaxed);
    Ok(json!(accept))
}

/// Handles the arm `"minecraft:serversettings/status_heartbeat_interval"`.
pub(super) fn status_heartbeat_interval(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(json!(
        server
            .management_hub
            .settings
            .status_heartbeat_interval
            .load(Ordering::Relaxed)
    ))
}

/// Handles the arm `"minecraft:serversettings/status_heartbeat_interval/set"`.
pub(super) fn status_heartbeat_interval_set(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    let sec: u64 = parse_param(params, "seconds")?;
    server
        .management_hub
        .settings
        .status_heartbeat_interval
        .store(sec, Ordering::Relaxed);
    Ok(json!(sec))
}

/// Handles the arm `"minecraft:serversettings/operator_user_permission_level"`.
pub(super) fn operator_user_permission_level(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(json!(
        server
            .management_hub
            .settings
            .operator_user_permission_level
            .load(Ordering::Relaxed)
    ))
}

/// Handles the arm `"minecraft:serversettings/operator_user_permission_level/set"`.
pub(super) fn operator_user_permission_level_set(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    let lvl: u8 = parse_param(params, "level")?;
    if lvl <= 4 {
        server
            .management_hub
            .settings
            .operator_user_permission_level
            .store(lvl, Ordering::Relaxed);
        Ok(json!(lvl))
    } else {
        Err(RpcError::invalid_params(Some(
            "Permission level must be between 0 and 4".to_string(),
        )))
    }
}

/// Handles the arm `"minecraft:serversettings/hide_online_players"`.
pub(super) fn hide_online_players(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(json!(
        server
            .management_hub
            .settings
            .hide_online_players
            .load(Ordering::Relaxed)
    ))
}

/// Handles the arm `"minecraft:serversettings/hide_online_players/set"`.
pub(super) fn hide_online_players_set(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    let hide: bool = parse_param(params, "hide")?;
    server
        .management_hub
        .settings
        .hide_online_players
        .store(hide, Ordering::Relaxed);
    Ok(json!(hide))
}

/// Handles the arm `"minecraft:serversettings/status_replies"`.
pub(super) fn status_replies(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(json!(
        server
            .management_hub
            .settings
            .status_replies
            .load(Ordering::Relaxed)
    ))
}

/// Handles the arm `"minecraft:serversettings/status_replies/set"`.
pub(super) fn status_replies_set(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    let enable: bool = parse_param(params, "enable")?;
    server
        .management_hub
        .settings
        .status_replies
        .store(enable, Ordering::Relaxed);
    Ok(json!(enable))
}

/// Handles the arm `"minecraft:serversettings/entity_broadcast_range"`.
pub(super) fn entity_broadcast_range(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(json!(
        server
            .management_hub
            .settings
            .entity_broadcast_range
            .load(Ordering::Relaxed)
    ))
}

/// Handles the arm `"minecraft:serversettings/entity_broadcast_range/set"`.
pub(super) fn entity_broadcast_range_set(
    params: Option<&Value>,
    server: &Arc<Server>,
) -> Result<Value, RpcError> {
    let pts: u32 = parse_param(params, "percentage_points")?;
    server
        .management_hub
        .settings
        .entity_broadcast_range
        .store(pts, Ordering::Relaxed);
    Ok(json!(pts))
}
