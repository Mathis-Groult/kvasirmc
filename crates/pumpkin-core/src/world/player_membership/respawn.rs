//! The steps of `World::respawn_player`: where the player died, where it respawns (respawn
//! point or level spawn, plugin events, the world change), the packets that tell the client,
//! and the player's reset and registration in the target world.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use pumpkin_data::dimension::Dimension;
use pumpkin_inventory::Clearable;
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::java::client::play::{
    CGameEvent, CPlayerSpawnPosition, CRespawn, GameEvent, PlayerSpawnData,
};
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};
use pumpkin_util::resource_location::ResourceLocation;
use pumpkin_world::biome;
use tracing::{debug, warn};

use crate::entity::{EntityBase, player::Player};
use crate::plugin::player::{
    player_change_world::PlayerChangeWorldEvent, player_respawn::PlayerRespawnEvent,
};
use crate::server::Server;
use crate::world::World;

/// Where the player died: its dimension and its last position, rounded.
pub(super) fn death_location_of(player: &Player) -> (ResourceLocation, BlockPos) {
    let last_pos = player.get_entity().last_pos.load();
    let death_dimension = ResourceLocation::from(player.world().dimension.minecraft_name);
    let death_location = BlockPos(Vector3::new(
        last_pos.x.round() as i32,
        last_pos.y.round() as i32,
        last_pos.z.round() as i32,
    ));
    (death_dimension, death_location)
}

/// The player's respawn point could not be used: tells the client when one was set, and
/// forgets it unless it was forced.
pub(super) async fn forget_unusable_respawn_point(player: &Player) {
    if player
        .respawn_point
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_some()
    {
        player
            .send_client_packet(&CGameEvent::new(GameEvent::NoRespawnBlockAvailable, 0.0))
            .await;
        let mut guard = player
            .respawn_point
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(point) = guard.as_ref()
            && !point.force
        {
            *guard = None;
        }
    }
}

/// The level spawn of `default_world`: on top of its column, or at `spawn_y` over nothing.
pub(super) async fn level_spawn_position(
    default_world: &Arc<World>,
    spawn_x: i32,
    spawn_y: i32,
    spawn_z: i32,
    spawn_yaw: f32,
    spawn_pitch: f32,
) -> (Vector3<f64>, f32, f32, Dimension) {
    // FIXME: This spawn position calculation is incorrect. Should use vanilla's
    // proper spawn position calculation (see #1381). The y-level calculation
    // needs to account for spawn radius and find a safe spawn position.
    let chunk_pos = Vector2::new(spawn_x >> 4, spawn_z >> 4);
    default_world
        .level
        .get_or_fetch_chunk(chunk_pos, |_| ())
        .await;
    let top = default_world.get_top_block(Vector2::new(spawn_x, spawn_z));
    let pos_y = if top > default_world.dimension.min_y {
        top + 1
    } else {
        spawn_y
    };

    (
        Vector3::new(
            f64::from(spawn_x) + 0.5,
            f64::from(pos_y),
            f64::from(spawn_z) + 0.5,
        ),
        spawn_yaw,
        spawn_pitch,
        default_world.dimension.clone(),
    )
}

/// Lets plugins move the spawn position.
pub(super) async fn fire_spawn_location_event(
    server: Option<&Arc<Server>>,
    player: &Arc<Player>,
    position: Vector3<f64>,
) -> Vector3<f64> {
    let mut spawn_loc_event =
        crate::plugin::api::events::player::player_spawn_location::PlayerSpawnLocationEvent::new(
            player.clone(),
            position,
        );
    if let Some(s) = server {
        s.plugin_manager.fire(s, &mut spawn_loc_event).await;
    }
    spawn_loc_event.spawn_pos
}

