//! Chat and system messages to all players.

use super::World;
use crate::entity::player::Player;
use pumpkin_data::world::RAW;
use pumpkin_protocol::java::client::play::CDisguisedChatMessage;
use pumpkin_protocol::java::client::play::CSystemChatMessage;
use pumpkin_protocol::{
    bedrock::server::text::SText, codec::var_int::VarInt, java::server::play::SChatMessage,
};
use pumpkin_util::text::TextComponent;
use std::sync::Arc;
use uuid::Uuid;

impl World {
    pub fn broadcast_system_message(&self, message: &TextComponent, overlay: bool) {
        let je_packet = CSystemChatMessage::new(message, overlay);
        let be_packet = Self::component_to_bedrock_text(message);
        self.broadcast_editioned(&je_packet, &be_packet);
    }

    pub(super) fn component_to_bedrock_text(message: &TextComponent) -> SText<'static> {
        match &*message.0.content {
            pumpkin_util::text::TextContent::Translate {
                translate,
                bedrock_translate,
                with,
            } => {
                let key = bedrock_translate.as_deref().unwrap_or(translate.as_ref());
                let parameters = with
                    .iter()
                    .map(pumpkin_util::text::TextComponentBase::to_bedrock_string)
                    .collect();
                SText::translation(key.to_string(), parameters)
            }
            _ => SText::system_message(
                message
                    .0
                    .to_bedrock_legacy(pumpkin_util::translation::Locale::EnUs),
            ),
        }
    }

    pub fn broadcast_message(
        &self,
        message: &TextComponent,
        sender_name: &TextComponent,
        chat_type: u8,
        target_name: Option<&TextComponent>,
    ) {
        let be_packet = SText::new(message.clone().get_text(), sender_name.clone().get_text());
        let je_packet =
            CDisguisedChatMessage::new(message, (chat_type + 1).into(), sender_name, target_name);

        self.broadcast_editioned(&je_packet, &be_packet);
    }

    pub fn broadcast_chat_message(
        &self,
        message: &crate::net::chat::PlayerChatMessage,
        is_filtered: impl Fn(&Player) -> bool,
        sender_player: Option<&Arc<Player>>,
        chat_type: VarInt,
        sender_name: &TextComponent,
        target_name: Option<&TextComponent>,
    ) {
        let tracked = crate::net::chat::OutgoingChatMessage::create(message.clone());
        let mut was_fully_filtered = false;

        let players = self.players.load();
        for player in players.iter() {
            let filtered = is_filtered(player);
            tracked.send_to_player(player, filtered, chat_type, sender_name, target_name);
            was_fully_filtered |= filtered && message.is_fully_filtered();
        }

        if was_fully_filtered && let Some(sender) = sender_player {
            let filter_notice =
                TextComponent::translate(pumpkin_data::translation::java::CHAT_FILTERED_FULL, [])
                    .color_named(pumpkin_util::text::color::NamedColor::Red)
                    .italic();
            sender.send_system_message(&filter_notice);
        }
    }

    pub fn broadcast_secure_player_chat(
        &self,
        sender: &Arc<Player>,
        chat_message: &SChatMessage<'_>,
        decorated_message: &TextComponent,
    ) {
        let messages_sent: i32 = sender
            .chat_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .messages_sent;
        let sender_last_seen = {
            let cache = sender
                .signature_cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            cache.last_seen.as_ref().to_vec()
        };

        let link = crate::net::chat::SignedMessageLink::new(
            messages_sent,
            sender.gameprofile.id,
            Uuid::nil(),
        );
        let signed_body = crate::net::chat::SignedMessageBody::new(
            chat_message.message.to_string(),
            chat_message.timestamp,
            chat_message.salt,
            sender_last_seen,
        );
        let player_chat_msg = crate::net::chat::PlayerChatMessage::new(
            link,
            chat_message.signature.map(std::convert::Into::into),
            signed_body,
            Some(decorated_message.clone()),
            crate::net::chat::FilterMask::PassThrough,
        );

        self.broadcast_chat_message(
            &player_chat_msg,
            Player::is_text_filtering_enabled,
            Some(sender),
            (RAW + 1).into(),
            &TextComponent::empty(),
            None,
        );

        sender
            .chat_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .messages_sent += 1;
    }
}
