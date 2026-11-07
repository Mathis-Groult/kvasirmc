//! Bringing a Java player into the world.

use super::{World, chunker};
use crate::{
    command::client_suggestions,
    entity::{EntityBase, player::Player},
    net::ClientPlatform,
    plugin::player::player_join::PlayerJoinEvent,
    server::Server,
};
use bytes::BufMut;
use pumpkin_config::BasicConfiguration;
use pumpkin_data::data_component_impl::EquipmentSlot;
use pumpkin_data::entity::EntityType;
use pumpkin_data::packet::CURRENT_MC_VERSION;
use pumpkin_data::translation;
use pumpkin_inventory::crafting::recipe_provider::RecipeProvider;
use pumpkin_inventory::screen_handler::InventoryPlayer;
use pumpkin_protocol::bedrock::client::set_actor_data::{CSetActorData, PropertySyncData};
use pumpkin_protocol::java::client::play::PlayerSpawnData;
use pumpkin_protocol::java::client::play::{
    CPlayerSpawnPosition, CRecipeBookAdd, CRecipeBookSettings,
};
use pumpkin_protocol::java::client::play::{CSetEntityMetadata, Metadata};
use pumpkin_protocol::{
    bedrock::client::{
        common::BuildPlatform,
        player_list::{CPlayerList, PlayerListEntry},
    },
    codec::{var_int::VarInt, var_long::VarLong, var_ulong::VarULong},
    java::{
        self,
        client::play::{
            CGameEvent, CLogin, CPlayerInfoUpdate, CSetSelectedSlot, CSpawnEntity, GameEvent,
            InitChat, PlayerAction, PlayerInfoFlags,
        },
    },
};
use pumpkin_protocol::{
    codec::item_stack_seralizer::ItemStackSerializer, java::client::play::CSetEquipment,
};
use pumpkin_util::GameMode;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};
use pumpkin_util::resource_location::ResourceLocation;
use pumpkin_util::text::{TextComponent, color::NamedColor};
use pumpkin_util::version::JavaMinecraftVersion;
use pumpkin_world::biome;
use std::sync::Arc;
use std::{collections::HashMap, sync::atomic::Ordering};
use tracing::{debug, info};

/// Where and how a joining player appears.
#[derive(Clone, Copy)]
struct JoinPose {
    position: Vector3<f64>,
    yaw: f32,
    pitch: f32,
}

impl World {
    pub async fn spawn_java_player(
        &self,
        base_config: &BasicConfiguration,
        player: &Arc<Player>,
        server: &Arc<Server>,
    ) {
        // This code follows the vanilla packet order
        let entity_id = player.entity_id();
        let gamemode = player.gamemode.load();
        debug!(
            "spawning player {}, entity id {}",
            player.gameprofile.name, entity_id
        );

        let Some(client) = player.client.java() else {
            return;
        };
        self.send_login_packet(base_config, player, server, client, entity_id, gamemode)
            .await;

        self.pair_new_player_with_tracked_entities(player);

        self.send_server_state_to_joiner(player, server, client)
            .await;

        let pose = self.join_pose(player).await;

        if !self.place_and_send_spawn_chunk(player, client, &pose).await {
            return;
        }

        let velocity = player.living_entity.entity.velocity.load();

        debug!("Sending player teleport to {}", player.gameprofile.name);
        player.request_teleport(pose.position, pose.yaw, pose.pitch);

        self.announce_in_tab_list(player, entity_id, gamemode);
        self.send_tab_list_of_others(base_config, player, client)
            .await;
        self.spawn_for_others(player, entity_id, &pose, velocity);
        self.spawn_others_for(player, client).await;
        self.send_initial_state(player, client).await;
        self.send_weather(client).await;
        Self::send_bossbars(player, server);
        self.send_player_state(player, server).await;
        self.announce_join(player, server).await;
    }

