//! Sending packets and the client's loading state.

use super::{BedrockPlayer, JavaPlayer, Player};
use crate::log_at_level;
use crate::net::ClientPlatform;
use crate::plugin::server::packet::PacketSentEvent;
use bytes::Bytes;
use core::f32;
use pumpkin_protocol::java::client::play::{
    CAcknowledgeBlockChange, CGameEvent, CPlayServerLinks, CSetCamera,
};
use pumpkin_util::text::TextComponent;
use pumpkin_util::version::JavaMinecraftVersion;
use std::sync::Arc;
use std::sync::atomic::Ordering;

impl Player {
    pub fn send_game_event(
        &self,
        event: pumpkin_protocol::java::client::play::GameEvent,
        value: f32,
    ) {
        let packet = CGameEvent::new(event, value);
        self.try_send_client_packet(&packet);
    }

    /// Sends custom server links to the player (displayed in the client Esc pause menu).
    pub fn set_server_links(&self, links: &[pumpkin_protocol::Link<'_>]) {
        if let Some(player_arc) = self.world().get_player_by_uuid(self.gameprofile.id)
            && let Some(server) = self.world().server.upgrade()
        {
            let link_strings = links.iter().map(|l| l.url.clone()).collect();
            let mut event =
                crate::plugin::api::events::player::player_links_send::PlayerLinksSendEvent::new(
                    player_arc,
                    link_strings,
                );
            server.plugin_manager.fire_blocking(&server, &mut event);
            if event.cancelled {
                return;
            }
        }
        let packet = CPlayServerLinks::new(links);
        self.try_send_client_packet(&packet);
    }

    pub fn process_inbound_packets(&self) {
        const MAX_PACKETS_PER_TICK: usize = 64;

        // Player::tick runs after the world's block-update flush. Acknowledge the previous tick's
        // predictions here so Java clients receive the authoritative block states before resolving
        // those predictions. Sending the ACK from the packet loop would make doors and other
        // predicted blocks briefly revert because their updates are not flushed until the next tick.
        if let ClientPlatform::Java(client) = self.client.as_ref() {
            let seq = client.packet_sequence.swap(-1, Ordering::Relaxed);
            if seq != -1 {
                client.try_send_packet(&CAcknowledgeBlockChange::new(seq.into()));
            }
        }

        let Some(player_arc) = self.world().get_player_by_uuid(self.gameprofile.id) else {
            return;
        };
        let Some(server_arc) = self.world().server.upgrade() else {
            return;
        };

        let mut count = 0;

        while let Some(packet) = self.inbound_packets.pop() {
            if self.client.closed() {
                break;
            }

            match self.client.as_ref() {
                ClientPlatform::Java(client) => {
                    if let Err(e) = client.handle_play_packet(&player_arc, &server_arc, &packet) {
                        if e.is_kick() {
                            if let Some(kick_reason) = e.client_kick_reason() {
                                client.try_kick(&TextComponent::text(kick_reason));
                            } else {
                                client.try_kick(&TextComponent::text(format!(
                                    "Error while handling incoming packet {e}"
                                )));
                            }
                        }
                        log_at_level!(
                            e.severity(),
                            "Failed to handle play packet id {} (payload {} bytes): {}",
                            packet.id,
                            packet.payload.len(),
                            e
                        );
                    }
                }
                ClientPlatform::Bedrock(client) => {
                    let cancelled = if server_arc
                        .plugin_manager
                        .has_handlers::<crate::plugin::server::packet::PacketReceivedEvent>(
                    ) {
                        let mut event = crate::plugin::server::packet::PacketReceivedEvent::new(
                            player_arc.clone(),
                            packet.id,
                            packet.payload.clone(),
                        );
                        server_arc
                            .plugin_manager
                            .fire_blocking(&server_arc, &mut event);
                        event.cancelled
                    } else {
                        false
                    };
                    if !cancelled
                        && let Err(err) =
                            client.handle_play_packet(&player_arc, &server_arc, &packet)
                    {
                        tracing::error!("Failed to handle Bedrock play packet: {err}");
                    }
                }
            }

            count += 1;
            if count >= MAX_PACKETS_PER_TICK {
                break;
            }
        }
    }

    #[must_use]
    pub fn supports_player_loaded(&self) -> bool {
        match self.client.as_ref() {
            ClientPlatform::Java(client) => client.version.load() >= JavaMinecraftVersion::V_1_21_4,
            ClientPlatform::Bedrock(_) => true,
        }
    }

    #[must_use]
    pub fn has_client_loaded(&self) -> bool {
        if !self.supports_player_loaded() {
            return true;
        }
        self.client_loaded.load(Ordering::Relaxed)
            || self.client_loaded_timeout.load(Ordering::Relaxed) == 0
    }

    pub fn set_client_loaded(&self, loaded: bool) {
        if !self.supports_player_loaded() {
            self.client_loaded.store(true, Ordering::Relaxed);
            self.client_loaded_timeout.store(0, Ordering::Relaxed);
            return;
        }
        if !loaded {
            self.client_loaded_timeout.store(60, Ordering::Relaxed);
        }
        self.client_loaded.store(loaded, Ordering::Relaxed);
    }

    pub async fn fire_packet_sent<P: Send + Sync + std::any::Any>(
        self: &Arc<Self>,
        packet: P,
        packet_id: i32,
        payload: Bytes,
    ) -> bool {
        let server = self.world().server.upgrade();
        if let Some(server) = server {
            let mut event =
                PacketSentEvent::new(self.clone(), packet_id, payload, Arc::new(packet));
            server.plugin_manager.fire(&server, &mut event).await;
            return event.cancelled;
        }
        false
    }

    pub(crate) async fn fire_packet_sent_event_no_obj(
        self: &Arc<Self>,
        packet_id: i32,
        payload: Bytes,
    ) -> PacketSentEvent {
        let mut event = PacketSentEvent::new_raw(self.clone(), packet_id, payload);
        if let Some(server) = self.world().server.upgrade() {
            server.plugin_manager.fire(&server, &mut event).await;
        }
        event
    }

    pub async fn fire_packet_sent_no_obj(self: &Arc<Self>, packet_id: i32, payload: Bytes) -> bool {
        self.fire_packet_sent_event_no_obj(packet_id, payload)
            .await
            .cancelled
    }

    /// Sets the player's camera target entity ID.
    /// If `target_id` matches the player's own entity ID, resets the camera back to the player.
    pub fn set_camera_entity_id(&self, target_id: i32) {
        if target_id == self.entity_id() {
            self.camera_target_id.store(None);
            self.try_send_client_packet(&CSetCamera::new(self.entity_id().into()));
        } else {
            self.camera_target_id.store(Some(target_id));
            self.try_send_client_packet(&CSetCamera::new(target_id.into()));
        }
    }

    /// Resets the player's camera back to their own perspective.
    pub fn reset_camera(&self) {
        self.camera_target_id.store(None);
        self.try_send_client_packet(&CSetCamera::new(self.entity_id().into()));
    }

    /// Gets the entity ID of the entity that the player's camera is currently attached to,
    /// or the player's own entity ID if not overridden.
    pub fn get_camera_entity_id(&self) -> i32 {
        self.camera_target_id
            .load()
            .unwrap_or_else(|| self.entity_id())
    }

    pub async fn send_editioned<
        J: pumpkin_protocol::ClientPacket + Sync,
        B: pumpkin_protocol::BClientPacket + Sync,
    >(
        &self,
        je_packet: &J,
        be_packet: &B,
    ) {
        self.client
            .enqueue_packet_editioned(je_packet, be_packet)
            .await;
    }

    pub async fn enqueue_packet_editioned<
        J: pumpkin_protocol::ClientPacket + Sync,
        B: pumpkin_protocol::BClientPacket + Sync,
    >(
        &self,
        je_packet: &J,
        be_packet: &B,
    ) {
        self.client
            .enqueue_packet_editioned(je_packet, be_packet)
            .await;
    }

    pub fn try_enqueue_packet_editioned<
        J: pumpkin_protocol::ClientPacket + Sync,
        B: pumpkin_protocol::BClientPacket + Sync,
    >(
        &self,
        je_packet: &J,
        be_packet: &B,
    ) {
        self.client
            .try_enqueue_packet_editioned(je_packet, be_packet);
    }

    pub async fn send_packet_now_editioned<
        J: pumpkin_protocol::ClientPacket + Sync,
        B: pumpkin_protocol::BClientPacket + Sync,
    >(
        &self,
        je_packet: &J,
        be_packet: &B,
    ) {
        self.client
            .send_packet_now_editioned(je_packet, be_packet)
            .await;
    }

    pub fn try_send_client_packet<C: pumpkin_protocol::ClientPacket + Sync>(&self, packet: &C) {
        if let ClientPlatform::Java(client) = self.client.as_ref()
            && let Ok(data) = client.serialize_packet(packet)
        {
            client.try_enqueue_packet(data);
        }
    }

    pub async fn send_client_packet<C: pumpkin_protocol::ClientPacket + Sync>(&self, packet: &C) {
        if let ClientPlatform::Java(client) = self.client.as_ref()
            && let Ok(data) = client.serialize_packet(packet)
        {
            client.enqueue_packet(data).await;
        }
    }

    #[must_use]
    pub fn as_java(&self) -> Option<JavaPlayer<'_>> {
        matches!(self.client.as_ref(), ClientPlatform::Java(_)).then(|| JavaPlayer(self))
    }

