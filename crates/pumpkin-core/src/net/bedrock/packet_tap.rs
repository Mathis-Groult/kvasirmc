//! Test only: a Bedrock client's outgoing queue, read by the test instead of a writer task.

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use bytes::Bytes;
use tokio::sync::mpsc::UnboundedReceiver;

use super::{BedrockClient, OutgoingPacket};
use crate::net::decrement_pending_bytes;

/// Test only: what the server queues for one Bedrock client, taken before any writer task runs.
/// Draining releases the queued bytes as the writer task would, so a long test does not trip
/// the outbound buffer limit.
pub struct BedrockPacketTap {
    packets: UnboundedReceiver<OutgoingPacket>,
    pending_bytes: Arc<AtomicUsize>,
}

impl BedrockPacketTap {
    /// The encoded game packets queued since the last call.
    pub fn drain(&mut self) -> Vec<Bytes> {
        let mut packets = Vec::new();
        while let Ok(packet) = self.packets.try_recv() {
            decrement_pending_bytes(&self.pending_bytes, packet.data.len());
            packets.push(packet.data);
        }
        packets
    }
}

impl BedrockClient {
    /// Test only: takes the outgoing queue, so that nothing but the test reads it.
    pub fn take_packet_tap(&self) -> Option<BedrockPacketTap> {
        let packets = self.outgoing_packet_queue_recv.try_lock().ok()?.take()?;
        Some(BedrockPacketTap {
            packets,
            pending_bytes: Arc::clone(&self.pending_bytes),
        })
    }
}
