//! Position, rotation and facing.

use super::Entity;
use pumpkin_data::Block;
use pumpkin_data::BlockState;
use pumpkin_data::block_properties::{Facing, HorizontalFacing};
use pumpkin_util::math::{
    boundingbox::BoundingBox, get_section_cord, position::BlockPos, vector2::Vector2,
    vector3::Vector3, wrap_degrees,
};
use std::sync::{
    Arc,
    atomic::Ordering::{self},
};

impl Entity {
    /// Updates the entity's position, block position, and chunk position.
    ///
    /// This function calculates the new position, block position, and chunk position based on the provided coordinates. If any of these values change, the corresponding fields are updated.
    pub fn set_pos(&self, new_position: Vector3<f64>) {
        let pos = self.pos.load();
        if pos != new_position {
            self.pos.store(new_position);
            self.bounding_box.store(BoundingBox::new_from_pos(
                new_position.x,
                new_position.y,
                new_position.z,
                &self.entity_dimension.load(),
            ));

            let floor_x = new_position.x.floor() as i32;
            let floor_y = new_position.y.floor() as i32;
            let floor_z = new_position.z.floor() as i32;

            let block_pos = self.block_pos.load();
            let block_pos_vec = block_pos.0;
            if floor_x != block_pos_vec.x
                || floor_y != block_pos_vec.y
                || floor_z != block_pos_vec.z
            {
                let new_block_pos = Vector3::new(floor_x, floor_y, floor_z);
                let new_bp = BlockPos(new_block_pos);
                self.block_pos.store(new_bp);

                let world = self.world.load();
                let biome = world.level.get_rough_biome(&new_bp);
                self.current_biome.store(Arc::new(biome));
                self.last_biome_update_pos.store(new_bp);

                let chunk_pos = self.chunk_pos.load();
                if get_section_cord(floor_x) != chunk_pos.x
                    || get_section_cord(floor_z) != chunk_pos.y
                {
                    self.chunk_pos.store(Vector2::new(
                        get_section_cord(new_block_pos.x),
                        get_section_cord(new_block_pos.z),
                    ));
                }
            }
        }
    }

    /// Returns entity rotation as vector
    pub fn rotation(&self) -> Vector3<f32> {
        let pitch_rad = self.pitch.load().to_radians();
        let yaw_rad = -self.yaw.load().to_radians();

        let cos_yaw = yaw_rad.cos();
        let sin_yaw = yaw_rad.sin();
        let cos_pitch = pitch_rad.cos();
        let sin_pitch = pitch_rad.sin();

        Vector3::new(sin_yaw * cos_pitch, -sin_pitch, cos_yaw * cos_pitch)
    }

    /// Changes this entity's pitch and yaw to look at target
    pub fn look_at(&self, target: Vector3<f64>) {
        let position = self.pos.load();
        let delta = target.sub(&position);
        let root = delta.x.hypot(delta.z);
        let pitch = wrap_degrees((-delta.y.atan2(root) as f32).to_degrees());
        let yaw = wrap_degrees((delta.z.atan2(delta.x) as f32).to_degrees() - 90.0);
        self.pitch.store(pitch);
        self.yaw.store(yaw);
    }

    /// Returns the block position of the block the (non-player) entity is standing on, if any.
    pub fn get_supporting_block_pos(&self) -> Option<BlockPos> {
        // Check if the entity is on the ground
        if !self.on_ground.load(Ordering::Relaxed) {
            return None;
        }

        self.supporting_block_pos.load()
    }

    pub(super) fn get_pos_with_y_offset(
        &self,
        offset: f64,
    ) -> (
        BlockPos,
        Option<&'static Block>,
        Option<&'static BlockState>,
    ) {
        if let Some(mut supporting_block) = self.supporting_block_pos.load() {
            if offset > 1.0e-5 {
                let (block, state) = self.world.load().get_block_and_state(&supporting_block);

                // if let Some(props) = block.properties(state.id) {
                //     let name = props.;

                //     if offset <= 0.5
                //         && (name == "OakFenceLikeProperties"
                //             || name == "ResinBrickWallLikeProperties"
                //             || name == "OakFenceGateLikeProperties"
                //                 && OakFenceGateLikeProperties::from_state_id(state.id)
                //                     .r#open)
                //     {
                //         return (supporting_block, Some(block), Some(state));
                //     }
                // }

                supporting_block.0.y = (self.pos.load().y - offset).floor() as i32;

                return (supporting_block, Some(block), Some(state));
            }

            return (supporting_block, None, None);
        }

        let mut block_pos = self.block_pos.load();

        block_pos.0.y = (self.pos.load().y - offset).floor() as i32;

        (block_pos, None, None)
    }

