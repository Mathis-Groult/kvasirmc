//! Moving the player between worlds and positions.

use super::super::EntityBase;
use super::Player;
use crate::net::ClientPlatform;
use crate::plugin::player::player_change_world::PlayerChangeWorldEvent;
use crate::plugin::player::player_teleport::PlayerTeleportEvent;
use crate::world::World;
use core::f32;
use pumpkin_data::dimension::Dimension;
use pumpkin_inventory::screen_handler::InventoryPlayer;
use pumpkin_macros::send_cancellable;
use pumpkin_protocol::bedrock::client::move_player::CMovePlayer as CBedrockMovePlayer;
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::codec::var_ulong::VarULong;
use pumpkin_protocol::java::client::play::{
    CPlayerPosition, CRespawn, CSetSelectedSlot, PlayerSpawnData,
};
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};
use pumpkin_util::resource_location::ResourceLocation;
use pumpkin_world::biome;
use std::sync::Arc;
use std::sync::atomic::Ordering;

impl Player {
    /// Teleports the player to a different world or dimension with an optional position, yaw, and pitch.
    #[expect(clippy::too_many_lines)]
    pub async fn teleport_world(
        self: &Arc<Self>,
        new_world: Arc<World>,
        position: Vector3<f64>,
        yaw: Option<f32>,
        pitch: Option<f32>,
    ) {
        let current_world = self.living_entity.entity.world.load_full();
        let yaw = yaw.unwrap_or(new_world.level_info.load().spawn_yaw);
        let pitch = pitch.unwrap_or(new_world.level_info.load().spawn_pitch);

        let Some(server) = new_world.server.upgrade() else {
            return;
        };

        send_cancellable! {{
            server;
            PlayerChangeWorldEvent {
                player: self.clone(),
                previous_world: current_world.clone(),
                new_world: new_world.clone(),
                position,
                yaw,
                pitch,
                cancelled: false,
            };

            'after: {
                // TODO: this is duplicate code from world
                let position = event.position;
                let yaw = event.yaw;
                let pitch = event.pitch;
                let new_world = event.new_world;

                self.set_client_loaded(false);
                let Some(player) = current_world.remove_player(self, false).await else {
                    return;
                };
               new_world.players.rcu(|current_list| {
                    let mut new_list = (**current_list).clone();
                    new_list.push(player.clone());
                    new_list
                });
                self.unload_watched_chunks(&current_world).await;

                self.change_world_chunks(&current_world.level, &new_world);
                self.living_entity.entity.set_world(new_world.clone());

                if new_world.dimension == pumpkin_data::dimension::Dimension::THE_NETHER {
                    self.trigger_advancement(crate::entity::player::advancement::trigger::AdvancementTrigger::EnterDimension {
                        dimension: "the_nether".to_string(),
                    });
                } else if new_world.dimension == pumpkin_data::dimension::Dimension::THE_END {
                    self.trigger_advancement(crate::entity::player::advancement::trigger::AdvancementTrigger::EnterDimension {
                        dimension: "the_end".to_string(),
                    });
                }

                let last_pos = self.living_entity.entity.last_pos.load();
                let death_dimension = ResourceLocation::from(self.world().dimension.minecraft_name);
                let death_location = BlockPos(Vector3::new(
                    last_pos.x.round() as i32,
                    last_pos.y.round() as i32,
                    last_pos.z.round() as i32,
                ));
                match self.client.as_ref() {
                    ClientPlatform::Java(java) => {
                        let packet = CRespawn::new(
                            PlayerSpawnData::new(
                                new_world.dimension.clone(),
                                biome::hash_seed(new_world.level.seed.0), // seed
                                self.gamemode.load() as u8,
                                self.previous_gamemode.load().unwrap_or(self.gamemode.load()) as i8,
                                false,
                                new_world.is_flat(),
                                Some((death_dimension, death_location)),
                                VarInt(self.get_entity().portal_cooldown.load(Ordering::Relaxed) as i32),
                                new_world.sea_level.into(),
                            ),
                            CRespawn::KEEP_ALL_DATA,
                        );
                        if let Ok(data) = java.serialize_packet(&packet) {
                            java.send_packet_now(data).await;
                        }
                    }
                    ClientPlatform::Bedrock(bedrock) => {
                        let bedrock_dimension = if new_world.dimension == Dimension::OVERWORLD {
                            0
                        } else if new_world.dimension == Dimension::THE_NETHER {
                            1
                        } else if new_world.dimension == Dimension::THE_END {
                            2
                        } else {
                            0
                        };
                        let pos_f32 = Vector3::new(position.x as f32, position.y as f32, position.z as f32);
                        let change_dim_packet = pumpkin_protocol::bedrock::client::CChangeDimension {
                            dimension_id: bedrock_dimension.into(),
                            position: pos_f32,
                            respawn: false,
                            loading_screen_id: None
                        };
                        if let Ok(data) = bedrock.serialize_packet(&change_dim_packet) {
                            bedrock.enqueue_packet(data).await;
                        }
                        self.bedrock_spawned.store(false, Ordering::Relaxed);
                    }
                }

                self.send_permission_lvl_update();

                player.get_entity().set_pos(position);
                player.get_entity().set_rotation(yaw, pitch);
                player.get_entity().last_pos.store(position);

                // Registered after positioning, so spawns carry the new position.
                new_world.add_arriving_player(&player);

                self.send_abilities_update();

                self.enqueue_set_held_item_packet(&CSetSelectedSlot::new(
                    self.get_inventory().get_selected_slot() as i8,
                ));

                self.on_screen_handler_opened(&self.player_screen_handler);

                self.send_health();

                new_world.send_world_info(&player);
                new_world.send_center_chunk(&player).await;

                player.request_teleport(position, yaw, pitch);

                let mut changed_world_event = crate::plugin::api::events::player::player_changed_world::PlayerChangedWorldEvent {
                    player: player.clone(),
                    from_world: current_world,
                    to_world: new_world,
                    cancelled: false,
                };
                server.plugin_manager.fire(&server, &mut changed_world_event).await;
            }
        }}
    }

    /// `yaw` and `pitch` are in degrees.
    /// Rarly used, for example when waking up the player from a bed or their first time spawn. Otherwise, the `teleport` method should be used.
    /// The player should respond with the `SConfirmTeleport` packet.
    pub fn request_teleport(&self, position: Vector3<f64>, yaw: f32, pitch: f32) {
        // This is the ultra special magic code used to create the teleport id
        // This returns the old value
        // This operation wraps around on overflow.
        let Some(server) = self.world().server.upgrade() else {
            return;
        };
        if let Some(player_arc) = self.world().get_player_by_uuid(self.gameprofile.id) {
            let mut event = PlayerTeleportEvent {
                player: player_arc,
                from: self.living_entity.entity.pos.load(),
                to: position,
                cancelled: false,
            };
            server.plugin_manager.fire_blocking(&server, &mut event);
            if event.cancelled {
                return;
            }
        }

        let i = self.teleport_id_count.fetch_add(1, Ordering::Relaxed);
        self.chunk_send_epoch.fetch_add(1, Ordering::Relaxed);
        let teleport_id = i + 1;
        self.living_entity.entity.set_pos(position);
        let entity = &self.living_entity.entity;
        entity.set_rotation(yaw, pitch);
        match self.client.as_ref() {
            ClientPlatform::Java(client) => {
                *self
                    .awaiting_teleport
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    Some((teleport_id.into(), position));
                let packet = CPlayerPosition::new(
                    teleport_id.into(),
                    position,
                    Vector3::new(0.0, 0.0, 0.0),
                    yaw,
                    pitch,
                    // TODO
                    Vec::new(),
                );
                if let Ok(data) = client.serialize_packet(&packet) {
                    client.try_enqueue_packet(data);
                }
            }
            ClientPlatform::Bedrock(client) => {
                let packet = CBedrockMovePlayer::new(
                    VarULong(self.entity_id() as u64),
                    Vector3::new(
                        position.x as f32,
                        position.y as f32 + entity.entity_type.eye_height,
                        position.z as f32,
                    ),
                    pitch,
                    yaw,
                    yaw,
                    CBedrockMovePlayer::MODE_TELEPORT,
                    false,
                    VarULong(0),
                    0,
                    0,
                    VarULong(self.tick_counter.load(Ordering::Relaxed).max(0) as u64),
                );
                if let Ok(data) = client.serialize_packet(&packet) {
                    client.try_enqueue_packet(data);
                }
            }
        }
    }
}