impl World {
    /// The world a respawn in `respawn_dimension` lands in, after the cancellable
    /// `PlayerChangeWorldEvent`: `None` keeps the player in this world. A transfer to another
    /// world happens here.
    pub(super) async fn resolve_respawn_world(
        self: &Arc<Self>,
        server: Option<&Arc<Server>>,
        player: &Arc<Player>,
        respawn_dimension: &Dimension,
        (position, yaw, pitch): (Vector3<f64>, f32, f32),
    ) -> (Option<Arc<Self>>, Vector3<f64>, f32, f32) {
        // Candidate destination world for a cross-dimension respawn.
        let candidate_world = if *respawn_dimension == self.dimension {
            None
        } else {
            server.map_or_else(
                || {
                    warn!("Could not get server for cross-dimension respawn");
                    None
                },
                |s| {
                    let worlds = s.worlds.load();
                    worlds
                        .iter()
                        .find(|w| w.dimension == *respawn_dimension)
                        .cloned()
                },
            )
        };

        // Fire PlayerChangeWorldEvent (cancellable) before the transfer; it runs before
        // the non-cancellable PlayerRespawnEvent, which observes the resolved world.
        if let Some(new_world) = candidate_world {
            if let Some(s) = server {
                let mut event = PlayerChangeWorldEvent {
                    player: player.clone(),
                    previous_world: self.clone(),
                    new_world: new_world.clone(),
                    position,
                    yaw,
                    pitch,
                    cancelled: false,
                };
                s.plugin_manager.fire(s, &mut event).await;

                if event.cancelled {
                    (None, position, yaw, pitch)
                } else {
                    let destination = event.new_world;
                    let position = event.position;
                    let yaw = event.yaw;
                    let pitch = event.pitch;

                    // Skip the transfer if redirected back to the current world.
                    if destination.uuid != self.uuid {
                        self.move_respawning_player(player, &destination).await;
                    }

                    (Some(destination), position, yaw, pitch)
                }
            } else {
                warn!("Server dropped during cross-dimension respawn");
                (None, position, yaw, pitch)
            }
        } else {
            if *respawn_dimension != self.dimension {
                warn!(
                    "Target world {:?} not found, using world spawn in {:?}",
                    respawn_dimension, self.dimension
                );
            }
            (None, position, yaw, pitch)
        }
    }

    /// Moves a respawning player from this world into `destination`.
    async fn move_respawning_player(
        self: &Arc<Self>,
        player: &Arc<Player>,
        destination: &Arc<Self>,
    ) {
        debug!(
            "Cross-dimension respawn: {} -> {}",
            self.dimension.minecraft_name, destination.dimension.minecraft_name
        );

        // Detach from the old world before publishing into the new one, so no
        // observer sees the player in a world whose chunk manager doesn't match.
        self.remove_player(player, false).await;
        player.unload_watched_chunks(self).await;
        player.change_world_chunks(&self.level, destination);
        player.living_entity.entity.set_world(destination.clone());
        destination.players.rcu(|current_list| {
            let mut new_list = (**current_list).clone();
            new_list.push(player.clone());
            new_list
        });
    }

    /// Notifies plugins that the player has respawned (non-cancellable).
    pub(super) async fn fire_respawn_event(
        self: &Arc<Self>,
        player: &Arc<Player>,
        target_world: &Arc<Self>,
        (position, yaw, pitch): (Vector3<f64>, f32, f32),
        alive: bool,
    ) {
        if let Some(server) = self.server.upgrade() {
            server
                .plugin_manager
                .fire(
                    &server,
                    &mut PlayerRespawnEvent::new(
                        player.clone(),
                        self.clone(),
                        target_world.clone(),
                        position,
                        yaw,
                        pitch,
                        alive,
                    ),
                )
                .await;
        }
    }

