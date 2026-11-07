//! Portals, teleporting and changing world.

use super::{Entity, EntityBase};
use crate::{
    entity::item::ItemEntity,
    world::{
        World,
        portal::{NetherPortal, PortalProcessor, PortalType, SourcePortalInfo},
    },
};
use pumpkin_data::dimension::Dimension;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_protocol::java::client::play::CEntityPositionSync;
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};
use std::sync::{
    Arc,
    atomic::Ordering::{self, Relaxed},
};

impl Entity {
    /// Updates the world reference for this entity.
    /// Called when the entity changes dimensions (e.g., through a nether portal).
    pub fn set_world(&self, world: Arc<World>) {
        let block_pos = self.block_pos.load();
        let biome = world.level.get_rough_biome(&block_pos);
        self.current_biome.store(Arc::new(biome));
        self.last_biome_update_pos.store(block_pos);
        if self.entity_type != &EntityType::PLAYER {
            self.bounding_box.set_index(&world.entity_index);
        }
        self.world.store(world);
    }

    pub(super) fn default_portal_cooldown(&self) -> u32 {
        if self.entity_type == &EntityType::PLAYER {
            10
        } else {
            300
        }
    }

    pub(super) fn tick_portal(&self, caller: &dyn EntityBase) {
        if self.portal_cooldown.load(Ordering::Relaxed) > 0 {
            self.portal_cooldown.fetch_sub(1, Ordering::Relaxed);
        }
        let Ok(mut manager_guard) = self.portal_manager.try_lock() else {
            return;
        };
        let mut should_remove = false;
        if let Some(portal_processor) = manager_guard.as_mut() {
            if portal_processor.process_portal_teleportation(&self.world.load(), caller, true) {
                self.portal_cooldown
                    .store(self.default_portal_cooldown(), Ordering::Relaxed);

                let world_clone = self.world.load_full();
                let portal_type = portal_processor.portal_type;
                let dest_world_opt = portal_processor.destination_world.clone();
                let src_portal = portal_processor.source_portal.clone();
                let entity_id = self.entity_id;
                let yaw = self.yaw.load();

                if let Some(server) = world_clone.server.upgrade() {
                    server.spawn_task(async move {
                        let Some(entity_arc) = world_clone.get_entity_by_id(entity_id) else {
                            return;
                        };
                        let transition = portal_type
                            .get_portal_destination(
                                &world_clone,
                                dest_world_opt,
                                entity_arc.as_ref(),
                                src_portal.as_ref(),
                            )
                            .await;

                        if let Some(transition) = transition {
                            let dest_world = transition.new_world.clone();
                            let yaw_val = transition.yaw;
                            let pitch = transition.pitch;
                            let teleport_pos = transition.position;

                            // Teleport the main entity
                            entity_arc.teleport(teleport_pos, yaw_val, pitch, dest_world.clone());

                            // Teleport all passengers recursively along with the vehicle
                            let yaw_delta = yaw_val.map(|y| y - yaw);
                            Self::teleport_passengers_recursive(
                                entity_arc.get_entity(),
                                teleport_pos,
                                yaw_delta,
                                &dest_world,
                            );
                        }
                    });
                }
            } else if portal_processor.portal_time == 0 {
                should_remove = true;
            }
        }
        if should_remove {
            *manager_guard = None;
        }
    }

    /// Recursively teleports all passengers (and their passengers) to the destination
    pub(super) fn teleport_passengers_recursive(
        entity: &Self,
        position: Vector3<f64>,
        yaw_delta: Option<f32>,
        dest_world: &Arc<World>,
    ) {
        let passengers = entity
            .passengers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        for passenger in passengers {
            let passenger_entity = passenger.get_entity();
            let passenger_yaw = yaw_delta.map(|delta| passenger_entity.yaw.load() + delta);
            passenger_entity.portal_cooldown.store(
                passenger_entity.default_portal_cooldown(),
                Ordering::Relaxed,
            );

            // Get nested passengers before teleporting
            let nested_passengers = passenger_entity
                .passengers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();

            passenger.teleport(position, passenger_yaw, None, dest_world.clone());

            // Recursively teleport nested passengers
            for nested in nested_passengers {
                let nested_entity = nested.get_entity();
                Self::teleport_passengers_recursive(nested_entity, position, yaw_delta, dest_world);
            }
        }
    }

