//! The Java-only player calls a plugin can make (`java-player`), with their conversions.

use crate::pumpkin::plugin::java_dialogs::Dialog;
use crate::{
    pumpkin::{
        self,
        plugin::{player::SkinParts, uuid::Uuid},
    },
    uuid::UuidExt,
};
use pumpkin_protocol::java::client::dialog::DialogNBT;
use pumpkin_util::version::JavaMinecraftVersion;
use pumpkin_wasm_host_common::state::PluginHostState;
use wasmtime::component::{Access, HasSelf, Resource};

const fn from_wit_client_game_event(
    event: pumpkin::plugin::player::ClientGameEvent,
) -> pumpkin_protocol::java::client::play::GameEvent {
    use pumpkin::plugin::player::ClientGameEvent as W;
    use pumpkin_protocol::java::client::play::GameEvent as G;
    match event {
        W::NoRespawnBlockAvailable => G::NoRespawnBlockAvailable,
        W::BeginRaining => G::BeginRaining,
        W::EndRaining => G::EndRaining,
        W::ChangeGameMode => G::ChangeGameMode,
        W::WinGame => G::WinGame,
        W::DemoEvent => G::DemoEvent,
        W::ArrowHitPlayer => G::ArrowHitPlayer,
        W::RainLevelChange => G::RainLevelChange,
        W::ThunderLevelChange => G::ThunderLevelChange,
        W::PlayPufferfishStringSound => G::PlayPufferfishStringSound,
        W::PlayElderGuardianMobAppearance => G::PlayElderGuardianMobAppearance,
        W::EnabledRespawnScreen => G::EnabledRespawnScreen,
        W::LimitedCrafting => G::LimitedCrafting,
        W::StartWaitingChunks => G::StartWaitingChunks,
    }
}

