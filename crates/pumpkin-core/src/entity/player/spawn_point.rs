//! Respawn points, beds and sleeping.

use super::super::EntityBase;
use super::{CalculatedRespawnPoint, Player, RespawnPoint, statistics};
use crate::block::blocks::straw_bed::StrawBedBlock;
use core::f32;
use pumpkin_data::block_properties::HorizontalFacing;
use pumpkin_data::dimension::Dimension;
use pumpkin_data::entity::EntityPose;
use pumpkin_data::tag::Taggable;
use pumpkin_data::{Block, BlockStateId, tag};
use pumpkin_protocol::java::client::play::{Animation, CEntityAnimation, CPlayerSpawnPosition};
use pumpkin_util::math::{
    boundingbox::BoundingBox, position::BlockPos, vector2::Vector2, vector3::Vector3,
};
use std::sync::Arc;
use tracing::debug;

#[cfg(test)]
mod golden;

impl Player {
    pub fn set_respawn_point(
        &self,
        dimension: Dimension,
        block_pos: BlockPos,
        yaw: f32,
        pitch: f32,
        forced: bool,
    ) -> bool {
        if !forced
            && let Some(respawn_point) = self
                .respawn_point
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_ref()
            && dimension == respawn_point.dimension
            && block_pos == respawn_point.position
        {
            return false;
        }

        let mut final_block_pos = block_pos;
        if let Some(player_arc) = self.world().get_player_by_uuid(self.gameprofile.id)
            && let Some(server) = self.world().server.upgrade()
        {
            let mut event =
                crate::plugin::api::events::player::player_spawn_change::PlayerSpawnChangeEvent {
                    player: player_arc,
                    new_spawn: Some(block_pos),
                    forced,
                    cancelled: false,
                };
            server.plugin_manager.fire_blocking(&server, &mut event);
            if event.cancelled {
                return false;
            }
            if let Some(pos) = event.new_spawn {
                final_block_pos = pos;
            }
        }

        let bedrock_dimension = match dimension.minecraft_name {
            "minecraft:the_nether" => 1,
            "minecraft:the_end" => 2,
            _ => 0,
        };
        self.client.try_enqueue_packet_editioned(
            &CPlayerSpawnPosition::new(
                final_block_pos,
                yaw,
                pitch,
                dimension.minecraft_name.to_owned(),
            ),
            &pumpkin_protocol::bedrock::client::CSetSpawnPosition {
                spawn_position_type:
                    pumpkin_protocol::bedrock::client::SpawnPositionType::PlayerRespawn,
                block_position: final_block_pos,
                dimension_type: bedrock_dimension.into(),
                spawn_block_pos: final_block_pos,
            },
        );

        *self
            .respawn_point
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(RespawnPoint {
            dimension,
            position: final_block_pos,
            yaw,
            force: forced,
        });
        true
    }

    /// Calculates the player's respawn point based on stored spawn data.
    ///
    /// Returns `Some(CalculatedRespawnPoint)` if a valid respawn point exists, `None` otherwise.
    ///
    /// # Behavior
    /// - If `force` flag is set (via `/spawnpoint` command), validates the spawn position is safe
    ///   (both the block and block above allow mob spawn).
    /// - For beds: validates the bed block still exists and finds a valid spawn position around it.
    /// - For respawn anchors (Nether): validates the anchor has charges and finds a valid spawn position.
    /// - Returns `None` if the spawn block is invalid/missing (caller should send
    ///   `NoRespawnBlockAvailable` game event and use world spawn).
    ///
    /// # Note
    /// This function does NOT send any packets. The caller is responsible for
    /// sending `NoRespawnBlockAvailable` if this returns `None`.
    pub async fn calculate_respawn_point(&self) -> Option<CalculatedRespawnPoint> {
        let respawn_point = {
            let respawn_guard = self
                .respawn_point
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            respawn_guard.clone()?
        };
        let world = self.respawn_world(&respawn_point.dimension);
        let pos = &respawn_point.position;

        Self::fetch_chunks_around(&world, pos).await;

        // If force is set (from /spawnpoint command), validate position is safe
        if respawn_point.force {
            return Self::forced_respawn_point(&world, &respawn_point);
        }

        let (block, state_id) = world.get_block_and_state_id(pos);
        if block.has_tag(&tag::Block::MINECRAFT_BEDS) {
            return Self::bed_respawn_point(&world, &respawn_point, state_id);
        }
        if block == &Block::RESPAWN_ANCHOR {
            return Self::anchor_respawn_point(&world, &respawn_point, state_id);
        }

        None
    }

