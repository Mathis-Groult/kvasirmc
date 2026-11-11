#[allow(clippy::wildcard_imports)]
use super::*;

impl JavaClient {
    /// Vanilla 26.3: each request REPLACES the player's set of debug subscriptions (an empty set
    /// ends them all). Only the server tick chart exists here; it is sent to operators only (see
    /// `Server`'s tick sample broadcast), as vanilla answers a non-operator's set as empty.
    pub fn handle_debug_subscription_request(
        &self,
        player: &Arc<Player>,
        packet: &pumpkin_protocol::java::server::play::SDebugSubscriptionRequest,
    ) {
        let tick_time = packet.subscriptions.iter().any(|id| {
            id.0 == pumpkin_protocol::java::server::play::SDebugSubscriptionRequest::TICK_TIME
        });
        player
            .subscribed_debug_sample
            .store(tick_time, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use pumpkin_protocol::ServerPacket;
    use pumpkin_protocol::java::server::play::SDebugSubscriptionRequest;
    use pumpkin_util::version::JavaMinecraftVersion;
    use pumpkin_util::{GameMode, PermissionLvl};

    use crate::entity::ai::test_support;
    use crate::net::ClientPlatform;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// Vanilla 26.3 sends the set of `debug_subscription` ids the client wants (`VarInt` count,
    /// then the ids; `dedicated_server_tick_time` = 0) and each request replaces the last one.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_tick_chart_follows_the_requested_set() -> TestResult {
        let folder = tempfile::tempdir()?;
        let server = test_support::server(&folder).await?;
        let world = server
            .worlds
            .load()
            .first()
            .cloned()
            .ok_or("the server has no world")?;
        let player = test_support::java_player(&world, GameMode::Creative);
        player.permission_lvl.store(PermissionLvl::Four);
        let ClientPlatform::Java(client) = player.client.as_ref() else {
            return Err("not a Java player".into());
        };
        let mut seen = Vec::new();
        for bytes in [&[1u8, 0][..], &[0], &[2, 3, 0], &[1, 3]] {
            let mut payload = bytes;
            let packet =
                SDebugSubscriptionRequest::read(&mut payload, &JavaMinecraftVersion::V_26_3)?;
            client.handle_debug_subscription_request(&player, &packet);
            seen.push(player.subscribed_debug_sample.load(Ordering::Relaxed));
        }
        server.stop();
        // Open the chart, close it, a set that holds it among others, a set without it.
        assert_eq!(seen, [true, false, true, false]);
        Ok(())
    }
}
