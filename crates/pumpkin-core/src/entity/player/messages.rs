//! Chat, system messages and titles.

use super::{Player, TitleMode};
use pumpkin_protocol::bedrock::server::text::SText;
use pumpkin_protocol::java::client::play::{
    CActionBar, CDisguisedChatMessage, CSubtitle, CSystemChatMessage, CTitleAnimation, CTitleText,
};
use pumpkin_util::text::TextComponent;
use pumpkin_util::translation::Locale;
use std::str::FromStr;
use std::sync::Arc;

impl Player {
    pub fn show_title(&self, text: &TextComponent, mode: &TitleMode) {
        match mode {
            TitleMode::Title => {
                self.client.try_enqueue_packet_editioned(
                    &CTitleText::new(text),
                    &pumpkin_protocol::bedrock::client::set_title::CSetTitle::new(
                        pumpkin_protocol::bedrock::client::TitleType::Title,
                        text.clone().get_text(),
                        0,
                        0,
                        0,
                    ),
                );
            }
            TitleMode::SubTitle => {
                self.client.try_enqueue_packet_editioned(
                    &CSubtitle::new(text),
                    &pumpkin_protocol::bedrock::client::set_title::CSetTitle::new(
                        pumpkin_protocol::bedrock::client::TitleType::Subtitle,
                        text.clone().get_text(),
                        0,
                        0,
                        0,
                    ),
                );
            }
            TitleMode::ActionBar => {
                self.client.try_enqueue_packet_editioned(
                    &CActionBar::new(text),
                    &pumpkin_protocol::bedrock::client::set_title::CSetTitle::new(
                        pumpkin_protocol::bedrock::client::TitleType::Actionbar,
                        text.clone().get_text(),
                        0,
                        0,
                        0,
                    ),
                );
            }
        }
    }

    pub fn send_title_animation(&self, fade_in: i32, stay: i32, fade_out: i32) {
        self.try_enqueue_packet_editioned(
            &CTitleAnimation::new(fade_in, stay, fade_out),
            &pumpkin_protocol::bedrock::client::set_title::CSetTitle::new(
                pumpkin_protocol::bedrock::client::TitleType::Times,
                String::new(),
                fade_in,
                stay,
                fade_out,
            ),
        );
    }

    pub fn send_message(
        &self,
        message: &TextComponent,
        chat_type: u8,
        sender_name: &TextComponent,
        target_name: Option<&TextComponent>,
    ) {
        self.try_send_client_packet(&CDisguisedChatMessage::new(
            message,
            (chat_type + 1).into(),
            sender_name,
            target_name,
        ));
    }

    #[must_use]
    pub fn is_text_filtering_enabled(&self) -> bool {
        self.config.load().text_filtering
    }

    pub fn send_chat_message(
        self: &Arc<Self>,
        tracked: &crate::net::chat::OutgoingChatMessage,
        filtered: bool,
        chat_type: pumpkin_protocol::codec::var_int::VarInt,
        sender_name: &TextComponent,
        target_name: Option<&TextComponent>,
    ) {
        tracked.send_to_player(self, filtered, chat_type, sender_name, target_name);
    }

    pub fn send_system_message(&self, text: &TextComponent) {
        self.send_system_message_raw(text, false);
    }

    pub fn send_system_message_raw(&self, text: &TextComponent, overlay: bool) {
        let je_packet = CSystemChatMessage::new(text, overlay);
        let locale = Locale::from_str(&self.config.load().locale).unwrap_or(Locale::EnUs);
        let be_packet = match &*text.0.content {
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
            _ => SText::system_message(text.0.to_bedrock_legacy(locale)),
        };
        self.try_enqueue_packet_editioned(&je_packet, &be_packet);
    }
}
