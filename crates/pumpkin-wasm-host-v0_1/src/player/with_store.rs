//! The player calls that need the store (`player` with-store half): async calls into the engine.

use super::{from_wit_permission_level, parse_ban_expiry};
use crate::{
    events::{from_wasm_game_mode, from_wasm_position},
    living_entity::from_wit_damage_type,
    pumpkin::{
        self,
        plugin::{damage_types::DamageType as WitDamageType, player::Player, world::World},
    },
};
use pumpkin_core::entity::{
    EntityBase,
    player::advancement::{AdvancementAward, PlayerAdvancement},
};
use pumpkin_core::plugin::api::gui::PluginScreenHandler;
use pumpkin_wasm_host_common::{plugin::WasmPlugin, state::PluginHostState};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use wasmtime::component::{Access, HasSelf, Resource};

fn plugin_from_state(state: &PluginHostState) -> wasmtime::Result<Arc<WasmPlugin>> {
    state
        .plugin
        .as_ref()
        .and_then(std::sync::Weak::upgrade)
        .ok_or_else(|| wasmtime::Error::msg("Plugin instance not available"))
}

impl pumpkin::plugin::player::HostPlayerWithStore<PluginHostState> for HasSelf<PluginHostState> {
    async fn open_ender_chest(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<Player>,
    ) -> wasmtime::Result<()> {
        let (player, plugin) = {
            let state = host.get();
            (state.get(&player)?.clone(), plugin_from_state(state)?)
        };

        plugin
            .store
            .pump_blocking(&mut host, move || {
                player.open_ender_chest();
            })
            .await
    }

    async fn set_gamemode(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<Player>,
        mode: pumpkin::plugin::common::GameMode,
    ) -> wasmtime::Result<bool> {
        let (player, plugin) = {
            let state = host.get();
            (state.get(&player)?.clone(), plugin_from_state(state)?)
        };
        let mode = from_wasm_game_mode(mode);

        plugin
            .store
            .pump_blocking(&mut host, move || player.set_gamemode(mode))
            .await
    }

    async fn set_permission_level(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<Player>,
        level: pumpkin::plugin::permission::PermissionLevel,
    ) -> wasmtime::Result<()> {
        let (player, server, plugin) = {
            let state = host.get();
            (
                state.get(&player)?.clone(),
                state.server.as_ref().expect("server not available").clone(),
                plugin_from_state(state)?,
            )
        };
        let level = from_wit_permission_level(level);

        plugin
            .store
            .pump_blocking(&mut host, move || {
                let command_dispatcher = server.command_dispatcher.load();
                player.set_permission_lvl(&server, level, &command_dispatcher);
            })
            .await
    }

    async fn has_permission(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<Player>,
        node: String,
    ) -> wasmtime::Result<bool> {
        let (player, server, plugin) = {
            let state = host.get();
            (
                state.get(&player)?.clone(),
                state.server.as_ref().expect("server not available").clone(),
                plugin_from_state(state)?,
            )
        };

        plugin
            .store
            .pump_blocking(&mut host, move || player.has_permission(&server, &node))
            .await
    }

    async fn add_effect(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<Player>,
        effect: pumpkin::plugin::status_effect::StatusEffectInstance,
    ) -> wasmtime::Result<()> {
        let (player, plugin) = {
            let state = host.get();
            (state.get(&player)?.clone(), plugin_from_state(state)?)
        };
        let effect_type =
            super::super::status_effect::from_wasm_status_effect_type(effect.effect_type);
        let Some(status_effect) =
            pumpkin_data::effect::StatusEffect::from_name(effect_type.to_name())
        else {
            return Ok(());
        };
        let effect = pumpkin_data::potion::Effect {
            effect_type: status_effect,
            duration: effect.duration as i32,
            amplifier: effect.amplifier,
            ambient: effect.ambient,
            show_particles: effect.show_particles,
            show_icon: effect.show_icon,
            blend: false,
        };

        plugin
            .store
            .pump_blocking(&mut host, move || player.add_effect(effect))
            .await
    }

    async fn heal(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<Player>,
        amount: f32,
    ) -> wasmtime::Result<()> {
        let (player, plugin) = {
            let state = host.get();
            (state.get(&player)?.clone(), plugin_from_state(state)?)
        };

        plugin
            .store
            .pump_blocking(&mut host, move || player.heal(amount))
            .await
    }

    async fn damage(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<Player>,
        amount: f32,
        damage_type: WitDamageType,
    ) -> wasmtime::Result<()> {
        let (player, plugin) = {
            let state = host.get();
            (state.get(&player)?.clone(), plugin_from_state(state)?)
        };
        let damage_type = from_wit_damage_type(damage_type);

        plugin
            .store
            .pump_blocking(&mut host, move || {
                player.damage(&*player, amount, damage_type);
            })
            .await
    }

