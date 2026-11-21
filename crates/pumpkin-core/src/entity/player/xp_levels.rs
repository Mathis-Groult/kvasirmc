//! Experience points and levels.

use super::Player;
use crate::plugin::player::exp_change::PlayerExpChangeEvent;
use core::f32;
use pumpkin_data::Enchantment;
use pumpkin_data::data_component_impl::EquipmentSlot;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_inventory::player::player_inventory::PlayerInventory;
use pumpkin_protocol::bedrock::client::update_attributes::{
    AttributeData as BedrockAttribute, CUpdateAttributes as CBedrockAttributes,
};
use pumpkin_protocol::codec::item_stack_seralizer::ItemStackSerializer;
use pumpkin_protocol::codec::var_ulong::VarULong;
use pumpkin_protocol::java::client::play::{CSetExperience, CSetPlayerInventory};
use pumpkin_util::math::experience;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

/// Experience state beside level, bar and points.
pub struct XpState {
    /// The level last sent to the client (`-1`: nothing yet).
    pub last_sent_level: AtomicI32,
    /// Vanilla `Player.totalExperience`: the save's `XpTotal` as stored, grown by
    /// `add_experience_points` (clamped to `0..=i32::MAX`), never derived from level and bar.
    pub total: AtomicI32,
}

impl XpState {
    pub const fn new() -> Self {
        Self {
            last_sent_level: AtomicI32::new(-1),
            total: AtomicI32::new(0),
        }
    }
}

impl Player {
    // --- Experience & Leveling API ---
    pub fn add_experience(self: &Arc<Self>, points: i32) {
        self.add_experience_points(points);
    }

    pub fn add_levels(&self, levels: i32) {
        self.add_experience_levels(levels);
    }

    pub fn get_experience_level(&self) -> i32 {
        self.experience_level.load(Ordering::Relaxed)
    }

    pub fn get_experience_progress(&self) -> f32 {
        self.experience_progress.load()
    }

    pub fn get_total_experience(&self) -> i32 {
        self.experience_points.load(Ordering::Relaxed)
    }

    pub fn set_experience_progress(&self, progress: f32) {
        let level = self.get_experience_level();
        let max_points = experience::points_in_level(level);
        let points = (progress.clamp(0.0, 1.0) * max_points as f32) as i32;
        self.set_experience(level, progress, points);
    }

    pub fn set_total_experience(&self, points: i32) {
        self.set_experience_points(points);
    }

    pub fn tick_experience(&self) {
        if !self.has_client_loaded() {
            return;
        }

        let level = self.experience_level.load(Ordering::Relaxed);
        if self.xp_state.last_sent_level.load(Ordering::Relaxed) != level {
            let progress = self.experience_progress.load();
            let points = self.experience_points.load(Ordering::Relaxed);

            self.xp_state
                .last_sent_level
                .store(level, Ordering::Relaxed);

            let attribute = |name: &str, current_value, max_value| BedrockAttribute {
                min_value: 0.0,
                max_value,
                current_value,
                default_min_value: 0.0,
                default_max_value: max_value,
                default_value: 0.0,
                name: name.to_string(),
                modifiers: Vec::new(),
            };
            self.try_enqueue_packet_editioned(
                &CSetExperience::new(progress.clamp(0.0, 1.0), level.into(), points.into()),
                &CBedrockAttributes {
                    target_runtime_id: VarULong(self.entity_id() as u64),
                    attribute_list: vec![
                        attribute("minecraft:player.experience", progress.clamp(0.0, 1.0), 1.0),
                        attribute("minecraft:player.level", level.max(0) as f32, 24_791.0),
                    ],
                    tick: VarULong(0),
                },
            );
        }
    }

    /// Sets the player's experience level and notifies the client.
    pub fn set_experience(&self, level: i32, progress: f32, points: i32) {
        let old_level = self.experience_level.load(Ordering::Relaxed);
        if old_level != level
            && let Some(server) = self.world().server.upgrade()
            && let Some(player_arc) = self.world().get_player_by_uuid(self.gameprofile.id)
        {
            let mut event = crate::plugin::api::events::player::player_level_change::PlayerLevelChangeEvent::new(
                player_arc,
                old_level,
                level,
            );
            server.plugin_manager.fire_blocking(&server, &mut event);
        }
        // TODO: These should be atomic together, not isolated; make a struct containing these. can cause ABA issues
        self.experience_level.store(level, Ordering::Relaxed);
        self.experience_progress.store(progress.clamp(0.0, 1.0));
        self.experience_points.store(points, Ordering::Relaxed);
        self.xp_state.last_sent_level.store(-1, Ordering::Relaxed);
        self.tick_experience();

        if self.has_client_loaded() {
            self.try_send_client_packet(&CSetExperience::new(
                progress.clamp(0.0, 1.0),
                level.into(),
                points.into(),
            ));
        }
    }

    /// Back to nothing: level, bar, points and total (a respawn without keeping the inventory).
    pub fn reset_experience(&self) {
        self.xp_state.total.store(0, Ordering::Relaxed);
        self.set_experience(0, 0.0, 0);
    }