const fn to_wasm_java_version(
    version: JavaMinecraftVersion,
) -> pumpkin::plugin::player::JavaMinecraftVersion {
    match version {
        JavaMinecraftVersion::V_1_7_2 => pumpkin::plugin::player::JavaMinecraftVersion::V172,
        JavaMinecraftVersion::V_1_7_6 => pumpkin::plugin::player::JavaMinecraftVersion::V176,
        JavaMinecraftVersion::V_1_8 => pumpkin::plugin::player::JavaMinecraftVersion::V18,
        JavaMinecraftVersion::V_1_9 => pumpkin::plugin::player::JavaMinecraftVersion::V19,
        JavaMinecraftVersion::V_1_9_1 => pumpkin::plugin::player::JavaMinecraftVersion::V191,
        JavaMinecraftVersion::V_1_9_2 => pumpkin::plugin::player::JavaMinecraftVersion::V192,
        JavaMinecraftVersion::V_1_9_3 => pumpkin::plugin::player::JavaMinecraftVersion::V193,
        JavaMinecraftVersion::V_1_10 => pumpkin::plugin::player::JavaMinecraftVersion::V110,
        JavaMinecraftVersion::V_1_11 => pumpkin::plugin::player::JavaMinecraftVersion::V111,
        JavaMinecraftVersion::V_1_11_1 => pumpkin::plugin::player::JavaMinecraftVersion::V1111,
        JavaMinecraftVersion::V_1_12 => pumpkin::plugin::player::JavaMinecraftVersion::V112,
        JavaMinecraftVersion::V_1_12_1 => pumpkin::plugin::player::JavaMinecraftVersion::V1121,
        JavaMinecraftVersion::V_1_12_2 => pumpkin::plugin::player::JavaMinecraftVersion::V1122,
        JavaMinecraftVersion::V_1_13 => pumpkin::plugin::player::JavaMinecraftVersion::V113,
        JavaMinecraftVersion::V_1_13_1 => pumpkin::plugin::player::JavaMinecraftVersion::V1131,
        JavaMinecraftVersion::V_1_13_2 => pumpkin::plugin::player::JavaMinecraftVersion::V1132,
        JavaMinecraftVersion::V_1_14 => pumpkin::plugin::player::JavaMinecraftVersion::V114,
        JavaMinecraftVersion::V_1_14_1 => pumpkin::plugin::player::JavaMinecraftVersion::V1141,
        JavaMinecraftVersion::V_1_14_2 => pumpkin::plugin::player::JavaMinecraftVersion::V1142,
        JavaMinecraftVersion::V_1_14_3 => pumpkin::plugin::player::JavaMinecraftVersion::V1143,
        JavaMinecraftVersion::V_1_14_4 => pumpkin::plugin::player::JavaMinecraftVersion::V1144,
        JavaMinecraftVersion::V_1_15 => pumpkin::plugin::player::JavaMinecraftVersion::V115,
        JavaMinecraftVersion::V_1_15_1 => pumpkin::plugin::player::JavaMinecraftVersion::V1151,
        JavaMinecraftVersion::V_1_15_2 => pumpkin::plugin::player::JavaMinecraftVersion::V1152,
        JavaMinecraftVersion::V_1_16 => pumpkin::plugin::player::JavaMinecraftVersion::V116,
        JavaMinecraftVersion::V_1_16_1 => pumpkin::plugin::player::JavaMinecraftVersion::V1161,
        JavaMinecraftVersion::V_1_16_2 => pumpkin::plugin::player::JavaMinecraftVersion::V1162,
        JavaMinecraftVersion::V_1_16_3 => pumpkin::plugin::player::JavaMinecraftVersion::V1163,
        JavaMinecraftVersion::V_1_16_4 => pumpkin::plugin::player::JavaMinecraftVersion::V1164,
        JavaMinecraftVersion::V_1_17 => pumpkin::plugin::player::JavaMinecraftVersion::V117,
        JavaMinecraftVersion::V_1_17_1 => pumpkin::plugin::player::JavaMinecraftVersion::V1171,
        JavaMinecraftVersion::V_1_18 => pumpkin::plugin::player::JavaMinecraftVersion::V118,
        JavaMinecraftVersion::V_1_18_2 => pumpkin::plugin::player::JavaMinecraftVersion::V1182,
        JavaMinecraftVersion::V_1_19 => pumpkin::plugin::player::JavaMinecraftVersion::V119,
        JavaMinecraftVersion::V_1_19_1 => pumpkin::plugin::player::JavaMinecraftVersion::V1191,
        JavaMinecraftVersion::V_1_19_3 => pumpkin::plugin::player::JavaMinecraftVersion::V1193,
        JavaMinecraftVersion::V_1_19_4 => pumpkin::plugin::player::JavaMinecraftVersion::V1194,
        JavaMinecraftVersion::V_1_20 => pumpkin::plugin::player::JavaMinecraftVersion::V120,
        JavaMinecraftVersion::V_1_20_2 => pumpkin::plugin::player::JavaMinecraftVersion::V1202,
        JavaMinecraftVersion::V_1_20_3 => pumpkin::plugin::player::JavaMinecraftVersion::V1203,
        JavaMinecraftVersion::V_1_20_5 => pumpkin::plugin::player::JavaMinecraftVersion::V1205,
        JavaMinecraftVersion::V_1_21 => pumpkin::plugin::player::JavaMinecraftVersion::V121,
        JavaMinecraftVersion::V_1_21_2 => pumpkin::plugin::player::JavaMinecraftVersion::V1212,
        JavaMinecraftVersion::V_1_21_4 => pumpkin::plugin::player::JavaMinecraftVersion::V1214,
        JavaMinecraftVersion::V_1_21_5 => pumpkin::plugin::player::JavaMinecraftVersion::V1215,
        JavaMinecraftVersion::V_1_21_6 => pumpkin::plugin::player::JavaMinecraftVersion::V1216,
        JavaMinecraftVersion::V_1_21_7 => pumpkin::plugin::player::JavaMinecraftVersion::V1217,
        JavaMinecraftVersion::V_1_21_9 => pumpkin::plugin::player::JavaMinecraftVersion::V1219,
        JavaMinecraftVersion::V_1_21_11 => pumpkin::plugin::player::JavaMinecraftVersion::V12111,
        JavaMinecraftVersion::V_26_1 => pumpkin::plugin::player::JavaMinecraftVersion::V261,
        JavaMinecraftVersion::V_26_2 => pumpkin::plugin::player::JavaMinecraftVersion::V262,
        JavaMinecraftVersion::V_26_3 => pumpkin::plugin::player::JavaMinecraftVersion::V263,
        JavaMinecraftVersion::Unknown => pumpkin::plugin::player::JavaMinecraftVersion::Unknown,
    }
}