    pub(super) fn get_block_with_y_offset(
        &self,
        offset: f64,
    ) -> (BlockPos, &'static Block, &'static BlockState) {
        let (pos, block, state) = self.get_pos_with_y_offset(offset);

        if let (Some(b), Some(s)) = (block, state) {
            (pos, b, s)
        } else {
            let (b, s) = self.world.load().get_block_and_state(&pos);

            (pos, b, s)
        }
    }

    /// Sets the `Entity` yaw & pitch rotation
    pub fn set_rotation(&self, yaw: f32, pitch: f32) {
        // TODO
        self.yaw.store(yaw);
        // Java players have no separate head-yaw packet from the client- look yaw is also head yaw.
        // Bedrock `MovePlayer` / `MoveActorDelta` need this field, not only `yaw`.
        self.head_yaw.store(yaw);
        self.set_pitch(pitch);
    }

    pub fn set_pitch(&self, pitch: f32) {
        self.pitch.store(pitch.clamp(-90.0, 90.0) % 360.0);
    }

    pub fn get_horizontal_facing(&self) -> HorizontalFacing {
        let yaw = self.yaw.load();
        // Use vanilla's formula: floor(angle / 90.0 + 0.5) & 3
        let quarter_turns = ((yaw / 90.0) + 0.5).floor() as i32 & 3;
        match quarter_turns {
            0 => HorizontalFacing::South,
            1 => HorizontalFacing::West,
            2 => HorizontalFacing::North,
            _ => HorizontalFacing::East,
        }
    }

    pub fn get_rotation_16(&self) -> u8 {
        let adjusted_yaw = self.yaw.load().rem_euclid(360.0);

        ((adjusted_yaw / 22.5).round() as u8) % 16
    }

    pub fn get_flipped_rotation_16(&self) -> u8 {
        (self.get_rotation_16() + 8) % 16
    }

    pub fn get_facing(&self) -> Facing {
        let pitch = self.pitch.load().to_radians();
        let yaw = -self.yaw.load().to_radians();

        let (sin_p, cos_p) = pitch.sin_cos();
        let (sin_y, cos_y) = yaw.sin_cos();

        let x = sin_y * cos_p;
        let y = -sin_p;
        let z = cos_y * cos_p;

        let ax = x.abs();
        let ay = y.abs();
        let az = z.abs();

        if ax > ay && ax > az {
            if x > 0.0 { Facing::East } else { Facing::West }
        } else if ay > ax && ay > az {
            if y > 0.0 { Facing::Up } else { Facing::Down }
        } else if z > 0.0 {
            Facing::South
        } else {
            Facing::North
        }
    }

    pub fn get_entity_facing_order(&self) -> [Facing; 6] {
        let pitch = self.pitch.load().to_radians();
        let yaw = -self.yaw.load().to_radians();

        let sin_p = pitch.sin();
        let cos_p = pitch.cos();
        let sin_y = yaw.sin();
        let cos_y = yaw.cos();

        let east_west = if sin_y > 0.0 {
            Facing::East
        } else {
            Facing::West
        };
        let up_down = if sin_p < 0.0 {
            Facing::Up
        } else {
            Facing::Down
        };
        let south_north = if cos_y > 0.0 {
            Facing::South
        } else {
            Facing::North
        };

        let x_axis = sin_y.abs();
        let y_axis = sin_p.abs();
        let z_axis = cos_y.abs();
        let x_weight = x_axis * cos_p;
        let z_weight = z_axis * cos_p;

        let (first, second, third) = if x_axis > z_axis {
            if y_axis > x_weight {
                (up_down, east_west, south_north)
            } else if z_weight > y_axis {
                (east_west, south_north, up_down)
            } else {
                (east_west, up_down, south_north)
            }
        } else if y_axis > z_weight {
            (up_down, south_north, east_west)
        } else if x_weight > y_axis {
            (south_north, east_west, up_down)
        } else {
            (south_north, up_down, east_west)
        };

        [
            first,
            second,
            third,
            third.opposite(),
            second.opposite(),
            first.opposite(),
        ]
    }

    /// No visual-blocking block between the two eye positions (matching vanilla `ClipContext.Block.VISUAL`).
    #[must_use]
    pub fn has_line_of_sight(&self, other: &Self) -> bool {
        let from = self.get_eye_pos();
        let to = other.get_eye_pos();
        if from.squared_distance_to_vec(&to) > 128.0 * 128.0 {
            return false;
        }
        self.world
            .load_full()
            .clip(&pumpkin_world::clip::ClipContext::new(
                from,
                to,
                pumpkin_world::clip::ClipBlock::Visual,
                pumpkin_world::clip::ClipFluid::None,
            ))
            .is_miss
    }
}
