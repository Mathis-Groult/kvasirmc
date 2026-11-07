//! Entity flags, pose, name and tags.

use super::{Entity, Flag, MAX_SCOREBOARD_TAGS};
use pumpkin_data::entity::{EntityPose, EntityType};
use pumpkin_data::tracked_data;
use pumpkin_protocol::{
    bedrock::client::set_actor_data::{
        CSetActorData, MetadataValue, PropertySyncData, SyncedActorDataList, entity_data_flag,
        entity_data_key,
    },
    codec::var_int::VarInt,
    codec::var_ulong::VarULong,
};
use pumpkin_util::text::TextComponent;
use std::sync::{
    Arc,
    atomic::Ordering::{self, Relaxed},
};

impl Entity {
    /// Sets the entity's growth age. Negative values indicate that the entity is a baby.
    pub fn set_age(&self, age: i32) {
        self.age.store(age, Relaxed);
    }

    /// Adds a scoreboard tag to this entity.
    ///
    /// Returns `false` if the entity already has the tag or already carries
    /// [`MAX_SCOREBOARD_TAGS`] tags.
    pub fn add_scoreboard_tag(&self, tag: &str) -> bool {
        let mut tags = self
            .scoreboard_tags
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        tags.len() < MAX_SCOREBOARD_TAGS && tags.insert(tag.to_owned())
    }

    /// Removes a scoreboard tag from this entity.
    ///
    /// Returns `false` if the entity did not have the tag.
    pub fn remove_scoreboard_tag(&self, tag: &str) -> bool {
        self.scoreboard_tags
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(tag)
    }

    /// Sets a custom name for the entity, typically used with nametags
    pub fn set_custom_name(&self, name: TextComponent) {
        self.custom_name.store(Arc::new(Some(name.clone())));
        let mut bedrock_meta = SyncedActorDataList::new();
        bedrock_meta.set(
            entity_data_key::NAME,
            MetadataValue::String(name.clone().get_text()),
        );
        let visible = self.custom_name_visible.load(Ordering::Relaxed);
        bedrock_meta.set_flag(
            entity_data_key::FLAGS,
            entity_data_flag::SHOW_NAME as u8,
            visible,
        );
        bedrock_meta.set_flag(
            entity_data_key::FLAGS,
            entity_data_flag::ALWAYS_SHOW_NAME as u8,
            visible,
        );
        self.set_synced_data(tracked_data::entity::DATA_CUSTOM_NAME, Some(name));
        self.send_bedrock_actor_data(&bedrock_meta);
    }

    pub fn clear_custom_name(&self) {
        if self.custom_name.load().is_none() {
            return;
        }
        self.custom_name.store(Arc::new(None));
        let mut bedrock_meta = SyncedActorDataList::new();
        bedrock_meta.set(entity_data_key::NAME, MetadataValue::String(String::new()));
        self.set_synced_data(
            tracked_data::entity::DATA_CUSTOM_NAME,
            None::<TextComponent>,
        );
        self.send_bedrock_actor_data(&bedrock_meta);
    }

    pub fn set_custom_name_visible(&self, visible: bool) {
        self.custom_name_visible.store(visible, Ordering::Relaxed);
        let mut bedrock_meta = SyncedActorDataList::new();
        if let Some(name) = &**self.custom_name.load() {
            bedrock_meta.set(
                entity_data_key::NAME,
                MetadataValue::String(name.clone().get_text()),
            );
        }
        bedrock_meta.set_flag(
            entity_data_key::FLAGS,
            entity_data_flag::SHOW_NAME as u8,
            visible,
        );
        bedrock_meta.set_flag(
            entity_data_key::FLAGS,
            entity_data_flag::ALWAYS_SHOW_NAME as u8,
            visible,
        );
        self.set_synced_data(tracked_data::entity::DATA_CUSTOM_NAME_VISIBLE, visible);
        self.send_bedrock_actor_data(&bedrock_meta);
    }