    async fn kill(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<Player>,
    ) -> wasmtime::Result<()> {
        let (player, plugin) = {
            let state = host.get();
            (state.get(&player)?.clone(), plugin_from_state(state)?)
        };

        plugin
            .store
            .pump_blocking(&mut host, move || player.kill())
            .await
    }

    async fn teleport(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<Player>,
        position: pumpkin::plugin::common::Position,
        yaw: Option<f32>,
        pitch: Option<f32>,
        world: Resource<World>,
    ) -> wasmtime::Result<()> {
        let (player, world, plugin) = {
            let state = host.get();
            (
                state.get(&player)?.clone(),
                state.take(world)?,
                plugin_from_state(state)?,
            )
        };
        let position = from_wasm_position(position);

        plugin
            .store
            .pump_blocking(&mut host, move || {
                player.teleport(position, yaw, pitch, world);
            })
            .await
    }

    async fn teleport_world(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<Player>,
        world: Resource<World>,
        position: pumpkin::plugin::common::Position,
        yaw: Option<f32>,
        pitch: Option<f32>,
    ) -> wasmtime::Result<()> {
        let (player, world, plugin) = {
            let state = host.get();
            (
                state.get(&player)?.clone(),
                state.take(world)?,
                plugin_from_state(state)?,
            )
        };
        let position = from_wasm_position(position);
        let runtime = tokio::runtime::Handle::current();

        plugin
            .store
            .pump_blocking(&mut host, move || {
                runtime.block_on(player.teleport_world(world, position, yaw, pitch));
            })
            .await
    }

    async fn respawn(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<Player>,
    ) -> wasmtime::Result<()> {
        let (player, plugin) = {
            let state = host.get();
            (state.get(&player)?.clone(), plugin_from_state(state)?)
        };
        let runtime = tokio::runtime::Handle::current();

        plugin
            .store
            .pump_blocking(&mut host, move || runtime.block_on(player.respawn()))
            .await
    }

    async fn open_gui(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<Player>,
        gui: Resource<pumpkin::plugin::gui::Gui>,
    ) -> wasmtime::Result<()> {
        let (player, gui, plugin) = {
            let state = host.get();
            let gui = state.take(gui)?;
            (state.get(&player)?.clone(), gui, plugin_from_state(state)?)
        };

        let (window_type, inventory, allow_grab_items, allow_put_items, title) = plugin
            .store
            .pump_reentry(&mut host, async move {
                let gui = gui.lock().await;
                (
                    gui.window_type,
                    gui.inventory.clone(),
                    gui.allow_grab_items,
                    gui.allow_put_items,
                    gui.title.clone(),
                )
            })
            .await?;

        plugin
            .store
            .pump_blocking(&mut host, move || {
                player.increment_screen_handler_sync_id();
                let sync_id = player.screen_handler_sync_id.load(Ordering::Relaxed);
                let screen_handler: Arc<
                    std::sync::Mutex<dyn pumpkin_inventory::screen_handler::ScreenHandler>,
                > = Arc::new(std::sync::Mutex::new(PluginScreenHandler::new(
                    sync_id,
                    window_type,
                    player.inventory(),
                    &inventory,
                    allow_grab_items,
                    allow_put_items,
                )));
                player.open_handled_screen_direct(screen_handler, &title);
            })
            .await
    }

    async fn ban(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<Player>,
        options: pumpkin::plugin::player::BanPlayerOptions,
    ) -> wasmtime::Result<()> {
        let pumpkin::plugin::player::BanPlayerOptions {
            reason,
            source,
            expires_at_utc,
            duration_seconds,
            kick_if_online,
            log_to_console,
        } = options;
        let (player, server, reason, plugin) = {
            let state = host.get();
            (
                state.get(&player)?.clone(),
                state.server.as_ref().expect("server not available").clone(),
                reason.map(|t| state.take(t)).transpose()?,
                plugin_from_state(state)?,
            )
        };
        let expires = parse_ban_expiry(expires_at_utc, duration_seconds);

        plugin
            .store
            .pump_blocking(&mut host, move || {
                player.ban_explicit(
                    &server,
                    reason,
                    source,
                    expires,
                    kick_if_online,
                    log_to_console,
                );
            })
            .await
    }