    /// Sets the player's experience level directly.
    pub fn set_experience_level(&self, new_level: i32, keep_progress: bool) {
        let progress = self.experience_progress.load();
        let mut points = self.experience_points.load(Ordering::Relaxed);

        // If `keep_progress` is `true` then calculate the number of points needed to keep the same progress scaled.
        if keep_progress {
            // Get our current level
            let current_level = self.experience_level.load(Ordering::Relaxed);
            let current_max_points = experience::points_in_level(current_level);
            // Calculate the max value for the new level
            let new_max_points = experience::points_in_level(new_level);
            // Calculate the scaling factor
            let scale = new_max_points as f32 / current_max_points as f32;
            // Scale the points (Vanilla doesn't seem to recalculate progress so we won't)
            points = (points as f32 * scale) as i32;
        }

        self.set_experience(new_level, progress, points);
    }

    /// Add experience levels to the player.
    pub fn add_experience_levels(&self, added_levels: i32) {
        let current_level = self.experience_level.load(Ordering::Relaxed);
        let new_level = current_level + added_levels;
        self.set_experience_level(new_level, true);
    }

    /// Set the player's experience points directly. Returns `true` if successful.
    pub fn set_experience_points(&self, new_points: i32) -> bool {
        let current_points = self.experience_points.load(Ordering::Relaxed);

        if new_points == current_points {
            return true;
        }

        let current_level = self.experience_level.load(Ordering::Relaxed);
        let max_points = experience::points_in_level(current_level);

        if new_points < 0 || new_points > max_points {
            return false;
        }

        let progress = new_points as f32 / max_points as f32;
        self.set_experience(current_level, progress, new_points);
        true
    }

    /// Add experience points to the player.
    pub fn add_experience_points(self: &Arc<Self>, mut added_points: i32) {
        let server = self.world().server.upgrade();
        if let Some(server) = server {
            let mut event = PlayerExpChangeEvent::new(self.clone(), added_points);
            server.plugin_manager.fire_blocking(&server, &mut event);
            added_points = event.amount;
        }

        let current_level = self.experience_level.load(Ordering::Relaxed);
        let current_points = self.experience_points.load(Ordering::Relaxed);

        let total_exp = experience::points_to_level(current_level) as i64 + current_points as i64;
        let new_total_exp = total_exp + added_points as i64;
        let safe_new_total = new_total_exp.clamp(0, i32::MAX as i64) as i32;

        // vanilla `giveExperiencePoints`: totalExperience = clamp(total + points, 0, MAX)
        let total =
            i64::from(self.xp_state.total.load(Ordering::Relaxed)) + i64::from(added_points);
        self.xp_state.total.store(
            total.clamp(0, i64::from(i32::MAX)) as i32,
            Ordering::Relaxed,
        );

        let (new_level, new_points) = experience::total_to_level_and_points(safe_new_total);
        let progress = experience::progress_in_level(new_points, new_level);

        self.set_experience(new_level, progress, new_points);
    }

    pub fn apply_mending_from_xp(&self, mut xp: i32) -> i32 {
        if xp <= 0 {
            return xp;
        }

        let mut candidates: Vec<(usize, EquipmentSlot, ItemStack)> = Vec::new();

        let selected_slot = self.inventory.get_selected_slot() as usize;
        let mut slot_pairs: Vec<(usize, EquipmentSlot)> = vec![
            (selected_slot, EquipmentSlot::MAIN_HAND),
            (PlayerInventory::OFF_HAND_SLOT, EquipmentSlot::OFF_HAND),
        ];
        for (slot_index, slot) in self.inventory.equipment_slots.iter() {
            if slot.is_armor_slot() {
                slot_pairs.push((*slot_index, slot.clone()));
            }
        }

        for (slot_index, equipment_slot) in slot_pairs {
            let stack = self.inventory.get_slot(slot_index);
            if stack.get_enchantment_level(&Enchantment::MENDING) > 0 && stack.get_damage() > 0 {
                candidates.push((slot_index, equipment_slot, stack));
            }
        }

        if candidates.is_empty() {
            return xp;
        }

        let idx = rand::random::<u32>() as usize % candidates.len();
        let (slot_index, equipment_slot, mut stack) = candidates.swap_remove(idx);

        let repaired = stack.repair_item(xp.saturating_mul(2));
        if repaired <= 0 {
            return xp;
        }

        let xp_used = (repaired + 1) / 2;

        if let Some(player_arc) = self.world().get_player_by_uuid(self.gameprofile.id)
            && let Some(server) = self.world().server.upgrade()
        {
            let mut event =
                crate::plugin::api::events::player::player_item_mend::PlayerItemMendEvent {
                    player: player_arc,
                    item_name: stack.item.registry_key.to_string(),
                    repair_amount: repaired,
                    exp_consumed: xp_used,
                    cancelled: false,
                };
            server.plugin_manager.fire_blocking(&server, &mut event);
            if event.cancelled {
                return xp;
            }
        }

        let updated_stack = stack.clone();
        self.inventory.set_slot(slot_index, updated_stack.clone());

        xp = xp.saturating_sub(xp_used);

        self.try_send_slot_set_packet(&CSetPlayerInventory::new(
            (slot_index as i32).into(),
            &ItemStackSerializer::from(updated_stack.clone()),
        ));
        self.sync_inventory_to_client();

        self.living_entity
            .send_equipment_changes(&[(equipment_slot, updated_stack)]);

        xp
    }
}