    /// Respawn packet with the target dimension, then the spawn position.
    pub(super) async fn send_respawn_packets(
        player: &Arc<Player>,
        target_world: &Arc<Self>,
        (death_dimension, death_location): (ResourceLocation, BlockPos),
        data_kept: u8,
        (position, yaw, pitch): (Vector3<f64>, f32, f32),
    ) {
        // Send respawn packet with target dimension (using send_packet_now to ensure proper order)
        player
            .send_client_packet(&CRespawn::new(
                PlayerSpawnData::new(
                    target_world.dimension.clone(),
                    biome::hash_seed(target_world.level.seed.0),
                    player.gamemode.load() as u8,
                    player.gamemode.load() as i8,
                    false,
                    target_world.is_flat(),
                    Some((death_dimension, death_location)),
                    VarInt(player.get_entity().portal_cooldown.load(Ordering::Relaxed) as i32),
                    target_world.sea_level.into(),
                ),
                data_kept,
            ))
            .await;

        // Inform the client of the default spawn position so the client doesn't
        // fall back to (0, 2, 0) while the world reloads (fixes rubberbanding).
        // This must be sent after the CRespawn packet for proper client positioning.
        let spawn_block_pos = BlockPos(Vector3::new(
            position.x.round() as i32,
            position.y.round() as i32,
            position.z.round() as i32,
        ));
        let bedrock_dimension = match target_world.dimension.minecraft_name {
            "minecraft:the_nether" => 1,
            "minecraft:the_end" => 2,
            _ => 0,
        };
        player
            .send_packet_now_editioned(
                &CPlayerSpawnPosition::new(
                    spawn_block_pos,
                    yaw,
                    pitch,
                    target_world.dimension.minecraft_name.to_string(),
                ),
                &pumpkin_protocol::bedrock::client::CSetSpawnPosition {
                    spawn_position_type:
                        pumpkin_protocol::bedrock::client::SpawnPositionType::WorldRespawn,
                    block_position: spawn_block_pos,
                    dimension_type: bedrock_dimension.into(),
                    spawn_block_pos,
                },
            )
            .await;
    }

    /// Registers the respawned player in `target_world`, sends the world and the center chunk,
    /// then the teleport.
    pub(super) async fn place_respawned_player(
        self: &Arc<Self>,
        player: &Arc<Player>,
        target_world: &Arc<Self>,
        (position, yaw, pitch): (Vector3<f64>, f32, f32),
    ) {
        // Registered after positioning; a same-dimension respawn keeps its client entities.
        if target_world.uuid == self.uuid {
            // Clients never remove a dead entity themselves, so viewers would keep the corpse.
            target_world
                .entity_tracker
                .respawn_entity(&(player.clone() as Arc<dyn EntityBase>), target_world);
        } else {
            target_world.add_arriving_player(player);
            target_world
                .entity_tracker
                .repair_respawned_player(player, target_world);
        }

        // Load chunks and send world info FIRST (before teleport packet)
        target_world.send_world_info(player);
        target_world.send_center_chunk(player).await;

        // Send teleport packet after at least the center chunk was delivered
        player.request_teleport(position, yaw, pitch);

        target_world.refresh_java_player_for_bedrock(player).await;
    }
}

/// Resets a respawned player (state, permission level, hunger; experience and inventory unless
/// the game rule keeps them) and puts it at the respawn position before chunks load.
pub(super) fn reset_respawned_player(
    player: &Arc<Player>,
    keep_inventory: bool,
    (position, yaw, pitch): (Vector3<f64>, f32, f32),
) {
    player.living_entity.reset_state();

    player.send_permission_lvl_update();

    player.hunger_manager.restart();

    if !keep_inventory {
        player.set_experience(0, 0.0, 0);
        player.inventory.clear();
    }

    // Set entity position BEFORE loading chunks, so chunks load at the right location
    // This mirrors the initial spawn flow where update_position is called before teleport
    player.get_entity().set_pos(position);
    player.get_entity().set_rotation(yaw, pitch);
    player.get_entity().last_pos.store(position);

    // TODO: difficulty, exp bar, status effect
}
