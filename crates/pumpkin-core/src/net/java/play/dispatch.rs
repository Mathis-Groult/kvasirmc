//! Routing of serverbound play packets to their handlers.
//!
//! `JavaClient::handle_play_packet` asks each router in turn; a router returns `Ok(false)` when
//! the packet id is not one of its own. Packet ids are unique within one protocol version (see
//! the test below), so the order of the routers decides nothing but the number of comparisons.

use std::sync::Arc;

use bytes::Bytes;
use pumpkin_data::packet::CURRENT_MC_VERSION;
use pumpkin_protocol::ServerPacket;
use pumpkin_protocol::java::server::play::{
    SAttack, SBlockEntityTagQuery, SBundleItemSelected, SChangeDifficulty, SChangeGameMode,
    SChatAck, SChatCommand, SChatCommandSigned, SChatMessage, SChunkBatch, SClickSlot,
    SClientCommand, SClientInformationPlay, SClientTickEnd, SCloseContainer, SCommandSuggestion,
    SConfigurationAcknowledged, SConfirmTeleport, SContainerButtonClick,
    SContainerSlotStateChanged, SCookieResponse as SPCookieResponse, SCustomPayload,
    SDebugSampleSubscription, SDebugSubscriptionRequest, SEditBook, SEntityTagQuery, SInteract,
    SJigsawGenerate, SLockDifficulty, SMoveVehicle, SPaddleBoat, SPickItemFromBlock, SPlaceRecipe,
    SPlayPingRequest, SPlayPong, SPlayResourcePack, SPlayerAbilities, SPlayerAction,
    SPlayerCommand, SPlayerInput, SPlayerLoaded, SPlayerPosition, SPlayerPositionRotation,
    SPlayerRotation, SPlayerSession, SRecipeBookChangeSettings, SRecipeBookSeenRecipe, SRenameItem,
    SSeenAdvancement, SSelectTrade, SSetBeacon, SSetCommandBlock, SSetCommandMinecart,
    SSetCreativeSlot, SSetGameRule, SSetHeldItem, SSetJigsawBlock, SSetPlayerGround,
    SSetStructureBlock, SSetTestBlock, SSpectateEntity, SSwingArm, STeleportToEntity,
    STestInstanceBlockAction, SUpdateSign, SUseItem, SUseItemOn,
};
use pumpkin_protocol::packet::MultiVersionJavaPacket;
use tracing::warn;

use crate::entity::player::Player;
use crate::error::PumpkinError;
use crate::net::ClientPlatform;
use crate::net::java::JavaClient;
use crate::plugin::player::player_custom_payload::PlayerCustomPayloadEvent;
use crate::server::Server;

impl JavaClient {
    /// Hands a play packet (already through the plugins' packet event) to its handler.
    ///
    /// The multiversion plugin has converted older clients' packets to 26.3 by now, so every
    /// router reads with `CURRENT_MC_VERSION`.
    pub(in crate::net::java) fn dispatch_play_packet(
        &self,
        player: &Arc<Player>,
        server: &Arc<Server>,
        packet_id: i32,
        payload: &[u8],
    ) -> Result<(), Box<dyn PumpkinError>> {
        let routed = self.route_movement(player, server, packet_id, payload)?
            || self.route_connection(player, server, packet_id, payload)?
            || self.route_chat(player, server, packet_id, payload)?
            || self.route_actions(player, server, packet_id, payload)?
            || self.route_containers(player, server, packet_id, payload)?
            || self.route_screens(player, server, packet_id, payload)?
            || self.route_operator(player, server, packet_id, payload)?
            || self.route_editor_blocks(player, packet_id, payload)?;
        if !routed {
            warn!("Failed to handle player packet id {}", packet_id);
        }
        Ok(())
    }