const fn to_wasm_chat_mode(
    mode: &pumpkin_core::entity::player::ChatMode,
) -> pumpkin::plugin::player::ChatMode {
    match mode {
        pumpkin_core::entity::player::ChatMode::Enabled => {
            pumpkin::plugin::player::ChatMode::Enabled
        }
        pumpkin_core::entity::player::ChatMode::CommandsOnly => {
            pumpkin::plugin::player::ChatMode::CommandsOnly
        }
        pumpkin_core::entity::player::ChatMode::Hidden => pumpkin::plugin::player::ChatMode::Hidden,
    }
}

impl pumpkin::plugin::player::HostJavaPlayer for PluginHostState {
    fn get_version(
        &mut self,
        player: Resource<pumpkin::plugin::player::JavaPlayer>,
    ) -> wasmtime::Result<pumpkin::plugin::player::JavaMinecraftVersion> {
        let player = self.get(&player)?.clone();

        let client = player
            .client
            .java()
            .ok_or_else(|| wasmtime::Error::msg("Not a java player"))?;
        Ok(to_wasm_java_version(client.version.load()))
    }

    fn get_brand(
        &mut self,
        player: Resource<pumpkin::plugin::player::JavaPlayer>,
    ) -> wasmtime::Result<String> {
        let player = self.get(&player)?.clone();

        let client = player
            .client
            .java()
            .ok_or_else(|| wasmtime::Error::msg("Not a java player"))?;
        Ok(client.brand.load().as_ref().clone().unwrap_or_default())
    }

    fn get_server_address(
        &mut self,
        player: Resource<pumpkin::plugin::player::JavaPlayer>,
    ) -> wasmtime::Result<String> {
        let player = self.get(&player)?.clone();

        let client = player
            .client
            .java()
            .ok_or_else(|| wasmtime::Error::msg("Not a java player"))?;
        Ok(client.server_address.clone())
    }

    fn get_settings(
        &mut self,
        player: Resource<pumpkin::plugin::player::JavaPlayer>,
    ) -> wasmtime::Result<pumpkin::plugin::player::JavaPlayerSettings> {
        let player = self.get(&player)?.clone();

        let config = player.config.load();
        let mask = config.skin_parts;
        let mut parts = SkinParts::empty();
        if mask & 0x01 != 0 {
            parts |= SkinParts::CAPE;
        }
        if mask & 0x02 != 0 {
            parts |= SkinParts::JACKET;
        }
        if mask & 0x04 != 0 {
            parts |= SkinParts::LEFT_SLEEVE;
        }
        if mask & 0x08 != 0 {
            parts |= SkinParts::RIGHT_SLEEVE;
        }
        if mask & 0x10 != 0 {
            parts |= SkinParts::LEFT_PANTS_LEG;
        }
        if mask & 0x20 != 0 {
            parts |= SkinParts::RIGHT_PANTS_LEG;
        }
        if mask & 0x40 != 0 {
            parts |= SkinParts::HAT;
        }

        Ok(pumpkin::plugin::player::JavaPlayerSettings {
            locale: config.locale.clone(),
            view_distance: config.view_distance.get(),
            chat_mode: to_wasm_chat_mode(&config.chat_mode),
            chat_colors: config.chat_colors,
            skin_parts: parts,
            main_hand: match config.main_hand {
                pumpkin_util::Hand::Left => pumpkin::plugin::common::Hand::Left,
                pumpkin_util::Hand::Right => pumpkin::plugin::common::Hand::Right,
            },
            text_filtering: config.text_filtering,
            server_listing: config.server_listing,
        })
    }

    async fn send_packet(
        &mut self,
        player: Resource<pumpkin::plugin::player::JavaPlayer>,
        packet: pumpkin::plugin::java_packets::ClientboundPacket,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?.clone();

        let client = player
            .client
            .java()
            .ok_or_else(|| wasmtime::Error::msg("Not a java player"))?;
        if let Some(bytes) = crate::generated_packets::serialize_java_packet(
            &packet,
            pumpkin_data::packet::CURRENT_MC_VERSION,
        ) {
            client.send_packet_now_data(bytes).await;
        }
        Ok(())
    }

    async fn send_custom_payload(
        &mut self,
        player: Resource<pumpkin::plugin::player::JavaPlayer>,
        channel: String,
        data: Vec<u8>,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?.clone();

        if let pumpkin_core::net::ClientPlatform::Java(_) = player.client.as_ref() {
            player
                .send_client_packet(&pumpkin_protocol::java::client::play::CCustomPayload::new(
                    &channel, &data,
                ))
                .await;
        }
        Ok(())
    }