    pub fn try_use_portal(&self, portal_world: Arc<World>, pos: BlockPos) {
        let mut portal_event =
            crate::plugin::api::events::entity::entity_portal::EntityPortalEvent::new(
                self.entity_id,
                pos,
            );
        if let Some(server) = self.world.load().server.upgrade() {
            server
                .plugin_manager
                .fire_blocking(&server, &mut portal_event);
        }
        if portal_event.cancelled {
            return;
        }

        // Passengers don't teleport independently - they wait for their vehicle
        if self.has_vehicle() {
            return;
        }

        if self.portal_cooldown.load(Ordering::Relaxed) > 0 {
            self.portal_cooldown
                .store(self.default_portal_cooldown(), Ordering::Relaxed);
            return;
        }

        let Some(server) = portal_world.server.upgrade() else {
            return;
        };

        if (portal_world.dimension == Dimension::THE_NETHER && !server.basic_config.allow_nether)
            || (portal_world.dimension == Dimension::THE_END && !server.basic_config.allow_end)
        {
            return;
        }

        let mut manager = self
            .portal_manager
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let world = self.world.load();
        if manager.is_none() {
            let portal_type = if portal_world.dimension == Dimension::THE_END
                || self.world.load().dimension == Dimension::THE_END
            {
                PortalType::End
            } else {
                PortalType::Nether
            };

            let mut new_manager = PortalProcessor::new(portal_type, pos, portal_world);

            let (block, state) = world.get_block_and_state(&pos);
            let source_axis = (block == &pumpkin_data::Block::NETHER_PORTAL).then(|| {
                let props = <pumpkin_data::block_properties::NetherPortalLikeProperties as pumpkin_data::block_properties::BlockProperties>::from_state_id(state.id, block);
                props.axis
            });

            if let Some(axis) = source_axis
                && let Some(portal) = NetherPortal::get_on_axis(&world, &pos, axis)
                && portal.was_already_valid()
            {
                new_manager.set_source_portal(SourcePortalInfo {
                    lower_corner: portal.lower_corner(),
                    axis: portal.axis(),
                    width: portal.width(),
                    height: portal.height(),
                });
            }

            *manager = Some(new_manager);
        } else if let Some(manager) = manager.as_mut() {
            manager.entry_position = pos;
            manager.inside_portal_this_tick = true;
            if manager.source_portal.is_none() {
                let (block, state) = world.get_block_and_state(&pos);
                if block == &pumpkin_data::Block::NETHER_PORTAL {
                    let props = <pumpkin_data::block_properties::NetherPortalLikeProperties as pumpkin_data::block_properties::BlockProperties>::from_state_id(state.id, block);
                    if let Some(portal) = NetherPortal::get_on_axis(&world, &pos, props.axis)
                        && portal.was_already_valid()
                    {
                        manager.set_source_portal(SourcePortalInfo {
                            lower_corner: portal.lower_corner(),
                            axis: portal.axis(),
                            width: portal.width(),
                            height: portal.height(),
                        });
                    }
                }
            }
        }
    }

    pub fn teleport(
        &self,
        position: Vector3<f64>,
        yaw: Option<f32>,
        pitch: Option<f32>,
        world: &World,
    ) {
        // Update server-side position and bounding box
        self.set_pos(position);
        if let Some(yaw) = yaw {
            self.yaw.store(yaw);
        }
        if let Some(pitch) = pitch {
            self.set_pitch(pitch);
        }
        // Update cache so we don't send rubberbanding deltas
        self.last_sent_pos.store(position);
        // TODO: use `pumpkin_util::math::pack_degrees`.
        if let Some(yaw) = yaw {
            self.last_sent_yaw
                .store((yaw * 256.0 / 360.0).rem_euclid(256.0) as u8, Relaxed);
            self.last_sent_head_yaw
                .store((yaw * 256.0 / 360.0).rem_euclid(256.0) as u8, Relaxed);
        }
        if let Some(pitch) = pitch {
            self.last_sent_pitch
                .store((pitch * 256.0 / 360.0).rem_euclid(256.0) as u8, Relaxed);
        }
        let chunk_pos = self.chunk_pos.load();
        world.broadcast_to_chunk(
            chunk_pos,
            &CEntityPositionSync::new(
                self.entity_id.into(),
                position,
                self.velocity.load(),
                yaw.unwrap_or(self.yaw.load()),
                pitch.unwrap_or(self.pitch.load()),
                self.on_ground.load(Ordering::SeqCst),
            ),
        );
    }

    pub fn spawn_at_location(&self, stack: ItemStack) {
        if stack.is_empty() {
            return;
        }
        let world = self.world.load();
        let item_entity = ItemEntity::new(
            Self::new(world.clone(), self.pos.load(), &EntityType::ITEM),
            stack,
        );
        world.spawn_entity(Arc::new(item_entity));
    }
}
