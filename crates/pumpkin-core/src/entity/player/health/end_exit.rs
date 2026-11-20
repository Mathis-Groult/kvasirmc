//! Leaving the End through the exit portal (vanilla 26.3 `EndPortalBlock.entityInside`,
//! `ServerPlayer.showEndCredits`, and the `wonGame` branch of `PERFORM_RESPAWN`).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use pumpkin_protocol::java::client::play::{CGameEvent, GameEvent};

use crate::entity::player::Player;
use crate::net::ClientPlatform;

/// Vanilla `seenCredits` (saved) and `wonGame` (the player left the End and waits for the
/// client's respawn request).
#[derive(Default)]
pub struct EndCredits {
    pub seen: AtomicBool,
    pub won: AtomicBool,
}

impl Player {
    /// The first time: the End Poem and credits (vanilla sends `WIN_GAME` with 0; the client
    /// shows them, then asks to respawn). Afterwards, and for Bedrock clients (Pumpkin does not
    /// read their credits-closed answer), the player is respawned at once, keeping everything.
    pub async fn leave_the_end(self: &Arc<Self>) {
        if self.end_credits.won.swap(true, Ordering::Relaxed) {
            return;
        }
        let seen = self.end_credits.seen.swap(true, Ordering::Relaxed);
        if !seen && let ClientPlatform::Java(client) = self.client.as_ref() {
            client.try_send_packet(&CGameEvent::new(GameEvent::WinGame, 0.0));
            return;
        }
        self.respawn_after_winning().await;
    }

    /// Vanilla `PlayerList.respawn(player, true, ..)` for a player who left the End: back at
    /// its respawn point with everything kept.
    pub async fn respawn_after_winning(self: &Arc<Self>) {
        self.world().respawn_player(self, true).await;
        self.end_credits.won.store(false, Ordering::Relaxed);
    }
}