    /// Sends the login packet that opens the play session of a Java player.
    async fn send_login_packet(
        &self,
        base_config: &BasicConfiguration,
        player: &Arc<Player>,
        server: &Arc<Server>,
        client: &crate::net::java::JavaClient,
        entity_id: i32,
        gamemode: GameMode,
    ) {
        let dimensions: Vec<ResourceLocation> = server
            .dimensions
            .iter()
            .map(|d| ResourceLocation::from(d.minecraft_name))
            .collect();

        // Send the login packet for our new player
        client
            .send_packet(&CLogin::new(
                entity_id,
                base_config.hardcore,
                &dimensions,
                server
                    .advanced_config
                    .networking
                    .java
                    .max_players
                    .try_into()
                    .unwrap_or(u16::MAX.into()),
                server
                    .advanced_config
                    .networking
                    .java
                    .view_distance
                    .get()
                    .into(), //  TODO: view distance
                server
                    .advanced_config
                    .networking
                    .java
                    .simulation_distance
                    .get()
                    .into(), // TODO: sim view dinstance
                false,
                true,
                false,
                PlayerSpawnData::new(
                    self.dimension.clone(),
                    biome::hash_seed(self.level.seed.0), // seed
                    gamemode as u8,
                    player
                        .previous_gamemode
                        .load()
                        .map_or(-1, |gamemode| gamemode as i8),
                    false,
                    self.is_flat(),
                    None,
                    VarInt(player.get_entity().portal_cooldown.load(Ordering::Relaxed) as i32),
                    self.sea_level.into(),
                ),
                server.advanced_config.networking.java.online_mode,
                // This should stay true even when reports are disabled.
                // It prevents the annoying popup when joining the server.
                true,
            ))
            .await;
    }

    /// Sends the server state a joining player needs before its position: ticking, permissions, difficulty, commands and tags.
    async fn send_server_state_to_joiner(
        &self,
        player: &Arc<Player>,
        server: &Arc<Server>,
        client: &crate::net::java::JavaClient,
    ) {
        // Send the current ticking state to the new player so they are in sync.
        server.tick_rate_manager.update_joining_player(player).await;

        // Permissions, i.e. the commands a player may use.
        player.send_permission_lvl_update();

        // Difficulty of the world
        player.send_difficulty_update();
        {
            let command_dispatcher = server.command_dispatcher.load();

            client_suggestions::send_c_commands_packet(player, server, &command_dispatcher);
        };
        if client.version.load() < JavaMinecraftVersion::V_1_20_2
            && client.version.load() >= JavaMinecraftVersion::V_1_13
        {
            let version = client.version.load();
            let mut tags = Vec::new();
            for &key in pumpkin_data::tag::RegistryKey::NETWORK_KEYS {
                if pumpkin_data::tag::get_registry_key_tags(version, key)
                    .is_some_and(|map| !map.is_empty())
                {
                    tags.push(key);
                }
            }
            let packet = pumpkin_protocol::java::client::play::CUpdateTagsPlay::new(&tags);
            client.send_packet(&packet).await;
        }
    }

    /// Where a joining player appears: its last position, or the world spawn for a first join.
    async fn join_pose(&self, player: &Arc<Player>) -> JoinPose {
        let (position, yaw, pitch) = if player.has_played_before.load(Ordering::Relaxed) {
            let position = player.position();
            let yaw = player.get_entity().yaw.load(); //info.spawn_angle;
            let pitch = player.get_entity().pitch.load();

            (position, yaw, pitch)
        } else {
            let info = &self.level_info.load();
            let spawn_position = Vector2::new(info.spawn_x, info.spawn_z);
            let chunk_pos = Vector2::new(info.spawn_x >> 4, info.spawn_z >> 4);
            self.level.get_or_fetch_chunk(chunk_pos, |_| ()).await;
            let top = self.get_top_block(spawn_position);
            let pos_y = if top > self.dimension.min_y {
                top + 1
            } else {
                info.spawn_y
            };

            let position = Vector3::new(
                f64::from(info.spawn_x) + 0.5,
                f64::from(pos_y),
                f64::from(info.spawn_z) + 0.5,
            );
            (position, info.spawn_yaw, info.spawn_pitch)
        };
        JoinPose {
            position,
            yaw,
            pitch,
        }
    }

