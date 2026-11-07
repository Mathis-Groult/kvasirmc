//! Vehicles and passengers.

use super::{Entity, EntityBase};
use pumpkin_data::BlockDirection;
use pumpkin_data::entity::EntityPose;
use pumpkin_data::fluid::Fluid;
use pumpkin_nbt::{compound::NbtCompound, tag::NbtTag};
use pumpkin_protocol::{
    PositionFlag,
    codec::var_int::VarInt,
    java::client::play::{CPlayerPosition, CSetPassengers},
};
use pumpkin_util::math::{boundingbox::BoundingBox, position::BlockPos, vector3::Vector3};
use std::sync::{Arc, atomic::Ordering::Relaxed};

impl Entity {
    #[must_use]
    pub fn root_vehicle_id(&self) -> i32 {
        let mut id = self.entity_id;
        let mut vehicle = self.get_vehicle();
        while let Some(current) = vehicle {
            id = current.get_entity().entity_id;
            vehicle = current.get_entity().get_vehicle();
        }
        id
    }

    pub fn has_passengers(&self) -> bool {
        !self
            .passengers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
    }

    pub fn has_passenger(&self, id: i32) -> bool {
        self.passengers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .any(|passenger| passenger.get_entity().entity_id == id)
    }

    pub fn has_vehicle(&self) -> bool {
        self.vehicle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
    }

