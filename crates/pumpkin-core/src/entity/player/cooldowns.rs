//! Item cooldowns.

use super::{ItemCooldown, Player};
use core::f32;
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::java::client::play::CItemCooldown;
use std::sync::atomic::Ordering;

impl Player {
    pub fn start_cooldown(&self, group: String, duration: i32) {
        let mut cooldowns = self
            .item_cooldowns
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cooldowns.insert(
            group.clone(),
            ItemCooldown {
                start_tick: self.tick_counter.load(Ordering::Relaxed),
                duration,
            },
        );
        self.try_send_client_packet(&CItemCooldown::new(group, VarInt(duration)));
    }

    pub fn get_cooldown(&self, group: &str) -> f32 {
        let cooldowns = self
            .item_cooldowns
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(cooldown) = cooldowns.get(group) {
            let current_tick = self.tick_counter.load(Ordering::Relaxed);
            let elapsed = current_tick - cooldown.start_tick;
            if elapsed < cooldown.duration {
                return 1.0 - (elapsed as f32 / cooldown.duration as f32);
            }
        }
        0.0
    }

    pub fn is_on_cooldown(&self, group: &str) -> bool {
        let mut cooldowns = self
            .item_cooldowns
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(cooldown) = cooldowns.get(group) {
            let current_tick = self.tick_counter.load(Ordering::Relaxed);
            if current_tick - cooldown.start_tick < cooldown.duration {
                return true;
            }
            cooldowns.remove(group);
        }
        false
    }

    // --- Item Cooldown System ---
    pub fn set_item_cooldown(&self, item_id: &str, ticks: i32) {
        self.start_cooldown(item_id.to_string(), ticks);
    }

    pub fn get_item_cooldown(&self, item_id: &str) -> Option<i32> {
        let cooldowns = self
            .item_cooldowns
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(cooldown) = cooldowns.get(item_id) {
            let current_tick = self.tick_counter.load(Ordering::Relaxed);
            let elapsed = current_tick - cooldown.start_tick;
            if elapsed < cooldown.duration {
                return Some(cooldown.duration - elapsed);
            }
        }
        None
    }

    pub fn has_item_cooldown(&self, item_id: &str) -> bool {
        self.is_on_cooldown(item_id)
    }
}