    /// Puts the joining player at its pose, then loads and sends the chunk under it. Returns `false` when a plugin cancelled the chunk.
    async fn place_and_send_spawn_chunk(
        &self,
        player: &Arc<Player>,
        client: &crate::net::java::JavaClient,
        pose: &JoinPose,
    ) -> bool {
        let JoinPose {
            position,
            yaw,
            pitch,
        } = *pose;
        // Load chunks around the real spawn position before teleporting the client there.
        player.living_entity.entity.set_pos(position);
        player.living_entity.entity.set_rotation(yaw, pitch);
        player.living_entity.entity.last_pos.store(position);
        chunker::update_position(player);

        let center_chunk = player.living_entity.entity.chunk_pos.load();
        let chunk = self
            .level
            .get_or_fetch_chunk(center_chunk, std::clone::Clone::clone)
            .await;
        if let Some(server) = self.server.upgrade() {
            let mut event =
                crate::plugin::world::chunk_send::ChunkSend::new(player.world(), chunk.clone());
            server.plugin_manager.fire(&server, &mut event).await;
            if event.cancelled {
                return false;
            }
        }
        client.send_chunks(&[chunk]).await;
        player
            .chunk_sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .mark_sent_out_of_band(center_chunk);
        true
    }

    /// Announces the joining player in the tab list of everyone, with its custom name when it has one.
    fn announce_in_tab_list(&self, player: &Arc<Player>, entity_id: i32, gamemode: GameMode) {
        let gameprofile = &player.gameprofile;
        let bedrock_player_list = CPlayerList {
            action: CPlayerList::ACTION_ADD,
            entries: vec![PlayerListEntry {
                uuid: gameprofile.id,
                entity_unique_id: VarLong(entity_id as i64),
                username: gameprofile.name.clone(),
                xuid: String::new(),
                platform_chat_id: String::new(),
                build_platform: BuildPlatform::Unknown,
                skin: (**player.bedrock_skin.load()).clone(),
                is_teacher: false,
                is_host: false,
                is_sub_client: false,
                player_color: [0, 0, 0, 0],
            }],
        };

        let player_actions = [
            PlayerAction::AddPlayer {
                name: &gameprofile.name,
                properties: &gameprofile.properties.load(),
            },
            PlayerAction::UpdateGameMode(VarInt(gamemode as i32)),
            PlayerAction::UpdateListed(true),
            PlayerAction::UpdateLatency(VarInt(0)),
            PlayerAction::UpdateListOrder(VarInt(0)),
            PlayerAction::UpdateHat(true),
        ];
        let java_player = [pumpkin_protocol::java::client::play::Player {
            uuid: gameprofile.id,
            actions: &player_actions,
        }];
        let player_info_update = CPlayerInfoUpdate::new(Self::TAB_LIST_ADD_FLAGS, &java_player);

        self.broadcast_editioned(&player_info_update, &bedrock_player_list);

        // If the player has a custom tab_list_name, send an update for it
        if let Some(tab_list_name) = player.get_tab_list_name() {
            let actions = [PlayerAction::UpdateDisplayName(Some(&tab_list_name))];
            let java_player = [pumpkin_protocol::java::client::play::Player {
                uuid: gameprofile.id,
                actions: &actions,
            }];
            self.broadcast_packet_all(&CPlayerInfoUpdate::new(
                PlayerInfoFlags::UPDATE_DISPLAY_NAME.bits(),
                &java_player,
            ));
        }
    }