    #[must_use]
    pub fn as_bedrock(&self) -> Option<BedrockPlayer<'_>> {
        matches!(self.client.as_ref(), ClientPlatform::Bedrock(_)).then(|| BedrockPlayer(self))
    }

    pub fn tick_client_load_timeout(&self) {
        if !self.supports_player_loaded() {
            return;
        }
        if !self.client_loaded.load(Ordering::Relaxed) {
            let timeout = self.client_loaded_timeout.load(Ordering::Relaxed);
            self.client_loaded_timeout
                .store(timeout.saturating_sub(1), Ordering::Relaxed);
        }
    }

    /// Send the player's skin layers and used hand to all players.
    pub fn send_client_information(&self) {
        let config = self.config.load();
        self.living_entity.entity.set_synced_data(
            pumpkin_data::tracked_data::player::PLAYER_MODE_CUSTOMISATION,
            config.skin_parts,
        );
        self.living_entity.entity.set_synced_data(
            pumpkin_data::tracked_data::player::PLAYER_MAIN_HAND,
            config.main_hand as u8,
        );
        self.living_entity.entity.set_synced_data(
            pumpkin_data::tracked_data::player::PLAYER_MODE_CUSTOMIZATION_ID,
            config.skin_parts,
        );
        self.living_entity.entity.set_synced_data(
            pumpkin_data::tracked_data::player::MAIN_ARM_ID,
            config.main_hand as u8,
        );
    }
}
