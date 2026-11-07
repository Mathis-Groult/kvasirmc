//! Bringing a Bedrock player into the world, and showing players across editions.

use super::World;
use crate::{
    command::client_suggestions,
    entity::{Entity, EntityBase, player::Player},
    net::ClientPlatform,
    plugin::player::player_join::PlayerJoinEvent,
    server::Server,
};
use pumpkin_config::BasicConfiguration;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::{BedrockItem, BedrockItemVersion};
use pumpkin_data::translation;
use pumpkin_protocol::bedrock::client::item_registry::{CItemRegistry, ItemData};
use pumpkin_protocol::bedrock::client::set_actor_data::{CSetActorData, PropertySyncData};
use pumpkin_protocol::bedrock::client::start_game::{CStartGame, ServerTelemetryData};
use pumpkin_protocol::bedrock::client::{CBiomeDefinitionList, CJigsawStructureData, CVoxelShapes};
use pumpkin_protocol::bedrock::network_item::NetworkItemDescriptor;
use pumpkin_protocol::{
    bedrock::client::{
        common::BuildPlatform,
        creative_content::{
            CCreativeContent, CreativeCategory, CreativeGroupInfoPayload, CreativeItemEntryPayload,
        },
        player_list::{CPlayerList, PlayerListEntry},
        remove_actor::CRemoveActor,
        start_game::{Experiments, GamePublishSetting, LevelSettings},
        update_attributes::{AttributeData, CUpdateAttributes},
    },
    codec::{var_int::VarInt, var_long::VarLong, var_uint::VarUInt, var_ulong::VarULong},
    java::client::play::{CPlayerInfoUpdate, CSpawnEntity, PlayerAction, PlayerInfoFlags},
};
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};
use pumpkin_util::text::{TextComponent, color::NamedColor};
use pumpkin_world::CURRENT_BEDROCK_MC_VERSION;
use std::sync::Arc;
use std::{collections::HashMap, sync::atomic::Ordering};
use tracing::info;
use uuid::Uuid;

