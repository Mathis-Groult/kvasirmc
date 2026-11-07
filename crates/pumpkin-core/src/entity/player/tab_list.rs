//! Tab list entries and who can see whom.

use super::Player;
use pumpkin_protocol::bedrock::client::{
    common::BuildPlatform,
    player_list::{CPlayerList, PlayerListEntry},
};
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::codec::var_long::VarLong;
use pumpkin_protocol::java::client::play::{
    CPlayerInfoUpdate, CTabList, PlayerAction, PlayerInfoFlags,
};
use pumpkin_util::text::TextComponent;
use std::sync::atomic::Ordering;

impl Player {
    /// Sets the tab list header and footer for Java Edition clients.
    ///
    /// Note: Tab list header and footer formatting is a Java Edition-specific protocol feature
    /// and is safely ignored on Bedrock Edition clients.
    pub fn set_tab_list(&self, tab_list: impl Into<crate::plugin::api::tab_list::TabList>) {
        let list = tab_list.into();
        self.set_tab_list_header_footer(&list.header, &list.footer);
    }

    pub fn set_tab_list_header_footer(&self, header: &TextComponent, footer: &TextComponent) {
        *self
            .tab_list_header
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = header.clone();
        *self
            .tab_list_footer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = footer.clone();
        self.try_send_client_packet(&CTabList::new(header, footer));
    }

    pub fn set_display_name(&self, display_name: Option<TextComponent>) {
        let mut guard = self
            .display_name
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = display_name;
        // Update the tab list for everyone
        let world = self.world();
        world.broadcast_packet_all(&CPlayerInfoUpdate::new(
            PlayerInfoFlags::UPDATE_DISPLAY_NAME.bits(),
            &[pumpkin_protocol::java::client::play::Player {
                uuid: self.gameprofile.id,
                actions: &[PlayerAction::UpdateDisplayName(guard.as_ref())],
            }],
        ));
    }

    pub fn get_tab_list_name(&self) -> Option<TextComponent> {
        self.tab_list_name
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn set_tab_list_name(&self, name: Option<TextComponent>) {
        let mut guard = self
            .tab_list_name
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = name;
        let world = self.world();
        world.broadcast_packet_all(&CPlayerInfoUpdate::new(
            PlayerInfoFlags::UPDATE_DISPLAY_NAME.bits(),
            &[pumpkin_protocol::java::client::play::Player {
                uuid: self.gameprofile.id,
                actions: &[PlayerAction::UpdateDisplayName(guard.as_ref())],
            }],
        ));
    }

    pub fn set_tab_list_order(&self, order: i32) {
        self.tab_list_order.store(order, Ordering::Relaxed);
        let world = self.world();
        world.broadcast_packet_all(&CPlayerInfoUpdate::new(
            PlayerInfoFlags::UPDATE_LIST_PRIORITY.bits(),
            &[pumpkin_protocol::java::client::play::Player {
                uuid: self.gameprofile.id,
                actions: &[PlayerAction::UpdateListOrder(VarInt(order))],
            }],
        ));
    }

    pub fn set_tab_list_latency(&self, latency: i32) {
        self.tab_list_latency.store(latency, Ordering::Relaxed);
        let world = self.world();
        world.broadcast_packet_all(&CPlayerInfoUpdate::new(
            PlayerInfoFlags::UPDATE_LATENCY.bits(),
            &[pumpkin_protocol::java::client::play::Player {
                uuid: self.gameprofile.id,
                actions: &[PlayerAction::UpdateLatency(VarInt(latency))],
            }],
        ));
    }

    pub fn set_tab_list_listed(&self, listed: bool) {
        self.tab_list_listed.store(listed, Ordering::Relaxed);
        let world = self.world();
        world.broadcast_packet_all(&CPlayerInfoUpdate::new(
            PlayerInfoFlags::UPDATE_LISTED.bits(),
            &[pumpkin_protocol::java::client::play::Player {
                uuid: self.gameprofile.id,
                actions: &[PlayerAction::UpdateListed(listed)],
            }],
        ));
    }

    /// Bedrock tab-list entry -> carries the skin, so it must precede `AddPlayer`.
    #[must_use]
    pub fn bedrock_player_list(&self) -> CPlayerList {
        CPlayerList {
            action: CPlayerList::ACTION_ADD,
            entries: vec![PlayerListEntry {
                uuid: self.gameprofile.id,
                entity_unique_id: VarLong(i64::from(self.entity_id())),
                username: self.gameprofile.name.clone(),
                xuid: String::new(),
                platform_chat_id: String::new(),
                build_platform: BuildPlatform::Unknown,
                skin: (**self.bedrock_skin.load()).clone(),
                is_teacher: false,
                is_host: false,
                is_sub_client: false,
                player_color: [0; 4],
            }],
        }
    }

    pub fn hide_player(&self, other_id: uuid::Uuid) {
        self.hidden_players
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(other_id);
    }

    pub fn show_player(&self, other_id: uuid::Uuid) {
        self.hidden_players
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&other_id);
    }

    pub fn can_see(&self, other_id: &uuid::Uuid) -> bool {
        !self
            .hidden_players
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(other_id)
    }

    pub fn can_see_player(&self, other_id: &uuid::Uuid) -> bool {
        self.can_see(other_id)
    }

    // --- Tab List & Display Names ---
    pub fn set_tab_list_ping(&self, latency_ms: i32) {
        self.set_tab_list_latency(latency_ms);
    }
}