    /// Sends the joining player the tab list entries of the players already in the world.
    async fn send_tab_list_of_others(
        &self,
        base_config: &BasicConfiguration,
        player: &Arc<Player>,
        client: &crate::net::java::JavaClient,
    ) {
        // Here, we send all the infos of players who already joined.
        let mut players_tab_list_names = Vec::new();
        {
            let players = self.players.load();
            let mut data_to_process = Vec::new();
            for p in players
                .iter()
                .filter(|p| p.gameprofile.id != player.gameprofile.id)
            {
                let props_guard = p.gameprofile.properties.load();
                data_to_process.push((props_guard, p));
            }

            let mut current_player_data = Vec::new();
            for (properties, player) in &data_to_process {
                let chat_session = player
                    .chat_session
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let tab_list_name = player.get_tab_list_name();

                let mut player_actions = vec![PlayerAction::AddPlayer {
                    name: &player.gameprofile.name,
                    properties,
                }];

                if base_config.allow_chat_reports {
                    let initialized = chat_session.session_id != uuid::Uuid::nil()
                        && !chat_session.public_key.is_empty()
                        && !chat_session.signature.is_empty();
                    player_actions.push(PlayerAction::InitializeChat(initialized.then(|| {
                        InitChat {
                            session_id: chat_session.session_id,
                            expires_at: chat_session.expires_at,
                            public_key: chat_session.public_key.clone(),
                            signature: chat_session.signature.clone(),
                        }
                    })));
                }

                player_actions.extend([
                    PlayerAction::UpdateGameMode(VarInt(player.gamemode.load() as i32)),
                    PlayerAction::UpdateListed(player.tab_list_listed.load(Ordering::Relaxed)),
                    PlayerAction::UpdateLatency(VarInt(
                        player.tab_list_latency.load(Ordering::Relaxed),
                    )),
                    PlayerAction::UpdateListOrder(VarInt(
                        player.tab_list_order.load(Ordering::Relaxed),
                    )),
                    PlayerAction::UpdateHat(true),
                ]);
                drop(chat_session);

                current_player_data.push((&player.gameprofile.id, player_actions));

                // Collect tab_list_names for sending later
                if tab_list_name.is_some() {
                    players_tab_list_names.push((player.gameprofile.id, tab_list_name));
                }
            }

            let mut action_flags = PlayerInfoFlags::ADD_PLAYER
                | PlayerInfoFlags::UPDATE_LISTED
                | PlayerInfoFlags::UPDATE_LATENCY
                | PlayerInfoFlags::UPDATE_LIST_PRIORITY
                | PlayerInfoFlags::UPDATE_GAME_MODE
                | PlayerInfoFlags::UPDATE_HAT;
            if base_config.allow_chat_reports {
                action_flags |= PlayerInfoFlags::INITIALIZE_CHAT;
            }

            let entries = current_player_data
                .iter()
                .map(|(id, actions)| java::client::play::Player {
                    uuid: **id,
                    actions,
                })
                .collect::<Vec<_>>();

            debug!("Sending player info to {}", player.gameprofile.name);
            client
                .enqueue_client_packet(&CPlayerInfoUpdate::new(action_flags.bits(), &entries))
                .await;

            Self::send_custom_tab_names(client, &players_tab_list_names).await;
        };
    }

    /// Sends the custom tab list names of the players already in the world.
    async fn send_custom_tab_names(
        client: &crate::net::java::JavaClient,
        names: &[(uuid::Uuid, Option<TextComponent>)],
    ) {
        for (player_id, tab_list_name) in names {
            if let Some(name) = tab_list_name {
                let actions = [PlayerAction::UpdateDisplayName(Some(name))];
                let java_player = [pumpkin_protocol::java::client::play::Player {
                    uuid: *player_id,
                    actions: &actions,
                }];
                client
                    .enqueue_client_packet(&CPlayerInfoUpdate::new(
                        PlayerInfoFlags::UPDATE_DISPLAY_NAME.bits(),
                        &java_player,
                    ))
                    .await;
            }
        }
    }

