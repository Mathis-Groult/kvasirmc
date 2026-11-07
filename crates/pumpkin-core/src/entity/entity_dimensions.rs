//! Size, eye height and bounding box refresh.

use super::Entity;
use pumpkin_data::entity::{EntityPose, EntityType};
use pumpkin_protocol::bedrock::client::set_actor_data::{
    MetadataValue, SyncedActorDataList, entity_data_flag, entity_data_key,
};
use pumpkin_util::math::{
    boundingbox::{BoundingBox, EntityDimensions},
    vector3::Vector3,
};
use std::sync::atomic::Ordering::{self, Relaxed};

impl Entity {
    pub fn bedrock_metadata(&self) -> SyncedActorDataList {
        if self.bedrock_flags.load(Ordering::Relaxed) == 0 {
            self.bedrock_flags.fetch_or(
                (1i64 << entity_data_flag::HAS_GRAVITY)
                    | (1i64 << entity_data_flag::CLIMB)
                    | (1i64 << entity_data_flag::HAS_COLLISION)
                    | (1i64 << entity_data_flag::BREATHING),
                Ordering::Relaxed,
            );
        }

        let mut metadata = SyncedActorDataList::new();
        let dimensions = self
            .default_dimensions
            .load()
            .unwrap_or_else(|| Self::type_dimensions(self.entity_type));
        metadata.set(
            entity_data_key::WIDTH,
            MetadataValue::Float(dimensions.width),
        );
        metadata.set(
            entity_data_key::HEIGHT,
            MetadataValue::Float(dimensions.height),
        );
        metadata.set(entity_data_key::SCALE, MetadataValue::Float(1.0));
        metadata.set(
            entity_data_key::FLAGS,
            MetadataValue::Int64(self.bedrock_flags.load(Ordering::Relaxed)),
        );
        metadata.set(
            entity_data_key::FLAGS_TWO,
            MetadataValue::Int64(self.bedrock_flags_two.load(Ordering::Relaxed)),
        );

        if let Some(name) = &**self.custom_name.load() {
            metadata.set(
                entity_data_key::NAME,
                MetadataValue::String(name.clone().get_text()),
            );
            if self.custom_name_visible.load(Ordering::Relaxed) {
                metadata.set_flag(
                    entity_data_key::FLAGS,
                    entity_data_flag::SHOW_NAME as u8,
                    true,
                );
                metadata.set_flag(
                    entity_data_key::FLAGS,
                    entity_data_flag::ALWAYS_SHOW_NAME as u8,
                    true,
                );
            }
        }

        metadata
    }

    /// The type's default size, vanilla `EntityType.getDimensions`.
    #[must_use]
    pub const fn type_dimensions(entity_type: &EntityType) -> EntityDimensions {
        EntityDimensions {
            width: entity_type.dimension[0],
            height: entity_type.dimension[1],
            eye_height: entity_type.eye_height,
        }
    }

    #[must_use]
    pub fn get_dimensions(&self, pose: EntityPose) -> EntityDimensions {
        if pose == EntityPose::Sleeping {
            return EntityDimensions::new(0.2, 0.2, 0.2);
        }
        if self.entity_type == &EntityType::PLAYER {
            match pose {
                EntityPose::FallFlying | EntityPose::Swimming | EntityPose::SpinAttack => {
                    return EntityDimensions::new(0.6, 0.6, 0.4);
                }
                EntityPose::Crouching => return EntityDimensions::new(0.6, 1.5, 1.27),
                EntityPose::Dying => return EntityDimensions::new(0.2, 0.2, 1.62),
                _ => {}
            }
        }
        self.default_dimensions
            .load()
            .unwrap_or_else(|| Self::type_dimensions(self.entity_type))
    }

    pub fn set_default_dimensions(&self, dimensions: EntityDimensions) {
        self.default_dimensions.store(Some(dimensions));
        self.refresh_dimensions();
    }

    pub fn refresh_dimensions(&self) {
        let mut metadata = SyncedActorDataList::new();
        self.refresh_dimensions_into(&mut metadata);
        self.send_bedrock_actor_data(&metadata);
    }

    pub(super) fn refresh_dimensions_into(&self, metadata: &mut SyncedActorDataList) {
        let old_dimensions = self.entity_dimension.load();
        let dimensions = self.get_dimensions(self.pose.load());
        self.entity_dimension.store(dimensions);
        let pos = self.pos.load();
        self.bounding_box
            .store(BoundingBox::new_from_pos(pos.x, pos.y, pos.z, &dimensions));
        metadata.set(
            entity_data_key::WIDTH,
            MetadataValue::Float(dimensions.width),
        );
        metadata.set(
            entity_data_key::HEIGHT,
            MetadataValue::Float(dimensions.height),
        );
        let is_small = dimensions.width <= 4.0 && dimensions.height <= 4.0;
        if self.tick_count.load(Relaxed) > 0
            && !self.no_physics.load(Relaxed)
            && is_small
            && (dimensions.width > old_dimensions.width
                || dimensions.height > old_dimensions.height)
            && self.entity_type != &EntityType::PLAYER
        {
            self.fudge_position_after_size_change(old_dimensions);
        }
    }

    pub fn fudge_position_after_size_change(&self, previous_dimensions: EntityDimensions) -> bool {
        let world = self.world.load();
        let Some(this) = world.get_entity_by_id(self.entity_id) else {
            return false;
        };
        let new_dimensions = self.get_dimensions(self.pose.load());
        let old_center =
            self.pos
                .load()
                .add_raw(0.0, f64::from(previous_dimensions.height) / 2.0, 0.0);
        let width_delta =
            f64::from((new_dimensions.width - previous_dimensions.width).max(0.0)) + 1.0E-6;
        let height_delta =
            f64::from((new_dimensions.height - previous_dimensions.height).max(0.0)) + 1.0E-6;
        let new_width = f64::from(new_dimensions.width);
        let new_height = f64::from(new_dimensions.height);
        let previous_height = f64::from(previous_dimensions.height);

        let allowed_centers =
            BoundingBox::of_size(old_center, width_delta, height_delta, width_delta);
        if let Some(free_position) = world.find_free_position(
            this.as_ref(),
            &allowed_centers,
            old_center,
            Vector3::new(new_width, new_height, new_width),
        ) {
            self.set_pos(free_position.add_raw(0.0, -new_height / 2.0, 0.0));
            return true;
        }
        if new_dimensions.width > previous_dimensions.width
            && new_dimensions.height > previous_dimensions.height
        {
            let allowed_centers_ignoring_y =
                BoundingBox::of_size(old_center, width_delta, 1.0E-6, width_delta);
            if let Some(free_position) = world.find_free_position(
                this.as_ref(),
                &allowed_centers_ignoring_y,
                old_center,
                Vector3::new(new_width, previous_height, new_width),
            ) {
                self.set_pos(free_position.add_raw(0.0, -previous_height / 2.0 + 1.0E-6, 0.0));
                return true;
            }
        }
        false
    }

    pub fn get_eye_height(&self) -> f64 {
        f64::from(self.entity_dimension.load().eye_height)
    }

    pub fn width(&self) -> f32 {
        self.entity_dimension.load().width
    }

    pub fn height(&self) -> f32 {
        self.entity_dimension.load().height
    }

    pub fn get_eye_pos(&self) -> Vector3<f64> {
        let pos = self.pos.load();
        Vector3::new(
            pos.x,
            pos.y + f64::from(self.entity_dimension.load().eye_height),
            pos.z,
        )
    }

    pub fn get_eye_y(&self) -> f64 {
        self.pos.load().y + f64::from(self.entity_dimension.load().eye_height)
    }
}