    fn get_scoreboard(
        &mut self,
        player: Resource<pumpkin::plugin::player::JavaPlayer>,
    ) -> wasmtime::Result<Resource<pumpkin::plugin::scoreboard::Scoreboard>> {
        let player = self.get(&player)?.clone();
        self.add(pumpkin_wasm_host_common::state::ScoreboardProvider::Player(
            player,
        ))
    }

    fn reset_scoreboard(
        &mut self,
        player: Resource<pumpkin::plugin::player::JavaPlayer>,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?.clone();
        player.reset_scoreboard();
        Ok(())
    }

    async fn send_resource_pack(
        &mut self,
        player: Resource<pumpkin::plugin::player::JavaPlayer>,
        pack: pumpkin::plugin::player::JavaResourcePack,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?.clone();

        let uuid = Uuid::from_wit(&pack.id);
        let prompt_message = pack.prompt_message.map(|t| self.take(t)).transpose()?;

        if let pumpkin_core::net::ClientPlatform::Java(client) = player.client.as_ref() {
            client
                .send_packet(
                    &pumpkin_protocol::java::client::play::CAddResourcePack::new(
                        &uuid,
                        &pack.url,
                        &pack.hash,
                        pack.forced,
                        prompt_message,
                    ),
                )
                .await;
        }
        Ok(())
    }

    async fn remove_resource_pack(
        &mut self,
        player: Resource<pumpkin::plugin::player::JavaPlayer>,
        id: pumpkin::plugin::uuid::Uuid,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?.clone();

        let uuid = Uuid::from_wit(&id);

        if let pumpkin_core::net::ClientPlatform::Java(client) = player.client.as_ref() {
            client
                .send_packet(
                    &pumpkin_protocol::java::client::play::CRemoveResourcePack::new(Some(&uuid)),
                )
                .await;
        }
        Ok(())
    }

    async fn clear_resource_packs(
        &mut self,
        player: Resource<pumpkin::plugin::player::JavaPlayer>,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?.clone();

        if let pumpkin_core::net::ClientPlatform::Java(client) = player.client.as_ref() {
            client
                .send_packet(&pumpkin_protocol::java::client::play::CRemoveResourcePack::new(None))
                .await;
        }
        Ok(())
    }

    fn send_game_event(
        &mut self,
        player: Resource<pumpkin::plugin::player::JavaPlayer>,
        event: pumpkin::plugin::player::ClientGameEvent,
        value: f32,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?.clone();

        let internal_event = from_wit_client_game_event(event);
        player.send_game_event(internal_event, value);
        Ok(())
    }

    fn send_entity_status(
        &mut self,
        player: Resource<pumpkin::plugin::player::JavaPlayer>,
        entity_id: i32,
        status: pumpkin::plugin::entity_statuses::EntityStatus,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?.clone();

        // SAFETY: The WIT enum variants and discriminants are 1:1 generated from entity_statuses.json
        let internal_status: pumpkin_data::entity_status::EntityStatus =
            unsafe { std::mem::transmute(status as u8) };
        let packet = pumpkin_protocol::java::client::play::CEntityStatus::new(
            entity_id,
            internal_status as u8 as i8,
        );
        player.try_send_client_packet(&packet);
        Ok(())
    }

    fn drop(&mut self, rep: Resource<pumpkin::plugin::player::JavaPlayer>) -> wasmtime::Result<()> {
        self.drop(rep)
    }
}