    pub fn is_silent(&self) -> bool {
        self.silent.load(Ordering::Relaxed)
    }

    pub fn set_silent(&self, silent: bool) {
        self.silent.store(silent, Ordering::Relaxed);
        self.set_synced_data(tracked_data::entity::DATA_SILENT, silent);
    }

    pub fn has_no_gravity(&self) -> bool {
        self.has_no_gravity.load(Ordering::Relaxed)
    }

    pub fn set_has_no_gravity(&self, no_gravity: bool) {
        self.has_no_gravity.store(no_gravity, Ordering::Relaxed);
        self.set_synced_data(tracked_data::entity::DATA_NO_GRAVITY, no_gravity);
    }

    pub fn set_sneaking(&self, sneaking: bool) {
        //assert!(self.sneaking.load(Relaxed) != sneaking);
        self.sneaking.store(sneaking, Relaxed);
        self.set_flag(Flag::Sneaking, sneaking);
    }

    pub fn is_sneaking(&self) -> bool {
        self.sneaking.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn is_swimming(&self) -> bool {
        self.swimming.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn is_visually_swimming(&self) -> bool {
        self.pose.load() == EntityPose::Swimming
    }

    #[must_use]
    pub fn is_visually_crawling(&self) -> bool {
        self.is_visually_swimming() && !self.is_in_water()
    }

    pub fn set_swimming(&self, swimming: bool) {
        if self.swimming.load(Ordering::Relaxed) != swimming {
            let mut event =
                crate::plugin::api::events::entity::entity_toggle_swim::EntityToggleSwimEvent::new(
                    self.entity_id,
                    swimming,
                );
            if let Some(server) = self.world.load().server.upgrade() {
                server.plugin_manager.fire_blocking(&server, &mut event);
            }
            if event.cancelled {
                return;
            }
            self.swimming.store(event.is_swimming, Relaxed);
            self.set_flag(Flag::Swimming, event.is_swimming);
        }
    }

    /// Sets whether the entity is invisible and sends updated metadata.
    pub fn set_invisible(&self, invisible: bool) {
        if self.invisible.load(Ordering::Relaxed) != invisible {
            self.invisible.store(invisible, Relaxed);
            self.set_flag(Flag::Invisible, invisible);
        }
    }

    pub fn set_glowing(&self, glowing: bool) {
        self.has_glowing_tag.store(glowing, Ordering::Relaxed);
        self.update_glowing_flag();
    }

    pub fn set_glowing_from_effect(&self, glowing: bool) {
        self.glowing_from_effect.store(glowing, Ordering::Relaxed);
        self.update_glowing_flag();
    }

    pub(super) fn update_glowing_flag(&self) {
        let glowing = self.has_glowing_tag.load(Ordering::Relaxed)
            || self.glowing_from_effect.load(Ordering::Relaxed);
        if self.glowing.load(Ordering::Relaxed) != glowing {
            self.glowing.store(glowing, Ordering::Relaxed);
            self.set_flag(Flag::Glowing, glowing);
        }
    }

    pub fn set_sprinting(&self, sprinting: bool) {
        //assert!(self.sprinting.load(Relaxed) != sprinting);
        self.sprinting.store(sprinting, Relaxed);
        self.set_flag(Flag::Sprinting, sprinting);
    }

    pub fn is_sprinting(&self) -> bool {
        self.sprinting.load(Ordering::Relaxed)
    }

    pub fn check_fall_flying(&self) -> bool {
        !self.on_ground.load(Relaxed)
    }

    pub fn set_fall_flying(&self, fall_flying: bool) {
        assert_ne!(self.fall_flying.load(Relaxed), fall_flying);
        self.fall_flying.store(fall_flying, Relaxed);
        self.set_flag(Flag::FallFlying, fall_flying);
    }

    pub fn is_fall_flying(&self) -> bool {
        self.fall_flying.load(Ordering::Relaxed)
    }

    pub(super) fn set_flag(&self, flag: Flag, value: bool) {
        let index = flag as u8;
        let mask = (1i8).wrapping_shl(index as u32);
        let new_je_flags = if value {
            self.flags.fetch_or(mask, Ordering::Relaxed) | mask
        } else {
            self.flags.fetch_and(!mask, Ordering::Relaxed) & !mask
        };

        self.set_synced_data(tracked_data::entity::DATA_SHARED_FLAGS_ID, new_je_flags);

        if let Some(bedrock_flag) = flag.to_bedrock() {
            let (key, index) = if bedrock_flag >= 64 {
                (entity_data_key::FLAGS_TWO, (bedrock_flag - 64) as u8)
            } else {
                (entity_data_key::FLAGS, bedrock_flag as u8)
            };

            if value {
                let mask = 1i64 << index;
                if key == entity_data_key::FLAGS {
                    self.bedrock_flags.fetch_or(mask, Ordering::Relaxed);
                } else {
                    self.bedrock_flags_two.fetch_or(mask, Ordering::Relaxed);
                }
            } else {
                let mask = !(1i64 << index);
                if key == entity_data_key::FLAGS {
                    self.bedrock_flags.fetch_and(mask, Ordering::Relaxed);
                } else {
                    self.bedrock_flags_two.fetch_and(mask, Ordering::Relaxed);
                }
            }

            let world = self.world.load();
            let chunk_pos = self.chunk_pos.load();
            let mut metadata = SyncedActorDataList(std::collections::HashMap::new());
            metadata.set(
                entity_data_key::FLAGS,
                MetadataValue::Int64(self.bedrock_flags.load(Ordering::Relaxed)),
            );
            metadata.set(
                entity_data_key::FLAGS_TWO,
                MetadataValue::Int64(self.bedrock_flags_two.load(Ordering::Relaxed)),
            );
            let packet = CSetActorData {
                target_runtime_id: VarULong(self.entity_id as u64),
                actor_data: metadata,
                synced_properties: PropertySyncData {
                    int_entries_list: std::collections::HashMap::new(),
                    float_entries_list: std::collections::HashMap::new(),
                },
                tick: VarULong(0),
            };
            world.broadcast_to_chunk_bedrock(chunk_pos, &packet);
        }
    }

    pub fn set_pose(&self, pose: EntityPose) {
        if self.pose.load() == pose {
            return;
        }

        let mut pose_event =
            crate::plugin::api::events::entity::entity_pose_change::EntityPoseChangeEvent::new(
                self.entity_id,
                (pose as u8).to_string(),
            );
        if let Some(server) = self.world.load().server.upgrade() {
            server
                .plugin_manager
                .fire_blocking(&server, &mut pose_event);
            if pose_event.cancelled {
                return;
            }
        }

        self.pose.store(pose);
        let pose = pose as i32;
        let mut bedrock_meta = SyncedActorDataList::new();
        bedrock_meta.set(entity_data_key::POSE_INDEX, MetadataValue::Int(pose));
        self.refresh_dimensions_into(&mut bedrock_meta);
        self.set_synced_data(tracked_data::entity::DATA_POSE, VarInt(pose));
        self.send_bedrock_actor_data(&bedrock_meta);
    }

    pub(super) fn join_team(&self, team_name: &str) {
        if self.entity_type == &EntityType::PLAYER {
            return;
        }
        let world = self.world.load();
        let scoreboard_name = self.entity_uuid.to_string();
        let mut scoreboard = world
            .scoreboard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if scoreboard.get_team(team_name).is_none() {
            tracing::warn!(
                "Unable to add entity to team \"{team_name}\" (that team probably doesn't exist)"
            );
            return;
        }
        if let Some(previous_team) = scoreboard
            .get_entity_team(&scoreboard_name)
            .map(|team| team.name.clone())
        {
            if previous_team == team_name {
                return;
            }
            scoreboard.remove_player_from_team(&*world, &previous_team, &scoreboard_name);
        }
        scoreboard.add_player_to_team(&*world, team_name, scoreboard_name);
    }
}