    pub fn get_vehicle(&self) -> Option<Arc<dyn EntityBase>> {
        self.vehicle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Vanilla `getRootVehicle`, `None` when not riding.
    pub fn get_root_vehicle(&self) -> Option<Arc<dyn EntityBase>> {
        let mut root = self.get_vehicle()?;
        while let Some(vehicle) = root.get_entity().get_vehicle() {
            root = vehicle;
        }
        Some(root)
    }

    /// Vanilla `isPassengerOfSameVehicle`: same root vehicle, or the same entity.
    #[must_use]
    pub fn is_passenger_of_same_vehicle(&self, other: &Self) -> bool {
        self.root_vehicle_id() == other.root_vehicle_id()
    }

    pub fn add_passenger(&self, vehicle: Arc<dyn EntityBase>, passenger: Arc<dyn EntityBase>) {
        let mut mount_event =
            crate::plugin::api::events::entity::entity_mount::EntityMountEvent::new(
                passenger.get_entity().entity_id,
                self.entity_id,
            );
        let mut vehicle_enter =
            crate::plugin::api::events::vehicle::vehicle_enter::VehicleEnterEvent::new(
                self.entity_id,
                passenger.get_entity().entity_id,
            );
        if let Some(server) = self.world.load().server.upgrade() {
            server
                .plugin_manager
                .fire_blocking(&server, &mut mount_event);
            server
                .plugin_manager
                .fire_blocking(&server, &mut vehicle_enter);
        }
        if mount_event.cancelled || vehicle_enter.cancelled {
            return;
        }

        let passenger_entity = passenger.get_entity();
        *passenger_entity
            .vehicle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(vehicle);

        let mut passengers = self
            .passengers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if let Some(player) = passenger.get_player() {
            let other_passenger_type = passengers
                .first()
                .map(|p| format!("minecraft:{}", p.get_entity().entity_type.resource_name));
            player.trigger_advancement(
                crate::entity::player::advancement::trigger::AdvancementTrigger::StartedRiding {
                    vehicle_type: Some(format!("minecraft:{}", self.entity_type.resource_name)),
                    passenger_type: other_passenger_type,
                },
            );
        } else {
            for p in passengers.iter() {
                if let Some(player) = p.get_player() {
                    player.trigger_advancement(
                        crate::entity::player::advancement::trigger::AdvancementTrigger::StartedRiding {
                            vehicle_type: Some(format!(
                                "minecraft:{}",
                                self.entity_type.resource_name
                            )),
                            passenger_type: Some(format!(
                                "minecraft:{}",
                                passenger_entity.entity_type.resource_name
                            )),
                        },
                    );
                }
            }
        }

        passengers.push(passenger);

        let passenger_ids: Vec<VarInt> = passengers
            .iter()
            .map(|p| VarInt(p.get_entity().entity_id))
            .collect();

        let world = self.world.load();
        let chunk_pos = self.chunk_pos.load();
        world.broadcast_to_chunk(
            chunk_pos,
            &CSetPassengers::new(VarInt(self.entity_id), &passenger_ids),
        );
    }

    pub fn add_passenger_silently(
        &self,
        vehicle: Arc<dyn EntityBase>,
        passenger: Arc<dyn EntityBase>,
    ) {
        *passenger
            .get_entity()
            .vehicle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(vehicle);
        self.passengers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(passenger);
    }

    pub(super) fn write_passengers(&self, nbt: &mut NbtCompound) {
        let passengers = self
            .passengers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let passengers: Vec<NbtTag> = passengers
            .iter()
            .filter(|passenger| {
                passenger.get_player().is_none() && !passenger.get_entity().is_removed()
            })
            .map(|passenger| {
                let mut tag = NbtCompound::new();
                passenger.write_nbt(&mut tag);
                NbtTag::Compound(tag)
            })
            .collect();
        if !passengers.is_empty() {
            nbt.put_list("Passengers", passengers);
        }
    }

    pub(crate) fn remove_passenger_on_disconnect(&self, passenger_id: i32) {
        let mut passengers = self
            .passengers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(index) = passengers
            .iter()
            .position(|passenger| passenger.get_entity().entity_id == passenger_id)
        {
            let passenger = passengers.remove(index);
            *passenger
                .get_entity()
                .vehicle
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        }

        let passenger_ids: Vec<VarInt> = passengers
            .iter()
            .map(|passenger| VarInt(passenger.get_entity().entity_id))
            .collect();
        drop(passengers);

        self.world.load().broadcast_to_chunk(
            self.chunk_pos.load(),
            &CSetPassengers::new(VarInt(self.entity_id), &passenger_ids),
        );
    }

    pub fn remove_passenger_sync(&self, passenger_id: i32) {
        self.remove_passenger_on_disconnect(passenger_id);
    }

    pub fn remove_passenger(&self, passenger_id: i32) {
        self.remove_passenger_internal(passenger_id, true);
    }

    pub fn remove_passenger_before_teleport(&self, passenger_id: i32) {
        self.remove_passenger_internal(passenger_id, false);
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn remove_passenger_internal(&self, passenger_id: i32, reposition: bool) {
        let mut dismount_event =
            crate::plugin::api::events::entity::entity_dismount::EntityDismountEvent::new(
                passenger_id,
                self.entity_id,
            );
        let mut vehicle_exit =
            crate::plugin::api::events::vehicle::vehicle_exit::VehicleExitEvent::new(
                self.entity_id,
                passenger_id,
            );
        if let Some(server) = self.world.load().server.upgrade() {
            server
                .plugin_manager
                .fire_blocking(&server, &mut dismount_event);
            server
                .plugin_manager
                .fire_blocking(&server, &mut vehicle_exit);
        }
        if dismount_event.cancelled || vehicle_exit.cancelled {
            return;
        }

        let (removed_passenger, passenger_ids) = {
            let mut passengers = self
                .passengers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let removed_passenger = passengers
                .iter()
                .position(|p| p.get_entity().entity_id == passenger_id)
                .map(|idx| {
                    let passenger = passengers.remove(idx);
                    *passenger
                        .get_entity()
                        .vehicle
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
                    passenger
                });

            let passenger_ids: Vec<VarInt> = passengers
                .iter()
                .map(|p| VarInt(p.get_entity().entity_id))
                .collect();
            (removed_passenger, passenger_ids)
        };

        let chunk_pos = self.chunk_pos.load();

        if let Some(passenger) = removed_passenger {
            let vehicle_box = self.bounding_box.load();
            let passenger_entity = passenger.get_entity();

            // Pre-allocate teleport ID and block movement packets BEFORE sending
            // CSetPassengers. This prevents a race condition where the client receives
            // the dismount packet, sends stale position packets from the old riding
            // position, and the server processes them before the teleport arrives.
            let teleport_id = if reposition && let Some(player) = passenger.get_player() {
                let id = player
                    .teleport_id_count
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    + 1;
                // Use fallback position as placeholder — updated below with real position
                let placeholder =
                    Vector3::new(self.pos.load().x, vehicle_box.max.y, self.pos.load().z);
                *player
                    .awaiting_teleport
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    Some((id.into(), placeholder));
                Some(id)
            } else {
                None
            };

            // Vanilla: ridingCooldown = 60 (prevents immediate re-mount)
            passenger_entity.riding_cooldown.store(60, Relaxed);
            // TODO: world.emitGameEvent(passenger, GameEvent.ENTITY_DISMOUNT, vehicle.pos)

            // Send CSetPassengers directly to the dismounting player before broadcasting it.
            let world = self.world.load();
            let passengers_packet = CSetPassengers::new(VarInt(self.entity_id), &passenger_ids);
            if let Some(player) = passenger.get_player() {
                player.try_send_client_packet(&passengers_packet);
                world.broadcast_to_chunk_except(
                    chunk_pos,
                    &[player.get_entity().entity_uuid],
                    &passengers_packet,
                );
            } else {
                world.broadcast_to_chunk(chunk_pos, &passengers_packet);
            }

            if !reposition {
                return;
            }

            // Calculate dismount directions and offsets (vanilla DismountHelper)
            let vehicle_yaw = self.yaw.load();
            // Wrap yaw to 0..360 range
            let wrapped_yaw = (vehicle_yaw % 360.0 + 360.0) % 360.0;
            let forward_dir = if !(45.0..315.0).contains(&wrapped_yaw) {
                BlockDirection::South
            } else if (45.0..135.0).contains(&wrapped_yaw) {
                BlockDirection::West
            } else if (135.0..225.0).contains(&wrapped_yaw) {
                BlockDirection::North
            } else {
                BlockDirection::East
            };

            let get_step = |dir: BlockDirection| -> (i32, i32) {
                match dir {
                    BlockDirection::North => (0, -1),
                    BlockDirection::South => (0, 1),
                    BlockDirection::East => (1, 0),
                    BlockDirection::West => (-1, 0),
                    _ => (0, 0),
                }
            };

            let get_clockwise = |dir: BlockDirection| -> BlockDirection {
                match dir {
                    BlockDirection::North => BlockDirection::East,
                    BlockDirection::East => BlockDirection::South,
                    BlockDirection::South => BlockDirection::West,
                    BlockDirection::West => BlockDirection::North,
                    other => other,
                }
            };

            let get_opposite = |dir: BlockDirection| -> BlockDirection {
                match dir {
                    BlockDirection::North => BlockDirection::South,
                    BlockDirection::South => BlockDirection::North,
                    BlockDirection::East => BlockDirection::West,
                    BlockDirection::West => BlockDirection::East,
                    other => other,
                }
            };

            let right_dir = get_clockwise(forward_dir);
            let left_dir = get_opposite(right_dir);
            let back_dir = get_opposite(forward_dir);

            let (fx, fz) = get_step(forward_dir);
            let (rx, rz) = get_step(right_dir);
            let (lx, lz) = get_step(left_dir);
            let (bx, bz) = get_step(back_dir);

            let offsets = [
                (rx, rz),
                (lx, lz),
                (bx + rx, bz + rz),
                (bx + lx, bz + lz),
                (fx + rx, fz + rz),
                (fx + lx, fz + lz),
                (bx, bz),
                (fx, fz),
            ];

            let target_block_y = vehicle_box.max.y.floor() as i32;
            let below_pos = BlockPos(Vector3::new(
                self.pos.load().x.floor() as i32,
                target_block_y - 1,
                self.pos.load().z.floor() as i32,
            ));

            let below_state_id = world.get_block_state_id(&below_pos);
            // Vanilla: isWater checks specifically for water fluid, not any fluid
            let is_water = Fluid::from_state_id(below_state_id)
                .is_some_and(|f| f.id == Fluid::WATER.id || f.id == Fluid::FLOWING_WATER.id);

            let fallback_pos =
                Vector3::new(self.pos.load().x, vehicle_box.max.y, self.pos.load().z);

            let dismount_pos = if is_water {
                fallback_pos
            } else {
                // Vanilla checks Standing, Crouching, Swimming poses and their respective height checks
                let poses_and_heights = [
                    (EntityPose::Standing, vec![0, 1, -1]),
                    (EntityPose::Crouching, vec![0, 1, -1]),
                    (EntityPose::Swimming, vec![0, 1]),
                ];

                let vehicle_block_pos = self.block_pos.load();
                let mut found = None;

                'search: for (pose, y_offsets) in poses_and_heights {
                    let dims = passenger_entity.get_dimensions(pose);

                    for y_offset in y_offsets {
                        for &(ox, oz) in &offsets {
                            let target_block_x = vehicle_block_pos.0.x + ox;
                            let target_block_y = vehicle_block_pos.0.y + y_offset;
                            let target_block_z = vehicle_block_pos.0.z + oz;

                            let target_pos = BlockPos(Vector3::new(
                                target_block_x,
                                target_block_y,
                                target_block_z,
                            ));
                            let height = world.get_dismount_height(&target_pos);

                            if height.is_finite() && height < 1.0 {
                                let location = Vector3::new(
                                    f64::from(target_block_x) + 0.5,
                                    f64::from(target_block_y) + height,
                                    f64::from(target_block_z) + 0.5,
                                );

                                let bbox = BoundingBox::new_from_pos(
                                    location.x, location.y, location.z, &dims,
                                );
                                if world.is_space_empty(bbox) {
                                    found = Some((location, pose));
                                    break 'search;
                                }
                            }
                        }
                    }
                }

                if let Some((pos, pose)) = found {
                    if pose != EntityPose::Standing {
                        passenger_entity.set_pose(pose);
                    }
                    pos
                } else {
                    // Try dismounting directly on top of the vehicle as fallback
                    let mut found_fallback = None;
                    let vehicle_top = vehicle_box.max.y;

                    let poses = [
                        EntityPose::Standing,
                        EntityPose::Crouching,
                        EntityPose::Swimming,
                    ];

                    for pose in poses {
                        let dims = passenger_entity.get_dimensions(pose);
                        let bbox = BoundingBox::new_from_pos(
                            self.pos.load().x,
                            vehicle_top,
                            self.pos.load().z,
                            &dims,
                        );
                        if world.is_space_empty(bbox) {
                            found_fallback = Some((
                                Vector3::new(self.pos.load().x, vehicle_top, self.pos.load().z),
                                pose,
                            ));
                            break;
                        }
                    }

                    if let Some((pos, pose)) = found_fallback {
                        if pose != EntityPose::Standing {
                            passenger_entity.set_pose(pose);
                        }
                        pos
                    } else {
                        fallback_pos
                    }
                }
            };

            // Clean up any remaining reference to the dismounted passenger.
            passenger_entity.set_pos(dismount_pos);

            // Phase 2: Teleport to safety (unblocks movement)
            if let Some(player) = passenger.get_player() {
                if let Some(id) = teleport_id {
                    player.get_entity().set_pos(dismount_pos);
                    // Update awaiting_teleport with the real dismount position
                    *player
                        .awaiting_teleport
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) =
                        Some((id.into(), dismount_pos));
                    // Use send_client_packet so the teleport goes through
                    // the same packet queue as CSetPassengers, preserving send order.
                    // Vanilla uses DELTA | ROT flags: position absolute, delta/rotation relative.
                    // With rotation relative and yaw/pitch=0, the client preserves its current look.
                    player.try_send_client_packet(&CPlayerPosition::new(
                        id.into(),
                        dismount_pos,
                        Vector3::new(0.0, 0.0, 0.0),
                        0.0,
                        0.0,
                        vec![
                            PositionFlag::DeltaX,
                            PositionFlag::DeltaY,
                            PositionFlag::DeltaZ,
                            PositionFlag::YRot,
                            PositionFlag::XRot,
                        ],
                    ));
                }

                // Vanilla: setSneaking(false) after dismount via sneak input
                if passenger_entity.sneaking.load(Relaxed) {
                    passenger_entity.set_sneaking(false);
                }
            } else {
                passenger_entity.set_pos(dismount_pos);
            }
        } else {
            // No passenger was removed, still need to broadcast the passenger list
            let world = self.world.load();
            world.broadcast_to_chunk(
                chunk_pos,
                &CSetPassengers::new(VarInt(self.entity_id), &passenger_ids),
            );
        }
    }
}