impl pumpkin::plugin::player::HostJavaPlayerWithStore<PluginHostState>
    for HasSelf<PluginHostState>
{
    #[allow(clippy::too_many_lines)]
    async fn show_dialog(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<pumpkin::plugin::player::JavaPlayer>,
        dialog: Dialog,
    ) -> wasmtime::Result<()> {
        let (player, protocol_dialog, plugin) = {
            let state = host.get();
            let player = state.get(&player)?.clone();
            let protocol_dialog =
                super::super::events::dialog::protocol_dialog_from_wasm(state, &dialog);
            let plugin = state
                .plugin
                .as_ref()
                .and_then(std::sync::Weak::upgrade)
                .ok_or_else(|| wasmtime::Error::msg("Plugin instance not available"))?;
            (player, protocol_dialog, plugin)
        };

        let operation = async move {
            if let Some(server) = player.world().server.upgrade() {
                let mut event =
                    pumpkin_core::plugin::api::events::dialog::dialog_show::DialogShowEvent::new(
                        player.clone(),
                        protocol_dialog.clone(),
                    );
                server.plugin_manager.fire(&server, &mut event).await;
                if event.cancelled {
                    return Ok(());
                }
            }

            if let pumpkin_core::net::ClientPlatform::Java(client) = player.client.as_ref() {
                match client.connection_state.load() {
                    pumpkin_protocol::ConnectionState::Config => {
                        client
                            .send_packet(
                                &pumpkin_protocol::java::client::config::CConfigShowDialog::new(
                                    pumpkin_protocol::IdOr::Value(DialogNBT::from_dialog(
                                        &protocol_dialog,
                                    )),
                                ),
                            )
                            .await;
                    }
                    pumpkin_protocol::ConnectionState::Play => {
                        client
                            .send_packet(
                                &pumpkin_protocol::java::client::play::CPlayShowDialog::new(
                                    pumpkin_protocol::IdOr::Value(DialogNBT::from_dialog(
                                        &protocol_dialog,
                                    )),
                                ),
                            )
                            .await;
                    }
                    _ => {}
                }
            }

            Ok(())
        };

        plugin.store.pump_reentry(&mut host, operation).await?
    }

    async fn clear_dialog(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<pumpkin::plugin::player::JavaPlayer>,
    ) -> wasmtime::Result<()> {
        let (player, plugin) = {
            let state = host.get();
            let player = state.get(&player)?.clone();
            let plugin = state
                .plugin
                .as_ref()
                .and_then(std::sync::Weak::upgrade)
                .ok_or_else(|| wasmtime::Error::msg("Plugin instance not available"))?;
            (player, plugin)
        };

        let operation = async move {
            if let Some(server) = player.world().server.upgrade() {
                let mut event =
                    pumpkin_core::plugin::api::events::dialog::dialog_clear::DialogClearEvent::new(
                        player.clone(),
                    );
                server.plugin_manager.fire(&server, &mut event).await;
                if event.cancelled {
                    return Ok(());
                }
            }

            if let pumpkin_core::net::ClientPlatform::Java(client) = player.client.as_ref() {
                match client.connection_state.load() {
                    pumpkin_protocol::ConnectionState::Config => {
                        client
                            .send_packet(
                                &pumpkin_protocol::java::client::config::CConfigClearDialog::new(),
                            )
                            .await;
                    }
                    pumpkin_protocol::ConnectionState::Play => {
                        client
                            .send_packet(
                                &pumpkin_protocol::java::client::play::CPlayClearDialog::new(),
                            )
                            .await;
                    }
                    _ => {}
                }
            }

            Ok(())
        };

        plugin.store.pump_reentry(&mut host, operation).await?
    }

    async fn kick(
        mut host: Access<'_, PluginHostState, Self>,
        player: Resource<pumpkin::plugin::player::JavaPlayer>,
        options: pumpkin::plugin::player::JavaKickOptions,
    ) -> wasmtime::Result<()> {
        let (player, reason, plugin) = {
            let state = host.get();
            let player = state.get(&player)?.clone();
            let reason = state.take(options.reason)?;
            let plugin = state
                .plugin
                .as_ref()
                .and_then(std::sync::Weak::upgrade)
                .ok_or_else(|| wasmtime::Error::msg("Plugin instance not available"))?;
            (player, reason, plugin)
        };

        if options.log_to_console {
            tracing::info!(
                "Kicking Java player {} ({}): {}",
                player.gameprofile.name,
                player.gameprofile.id,
                reason.clone().to_pretty_console()
            );
        }

        let operation = async move {
            if let Some(server) = player.world().server.upgrade()
                && let Some(player_arc) = player.world().get_player_by_uuid(player.gameprofile.id)
            {
                let mut event =
                    pumpkin_core::plugin::api::events::player::player_kick::PlayerKickEvent::new(
                        player_arc,
                        reason.clone().to_pretty_console(),
                    );
                server.plugin_manager.fire(&server, &mut event).await;
                if event.cancelled {
                    return Ok(());
                }
            }

            let send_packet = options.teardown_policy
                != pumpkin::plugin::player::SocketTeardownPolicy::DropConnection;
            if let pumpkin_core::net::ClientPlatform::Java(java) = player.client.as_ref() {
                java.kick_explicit(&reason, send_packet).await;
            }

            Ok(())
        };

        plugin.store.pump_reentry(&mut host, operation).await?
    }
}