    /// Movement, vehicles, abilities and camera.
    fn route_movement(
        &self,
        player: &Arc<Player>,
        server: &Arc<Server>,
        packet_id: i32,
        mut payload: &[u8],
    ) -> Result<bool, Box<dyn PumpkinError>> {
        let version = CURRENT_MC_VERSION;
        match packet_id {
            id if id == SConfirmTeleport::to_id(version) => {
                self.handle_confirm_teleport(
                    player,
                    &SConfirmTeleport::read(&mut payload, &version)?,
                );
            }
            id if id == SPlayerInput::to_id(version) => {
                self.handle_player_input(
                    player,
                    &SPlayerInput::read(&mut payload, &version)?,
                    server,
                );
            }
            id if id == SMoveVehicle::to_id(version) => {
                self.handle_move_vehicle(player, &SMoveVehicle::read(&mut payload, &version)?);
            }
            id if id == SPaddleBoat::to_id(version) => {
                self.handle_paddle_boat(player, &SPaddleBoat::read(&mut payload, &version)?);
            }
            id if id == STeleportToEntity::to_id(version) => {
                self.handle_teleport_to_entity(
                    player,
                    &STeleportToEntity::read(&mut payload, &version)?,
                    server,
                );
            }
            id if id == SPlayerPosition::to_id(version) => {
                self.handle_position(
                    player,
                    server,
                    &SPlayerPosition::read(&mut payload, &version)?,
                );
            }
            id if id == SPlayerPositionRotation::to_id(version) => {
                self.handle_position_rotation(
                    player,
                    server,
                    &SPlayerPositionRotation::read(&mut payload, &version)?,
                );
            }
            id if id == SPlayerRotation::to_id(version) => {
                self.handle_rotation(player, &SPlayerRotation::read(&mut payload, &version)?);
            }
            id if id == SSetPlayerGround::to_id(version) => {
                self.handle_player_ground(player, &SSetPlayerGround::read(&mut payload, &version)?);
            }
            id if id == SPlayerAbilities::to_id(version) => {
                self.handle_player_abilities(
                    player,
                    &SPlayerAbilities::read(&mut payload, &version)?,
                    server,
                );
            }
            id if id == SPlayerCommand::to_id(version) => {
                self.handle_player_command(
                    player,
                    &SPlayerCommand::read(&mut payload, &version)?,
                    server,
                );
            }
            id if id == SSpectateEntity::to_id(version) => {
                self.handle_spectate_entity(
                    player,
                    &SSpectateEntity::read(&mut payload, &version)?,
                );
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Connection upkeep: settings, keep-alive, pings, chunk batches, resource packs.
    fn route_connection(
        &self,
        player: &Arc<Player>,
        server: &Arc<Server>,
        packet_id: i32,
        mut payload: &[u8],
    ) -> Result<bool, Box<dyn PumpkinError>> {
        let version = CURRENT_MC_VERSION;
        match packet_id {
            id if id == SClientInformationPlay::to_id(version) => {
                self.handle_client_information(
                    server,
                    player,
                    &SClientInformationPlay::read(&mut payload, &version)?,
                );
            }
            id if id == SClientCommand::to_id(version) => {
                self.handle_client_status(player, &SClientCommand::read(&mut payload, &version)?);
            }
            id if id == pumpkin_protocol::java::server::play::SKeepAlive::to_id(version) => {
                self.handle_keep_alive(
                    player,
                    &pumpkin_protocol::java::server::play::SKeepAlive::read(
                        &mut payload,
                        &version,
                    )?,
                );
            }
            id if id == SClientTickEnd::to_id(version) => {
                self.handle_client_tick_end(player);
            }
            id if id == SPlayerLoaded::to_id(version) => {
                Self::handle_player_loaded(player);
            }
            id if id == SPlayPingRequest::to_id(version) => {
                self.handle_play_ping_request(&SPlayPingRequest::read(&mut payload, &version)?);
            }
            id if id == SPCookieResponse::to_id(version) => {
                self.handle_cookie_response(&SPCookieResponse::read(&mut payload, &version)?);
            }
            id if id == SChunkBatch::to_id(version) => {
                self.handle_chunk_batch(player, &SChunkBatch::read(&mut payload, &version)?);
            }
            id if id == SPlayResourcePack::to_id(version) => {
                self.handle_play_resource_pack_response(
                    server,
                    player,
                    &SPlayResourcePack::read(&mut payload, &version)?,
                );
            }
            id if id == SPlayPong::to_id(version) => {
                self.handle_play_pong(player, &SPlayPong::read(&mut payload, &version)?);
            }
            id if id == SConfigurationAcknowledged::to_id(version) => {
                self.handle_configuration_acknowledged(player);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Chat, commands, chat sessions and plugin channels.
    fn route_chat(
        &self,
        player: &Arc<Player>,
        server: &Arc<Server>,
        packet_id: i32,
        mut payload: &[u8],
    ) -> Result<bool, Box<dyn PumpkinError>> {
        let version = CURRENT_MC_VERSION;
        match packet_id {
            id if id == SChatAck::to_id(version) => {
                let packet = SChatAck::read(&mut payload, &version)?;
                self.handle_chat_ack(player, &packet);
            }
            id if id == SChatCommand::to_id(version) => {
                let packet = SChatCommand::read(&mut payload, &version)?;
                spawn_chat_command(player, server, packet.command.to_string());
            }
            id if id == SChatCommandSigned::to_id(version) => {
                let mut signed_payload = payload;
                let cmd =
                    if let Ok(signed) = SChatCommandSigned::read(&mut signed_payload, &version) {
                        signed.command.to_string()
                    } else {
                        SChatCommand::read(&mut payload, &version)?
                            .command
                            .to_string()
                    };
                spawn_chat_command(player, server, cmd);
            }
            id if id == SChatMessage::to_id(version) => {
                let packet = SChatMessage::read(&mut payload, &version)?;
                spawn_chat_message(player, server, &packet);
            }
            id if id == SCommandSuggestion::to_id(version) => {
                self.handle_command_suggestion(
                    player,
                    &SCommandSuggestion::read(&mut payload, &version)?,
                    server,
                );
            }
            id if id == SPlayerSession::to_id(version) => {
                let session = SPlayerSession::read(&mut payload, &version)?;
                spawn_chat_session_update(player, server, session);
            }
            id if id == SCustomPayload::to_id(version) => {
                let payload = SCustomPayload::read(&mut payload, &version)?;
                fire_custom_payload_events(player, server, &payload);
            }
            id if id
                == pumpkin_protocol::java::server::play::SCustomClickAction::to_id(version) =>
            {
                let packet = pumpkin_protocol::java::server::play::SCustomClickAction::read(
                    &mut payload,
                    &version,
                )?;
                let mut event = crate::plugin::api::events::dialog::dialog_click_action::DialogClickActionEvent::new(
                    player.clone(),
                    packet.action_id.to_string(),
                    packet.payload.map(Bytes::copy_from_slice),
                );
                server.plugin_manager.fire_blocking(server, &mut event);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Actions on the world and on entities: attacks, use, mining, signs, books.
    fn route_actions(
        &self,
        player: &Arc<Player>,
        server: &Arc<Server>,
        packet_id: i32,
        mut payload: &[u8],
    ) -> Result<bool, Box<dyn PumpkinError>> {
        let version = CURRENT_MC_VERSION;
        match packet_id {
            id if id == SInteract::to_id(version) => {
                self.handle_interact(player, &SInteract::read(&mut payload, &version)?, server);
            }
            id if id == SAttack::to_id(version) => {
                self.handle_attack(player, &SAttack::read(&mut payload, &version)?, server);
            }
            id if id == SPickItemFromBlock::to_id(version) => {
                self.handle_pick_item_from_block(
                    player,
                    &SPickItemFromBlock::read(&mut payload, &version)?,
                );
            }
            id if id
                == pumpkin_protocol::java::server::play::SPickItemFromEntity::to_id(version) =>
            {
                self.handle_pick_item_from_entity(
                    player,
                    &pumpkin_protocol::java::server::play::SPickItemFromEntity::read(
                        &mut payload,
                        &version,
                    )?,
                );
            }
            id if id == SPlayerAction::to_id(version) => {
                self.handle_player_action(
                    player,
                    &SPlayerAction::read(&mut payload, &version)?,
                    server,
                );
            }
            id if id == SSwingArm::to_id(version) => {
                self.handle_swing_arm(server, player, &SSwingArm::read(&mut payload, &version)?);
            }
            id if id == SUpdateSign::to_id(version) => {
                self.handle_sign_update(player, &SUpdateSign::read(&mut payload, &version)?);
            }
            id if id == SEditBook::to_id(version) => {
                self.handle_edit_book(player, &SEditBook::read(&mut payload, &version)?);
            }
            id if id == SUseItemOn::to_id(version) => {
                self.handle_use_item_on(
                    player,
                    &SUseItemOn::read(&mut payload, &version)?,
                    server,
                )?;
            }
            id if id == SUseItem::to_id(version) => {
                self.handle_use_item(player, &SUseItem::read(&mut payload, &version)?, server);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Inventory and container slots.
    fn route_containers(
        &self,
        player: &Arc<Player>,
        server: &Arc<Server>,
        packet_id: i32,
        mut payload: &[u8],
    ) -> Result<bool, Box<dyn PumpkinError>> {
        let version = CURRENT_MC_VERSION;
        match packet_id {
            id if id == SBundleItemSelected::to_id(version) => {
                self.handle_bundle_item_selected(
                    player,
                    &SBundleItemSelected::read(&mut payload, &version)?,
                );
            }
            id if id == SClickSlot::to_id(version) => {
                player.on_slot_click(SClickSlot::read(&mut payload, &version)?, server);
            }
            id if id == SContainerButtonClick::to_id(version) => {
                player.on_container_button_click(&SContainerButtonClick::read(
                    &mut payload,
                    &version,
                )?);
            }
            id if id == SSetHeldItem::to_id(version) => {
                self.handle_set_held_item(
                    server,
                    player,
                    &SSetHeldItem::read(&mut payload, &version)?,
                );
            }
            id if id == SSetCreativeSlot::to_id(version) => {
                self.handle_set_creative_slot(
                    player,
                    SSetCreativeSlot::read(&mut payload, &version)?,
                )?;
            }
            id if id == SCloseContainer::to_id(version) => {
                let _ = SCloseContainer::read(&mut payload, &version)?;
                self.handle_close_container(player);
            }
            id if id == SContainerSlotStateChanged::to_id(version) => {
                self.handle_container_slot_state_changed(
                    player,
                    &SContainerSlotStateChanged::read(&mut payload, &version)?,
                );
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Recipe book, anvil, villager trades, advancements tab, beacon.
    fn route_screens(
        &self,
        player: &Arc<Player>,
        server: &Arc<Server>,
        packet_id: i32,
        mut payload: &[u8],
    ) -> Result<bool, Box<dyn PumpkinError>> {
        let version = CURRENT_MC_VERSION;
        match packet_id {
            id if id == SRecipeBookChangeSettings::to_id(version) => {
                self.handle_recipe_book_change_settings(
                    server,
                    player,
                    &SRecipeBookChangeSettings::read(&mut payload, &version)?,
                );
            }
            id if id == SRecipeBookSeenRecipe::to_id(version) => {
                self.handle_recipe_book_seen_recipe(
                    server,
                    player,
                    &SRecipeBookSeenRecipe::read(&mut payload, &version)?,
                );
            }
            id if id == SRenameItem::to_id(version) => {
                player.on_rename_item(&SRenameItem::read(&mut payload, &version)?);
            }
            id if id == SPlaceRecipe::to_id(version) => {
                let packet = SPlaceRecipe::read(&mut payload, &version)?;
                self.handle_place_recipe(server, player, &packet);
            }
            id if id == SSelectTrade::to_id(version) => {
                self.handle_select_trade(player, &SSelectTrade::read(&mut payload, &version)?);
            }
            id if id == SSeenAdvancement::to_id(version) => {
                self.handle_seen_advancement(
                    player,
                    &SSeenAdvancement::read(&mut payload, &version)?,
                );
            }
            id if id == SSetBeacon::to_id(version) => {
                self.handle_set_beacon(player, &SSetBeacon::read(&mut payload, &version)?);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Operator and debug requests: game mode, difficulty, game rules, queries.
    fn route_operator(
        &self,
        player: &Arc<Player>,
        server: &Arc<Server>,
        packet_id: i32,
        mut payload: &[u8],
    ) -> Result<bool, Box<dyn PumpkinError>> {
        let version = CURRENT_MC_VERSION;
        match packet_id {
            id if id == SChangeGameMode::to_id(version) => {
                self.handle_change_game_mode(
                    player,
                    &SChangeGameMode::read(&mut payload, &version)?,
                );
            }
            id if id == SDebugSubscriptionRequest::to_id(version) => {
                self.handle_debug_subscription_request(
                    player,
                    &SDebugSubscriptionRequest::read(&mut payload, &version)?,
                );
            }
            id if id == SDebugSampleSubscription::to_id(version) => {
                self.handle_debug_sample_subscription(
                    player,
                    &SDebugSampleSubscription::read(&mut payload, &version)?,
                );
            }
            id if id == SLockDifficulty::to_id(version) => {
                self.handle_lock_difficulty(
                    server,
                    player,
                    &SLockDifficulty::read(&mut payload, &version)?,
                );
            }
            id if id == SChangeDifficulty::to_id(version) => {
                self.handle_change_difficulty(
                    server,
                    player,
                    &SChangeDifficulty::read(&mut payload, &version)?,
                );
            }
            id if id == SSetGameRule::to_id(version) => {
                self.handle_set_game_rule(player, &SSetGameRule::read(&mut payload, &version)?);
            }
            id if id == SBlockEntityTagQuery::to_id(version) => {
                self.handle_block_entity_tag_query(
                    player,
                    &SBlockEntityTagQuery::read(&mut payload, &version)?,
                );
            }
            id if id == SEntityTagQuery::to_id(version) => {
                self.handle_entity_tag_query(
                    player,
                    &SEntityTagQuery::read(&mut payload, &version)?,
                );
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Editor blocks: test blocks, command blocks, jigsaws, structure blocks.
    fn route_editor_blocks(
        &self,
        player: &Arc<Player>,
        packet_id: i32,
        mut payload: &[u8],
    ) -> Result<bool, Box<dyn PumpkinError>> {
        let version = CURRENT_MC_VERSION;
        match packet_id {
            id if id == STestInstanceBlockAction::to_id(version) => {
                self.handle_test_instance_block_action(
                    player,
                    &STestInstanceBlockAction::read(&mut payload, &version)?,
                );
            }
            id if id == SSetTestBlock::to_id(version) => {
                self.handle_set_test_block(player, &SSetTestBlock::read(&mut payload, &version)?);
            }
            id if id == SSetCommandBlock::to_id(version) => {
                self.handle_set_command_block(
                    player,
                    &SSetCommandBlock::read(&mut payload, &version)?,
                );
            }
            id if id == SSetJigsawBlock::to_id(version) => {
                self.handle_set_jigsaw_block(
                    player,
                    &SSetJigsawBlock::read(&mut payload, &version)?,
                );
            }
            id if id == SJigsawGenerate::to_id(version) => {
                self.handle_jigsaw_generate(
                    player,
                    &SJigsawGenerate::read(&mut payload, &version)?,
                );
            }
            id if id == SSetCommandMinecart::to_id(version) => {
                self.handle_set_command_minecart(
                    player,
                    &SSetCommandMinecart::read(&mut payload, &version)?,
                );
            }
            id if id == SSetStructureBlock::to_id(version) => {
                self.handle_set_structure_block(
                    player,
                    &SSetStructureBlock::read(&mut payload, &version)?,
                );
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}

/// Runs a chat command on the server's task pool (commands may wait on the world).
fn spawn_chat_command(player: &Arc<Player>, server: &Arc<Server>, cmd: String) {
    let client_platform = player.client.clone();
    let player_c = player.clone();
    let server_c = server.clone();
    server.spawn_task(async move {
        if let ClientPlatform::Java(client) = client_platform.as_ref() {
            let packet = SChatCommand { command: &cmd };
            client
                .handle_chat_command(&player_c, &server_c, &packet)
                .await;
        }
    });
}

/// Copies a chat message out of the packet buffer and handles it on the server's task pool.
fn spawn_chat_message(player: &Arc<Player>, server: &Arc<Server>, packet: &SChatMessage<'_>) {
    let msg = packet.message.to_string();
    let signature = packet.signature.map(<[u8]>::to_vec);
    let ack = packet.acknowledged.to_vec();
    let ts = packet.timestamp;
    let salt = packet.salt;
    let count = packet.message_count;
    let checksum = packet.checksum;
    let client_platform = player.client.clone();
    let player_c = player.clone();
    let server_c = server.clone();
    server.spawn_task(async move {
        if let ClientPlatform::Java(client) = client_platform.as_ref() {
            let packet = SChatMessage {
                message: &msg,
                timestamp: ts,
                salt,
                signature: signature.as_deref(),
                message_count: count,
                acknowledged: &ack,
                checksum,
            };
            client
                .handle_chat_message(&server_c, &player_c, packet)
                .await;
        }
    });
}

/// Handles a new chat session (signing key) on the server's task pool.
fn spawn_chat_session_update(player: &Arc<Player>, server: &Arc<Server>, session: SPlayerSession) {
    let client_platform = player.client.clone();
    let player_c = player.clone();
    let server_c = server.clone();
    server.spawn_task(async move {
        if let ClientPlatform::Java(client) = client_platform.as_ref() {
            client
                .handle_chat_session_update(&player_c, &server_c, session)
                .await;
        }
    });
}

/// Fires the plugin events of a custom payload, plus one per channel (un)registered by it.
fn fire_custom_payload_events(
    player: &Arc<Player>,
    server: &Arc<Server>,
    payload: &SCustomPayload<'_>,
) {
    let channel_str = payload.channel.to_string();
    let mut event = PlayerCustomPayloadEvent::new(
        player.clone(),
        channel_str.clone(),
        Bytes::copy_from_slice(payload.data),
    );
    server.plugin_manager.fire_blocking(server, &mut event);

    if channel_str == "minecraft:register" {
        if let Ok(channels_data) = std::str::from_utf8(payload.data) {
            for ch in channels_data.split('\0') {
                if !ch.is_empty() {
                    let mut reg_event = crate::plugin::api::events::player::player_register_channel::PlayerRegisterChannelEvent::new(
                        player.clone(),
                        ch.to_string(),
                    );
                    server.plugin_manager.fire_blocking(server, &mut reg_event);
                    let mut ch_event =
                        crate::plugin::api::events::player::player_channel::PlayerChannelEvent {
                            player: player.clone(),
                            channel: ch.to_string(),
                            cancelled: false,
                        };
                    server.plugin_manager.fire_blocking(server, &mut ch_event);
                }
            }
        }
    } else if channel_str == "minecraft:unregister"
        && let Ok(channels_data) = std::str::from_utf8(payload.data)
    {
        for ch in channels_data.split('\0') {
            if !ch.is_empty() {
                let mut unreg_event = crate::plugin::api::events::player::player_unregister_channel::PlayerUnregisterChannelEvent::new(
                    player.clone(),
                    ch.to_string(),
                );
                server
                    .plugin_manager
                    .fire_blocking(server, &mut unreg_event);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pumpkin_util::version::JavaMinecraftVersion;

    use super::*;

    type IdOf = fn(JavaMinecraftVersion) -> i32;

    /// Every packet the routers handle, with the function that gives its id.
    const ROUTED: [(&str, IdOf); 70] = [
        ("SConfirmTeleport", SConfirmTeleport::to_id),
        ("SChangeGameMode", SChangeGameMode::to_id),
        ("SChatAck", SChatAck::to_id),
        ("SChatCommand", SChatCommand::to_id),
        ("SChatCommandSigned", SChatCommandSigned::to_id),
        ("SChatMessage", SChatMessage::to_id),
        ("SClientInformationPlay", SClientInformationPlay::to_id),
        ("SClientCommand", SClientCommand::to_id),
        ("SPlayerInput", SPlayerInput::to_id),
        ("SMoveVehicle", SMoveVehicle::to_id),
        ("SPaddleBoat", SPaddleBoat::to_id),
        ("SInteract", SInteract::to_id),
        ("SBundleItemSelected", SBundleItemSelected::to_id),
        ("SAttack", SAttack::to_id),
        ("STeleportToEntity", STeleportToEntity::to_id),
        (
            "SKeepAlive",
            pumpkin_protocol::java::server::play::SKeepAlive::to_id,
        ),
        ("SClientTickEnd", SClientTickEnd::to_id),
        ("STestInstanceBlockAction", STestInstanceBlockAction::to_id),
        ("SSetTestBlock", SSetTestBlock::to_id),
        (
            "SDebugSubscriptionRequest",
            SDebugSubscriptionRequest::to_id,
        ),
        ("SDebugSampleSubscription", SDebugSampleSubscription::to_id),
        ("SPlayerPosition", SPlayerPosition::to_id),
        ("SPlayerPositionRotation", SPlayerPositionRotation::to_id),
        ("SPlayerRotation", SPlayerRotation::to_id),
        ("SSetPlayerGround", SSetPlayerGround::to_id),
        ("SPickItemFromBlock", SPickItemFromBlock::to_id),
        (
            "SPickItemFromEntity",
            pumpkin_protocol::java::server::play::SPickItemFromEntity::to_id,
        ),
        ("SPlayerAbilities", SPlayerAbilities::to_id),
        ("SPlayerAction", SPlayerAction::to_id),
        ("SSetCommandBlock", SSetCommandBlock::to_id),
        ("SSetJigsawBlock", SSetJigsawBlock::to_id),
        ("SJigsawGenerate", SJigsawGenerate::to_id),
        ("SPlayerCommand", SPlayerCommand::to_id),
        ("SPlayerLoaded", SPlayerLoaded::to_id),
        ("SPlayPingRequest", SPlayPingRequest::to_id),
        ("SClickSlot", SClickSlot::to_id),
        ("SContainerButtonClick", SContainerButtonClick::to_id),
        ("SSetHeldItem", SSetHeldItem::to_id),
        ("SSetCreativeSlot", SSetCreativeSlot::to_id),
        ("SSwingArm", SSwingArm::to_id),
        ("SUpdateSign", SUpdateSign::to_id),
        ("SEditBook", SEditBook::to_id),
        ("SUseItemOn", SUseItemOn::to_id),
        ("SUseItem", SUseItem::to_id),
        ("SCommandSuggestion", SCommandSuggestion::to_id),
        ("SPCookieResponse", SPCookieResponse::to_id),
        ("SCloseContainer", SCloseContainer::to_id),
        ("SChunkBatch", SChunkBatch::to_id),
        ("SPlayerSession", SPlayerSession::to_id),
        ("SCustomPayload", SCustomPayload::to_id),
        (
            "SRecipeBookChangeSettings",
            SRecipeBookChangeSettings::to_id,
        ),
        ("SRecipeBookSeenRecipe", SRecipeBookSeenRecipe::to_id),
        ("SRenameItem", SRenameItem::to_id),
        ("SPlaceRecipe", SPlaceRecipe::to_id),
        (
            "SCustomClickAction",
            pumpkin_protocol::java::server::play::SCustomClickAction::to_id,
        ),
        ("SSelectTrade", SSelectTrade::to_id),
        ("SSeenAdvancement", SSeenAdvancement::to_id),
        ("SPlayResourcePack", SPlayResourcePack::to_id),
        ("SPlayPong", SPlayPong::to_id),
        ("SLockDifficulty", SLockDifficulty::to_id),
        ("SChangeDifficulty", SChangeDifficulty::to_id),
        ("SSetBeacon", SSetBeacon::to_id),
        (
            "SContainerSlotStateChanged",
            SContainerSlotStateChanged::to_id,
        ),
        ("SSpectateEntity", SSpectateEntity::to_id),
        ("SSetCommandMinecart", SSetCommandMinecart::to_id),
        ("SSetStructureBlock", SSetStructureBlock::to_id),
        ("SSetGameRule", SSetGameRule::to_id),
        ("SBlockEntityTagQuery", SBlockEntityTagQuery::to_id),
        ("SEntityTagQuery", SEntityTagQuery::to_id),
        (
            "SConfigurationAcknowledged",
            SConfigurationAcknowledged::to_id,
        ),
    ];

    /// The routers are asked in turn, which equals the old single `match` (first matching arm
    /// wins) only while no two routed packets share an id, or while the packets sharing one sit
    /// in the same router in their old order. In 26.3 exactly one pair shares id 23:
    /// `SDebugSampleSubscription` was already unreachable behind `SDebugSubscriptionRequest`, and
    /// `route_operator` keeps the request first. A new pair must be placed the same way.
    #[test]
    fn routed_play_packet_ids_are_unique_but_one_known_pair() {
        let mut first_with_id = HashMap::new();
        let mut shared = Vec::new();
        for (name, to_id) in ROUTED {
            let id = to_id(CURRENT_MC_VERSION);
            if let Some(first) = first_with_id.insert(id, name) {
                shared.push((first, name, id));
            }
        }
        assert_eq!(
            shared,
            [("SDebugSubscriptionRequest", "SDebugSampleSubscription", 23)]
        );
    }
}