impl World {
    #[allow(clippy::too_many_lines)]
    pub async fn spawn_bedrock_player(
        &self,
        base_config: &BasicConfiguration,
        player: Arc<Player>,
        server: &Arc<Server>,
    ) {
        static CREATIVE_CONTENT: std::sync::OnceLock<(
            Vec<CreativeGroupInfoPayload>,
            Vec<CreativeItemEntryPayload>,
        )> = std::sync::OnceLock::new();

        let level_info = server.level_info.load();
        let (rain_level, lightning_level) = {
            let weather = self
                .weather
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (weather.rain_level, weather.thunder_level)
        };
        let runtime_id = player.entity_id() as u64;
        let (position, yaw, pitch) = if player.has_played_before.load(Ordering::Relaxed) {
            let position = player.position();
            let yaw = player.get_entity().yaw.load(); //info.spawn_angle;
            let pitch = player.get_entity().pitch.load();

            (position, yaw, pitch)
        } else {
            let spawn_position = Vector2::new(level_info.spawn_x, level_info.spawn_z);
            let chunk_pos = Vector2::new(level_info.spawn_x >> 4, level_info.spawn_z >> 4);
            self.level.get_or_fetch_chunk(chunk_pos, |_| ()).await;
            let top = self.get_top_block(spawn_position);
            let pos_y = if top > self.dimension.min_y {
                top + 1
            } else {
                level_info.spawn_y
            };

            let position = Vector3::new(
                f64::from(level_info.spawn_x) + 0.5,
                f64::from(pos_y),
                f64::from(level_info.spawn_z) + 0.5,
            );
            (position, level_info.spawn_yaw, level_info.spawn_pitch)
        };

        // Keep the server-side transform aligned with the StartGame position. In
        // particular, this ensures an early disconnect persists the real spawn.
        player.living_entity.entity.set_pos(position);
        player.living_entity.entity.set_rotation(yaw, pitch);
        player.living_entity.entity.last_pos.store(position);

        // Todo make the data less spread
        let level_settings = LevelSettings {
            seed: self.level.seed.0,
            spawn_biome_type: 0,
            custom_biome_name: String::new(),
            dimension: VarInt(0),
            generator_type: VarInt(1),
            world_gamemode: server
                .defaultgamemode
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .gamemode
                .into(),
            hardcore: base_config.hardcore,
            difficulty: VarInt(level_info.difficulty as i32),
            spawn_position: BlockPos::new(
                level_info.spawn_x,
                level_info.spawn_y,
                level_info.spawn_z,
            ),
            has_achievements_disabled: false,
            editor_world_type: VarInt(0),
            is_created_in_editor: false,
            is_exported_from_editor: false,
            editor_level_migration_version: 0,
            day_cycle_stop_time: VarInt(-1),
            education_edition_offer: VarUInt(0),
            has_education_features_enabled: false,
            education_product_id: String::new(),
            rain_level,
            lightning_level,
            has_confirmed_platform_locked_content: false,
            was_multiplayer_intended: true,
            was_lan_broadcasting_intended: true,
            xbox_live_broadcast_setting: GamePublishSetting::Public,
            platform_broadcast_setting: GamePublishSetting::Public,
            commands_enabled: level_info.allow_commands,
            is_texture_packs_required: false,
            rule_data: Vec::new(),
            experiments: Experiments {
                toggles: Vec::new(),
                experiments_ever_toggled: false,
            },
            bonus_chest: false,
            has_start_with_map_enabled: false,
            // TODO Bedrock permission level are different
            permission_level: 2,
            server_simulation_distance: server
                .advanced_config
                .networking
                .bedrock
                .simulation_distance
                .get()
                .into(),
            has_locked_behavior_pack: false,
            has_locked_resource_pack: false,
            is_from_locked_world_template: false,
            is_using_msa_gamertags_only: false,
            is_from_world_template: false,
            is_world_template_option_locked: false,
            is_only_spawning_v1_villagers: false,
            is_disabling_personas: false,
            is_disabling_custom_skins: false,
            emote_chat_muted: false,
            game_version: CURRENT_BEDROCK_MC_VERSION.into(),
            limited_world_width: 0,
            limited_world_height: 0,
            new_nether: true,
            edu_shared_uri_button_name: String::new(),
            edu_shared_uri_link_uri: String::new(),
            override_force_experimental_gameplay_has_value: false,
            chat_restriction_level: 0,
            disable_player_interactions: false,
            server_editor_connection_policy: VarInt(0),
            allow_anonymous_block_drops_in_editor_worlds: false,
        };
        drop(level_info);

        let Some(client) = player.client.bedrock() else {
            return;
        };

        let start_game = CStartGame {
            entity_id: VarLong(runtime_id as _),
            runtime_entity_id: VarULong(runtime_id),
            player_gamemode: player.gamemode.load().into(),
            // Bedrock represents the local player at eye height; Pumpkin stores feet position.
            position: Vector3::new(
                position.x as f32,
                position.y as f32 + player.get_entity().entity_type.eye_height,
                position.z as f32,
            ),
            pitch,
            yaw,
            level_settings,
            level_id: String::new(),
            level_name: "Pumpkin world".to_string(),
            premium_world_template_id: String::new(),
            is_trial: false,
            rewind_history_size: VarInt(0),
            server_authoritative_block_breaking: true,
            current_level_time: self.get_world_age() as _,
            enchantment_seed: VarInt(0),
            block_properties: Vec::new(),
            // TODO Make this unique
            multiplayer_correlation_id: Uuid::default().to_string(),
            enable_itemstack_net_manager: true,
            server_version: "Pumpkin Rust Server".to_string(),
            compound_id: 10,
            compound_len: VarUInt(0),
            compound_end: 0,
            block_registry_checksum: 0,
            world_template_id: Uuid::nil(),
            enable_clientside_generation: false,
            blocknetwork_ids_are_hashed: true,
            server_auth_sounds: true,
            server_join_information: None,
            telemetry: ServerTelemetryData {
                server_id: String::new(),
                scenario_id: String::new(),
                world_id: String::new(),
                owner_id: String::new(),
            },
        };
        client.send_packet(&CJigsawStructureData).await;
        client.send_packet(&CVoxelShapes).await;
        if let Ok(data) = client.serialize_packet(&start_game) {
            client.send_game_packet(data).await;
        }

        if let Ok(data) = client.serialize_packet(&CBiomeDefinitionList) {
            client.send_game_packet(data).await;
        }

        let item_registry = CItemRegistry {
            items: BedrockItem::ALL_BEDROCK_ITEMS
                .iter()
                .map(|b| ItemData {
                    item_name: b.registry_key.into(),
                    item_id: b.id,
                    is_component_based: b.component_based,
                    item_version: VarInt::from(match b.version {
                        BedrockItemVersion::Legacy => 0,
                        BedrockItemVersion::DataDriven => 1,
                        BedrockItemVersion::None => 2,
                    }),
                    component_data: b.definition_components.into(),
                })
                .collect::<Vec<_>>(),
        };
        if let Ok(data) = client.serialize_packet(&item_registry) {
            client.send_game_packet(data).await;
        }

        let (groups, entries) = CREATIVE_CONTENT.get_or_init(|| {
            let groups = pumpkin_data::bedrock_creative::CREATIVE_GROUPS
                .iter()
                .map(|g| {
                    let creative_category = match g.category {
                        1 => CreativeCategory::Construction,
                        2 => CreativeCategory::Nature,
                        3 => CreativeCategory::Equipment,
                        4 => CreativeCategory::Items,
                        5 => CreativeCategory::ItemCommandOnly,
                        _ => CreativeCategory::Undefined,
                    };
                    let icon_item = if g.icon_item_id != 0 {
                        NetworkItemDescriptor {
                            id: VarInt::from(g.icon_item_id),
                            stack_size: 1,
                            aux_value: VarUInt(g.icon_item_aux_value),
                            block_runtime_id: VarInt(0),
                            nbt_data: pumpkin_nbt::Nbt::default(),
                            place_on_blocks: Vec::new(),
                            destroy_blocks: Vec::new(),
                            shield_blocking_tick: 0,
                        }
                    } else {
                        NetworkItemDescriptor::default()
                    };

                    CreativeGroupInfoPayload {
                        creative_category,
                        name: g.name.to_string(),
                        group_icon_item: icon_item,
                    }
                })
                .collect::<Vec<_>>();

            let entries = pumpkin_data::bedrock_creative::CREATIVE_ENTRIES
                .iter()
                .enumerate()
                .map(|(i, e)| CreativeItemEntryPayload {
                    id: VarUInt((i + 1) as u32),
                    item: NetworkItemDescriptor {
                        id: VarInt::from(e.item_id),
                        stack_size: 1,
                        aux_value: VarUInt(e.item_aux_value),
                        block_runtime_id: VarInt(0),
                        nbt_data: pumpkin_nbt::Nbt::default(),
                        place_on_blocks: Vec::new(),
                        destroy_blocks: Vec::new(),
                        shield_blocking_tick: 0,
                    },
                    group_index: VarUInt(e.group_index),
                })
                .collect::<Vec<_>>();

            (groups, entries)
        });
        let creative_content = CCreativeContent { groups, entries };
        if let Ok(data) = client.serialize_packet(&creative_content) {
            client.send_game_packet(data).await;
        }

        let crafting_data = pumpkin_protocol::bedrock::client::CCraftingData {
            recipes: crate::net::bedrock::recipe::crafting_data(&server.recipe_manager),
            clean_recipes: false,
        };
        if let Ok(data) = client.serialize_packet(&crafting_data) {
            client.send_game_packet(data).await;
        }

        player.on_screen_handler_opened(&player.player_screen_handler);

        {
            let mut abilities = player
                .abilities
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            abilities.set_for_gamemode(player.gamemode.load());
        };

        let entity = &player.get_entity();
        let metadata = entity.bedrock_metadata();

        let actor_data = CSetActorData {
            target_runtime_id: VarULong(runtime_id),
            actor_data: metadata,
            synced_properties: PropertySyncData {
                int_entries_list: HashMap::new(),
                float_entries_list: HashMap::new(),
            },
            tick: VarULong(0),
        };
        if let Ok(data) = client.serialize_packet(&actor_data) {
            client.send_game_packet(data).await;
        }
        player.send_abilities_update();

        {
            let command_dispatcher = server.command_dispatcher.load();
            client_suggestions::send_bedrock_commands_packet(&player, server, &command_dispatcher);
        };

        client
            .enqueue_client_packet(&CUpdateAttributes {
                target_runtime_id: VarULong(runtime_id),
                attribute_list: vec![
                    AttributeData {
                        min_value: 0.0,
                        max_value: 3.402_823_5E38,
                        current_value: 0.1,
                        default_min_value: 0.0,
                        default_max_value: 3.402_823_5E38,
                        default_value: 0.1,
                        name: "minecraft:movement".to_string(),
                        modifiers: Vec::new(),
                    },
                    AttributeData {
                        min_value: 0.0,
                        max_value: 3.402_823_5E38,
                        current_value: 0.02,
                        default_min_value: 0.0,
                        default_max_value: 3.402_823_5E38,
                        default_value: 0.02,
                        name: "minecraft:underwater_movement".to_string(),
                        modifiers: Vec::new(),
                    },
                    AttributeData {
                        min_value: 0.0,
                        max_value: 1.0,
                        current_value: 0.08,
                        default_min_value: 0.0,
                        default_max_value: 1.0,
                        default_value: 0.08,
                        name: "minecraft:gravity".to_string(),
                        modifiers: Vec::new(),
                    },
                    AttributeData {
                        min_value: 0.0,
                        max_value: 400.0,
                        current_value: 400.0,
                        default_min_value: 0.0,
                        default_max_value: 400.0,
                        default_value: 400.0,
                        name: "minecraft:air".to_string(),
                        modifiers: Vec::new(),
                    },
                    AttributeData {
                        min_value: 0.0,
                        max_value: 20.0,
                        current_value: player.living_entity.health.load(),
                        default_min_value: 0.0,
                        default_max_value: 20.0,
                        default_value: 20.0,
                        name: "minecraft:health".to_string(),
                        modifiers: Vec::new(),
                    },
                    AttributeData {
                        min_value: 0.0,
                        max_value: 20.0,
                        current_value: player.hunger_manager.level.load().into(),
                        default_min_value: 0.0,
                        default_max_value: 20.0,
                        default_value: 20.0,
                        name: "minecraft:player.hunger".to_string(),
                        modifiers: Vec::new(),
                    },
                ],
                tick: VarULong(0),
            })
            .await;

        // --- MULTIPLAYER BROADCASTING ---

        let gameprofile = &player.gameprofile;
        let velocity = player.get_entity().velocity.load();

        // 1. Broadcast the new Bedrock player to everyone else (Java + Bedrock)
        let bedrock_player_list = CPlayerList {
            action: CPlayerList::ACTION_ADD,
            entries: vec![PlayerListEntry {
                uuid: gameprofile.id,
                entity_unique_id: VarLong(runtime_id as i64),
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

        let gamemode = player.gamemode.load();
        self.broadcast_packet_except_editioned(
            &[gameprofile.id],
            &CPlayerInfoUpdate::new(
                (PlayerInfoFlags::ADD_PLAYER
                    | PlayerInfoFlags::UPDATE_GAME_MODE
                    | PlayerInfoFlags::UPDATE_LISTED
                    | PlayerInfoFlags::UPDATE_LATENCY
                    | PlayerInfoFlags::UPDATE_LIST_PRIORITY
                    | PlayerInfoFlags::UPDATE_HAT)
                    .bits(),
                &[pumpkin_protocol::java::client::play::Player {
                    uuid: gameprofile.id,
                    actions: &[
                        PlayerAction::AddPlayer {
                            name: &gameprofile.name,
                            properties: &gameprofile.properties.load(),
                        },
                        PlayerAction::UpdateGameMode(VarInt(gamemode as i32)),
                        PlayerAction::UpdateListed(true),
                        PlayerAction::UpdateLatency(VarInt(0)),
                        PlayerAction::UpdateListOrder(VarInt(0)),
                        PlayerAction::UpdateHat(true),
                    ],
                }],
            ),
            &bedrock_player_list,
        );

        // Bedrock gets `AddPlayer` from the tracker.
        self.broadcast_packet_except(
            &[gameprofile.id],
            &CSpawnEntity::new(
                (runtime_id as i32).into(),
                gameprofile.id,
                i32::from(EntityType::PLAYER.id).into(),
                position,
                pitch,
                yaw,
                yaw,
                0.into(),
                velocity,
            ),
        );

        self.send_player_equipment(&player);
        player
            .living_entity
            .send_current_equipment_attribute_modifiers();

        // Broadcast metadata to Java players so they can correctly interact with the new player
        let skin_parts = player.config.load().skin_parts;

        self.broadcast_skin_parts(
            &[gameprofile.id],
            runtime_id as i32,
            skin_parts,
            &actor_data,
        );

        // 2. Spawn existing players for our new Bedrock client
        let players = self.players.load();

        for existing_player in players
            .iter()
            .filter(|p| p.gameprofile.id != gameprofile.id)
        {
            client
                .send_packet(&existing_player.bedrock_player_list())
                .await;
        }
        // Actors come from the tracker, in range only.
        self.pair_new_player_with_tracked_entities(&player);

        player.has_played_before.store(true, Ordering::Relaxed);

        // 3. Trigger Join Event and Broadcast Join Message
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
            info!("{}", event.join_message.to_pretty_console());
        }
    }

    pub(crate) fn despawn_dead_java_player_for_bedrock(&self, subject: &Entity) {
        let Some(player) = self.get_player_by_id(subject.entity_id) else {
            return;
        };
        if matches!(player.client.as_ref(), ClientPlatform::Java(_)) {
            self.broadcast_to_chunk_bedrock(
                subject.chunk_pos.load(),
                &CRemoveActor::new(VarLong(subject.entity_id.into())),
            );
        }
    }

    pub(super) async fn refresh_java_player_for_bedrock(&self, subject: &Player) {
        if !matches!(subject.client.as_ref(), ClientPlatform::Java(_)) {
            return;
        }

        let (player_list, add_player) = subject.bedrock_spawn_packets();
        let remove = CRemoveActor::new(VarLong(subject.entity_id().into()));

        let Some(tracked) = self.entity_tracker.get_tracked_entity(subject.entity_id()) else {
            return;
        };
        for recipient in self.players.load().iter() {
            if tracked.is_seen_by(&recipient.gameprofile.id)
                && let ClientPlatform::Bedrock(client) = recipient.client.as_ref()
            {
                client.send_packet(&remove).await;
                client.send_packet(&player_list).await;
                client.send_packet(&add_player).await;
            }
        }
    }
}