    /// Spawns the joining player for every other Java client and sends its skin parts.
    fn spawn_for_others(
        &self,
        player: &Arc<Player>,
        entity_id: i32,
        pose: &JoinPose,
        velocity: Vector3<f64>,
    ) {
        let JoinPose {
            position,
            yaw,
            pitch,
        } = *pose;
        let gameprofile = &player.gameprofile;

        // Spawn the player for every Java client
        // Bedrock gets `AddPlayer` from the tracker.
        let spawn_entity = CSpawnEntity::new(
            entity_id.into(),
            gameprofile.id,
            i32::from(EntityType::PLAYER.id).into(),
            position,
            pitch,
            yaw,
            yaw,
            0.into(),
            velocity,
        );

        self.broadcast_packet_except(&[player.gameprofile.id], &spawn_entity);

        // Broadcast metadata to Java players so they can correctly interact with the new player
        let skin_parts = player.config.load().skin_parts;

        self.broadcast_skin_parts(
            &[gameprofile.id],
            entity_id,
            skin_parts,
            &CSetActorData {
                target_runtime_id: VarULong(entity_id as u64),
                actor_data: player.get_entity().bedrock_metadata(),
                synced_properties: PropertySyncData {
                    int_entries_list: HashMap::new(),
                    float_entries_list: HashMap::new(),
                },
                tick: VarULong(0),
            },
        );
    }

    /// Spawns the players already in the world for the joining player, with their metadata and equipment.
    async fn spawn_others_for(&self, player: &Arc<Player>, client: &crate::net::java::JavaClient) {
        // Spawn players for our client.
        let id = player.gameprofile.id;
        for existing_player in self
            .players
            .load()
            .iter()
            .filter(|c| c.gameprofile.id != id)
        {
            Self::with_player_list_entry(existing_player, |java, bedrock| {
                player.client.try_enqueue_packet_editioned(java, bedrock);
            });

            let config = existing_player.config.load();
            let mut buf = Vec::new();
            {
                let meta = Metadata::new(
                    pumpkin_data::tracked_data::player::PLAYER_MODE_CUSTOMISATION,
                    config.skin_parts,
                );
                let _ = meta.write(&mut buf, &CURRENT_MC_VERSION);
            };
            {
                let meta = Metadata::new(
                    pumpkin_data::tracked_data::player::PLAYER_MODE_CUSTOMIZATION_ID,
                    config.skin_parts,
                );
                let _ = meta.write(&mut buf, &CURRENT_MC_VERSION);
            };
            drop(config);
            // END
            buf.put_u8(255);
            client
                .enqueue_client_packet(&CSetEntityMetadata::new(
                    existing_player.get_entity().entity_id.into(),
                    buf.into(),
                ))
                .await;

            {
                let held_item = existing_player.inventory.held_item();
                let equipment_list = {
                    let mut equipment_list =
                        vec![(EquipmentSlot::MAIN_HAND.discriminant(), held_item.clone())];

                    let equipment_guard = existing_player
                        .inventory
                        .entity_equipment
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    for (slot, item_stack) in &equipment_guard.equipment {
                        equipment_list.push((slot.discriminant(), item_stack.clone()));
                    }
                    equipment_list
                };

                let equipment: Vec<(i8, ItemStackSerializer)> = equipment_list
                    .iter()
                    .map(|(slot, stack)| (*slot, ItemStackSerializer::from(stack.clone())))
                    .collect();

                let je_packet = CSetEquipment::new(existing_player.entity_id().into(), equipment);

                let be_mob_equipment = pumpkin_protocol::bedrock::client::CMobEquipment {
                    target_runtime_id: (existing_player.entity_id() as u64).into(),
                    item: (&held_item).into(),
                    slot: 0,
                    selected_slot: 0,
                    container_id: 0,
                };

                player
                    .client
                    .enqueue_packet_editioned(&je_packet, &be_mob_equipment)
                    .await;
            }
        }
    }