    async fn ban_ip(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<Player>,
        options: pumpkin::plugin::player::BanIpOptions,
    ) -> wasmtime::Result<()> {
        let pumpkin::plugin::player::BanIpOptions {
            reason,
            source,
            expires_at_utc,
            duration_seconds,
            kick_matching_players,
            log_to_console,
        } = options;
        let (player, server, reason, plugin) = {
            let state = host.get();
            (
                state.get(&player)?.clone(),
                state.server.as_ref().expect("server not available").clone(),
                reason.map(|t| state.take(t)).transpose()?,
                plugin_from_state(state)?,
            )
        };
        let expires = parse_ban_expiry(expires_at_utc, duration_seconds);

        plugin
            .store
            .pump_blocking(&mut host, move || {
                player.ban_ip_explicit(
                    &server,
                    reason,
                    source,
                    expires,
                    kick_matching_players,
                    log_to_console,
                );
            })
            .await
    }

    async fn set_food_level(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<Player>,
        level: u8,
    ) -> wasmtime::Result<()> {
        let (player, plugin) = {
            let state = host.get();
            (state.get(&player)?.clone(), plugin_from_state(state)?)
        };

        plugin
            .store
            .pump_blocking(&mut host, move || player.set_food_level(level))
            .await
    }

    async fn set_experience_level(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<Player>,
        level: i32,
    ) -> wasmtime::Result<()> {
        let (player, plugin) = {
            let state = host.get();
            (state.get(&player)?.clone(), plugin_from_state(state)?)
        };

        plugin
            .store
            .pump_blocking(&mut host, move || player.set_experience_level(level, true))
            .await
    }

    async fn set_experience_progress(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<Player>,
        progress: f32,
    ) -> wasmtime::Result<()> {
        let (player, plugin) = {
            let state = host.get();
            (state.get(&player)?.clone(), plugin_from_state(state)?)
        };

        plugin
            .store
            .pump_blocking(&mut host, move || {
                player.set_experience(
                    player.experience_level.load(Ordering::Relaxed),
                    progress,
                    player.experience_points.load(Ordering::Relaxed),
                );
            })
            .await
    }

    async fn set_experience_points(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<Player>,
        points: i32,
    ) -> wasmtime::Result<()> {
        let (player, plugin) = {
            let state = host.get();
            (state.get(&player)?.clone(), plugin_from_state(state)?)
        };

        plugin
            .store
            .pump_blocking(&mut host, move || {
                player.set_experience(
                    player.experience_level.load(Ordering::Relaxed),
                    player.experience_progress.load(),
                    points,
                );
            })
            .await
    }

    async fn add_experience_levels(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<Player>,
        levels: i32,
    ) -> wasmtime::Result<()> {
        let (player, plugin) = {
            let state = host.get();
            (state.get(&player)?.clone(), plugin_from_state(state)?)
        };

        plugin
            .store
            .pump_blocking(&mut host, move || player.add_experience_levels(levels))
            .await
    }

    async fn add_experience_points(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<Player>,
        points: i32,
    ) -> wasmtime::Result<()> {
        let (player, plugin) = {
            let state = host.get();
            (state.get(&player)?.clone(), plugin_from_state(state)?)
        };

        plugin
            .store
            .pump_blocking(&mut host, move || player.add_experience_points(points))
            .await
    }

    async fn award_advancement_criterion(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<Player>,
        advancement_id: String,
        criterion: String,
    ) -> wasmtime::Result<bool> {
        let (player, plugin) = {
            let state = host.get();
            (state.get(&player)?.clone(), plugin_from_state(state)?)
        };
        let Some(advancement) = crate::advancement::find_advancement(&advancement_id) else {
            return Ok(false);
        };

        plugin
            .store
            .pump_blocking(&mut host, move || {
                let result = {
                    let mut guard = player
                        .advancements
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    guard.award(advancement, &criterion)
                };

                PlayerAdvancement::finish_award(&player, advancement, result);
                if result.awarded() {
                    let mut guard = player
                        .advancements
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    guard.flush_dirty(&player, true);
                }
                result.awarded()
            })
            .await
    }

    async fn award_advancement(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<Player>,
        advancement_id: String,
    ) -> wasmtime::Result<bool> {
        let (player, plugin) = {
            let state = host.get();
            (state.get(&player)?.clone(), plugin_from_state(state)?)
        };
        let Some(advancement) = crate::advancement::find_advancement(&advancement_id) else {
            return Ok(false);
        };

        plugin
            .store
            .pump_blocking(&mut host, move || {
                let result = {
                    let mut guard = player
                        .advancements
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let progress = guard.progress.get_mut_or_start_progress(advancement);
                    if progress.is_done() {
                        return false;
                    }
                    let remaining: Vec<Arc<str>> = progress.get_remaining_criteria().collect();
                    let mut result = AdvancementAward::default();
                    for criterion in remaining {
                        result = result.combine(guard.award(advancement, &criterion));
                    }
                    result
                };

                PlayerAdvancement::finish_award(&player, advancement, result);
                if result.awarded() {
                    let mut guard = player
                        .advancements
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    guard.flush_dirty(&player, true);
                }
                result.awarded()
            })
            .await
    }
}