    /// The world the respawn point lives in: this player's own when the dimension matches, else
    /// the server's world for that dimension (this player's own without a server).
    fn respawn_world(&self, dimension: &Dimension) -> Arc<crate::world::World> {
        if self.world().dimension == *dimension {
            self.world()
        } else if let Some(server) = self.world().server.upgrade() {
            server.get_world_from_dimension(dimension)
        } else {
            self.world()
        }
    }

    /// Ensure chunks around the spawn position are fetched.
    async fn fetch_chunks_around(world: &Arc<crate::world::World>, pos: &BlockPos) {
        let min_chunk_x = (pos.0.x - 2) >> 4;
        let max_chunk_x = (pos.0.x + 2) >> 4;
        let min_chunk_z = (pos.0.z - 2) >> 4;
        let max_chunk_z = (pos.0.z + 2) >> 4;
        for cx in min_chunk_x..=max_chunk_x {
            for cz in min_chunk_z..=max_chunk_z {
                world
                    .level
                    .get_or_fetch_chunk(Vector2::new(cx, cz), |_| ())
                    .await;
            }
        }
    }

    /// A forced point (from /spawnpoint) holds while both the block and the block above allow a
    /// mob to stand there (neither is solid).
    fn forced_respawn_point(
        world: &Arc<crate::world::World>,
        respawn_point: &RespawnPoint,
    ) -> Option<CalculatedRespawnPoint> {
        let pos = &respawn_point.position;
        if world.get_block_state(pos).is_solid() || world.get_block_state(&pos.up()).is_solid() {
            return None;
        }
        let position = Vector3::new(
            f64::from(pos.0.x) + 0.5,
            f64::from(pos.0.y) + 0.1,
            f64::from(pos.0.z) + 0.5,
        );
        debug!(
            "Returning forced spawn point at {:?}, dimension: {:?}",
            position, respawn_point.dimension
        );
        Some(Self::respawn_point_at(respawn_point, position))
    }

    /// A bed point: the bed's facing picks the cells tried around it.
    fn bed_respawn_point(
        world: &Arc<crate::world::World>,
        respawn_point: &RespawnPoint,
        state_id: BlockStateId,
    ) -> Option<CalculatedRespawnPoint> {
        type BedProperties = pumpkin_data::block_properties::WhiteBedLikeProperties;

        let facing = BedProperties::from_state_id(state_id).facing;

        // Try positions around the bed based on facing direction
        // Vanilla tries multiple offset patterns; we use a simplified version
        let spawn_pos = Self::find_bed_spawn_position(world, &respawn_point.position, facing)?;
        Some(Self::respawn_point_at(respawn_point, spawn_pos))
    }

    /// An anchor point: needs a charge and a free cell around it, and uses one charge up.
    fn anchor_respawn_point(
        world: &Arc<crate::world::World>,
        respawn_point: &RespawnPoint,
        state_id: BlockStateId,
    ) -> Option<CalculatedRespawnPoint> {
        type AnchorProperties = pumpkin_data::block_properties::RespawnAnchorLikeProperties;

        let pos = &respawn_point.position;
        let anchor_props = AnchorProperties::from_state_id(state_id);
        let charges = anchor_props.charges;

        // Anchor needs at least 1 charge to work
        if charges == 0 {
            return None;
        }

        // Try positions around the anchor
        let spawn_pos = Self::find_anchor_spawn_position(world, pos)?;

        // Decrement charges after successful respawn position found
        let mut new_props = anchor_props;
        new_props.charges = charges - 1;
        world.set_block_state(
            pos,
            new_props.to_state_id(&Block::RESPAWN_ANCHOR),
            pumpkin_world::world::BlockFlags::NOTIFY_ALL,
        );

        Some(Self::respawn_point_at(respawn_point, spawn_pos))
    }

    fn respawn_point_at(
        respawn_point: &RespawnPoint,
        position: Vector3<f64>,
    ) -> CalculatedRespawnPoint {
        CalculatedRespawnPoint {
            position,
            yaw: respawn_point.yaw,
            pitch: 0.0,
            dimension: respawn_point.dimension.clone(),
        }
    }