    /// Sends client information, abilities, held slot, world border, time, scoreboard and the spawn position.
    async fn send_initial_state(
        &self,
        player: &Arc<Player>,
        client: &crate::net::java::JavaClient,
    ) {
        player.send_client_information();

        player.send_abilities_update();

        // Sync selected slot
        player.enqueue_set_held_item_packet(&CSetSelectedSlot::new(
            player.get_inventory().get_selected_slot() as i8,
        ));

        if client.version.load() >= JavaMinecraftVersion::V_1_20_2 {
            // Start waiting for level chunks. Sets the "Loading Terrain" screen (Added in 1.20.2)
            debug!("Sending waiting chunks to {}", player.gameprofile.name);
            client
                .send_packet(&CGameEvent::new(GameEvent::StartWaitingChunks, 0.0))
                .await;
        }

        self.worldborder
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .init_client(client);

        // Sends initial time
        player.send_time(self);

        // Sends initial scoreboard state
        player.send_scoreboard();

        let (spawn_block_pos, yaw, pitch) = {
            let level_info_lock = self.level_info.load();
            (
                BlockPos::new(
                    level_info_lock.spawn_x,
                    level_info_lock.spawn_y,
                    level_info_lock.spawn_z,
                ),
                level_info_lock.spawn_yaw,
                level_info_lock.spawn_pitch,
            )
        };

        client
            .send_packet(&CPlayerSpawnPosition::new(
                spawn_block_pos,
                yaw,
                pitch,
                self.dimension.minecraft_name.to_owned(),
            ))
            .await;
    }

    /// Sends the weather the joining player starts in.
    async fn send_weather(&self, client: &crate::net::java::JavaClient) {
        // Send initial weather state
        let (is_raining, rain_level, thunder_level) = {
            let weather = self
                .weather
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (
                weather.raining,
                weather.rain_level.clamp(0.0, 1.0),
                weather.thunder_level.clamp(0.0, 1.0),
            )
        };
        if is_raining {
            client
                .enqueue_client_packet(&CGameEvent::new(GameEvent::BeginRaining, 0.0))
                .await;

            client
                .enqueue_client_packet(&CGameEvent::new(GameEvent::RainLevelChange, rain_level))
                .await;
            client
                .enqueue_client_packet(&CGameEvent::new(
                    GameEvent::ThunderLevelChange,
                    thunder_level,
                ))
                .await;
        }
    }

    /// Sends the boss bars the joining player already sees.
    fn send_bossbars(player: &Arc<Player>, server: &Arc<Server>) {
        let player_bossbars = server
            .bossbars
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_player_bars(&player.gameprofile.id)
            .map(|bars| bars.into_iter().cloned().collect::<Vec<_>>());
        if let Some(bossbars) = player_bossbars {
            for bossbar in &bossbars {
                player.send_bossbar(bossbar);
            }
        }
    }

    /// Marks the player as having played, opens its screen handler and sends effects, air, equipment and recipes.
    async fn send_player_state(&self, player: &Arc<Player>, server: &Arc<Server>) {
        player.has_played_before.store(true, Ordering::Relaxed);
        player.on_screen_handler_opened(&player.player_screen_handler);

        player.send_active_effects();
        player.breath_manager.send_air_supply(player);
        self.send_player_equipment(player);
        player
            .living_entity
            .send_current_equipment_attribute_modifiers();

        if let crate::net::ClientPlatform::Java(java_client) = player.client.as_ref()
            && server.advanced_config.recipe.send_recipes
            && java_client.version.load() >= JavaMinecraftVersion::V_1_21_2
        {
            let settings_packet = CRecipeBookSettings::default_closed();
            if let Ok(data) = java_client.serialize_packet(&settings_packet) {
                java_client.send_packet_now(data).await;
            }
            let dynamic_recipes = server.recipe_manager.get_dynamic_recipes();
            let add_packet = CRecipeBookAdd::new(true, &dynamic_recipes);
            if let Ok(data) = java_client.serialize_packet(&add_packet) {
                java_client.send_packet_now(data).await;
            }
        }
    }

