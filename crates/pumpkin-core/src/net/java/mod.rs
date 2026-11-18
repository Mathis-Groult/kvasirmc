use pumpkin_protocol::java::client::play::{CChunkBatchEnd, CChunkBatchStart, CPlayDisconnect};
use pumpkin_world::level::SyncChunk;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use std::{io::Write, sync::Arc};

use bytes::Bytes;
use crossbeam::atomic::AtomicCell;
use pumpkin_data::packet::CURRENT_MC_VERSION;
use pumpkin_data::translation;
use pumpkin_protocol::packet::MultiVersionJavaPacket;
use pumpkin_protocol::{
    ClientPacket, ConnectionState, PacketDecodeError, RawPacket,
    codec::var_int::VarInt,
    java::{
        client::{config::CConfigDisconnect, login::CLoginDisconnect},
        packet_decoder::TCPNetworkDecoder,
        packet_encoder::TCPNetworkEncoder,
    },
    ser::{NetworkReadExt, NetworkWriteExt, WritingError},
};
use pumpkin_util::text::TextComponent;
use pumpkin_util::version::JavaMinecraftVersion;
use tokio::{
    io::{AsyncRead, AsyncWrite, BufReader, BufWriter},
    sync::oneshot,
};
use tokio::{
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{debug, error, warn};

/// The client-to-server half of a Java connection: a TCP socket, or an in-memory pipe when the
/// server is embedded in a client.
pub type JavaReadHalf = Box<dyn AsyncRead + Send + Sync + Unpin>;
/// The server-to-client half of a Java connection.
pub type JavaWriteHalf = Box<dyn AsyncWrite + Send + Sync + Unpin>;

pub mod chunk_data;
pub mod handshake;
pub mod login;
mod outgoing;
pub mod pending;
pub mod play;
pub mod recipe_helper;
pub mod status;

pub use chunk_data::{CChunkData, ChunkLightExt};
use outgoing::{DISCONNECT_FLUSH_TIMEOUT, OutgoingPacket, run_outgoing_packet_writer};

use arc_swap::ArcSwap;
use pending::PendingConnection;

use crate::entity::player::Player;
use crate::net::{
    GameProfile, MAX_PENDING_BYTES, PacketRateLimiter, PlayerConfig, decrement_pending_bytes,
};
use crate::plugin::api::events::world::chunk_send::ChunkSend;
use crate::plugin::server::packet::PacketSentEvent;
use crate::{error::PumpkinError, server::Server};

#[repr(C)]
pub struct JavaClient {
    pub id: u64,
    /// The protocol the client speaks. Play packets are always encoded/decoded as
    /// `CURRENT_MC_VERSION`. Older clients are not admitted; the packet events are the hook
    /// for a plugin that converts them.
    pub version: AtomicCell<JavaMinecraftVersion>,
    /// The client's game profile information. Direct field (lock-free).
    pub gameprofile: GameProfile,
    /// The client's configuration settings. Lock-free `ArcSwap`.
    pub config: ArcSwap<PlayerConfig>,
    /// The Address used to connect to the Server, Sent in the Handshake. Direct field.
    pub server_address: String,
    /// The current connection state of the client (e.g., Handshaking, Status, Play).
    pub connection_state: AtomicCell<ConnectionState>,
    /// The client's IP address. Direct field (lock-free).
    pub address: SocketAddr,
    /// The client's brand or modpack information. Lock-free `ArcSwap`.
    pub brand: ArcSwap<Option<String>>,
    /// Associated player reference. Lock-free `ArcSwap`.
    pub player: ArcSwap<Option<Arc<Player>>>,
    /// A collection of tasks associated with this client. The tasks await completion when removing the client.
    tasks: TaskTracker,
    rt_handle: tokio::runtime::Handle,
    /// An notifier that is triggered when this client is closed.
    close_token: CancellationToken,
    /// Per-connection FIFO of serialized packets (vanilla Netty eventLoop).
    /// Unbounded like vanilla; `MAX_PENDING_BYTES` is the limit.
    outgoing_packet_queue_send: UnboundedSender<OutgoingPacket>,
    outgoing_packet_queue_recv: Option<UnboundedReceiver<OutgoingPacket>>,
    /// Tracks total buffered payload bytes in the outgoing queue.
    pub pending_bytes: Arc<AtomicUsize>,
    /// The packet encoder for outgoing packets.
    network_writer: std::sync::Mutex<Option<TCPNetworkEncoder<BufWriter<JavaWriteHalf>>>>,
    /// The packet decoder for incoming packets.
    network_reader: std::sync::Mutex<Option<TCPNetworkDecoder<BufReader<JavaReadHalf>>>>,
    /// Keep Alive:
    ///
    /// Whether we are waiting for a response after sending a keep alive packet.
    pub wait_for_keep_alive: AtomicBool,
    /// Set to `true` when any movement packet is received this tick.
    /// On `SClientTickEnd` (≥1.21.4), if still `false`, the player's known
    /// movement is zeroed (they stood still). Matches vanilla's `receivedMovementThisTick`.
    pub received_movement_this_tick: AtomicBool,
    /// The keep alive packet payload we send. The client should respond with the same id.
    pub keep_alive_id: AtomicCell<i64>,
    /// The last time we sent a keep alive packet.
    pub last_keep_alive_time: AtomicCell<Instant>,
    /// The last time any packet was received from the client.
    pub last_packet_time: AtomicCell<Instant>,
    /// Recent in-flight keep alive IDs with their sent timestamps.
    pub pending_keep_alives: std::sync::Mutex<Vec<(i64, Instant)>>,

    pub packet_sequence: AtomicI32,
    /// Packet rate limiter for incoming client packets.
    pub packet_limiter: PacketRateLimiter,
    /// Vanilla `suspendFlushingOnServerThread`.
    suspend_flushing: Arc<AtomicBool>,
}

impl JavaClient {
    #[must_use]
    pub fn from_pending(
        pending: PendingConnection,
        gameprofile: GameProfile,
        config: PlayerConfig,
    ) -> Self {
        let (send, recv) = tokio::sync::mpsc::unbounded_channel();

        Self {
            id: pending.id,
            gameprofile,
            config: ArcSwap::from_pointee(config),
            server_address: pending.server_address,
            address: pending.address,
            connection_state: pending.connection_state,
            close_token: pending.close_token,
            tasks: TaskTracker::new(),
            rt_handle: tokio::runtime::Handle::current(),
            outgoing_packet_queue_send: send,
            outgoing_packet_queue_recv: Some(recv),
            pending_bytes: Arc::new(AtomicUsize::new(0)),
            version: pending.version,
            network_writer: std::sync::Mutex::new(Some(pending.network_writer)),
            network_reader: std::sync::Mutex::new(Some(pending.network_reader)),
            brand: ArcSwap::from_pointee(pending.brand),
            player: ArcSwap::from_pointee(None),
            wait_for_keep_alive: AtomicBool::new(false),
            received_movement_this_tick: AtomicBool::new(false),
            keep_alive_id: AtomicCell::new(0),
            last_keep_alive_time: AtomicCell::new(Instant::now()),
            last_packet_time: AtomicCell::new(Instant::now()),
            pending_keep_alives: std::sync::Mutex::new(Vec::new()),
            packet_sequence: AtomicI32::new(-1),
            packet_limiter: pending.packet_limiter,
            suspend_flushing: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Vanilla `ServerCommonPacketListenerImpl.suspendFlushing`.
    pub fn suspend_flushing(&self) {
        self.suspend_flushing.store(true, Ordering::Release);
    }

    /// Vanilla `resumeFlushing`: queue `flushChannel` then lift the hold.
    pub fn resume_flushing(&self) {
        self.flush_channel();
        self.suspend_flushing.store(false, Ordering::Release);
    }

    /// Flushes Channel even while suspended.
    pub fn flush_channel(&self) {
        if self
            .outgoing_packet_queue_send
            .send(OutgoingPacket::Flush)
            .is_err()
            && !self.close_token.is_cancelled()
        {
            warn!(
                "Failed to queue flush for client {}: channel closed",
                self.id
            );
            self.close();
        }
    }

    pub fn set_player(&self, player: Arc<Player>) {
        self.player.store(Arc::new(Some(player)));
    }

    pub async fn progress_player_packets(&self, player: &Arc<Player>, server: &Arc<Server>) {
        let Some(mut network_reader) = self
            .network_reader
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        else {
            return;
        };

        let keep_alive_time = server.advanced_config.networking.java.keep_alive_time;
        let mut keep_alive_interval =
            tokio::time::interval(std::time::Duration::from_secs(keep_alive_time.max(1)));
        let timeout_duration =
            std::time::Duration::from_secs(keep_alive_time.saturating_mul(2).max(1));

        // Skip the immediate first tick so we don't send a keep-alive the exact millisecond they join
        keep_alive_interval.tick().await;

        loop {
            tokio::select! {
                // KEEP-ALIVE TIMER
                _ = keep_alive_interval.tick() => {
                    // Check if the client has timed out on keep-alive responses or no packet activity
                    let has_timed_out = {
                        let pending = self
                            .pending_keep_alives
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        pending.iter().any(|(_, send_time)| send_time.elapsed() > timeout_duration)
                    } || (self.wait_for_keep_alive.load(Ordering::Relaxed) && self.last_keep_alive_time.load().elapsed() > timeout_duration)
                      || (self.last_packet_time.load().elapsed() > timeout_duration);

                    if has_timed_out {
                        self.kick(pumpkin_macros::translate_cross!(translation::java::DISCONNECT_TIMEOUT, translation::bedrock::DISCONNECT_TIMEOUT)).await;
                        break;
                    }

                    let keep_alive_id = i64::from(
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as i32,
                    );

                    self.keep_alive_id.store(keep_alive_id);
                    self.wait_for_keep_alive.store(true, Ordering::Relaxed);
                    self.last_keep_alive_time.store(Instant::now());
                    {
                        let mut pending = self
                            .pending_keep_alives
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        pending.push((keep_alive_id, Instant::now()));
                        if pending.len() > 16 {
                            pending.remove(0);
                        }
                    }
                    let packet = pumpkin_protocol::java::client::play::CKeepAlive::new(keep_alive_id);
                    self.enqueue_client_packet(&packet).await;
                }

                () = self.close_token.cancelled() => {
                    break;
                }

                // INCOMING PACKETS
                packet_opt = self.get_packet_with_reader(&mut network_reader) => {
                    let Some(packet) = packet_opt else {
                        break;
                    };
                    self.last_packet_time.store(Instant::now());

                    if !self.packet_limiter.check_packet() {
                        warn!(
                            "Client {} ({}) exceeded packet rate limit (rate: {}/s)",
                            self.id,
                            self.gameprofile.name,
                            self.packet_limiter.max_rate()
                        );
                        self.kick(TextComponent::text(
                            server
                                .advanced_config
                                .networking
                                .java
                                .packet_limiter
                                .kick_message
                                .clone(),
                        ))
                        .await;
                        break;
                    }

                    player.inbound_packets.push(packet);
                }
            }
        }
    }

    pub async fn await_tasks(&self) {
        self.tasks.close();
        self.tasks.wait().await;
    }

    /// Spawns a task associated with this client. All tasks spawned with this method are awaited
    /// when the client. This means tasks should complete in a reasonable amount of time or select
    /// on `Self::await_close_interrupt` to cancel the task when the client is closed
    ///
    /// Returns an `Option<JoinHandle<F::Output>>`. If the client is closed, this returns `None`.
    pub fn spawn_task<F>(&self, task: F) -> Option<JoinHandle<F::Output>>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        if self.close_token.is_cancelled() {
            None
        } else {
            Some(self.tasks.spawn_on(task, &self.rt_handle))
        }
    }

    pub async fn send_chunks(&self, chunks: &[SyncChunk]) {
        let player = self.player.load_full();
        let Some(player) = player.as_ref() else {
            return;
        };
        let Some(server) = player.world().server.upgrade() else {
            return;
        };

        let mut valid_chunks = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            let mut event = ChunkSend::new(player.world(), chunk.clone());
            server.plugin_manager.fire(&server, &mut event).await;
            if !event.cancelled {
                valid_chunks.push(chunk.clone());
            }
        }

        if valid_chunks.is_empty() {
            return;
        }

        let (tx, rx) = oneshot::channel();
        server.compute_pool.spawn(move || {
            let mut serialized = Vec::with_capacity(valid_chunks.len());
            for chunk in valid_chunks {
                let mut buf = Vec::with_capacity(32 * 1024);
                if let Err(err) = buf.write_var_int(&VarInt(CChunkData::to_id(CURRENT_MC_VERSION)))
                {
                    error!("Failed to write chunk data id: {err:?}");
                    continue;
                }
                if let Err(err) =
                    CChunkData(&chunk).write_packet_data(&mut buf, &CURRENT_MC_VERSION)
                {
                    error!("Failed to write chunk data: {err:?}");
                    continue;
                }
                serialized.push(Bytes::from(buf));
            }
            let _ = tx.send(serialized);
        });

        let Ok(serialized) = rx.await else {
            return;
        };
        let sent_count = serialized.len();
        if sent_count == 0 {
            return;
        }

        self.send_packet(&CChunkBatchStart).await;

        // One FIFO per connection: batch start/data/end stay in enqueue order.
        for chunk_data in serialized {
            self.send_packet_now_data(chunk_data).await;
        }

        self.send_packet(&CChunkBatchEnd::new(sent_count as u16))
            .await;
    }

    pub async fn enqueue_packet(&self, packet_data: Bytes) {
        self.enqueue_packet_data(packet_data).await;
    }

    #[allow(clippy::unused_async)]
    pub async fn enqueue_packet_data(&self, packet_data: Bytes) {
        self.try_enqueue_packet_data(packet_data);
    }

    /// Outbound choke point of all enqueue/send paths. `None` when the packet must be dropped.
    fn reserve_pending_bytes(&self, packet_data: Bytes, observe: bool) -> Option<(Bytes, usize)> {
        if self.close_token.is_cancelled() {
            return None;
        }
        let packet_data = self.translate_outgoing(packet_data, observe)?;

        // Reserve first, release again if it does not fit.
        let packet_len = packet_data.len();
        let prev_bytes = self.pending_bytes.fetch_add(packet_len, Ordering::AcqRel);
        let new_bytes = prev_bytes.saturating_add(packet_len);

        if new_bytes > MAX_PENDING_BYTES {
            decrement_pending_bytes(&self.pending_bytes, packet_len);
            if !self.close_token.is_cancelled() {
                warn!(
                    "Client {} outbound packet buffer overflow ({} bytes > {} bytes). Closing connection.",
                    self.id, new_bytes, MAX_PENDING_BYTES
                );
                self.close();
            }
            return None;
        }

        Some((packet_data, packet_len))
    }

    /// `PacketSentEvent` for clients the multiversion plugin admitted below
    /// `CURRENT_MC_VERSION`: it gets the 26.3 id + payload and rewrites both.
    /// `None` when cancelled.
    ///
    /// For clients on `CURRENT_MC_VERSION` the event is fired as a read-only observation
    /// (cancellable) for packets in the play state when `observe` is set, so packet-level
    /// plugins (anti-cheats, protocol libraries) see every outbound packet. `observe` is
    /// `false` for packets a plugin injected itself, to avoid echoing them back.
    fn translate_outgoing(&self, packet_data: Bytes, observe: bool) -> Option<Bytes> {
        let current_version = self.version.load() == CURRENT_MC_VERSION;
        if current_version
            && (!observe || !matches!(self.connection_state.load(), ConnectionState::Play))
        {
            return Some(packet_data);
        }
        // TODO: packets sent before `set_player` (e.g. an `add_player` kick) go out untranslated.
        let player = self.player.load_full();
        let Some(player) = player.as_ref() else {
            return Some(packet_data);
        };
        let Some(server) = player.world().server.upgrade() else {
            return Some(packet_data);
        };
        if !server.plugin_manager.has_handlers::<PacketSentEvent>() {
            return Some(packet_data);
        }

        let mut reader = &packet_data[..];
        let Ok(packet_id) = reader.get_var_int() else {
            return Some(packet_data);
        };
        let payload = packet_data.slice(packet_data.len() - reader.len()..);
        let mut event = PacketSentEvent::new_raw(player.clone(), packet_id.0, payload);
        server.plugin_manager.fire_blocking(&server, &mut event);
        if event.cancelled {
            return None;
        }
        if current_version {
            return Some(packet_data);
        }

        let mut framed = Vec::with_capacity(5 + event.payload.len());
        framed.write_var_int(&VarInt(event.packet_id)).ok()?;
        framed.extend_from_slice(&event.payload);
        Some(framed.into())
    }

    pub fn try_enqueue_packet(&self, packet_data: Bytes) {
        self.try_enqueue_packet_data(packet_data);
    }

    pub fn try_enqueue_packet_data(&self, packet_data: Bytes) {
        let Some((packet_data, packet_len)) = self.reserve_pending_bytes(packet_data, true) else {
            return;
        };
        self.queue_outgoing(OutgoingPacket::normal(packet_data), packet_len);
    }

    /// Like [`Self::try_enqueue_packet_data`] but does not fire `PacketSentEvent` for
    /// current-version clients. For packets that a plugin already observed itself.
    pub fn try_enqueue_packet_data_unobserved(&self, packet_data: Bytes) {
        let Some((packet_data, packet_len)) = self.reserve_pending_bytes(packet_data, false) else {
            return;
        };
        self.queue_outgoing(OutgoingPacket::normal(packet_data), packet_len);
    }

    /// `false` once the writer is gone. Then the connection is closed.
    fn queue_outgoing(&self, packet: OutgoingPacket, packet_len: usize) -> bool {
        if self.outgoing_packet_queue_send.send(packet).is_ok() {
            return true;
        }
        decrement_pending_bytes(&self.pending_bytes, packet_len);
        // It is expected that the packet will fail if closed
        if !self.close_token.is_cancelled() {
            warn!(
                "Failed to add packet to the outgoing packet queue for client {}: channel closed",
                self.id
            );
            // Connection to the client closed since the stream is in an unknown state
            self.close();
        }
        false
    }

    pub async fn await_close_interrupt(&self) {
        self.close_token.cancelled().await;
    }

    pub async fn get_packet_with_reader(
        &self,
        network_reader: &mut TCPNetworkDecoder<BufReader<JavaReadHalf>>,
    ) -> Option<RawPacket> {
        tokio::select! {
            () = self.await_close_interrupt() => {
                debug!("Canceling player packet processing");
                None
            },
            packet_result = network_reader.get_raw_packet() => {
                match packet_result {
                    Ok(packet) => Some(packet),
                    Err(err) => {
                        if !matches!(err, PacketDecodeError::ConnectionClosed) {
                            debug!("Failed to decode packet from client {}: {}", self.id, err);
                            let text = format!("Error while reading incoming packet {err}");
                            self.kick(TextComponent::text(text)).await;
                        }
                        None
                    }
                }
            }
        }
    }

    /// Disconnect packet for the current state. `None` in handshake/status.
    fn serialize_disconnect(&self, reason: &TextComponent) -> Option<Bytes> {
        match self.connection_state.load() {
            ConnectionState::Login => {
                // TextComponent implements Serialize and writes in bytes instead of String
                let packet = CLoginDisconnect::new(
                    serde_json::to_string(&reason.0).unwrap_or_else(|_| String::new()),
                );
                self.serialize_packet(&packet).ok()
            }
            ConnectionState::Config => {
                let reason_text = reason.clone().get_text();
                let packet = CConfigDisconnect::new(&reason_text);
                self.serialize_packet(&packet).ok()
            }
            ConnectionState::Play => {
                let packet = CPlayDisconnect::new(reason);
                self.serialize_packet(&packet).ok()
            }
            _ => None,
        }
    }

    pub fn try_kick(&self, reason: &TextComponent) {
        if let Some(data) = self
            .serialize_disconnect(reason)
            .and_then(|data| self.translate_outgoing(data, false))
        {
            let packet_len = data.len();
            let _ = self.pending_bytes.fetch_add(packet_len, Ordering::AcqRel);
            // The writer drains and flushes it after `close()`
            if self
                .outgoing_packet_queue_send
                .send(OutgoingPacket::normal(data))
                .is_err()
            {
                decrement_pending_bytes(&self.pending_bytes, packet_len);
                // Expected: the writer task is already gone.
                debug!(
                    "Disconnect packet for client {} dropped: outgoing packet queue closed",
                    self.id
                );
            }
        }
        let reason_text = reason.clone().get_text();
        warn!("Closing connection for {}: {reason_text}", self.id);
        self.close();
    }

    pub async fn kick(&self, reason: TextComponent) {
        self.kick_explicit(&reason, true).await;
    }

    pub async fn kick_explicit(&self, reason: &TextComponent, send_packet: bool) {
        if send_packet && let Some(data) = self.serialize_disconnect(reason) {
            // Stalled peer: never flushes -> Close anyway.
            let _ = tokio::time::timeout(
                DISCONNECT_FLUSH_TIMEOUT,
                self.send_and_wait(data, OutgoingPacket::flushed),
            )
            .await;
        }
        let reason_text = reason.clone().get_text();
        warn!("Closing connection for {}: {reason_text}", self.id);
        self.close();
    }

    pub async fn send_packet_now(&self, packet: Bytes) {
        self.send_packet_now_data(packet).await;
    }

    /// Enqueue on the per-connection FIFO and wait until the writer has
    /// `write_frame`d into the `BufWriter`. Never waits for a TCP flush.
    pub async fn send_packet_now_data(&self, packet: Bytes) {
        self.send_and_wait(packet, OutgoingPacket::high_priority)
            .await;
    }

    /// Enqueue and wait for the writer's completion, `Framed` or `Flushed` per `make`.
    async fn send_and_wait(
        &self,
        packet: Bytes,
        make: fn(Bytes, oneshot::Sender<()>) -> OutgoingPacket,
    ) {
        let Some((packet, packet_len)) = self.reserve_pending_bytes(packet, true) else {
            return;
        };

        let (completion_tx, completion_rx) = oneshot::channel();
        if !self.queue_outgoing(make(packet, completion_tx), packet_len) {
            return;
        }

        if completion_rx.await.is_err() && !self.close_token.is_cancelled() {
            // The outgoing packet task dropped before confirming the write.
            self.close();
        }
    }

    pub fn write_packet_for_version<P: ClientPacket>(
        packet: &P,
        version: JavaMinecraftVersion,
        write: impl Write,
    ) -> Result<(), WritingError> {
        pumpkin_protocol::java::packet_encoder::write_packet(packet, &version, write)
    }

    pub fn serialize_packet_for_version<P: ClientPacket>(
        packet: &P,
        version: JavaMinecraftVersion,
    ) -> Result<Bytes, WritingError> {
        pumpkin_protocol::java::packet_encoder::serialize_packet(packet, &version)
    }

    pub fn serialize_packet<P: ClientPacket>(&self, packet: &P) -> Result<Bytes, WritingError> {
        Self::serialize_packet_for_version(packet, CURRENT_MC_VERSION)
    }

    pub fn try_send_packet<P: ClientPacket>(&self, packet: &P) {
        if let Ok(data) = self.serialize_packet(packet) {
            self.try_enqueue_packet(data);
        }
    }

    pub async fn send_packet<P: ClientPacket>(&self, packet: &P) {
        if let Ok(data) = self.serialize_packet(packet) {
            self.send_packet_now(data).await;
        }
    }

    pub async fn enqueue_client_packet<P: ClientPacket>(&self, packet: &P) {
        if let Ok(data) = self.serialize_packet(packet) {
            self.enqueue_packet(data).await;
        }
    }

    pub fn write_packet<P: ClientPacket>(
        &self,
        packet: &P,
        write: impl Write,
    ) -> Result<(), WritingError> {
        Self::write_packet_for_version(packet, CURRENT_MC_VERSION, write)
    }

    /// Handles an incoming packet, routing it to the appropriate handler based on the current connection state.
    ///
    /// This function takes a `RawPacket` and routes it to the corresponding handler based on the current connection state.
    /// It supports the following connection states:
    ///
    /// - **Handshake:** Handles handshake packets.
    /// - **Status:** Handles status request and ping packets.
    /// - **Login/Transfer:** Handles login and transfer packets.
    /// - **Config:** Handles configuration packets.
    pub fn start_outgoing_packet_task(&mut self) {
        let Some(packet_receiver) = self.outgoing_packet_queue_recv.take() else {
            return;
        };
        let close_token = self.close_token.clone();
        let pending_bytes = self.pending_bytes.clone();
        let Some(writer) = self
            .network_writer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        else {
            return;
        };
        let id = self.id;
        let suspend_flushing = self.suspend_flushing.clone();
        self.spawn_task(async move {
            run_outgoing_packet_writer(
                packet_receiver,
                writer,
                close_token,
                suspend_flushing,
                pending_bytes,
                id,
            )
            .await;
        });
    }

    /// Closes the connection to the client.
    ///
    /// This function marks the connection as closed using an atomic flag. It's generally preferable
    /// to use the `kick` function if you want to send a specific message to the client explaining the reason for the closure.
    /// However, use `close` in scenarios where sending a message is not critical or might not be possible (e.g., sudden connection drop).
    ///
    /// # Notes
    ///
    /// This function does not attempt to send any disconnect packets to the client.
    /// Packets already queued are still written and flushed, bounded by `DISCONNECT_FLUSH_TIMEOUT`.
    pub fn close(&self) {
        self.close_token.cancel();
    }

    pub fn is_closed(&self) -> bool {
        self.close_token.is_cancelled()
    }

    pub fn handle_play_packet(
        &self,
        player: &Arc<Player>,
        server: &Arc<Server>,
        packet: &RawPacket,
    ) -> Result<(), Box<dyn PumpkinError>> {
        let event_payload;
        let (packet_id, payload) = if server
            .plugin_manager
            .has_handlers::<crate::plugin::server::packet::PacketReceivedEvent>(
        ) {
            let mut event = crate::plugin::server::packet::PacketReceivedEvent::new(
                player.clone(),
                packet.id,
                packet.payload.clone(),
            );
            server.plugin_manager.fire_blocking(server, &mut event);
            if event.cancelled {
                return Ok(());
            }
            event_payload = event.payload;
            (event.packet_id, &event_payload[..])
        } else {
            (packet.id, &packet.payload[..])
        };

        self.dispatch_play_packet(player, server, packet_id, payload)
    }
}

/// Test only: what the server queues for one client, taken before any writer task runs.
#[cfg(test)]
pub(crate) struct PacketTap(tokio::sync::mpsc::UnboundedReceiver<OutgoingPacket>);

#[cfg(test)]
impl PacketTap {
    /// The encoded packets queued since the last call (flush markers left out). Completes each
    /// packet's write notification like the writer task would, so a `send_packet_now` awaited
    /// while a test task drains the tap returns instead of waiting forever.
    pub(crate) fn drain(&mut self) -> Vec<Bytes> {
        let mut packets = Vec::new();
        while let Ok(packet) = self.0.try_recv() {
            if let OutgoingPacket::Data { data, completion } = packet {
                if let Some(
                    outgoing::Completion::Framed(done) | outgoing::Completion::Flushed(done),
                ) = completion
                {
                    let _ = done.send(());
                }
                packets.push(data);
            }
        }
        packets
    }
}

#[cfg(test)]
impl JavaClient {
    /// Test only: takes the outgoing queue, so that nothing but the test reads it.
    pub(crate) fn take_packet_tap(&mut self) -> Option<PacketTap> {
        self.outgoing_packet_queue_recv.take().map(PacketTap)
    }
}