    /// Find a valid spawn position around a bed.
    /// Vanilla uses a complex algorithm based on bed facing direction.
    /// We use a simplified version that tries cardinal directions first.
    pub(super) fn find_bed_spawn_position(
        world: &Arc<crate::world::World>,
        bed_pos: &BlockPos,
        facing: HorizontalFacing,
    ) -> Option<Vector3<f64>> {
        // Get offsets based on bed facing direction (vanilla-like order)
        let offsets = Self::get_bed_spawn_offsets(facing);

        for (dx, dz) in offsets {
            let check_pos = BlockPos(Vector3::new(
                bed_pos.0.x + dx,
                bed_pos.0.y,
                bed_pos.0.z + dz,
            ));

            if let Some(pos) = Self::find_respawn_pos(world, &check_pos) {
                return Some(pos);
            }

            // Also try one block down (for beds on elevated platforms)
            let check_pos_down = BlockPos(Vector3::new(
                bed_pos.0.x + dx,
                bed_pos.0.y - 1,
                bed_pos.0.z + dz,
            ));
            if let Some(pos) = Self::find_respawn_pos(world, &check_pos_down) {
                return Some(pos);
            }
        }

        // Try on the bed itself as last resort
        if let Some(pos) = Self::find_respawn_pos(world, bed_pos) {
            return Some(pos);
        }

        None
    }

    /// Get spawn position offsets around a bed based on facing direction.
    /// This is a simplified version of vanilla's getAroundBedOffsets.
    pub(super) fn get_bed_spawn_offsets(facing: HorizontalFacing) -> Vec<(i32, i32)> {
        let (fx, fz) = match facing {
            HorizontalFacing::North => (0, -1),
            HorizontalFacing::South => (0, 1),
            HorizontalFacing::West => (-1, 0),
            HorizontalFacing::East => (1, 0),
        };

        // Clockwise rotation
        let (rx, rz) = (-fz, fx);

        vec![
            (rx, rz),                   // Right of bed
            (-rx, -rz),                 // Left of bed
            (rx - fx, rz - fz),         // Right-back
            (-rx - fx, -rz - fz),       // Left-back
            (-fx, -fz),                 // Behind foot
            (-fx * 2, -fz * 2),         // Further behind
            (rx + fx, rz + fz),         // Right-front
            (-rx + fx, -rz + fz),       // Left-front
            (fx, fz),                   // In front
            (rx - fx * 2, rz - fz * 2), // Far right-back
        ]
    }

    /// Find a valid spawn position around a respawn anchor.
    pub(super) fn find_anchor_spawn_position(
        world: &Arc<crate::world::World>,
        anchor_pos: &BlockPos,
    ) -> Option<Vector3<f64>> {
        // Vanilla VALID_HORIZONTAL_SPAWN_OFFSETS
        let horizontal_offsets: [(i32, i32); 8] = [
            (0, -1),
            (-1, 0),
            (0, 1),
            (1, 0),
            (-1, -1),
            (1, -1),
            (-1, 1),
            (1, 1),
        ];

        // Try at same level, then one down, then one up
        for dy in [0, -1, 1] {
            for (dx, dz) in horizontal_offsets {
                let check_pos = BlockPos(Vector3::new(
                    anchor_pos.0.x + dx,
                    anchor_pos.0.y + dy,
                    anchor_pos.0.z + dz,
                ));

                if let Some(pos) = Self::find_respawn_pos(world, &check_pos) {
                    return Some(pos);
                }
            }
        }

        // Also try directly above the anchor
        let above_pos = anchor_pos.up();
        Self::find_respawn_pos(world, &above_pos)
    }

    /// Check if a position is valid for respawning (vanilla Dismounting.findRespawnPos logic).
    /// Returns the spawn position if valid, None otherwise.
    pub(super) fn find_respawn_pos(
        world: &Arc<crate::world::World>,
        pos: &BlockPos,
    ) -> Option<Vector3<f64>> {
        let (block, state) = world.get_block_and_state(pos);
        let below_state = world.get_block_state(&pos.down());

        // Check if block at position is invalid for spawn (e.g., inside solid block)
        if block.has_tag(&tag::Block::MINECRAFT_INVALID_SPAWN_INSIDE) {
            return None;
        }

        // Check if block above is also invalid
        let above_block = world.get_block(&pos.up());
        if above_block.has_tag(&tag::Block::MINECRAFT_INVALID_SPAWN_INSIDE) {
            return None;
        }

        // Need solid floor below or at position
        let has_floor = below_state.is_solid() || state.is_solid();
        if !has_floor {
            return None;
        }

        // Position must not be inside a solid block
        if state.is_solid() && !state.is_air() {
            return None;
        }

        // Create player-sized bounding box at this position
        let x = f64::from(pos.0.x) + 0.5;
        let y = f64::from(pos.0.y) + 0.1;
        let z = f64::from(pos.0.z) + 0.5;
        let spawn_pos = Vector3::new(x, y, z);

        // Player dimensions: 0.6 wide, 1.8 tall
        let half_width = 0.3;
        let height = 1.8;
        let player_box = BoundingBox::new(
            Vector3::new(x - half_width, y, z - half_width),
            Vector3::new(x + half_width, y + height, z + half_width),
        );

        // Check if the space is empty (no block collisions)
        if !world.is_space_empty(player_box) {
            return None;
        }

        Some(spawn_pos)
    }