    /// Fires the join event and announces the player when no plugin cancelled it.
    async fn announce_join(&self, player: &Arc<Player>, server: &Arc<Server>) {
        let msg_comp = TextComponent::translate_cross(
            translation::java::MULTIPLAYER_PLAYER_JOINED,
            translation::bedrock::MULTIPLAYER_PLAYER_JOINED,
            [TextComponent::text(player.gameprofile.name.clone())],
        )
        .color_named(NamedColor::Yellow);
        let mut event = PlayerJoinEvent::new(player.clone(), msg_comp);

        server.plugin_manager.fire(server, &mut event).await;

        if !event.cancelled {
            self.broadcast_system_message(&event.join_message, false);
            // TODO: Switch to structured logging, e.g. info!(player = %name, "connected")
            info!("{}", event.join_message.to_pretty_console());
        }
    }

    pub(super) fn send_player_equipment(&self, from: &Player) {
        let held_item = from.inventory.held_item();
        let mut equipment_list = vec![(EquipmentSlot::MAIN_HAND.discriminant(), held_item.clone())];

        let equipment_guard = from
            .inventory
            .entity_equipment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (slot, item_stack) in &equipment_guard.equipment {
            equipment_list.push((slot.discriminant(), item_stack.clone()));
        }
        drop(equipment_guard);

        let equipment: Vec<(i8, ItemStackSerializer)> = equipment_list
            .iter()
            .map(|(slot, stack)| (*slot, ItemStackSerializer::from(stack.clone())))
            .collect();
        let je_packet = CSetEquipment::new(from.entity_id().into(), equipment);

        let be_mob_equipment = pumpkin_protocol::bedrock::client::CMobEquipment {
            target_runtime_id: (from.entity_id() as u64).into(),
            item: (&held_item).into(),
            slot: 0,
            selected_slot: 0,
            container_id: 0,
        };

        self.send_to_tracking_players_editioned(from.get_entity(), &je_packet, &be_mob_equipment);
    }

    pub fn send_world_info(&self, player: &Arc<Player>) {
        if let ClientPlatform::Java(client) = player.client.as_ref() {
            self.worldborder
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .init_client(client);
        }

        // TODO: World spawn (compass stuff)

        if let ClientPlatform::Java(client) = player.client.as_ref()
            && client.version.load() >= JavaMinecraftVersion::V_1_20_2
        {
            player.try_send_client_packet(&CGameEvent::new(GameEvent::StartWaitingChunks, 0.0));
        }

        player.send_client_information();
        // The client just re-created the player entity (respawn or dimension change), so
        // it dropped its entity data; push it again or things like the second skin layer
        // stay hidden until the next change.
        player.get_entity().refresh_synced_data();

        chunker::update_position(player);
        // Update commands

        player.set_health(20.0);
    }

    /// Sends the centre chunk ahead of the teleport and records it as held.
    pub async fn send_center_chunk(&self, player: &Player) {
        let ClientPlatform::Java(java_client) = player.client.as_ref() else {
            return;
        };
        let center_chunk = player.get_entity().chunk_pos.load();
        let chunk = self
            .level
            .get_or_fetch_chunk(center_chunk, std::clone::Clone::clone)
            .await;
        java_client.send_chunks(&[chunk]).await;
        player
            .chunk_sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .mark_sent_out_of_band(center_chunk);
    }

    /// Must only be called after the player's own `CLogin` packet has been sent.
    pub fn pair_new_player_with_tracked_entities(&self, player: &Arc<Player>) {
        self.entity_tracker
            .pair_new_player_with_tracked_entities(player, self);
    }
}