    pub fn sleep(&self, bed_head_pos: BlockPos) {
        // TODO: Stop riding

        self.get_entity().set_pose(EntityPose::Sleeping);
        // Vanilla `setPosToBed`; `to_f64` already centers X and Z.
        self.living_entity
            .entity
            .set_pos(bed_head_pos.to_f64().add_raw(0.0, 0.6875, 0.0));
        self.get_entity().set_synced_data(
            pumpkin_data::tracked_data::player::SLEEPING_POS_ID,
            Some(bed_head_pos),
        );
        self.get_entity().set_velocity(Vector3::default());

        self.sleeping_since.store(Some(0));
        self.sleeping_bed_pos.store(Some(bed_head_pos));
        self.set_stat(
            statistics::StatisticCategory::Custom,
            statistics::CustomStatistic::TimeSinceRest as i32,
            0,
        );
    }

    #[must_use]
    pub fn is_sleeping(&self) -> bool {
        self.sleeping_bed_pos.load().is_some()
    }

    pub fn wake_up(&self) {
        let world = self.world();
        let Some(bed_pos) = self.sleeping_bed_pos.load() else {
            self.living_entity.entity.set_pose(EntityPose::Standing);
            self.sleeping_since.store(None);
            return;
        };

        if let Some(server) = world.server.upgrade()
            && let Some(player_arc) = world.get_player_by_uuid(self.gameprofile.id)
        {
            let mut event =
                crate::plugin::api::events::player::player_bed::PlayerBedLeaveEvent::new(
                    player_arc, bed_pos,
                );
            server.plugin_manager.fire_blocking(&server, &mut event);
        }

        let (bed, bed_state) = world.get_block_and_state_id(&bed_pos);
        if bed == &Block::STRAW_BED {
            StrawBedBlock::destroy_after_use(&world, bed_pos);
        } else if bed.has_tag(&tag::Block::MINECRAFT_BEDS) {
            crate::block::blocks::bed::BedBlock::set_occupied(
                false, &world, bed, &bed_pos, bed_state,
            );
        }

        self.living_entity.entity.set_pose(EntityPose::Standing);
        self.living_entity.entity.set_pos(self.position());
        self.living_entity.entity.set_synced_data(
            pumpkin_data::tracked_data::player::SLEEPING_POS_ID,
            None::<BlockPos>,
        );

        self.set_stat(
            statistics::StatisticCategory::Custom,
            statistics::CustomStatistic::TimeSinceRest as i32,
            0,
        );

        let chunk_pos = self.living_entity.entity.chunk_pos.load();
        world.broadcast_to_chunk(
            chunk_pos,
            &CEntityAnimation::new(self.entity_id().into(), Animation::LeaveBed),
        );

        self.sleeping_since.store(None);
        self.sleeping_bed_pos.store(None);
    }

    pub fn set_compass_target(&self, pos: pumpkin_util::math::position::BlockPos) {
        use pumpkin_protocol::java::client::play::CPlayerSpawnPosition;
        self.compass_target.store(Some(pos));
        self.try_send_client_packet(&CPlayerSpawnPosition::new(pos, 0.0, 0.0, String::new()));
    }

    pub fn get_compass_target(&self) -> Option<pumpkin_util::math::position::BlockPos> {
        self.compass_target.load()
    }

    pub fn set_respawn_location(&self, pos: pumpkin_util::math::position::BlockPos) {
        self.respawn_location.store(Some(pos));
    }

    pub fn get_respawn_location(&self) -> Option<pumpkin_util::math::position::BlockPos> {
        self.respawn_location.load()
    }
}
