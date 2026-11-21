pub mod advancement;
mod attacks;
mod chunk_watching;
mod client_time_weather;
mod connection;
mod cooldowns;
mod effects;
mod food;
mod health;
mod items;
mod messages;
mod mining;
mod moderation;
mod movement;
mod permissions;
mod progress;
mod save_data;
mod scoreboard;
mod screens;
mod sounds_particles;
mod spawn_point;
pub mod statistics;
mod tab_list;
mod teleport;
mod tick;
mod xp_levels;

use core::f32;
use std::collections::{HashMap, VecDeque};
use std::num::NonZero;
use std::sync::atomic::{AtomicBool, AtomicI8, AtomicI32, AtomicU8, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Instant;

use crate::plugin::api::events::enchantment::{EnchantItemEvent, PrepareItemEnchantEvent};
use crate::world::scoreboard::{BedrockScoreboard, Scoreboard};
use advancement::PlayerAdvancement;
use arc_swap::ArcSwap;
use crossbeam::atomic::AtomicCell;
use crossbeam::channel::Receiver;
use crossbeam::queue::SegQueue;
use pumpkin_data::dimension::Dimension;
use pumpkin_inventory::player::ender_chest_inventory::EnderChestInventory;
use pumpkin_protocol::RawPacket;
use pumpkin_protocol::bedrock::client::{
    CommandPermissionLevel, PlayerPermissionLevel, SerializedAbilitiesData,
};
use pumpkin_protocol::bedrock::client::{
    SerializedAbilitiesDataSerializedLayer, add_player::CAddPlayer, common::BuildPlatform,
    player_list::CPlayerList, set_actor_data::PropertySyncData,
};
use pumpkin_util::version::JavaMinecraftVersion;
use pumpkin_world::chunk::ChunkData;
use tokio::task::JoinHandle;
use tracing::debug;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub enum CustomScoreboard {
    Java(Scoreboard),
    Bedrock(BedrockScoreboard),
}

impl From<Scoreboard> for CustomScoreboard {
    fn from(sb: Scoreboard) -> Self {
        Self::Java(sb)
    }
}

impl From<BedrockScoreboard> for CustomScoreboard {
    fn from(sb: BedrockScoreboard) -> Self {
        Self::Bedrock(sb)
    }
}

#[derive(Copy, Clone)]
pub struct JavaPlayer<'a>(pub &'a Player);

impl JavaPlayer<'_> {
    pub async fn send_packet<C: pumpkin_protocol::ClientPacket + Sync>(&self, packet: &C) {
        if let ClientPlatform::Java(client) = self.0.client.as_ref()
            && let Ok(data) = client.serialize_packet(packet)
        {
            client.enqueue_packet(data).await;
        }
    }

    pub async fn send_custom_payload(&self, channel: &str, data: &[u8]) {
        let packet = CCustomPayload::new(channel, data);
        self.send_packet(&packet).await;
    }

    pub fn send_stats(&self) {
        self.0.send_stats();
    }

    pub fn set_scoreboard(&self, scoreboard: Option<Scoreboard>) {
        *self
            .0
            .custom_scoreboard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            scoreboard.map(CustomScoreboard::Java);
        self.0.send_scoreboard();
    }

    pub fn reset_scoreboard(&self) {
        self.set_scoreboard(None);
    }

    pub fn get_scoreboard(&self) -> Option<Scoreboard> {
        let guard = self
            .0
            .custom_scoreboard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(CustomScoreboard::Java(sb)) = guard.as_ref() {
            Some(sb.clone())
        } else {
            None
        }
    }
}

#[derive(Copy, Clone)]
pub struct BedrockPlayer<'a>(pub &'a Player);

impl BedrockPlayer<'_> {
    pub async fn send_packet<P: pumpkin_protocol::BClientPacket + Sync>(&self, packet: &P) {
        if let ClientPlatform::Bedrock(client) = self.0.client.as_ref()
            && let Ok(data) = client.serialize_packet(packet)
        {
            client.enqueue_packet(data).await;
        }
    }

    pub fn set_scoreboard(&self, scoreboard: Option<BedrockScoreboard>) {
        *self
            .0
            .custom_scoreboard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            scoreboard.map(CustomScoreboard::Bedrock);
        self.0.send_scoreboard();
    }

    pub fn reset_scoreboard(&self) {
        self.set_scoreboard(None);
    }

    pub fn get_scoreboard(&self) -> Option<BedrockScoreboard> {
        let guard = self
            .0
            .custom_scoreboard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(CustomScoreboard::Bedrock(sb)) = guard.as_ref() {
            Some(sb.clone())
        } else {
            None
        }
    }

    #[must_use]
    pub fn get_team(&self) -> Option<crate::world::scoreboard::Team> {
        self.0.get_team()
    }

    #[must_use]
    pub fn client_data(&self) -> Option<Arc<pumpkin_protocol::bedrock::server::login::ClientData>> {
        if let ClientPlatform::Bedrock(client) = self.0.client.as_ref() {
            let data = client.client_data.load();
            (**data).clone()
        } else {
            None
        }
    }

    #[must_use]
    pub fn device_os(&self) -> Option<i32> {
        self.client_data().map(|d| d.device_os)
    }

    #[must_use]
    pub fn device_id(&self) -> Option<String> {
        self.client_data().map(|d| d.device_id.clone())
    }

    #[must_use]
    pub fn device_model(&self) -> Option<String> {
        self.client_data().map(|d| d.device_model.clone())
    }

    #[must_use]
    pub fn game_version(&self) -> Option<String> {
        self.client_data().map(|d| d.game_version.clone())
    }

    #[must_use]
    pub fn language_code(&self) -> Option<String> {
        self.client_data().map(|d| d.language_code.clone())
    }

    #[must_use]
    pub fn current_input_mode(&self) -> Option<i32> {
        self.client_data().map(|d| d.current_input_mode)
    }

    #[must_use]
    pub fn default_input_mode(&self) -> Option<i32> {
        self.client_data().map(|d| d.default_input_mode)
    }

    #[must_use]
    pub fn ui_profile(&self) -> Option<i32> {
        self.client_data().map(|d| d.ui_profile)
    }

    #[must_use]
    pub fn gui_scale(&self) -> Option<i32> {
        self.client_data().map(|d| d.gui_scale)
    }

    #[must_use]
    pub fn max_view_distance(&self) -> Option<i32> {
        self.client_data().map(|d| d.max_view_distance)
    }

    #[must_use]
    pub fn memory_tier(&self) -> Option<i32> {
        self.client_data().map(|d| d.memory_tier)
    }

    #[must_use]
    pub fn graphics_mode(&self) -> Option<i32> {
        self.client_data().map(|d| d.graphics_mode)
    }
}
use pumpkin_data::damage::DamageType;
use pumpkin_data::data_component_impl::{EquipmentSlot, EquippableImpl};
use pumpkin_data::entity::{EntityType, MovementEmission};
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::sound::{Sound, SoundCategory};
use pumpkin_data::statistic::StatisticCategory;
use pumpkin_data::tag::Taggable;
use pumpkin_data::{BlockState, screen::WindowType, tag};
use pumpkin_inventory::player::{
    player_inventory::PlayerInventory, player_screen_handler::PlayerScreenHandler,
};
use pumpkin_inventory::screen_handler::{
    InventoryPlayer, ScreenHandler, ScreenHandlerBehaviour, ScreenHandlerListener,
};
use pumpkin_inventory::sync_handler::SyncHandler;
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_nbt::tag::NbtTag;
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::codec::var_ulong::VarULong;
use pumpkin_protocol::java::client::play::{
    CCustomPayload, CEntityPositionSync, CSetContainerContent, CSetContainerProperty,
    CSetContainerSlot, CSetCursorItem, CSetPlayerInventory, CSetSelectedSlot, PreviousMessage,
};
use pumpkin_util::math::{position::BlockPos, vector2::Vector2, vector3::Vector3};
use pumpkin_util::permission::PermissionLvl;
use pumpkin_util::text::TextComponent;
use pumpkin_util::text::click::ClickEvent;
use pumpkin_util::text::hover::HoverEvent;
use pumpkin_util::{GameMode, Hand};
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;

use crate::net::PlayerConfig;
use crate::net::{ClientPlatform, GameProfile};
use crate::server::Server;
use crate::world::World;

use super::breath::BreathManager;
use super::hunger::HungerManager;
use super::living::LivingEntity;
use super::mob::warden_spawn_tracker::WardenSpawnTracker;
use super::{Entity, EntityBase, NBTStorage, NBTStorageInit};
const MAX_CACHED_SIGNATURES: u8 = 128; // Vanilla: 128
const MAX_PREVIOUS_MESSAGES: u8 = 20; // Vanilla: 20

fn write_root_vehicle(nbt: &mut NbtCompound, uuid: Uuid) {
    let value = uuid.as_u128();
    let mut root_vehicle = NbtCompound::new();
    root_vehicle.put(
        "Attach",
        NbtTag::IntArray(vec![
            (value >> 96) as i32,
            (value >> 64) as i32,
            (value >> 32) as i32,
            value as i32,
        ]),
    );
    nbt.put("RootVehicle", NbtTag::Compound(root_vehicle));
}

fn read_root_vehicle(nbt: &NbtCompound) -> Option<Uuid> {
    let root_vehicle = nbt.get_compound("RootVehicle")?;
    let uuid = if let Some([most, more, less, least]) = root_vehicle.get_int_array("Attach") {
        [*most, *more, *less, *least]
    } else {
        let [most, more, less, least] = root_vehicle.get_list("Attach")? else {
            return None;
        };
        [
            most.extract_int()?,
            more.extract_int()?,
            less.extract_int()?,
            least.extract_int()?,
        ]
    };

    Some(Uuid::from_u128(
        (uuid[0] as u32 as u128) << 96
            | (uuid[1] as u32 as u128) << 64
            | (uuid[2] as u32 as u128) << 32
            | uuid[3] as u32 as u128,
    ))
}

pub const DATA_VERSION: i32 = 5023; // 26.3

/// Food exhaustion applied for every block a player mines.
///
/// Vanilla: `Block#playerDestroy` calls `player.causeFoodExhaustion(0.005F)`.
/// `ServerPlayerGameMode#destroyBlock` only reaches `playerDestroy` for
/// non-creative players holding a tool that can harvest the block, so callers
/// must apply the same gating.
pub const MINE_BLOCK_EXHAUSTION: f32 = 0.005; // Vanilla: 0.005F

const fn bedrock_inventory_slot(player_screen_slot: i16) -> Option<u32> {
    match player_screen_slot {
        9..=35 => Some(player_screen_slot as u32),
        36..=44 => Some((player_screen_slot - 36) as u32),
        _ => None,
    }
}

/// Represents a Minecraft player entity.
///
/// A `Player` is a special type of entity that represents a human player connected to the server.
#[derive(Clone, Copy, Debug)]
pub struct ItemCooldown {
    pub start_tick: i32,
    pub duration: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerWeather {
    Clear,
    Downfall,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpamType {
    Chat,
    Command,
}

/// Center chunk and levels of the view and simulation tickets a player holds.
pub type HeldChunkTickets = (Vector2<i32>, Option<i8>, Option<i8>);

#[repr(C)]
pub struct Player {
    /// The underlying living entity object that represents the player.
    pub living_entity: LivingEntity,
    /// The player's game profile information, including their username and UUID.
    pub gameprofile: GameProfile,
    /// The client connection associated with the player.
    pub client: Arc<ClientPlatform>,
    /// The player's inventory.
    pub inventory: Arc<PlayerInventory>,
    /// The player's `EnderChest` inventory.
    pub ender_chest_inventory: Arc<EnderChestInventory>,
    /// The player's configuration settings. Changes when the player changes their settings.
    pub config: ArcSwap<PlayerConfig>,
    /// The player's current gamemode (e.g., Survival, Creative, Adventure).
    pub gamemode: AtomicCell<GameMode>,
    /// The player's previous gamemode
    pub previous_gamemode: AtomicCell<Option<GameMode>>,
    /// The entity ID of the entity that the player is currently spectating/camera targeting.
    pub camera_target_id: AtomicCell<Option<i32>>,
    /// The player's spawnpoint
    pub respawn_point: std::sync::Mutex<Option<RespawnPoint>>,
    /// The player's sleep status
    pub sleeping_since: AtomicCell<Option<u8>>,
    /// Head position of the bed the player is currently sleeping in.
    pub sleeping_bed_pos: AtomicCell<Option<BlockPos>>,
    /// Whether the player is ignored when calculating the percentage of sleeping players needed to skip the night.
    pub sleeping_ignored: AtomicBool,
    /// Manages the player's breath level
    pub breath_manager: BreathManager,
    /// Manages the player's hunger level.
    pub hunger_manager: HungerManager,
    /// The ID of the currently open container (if any).
    pub open_container: AtomicCell<Option<u64>>,
    /// The block position of the currently open container screen (if any).
    pub open_container_pos: AtomicCell<Option<BlockPos>>,
    /// The village position where Raid Omen was triggered.
    pub raid_omen_position: AtomicCell<Option<BlockPos>>,
    /// Starting position when riding a vehicle in lava (for advancement tracking).
    pub entered_lava_pos: AtomicCell<Option<Vector3<f64>>>,
    /// The item currently being held by the player.
    pub carried_item: Mutex<Option<ItemStack>>,
    /// The player's abilities and special powers.
    ///
    /// This field represents the various abilities that the player possesses, such as flight, invulnerability, and other special effects.
    ///
    /// **Note:** When the `abilities` field is updated, the server should send a `send_abilities_update` packet to the client to notify them of the changes.
    pub abilities: std::sync::Mutex<Abilities>,
    /// Player statistics
    pub stats: std::sync::Mutex<statistics::Statistics>,
    /// The current stage of block destruction of the block the player is breaking.
    pub current_block_destroy_stage: AtomicI32,
    /// The per-tick block destruction progress last sent to Bedrock clients.
    pub current_block_breaking_speed: AtomicU32,
    /// The held item's Efficiency level last synced to the client via the `mining_efficiency`
    /// attribute. -1 means never synced
    pub synced_mining_efficiency_level: AtomicI32,
    /// Indicates if the player is currently mining a block.
    pub mining: AtomicBool,
    pub start_mining_time: AtomicI32,
    pub tick_counter: AtomicI32,
    pub mining_pos: Mutex<BlockPos>,
    pub last_input: AtomicI8,
    /// A counter for teleport IDs used to track pending teleports.
    pub teleport_id_count: AtomicI32,
    /// The pending teleport information, including the teleport ID and target location.
    pub awaiting_teleport: Mutex<Option<(VarInt, Vector3<f64>)>>,
    /// The coordinates of the chunk section the player is currently watching.
    pub watched_section: AtomicCell<Cylindrical>,
    /// The last time the player performed an action (for idle timeout).
    pub last_action_time: AtomicCell<Instant>,
    /// The ping in millis.
    pub ping: AtomicU32,
    /// The amount of ticks since the player's last attack.
    pub last_attacked_ticks: AtomicU32,
    /// The level last sent to the client and vanilla's `totalExperience`.
    pub xp_state: xp_levels::XpState,
    pub last_sent_health: AtomicI32,
    pub last_sent_food: AtomicU8,
    pub last_food_saturation: AtomicBool,
    /// The player's permission level.
    pub permission_lvl: AtomicCell<PermissionLvl>,
    pub subscribed_debug_sample: AtomicBool,
    /// Whether the client has reported that it has loaded.
    pub client_loaded: AtomicBool,
    pub bedrock_spawned: AtomicBool,
    /// Whether the player is frozen in place (movement locked for dialogues/cutscenes).
    pub is_movement_locked: AtomicBool,
    /// The amount of time (in ticks) the client has to report having finished loading before being timed out.
    pub client_loaded_timeout: AtomicU32,
    /// Counter for tracking chat and command spam. Decays each server tick.
    pub chat_spam_tick_count: AtomicU32,
    /// Item usage tracking for bows, crossbows, etc.
    pub using_item: AtomicBool,
    pub item_use_start_time: AtomicI32,
    pub using_hand: AtomicCell<Option<Hand>>,
    /// The player's experience level.
    pub experience_level: AtomicI32,
    /// The player's experience progress (`0.0` to `1.0`)
    pub experience_progress: AtomicCell<f32>,
    /// The player's total experience points.
    pub experience_points: AtomicI32,
    pub item_cooldowns: std::sync::Mutex<HashMap<String, ItemCooldown>>,
    pub experience_pick_up_delay: Mutex<u32>,
    pub chunk_sender: Mutex<crate::net::ChunkSender>,
    pub chunk_listener: Mutex<Receiver<(Vector2<i32>, Weak<ChunkData>)>>,
    pub held_chunk_tickets: Mutex<Option<HeldChunkTickets>>,
    pub chunk_send_epoch: AtomicU32,
    pub has_played_before: AtomicBool,
    root_vehicle_uuid: AtomicCell<Option<Uuid>>,
    pub chat_session: Arc<Mutex<ChatSession>>,
    pub signature_cache: Mutex<MessageCache>,
    pub player_screen_handler: Arc<std::sync::Mutex<PlayerScreenHandler>>,
    pub current_screen_handler: std::sync::Mutex<Arc<std::sync::Mutex<dyn ScreenHandler>>>,
    pub screen_handler_sync_id: AtomicU8,
    pub screen_handler_listener: Arc<dyn ScreenHandlerListener>,
    pub inventory_changed: Arc<AtomicBool>,
    pub screen_handler_sync_handler: Arc<SyncHandler>,
    pub tab_list_header: Mutex<TextComponent>,
    pub tab_list_footer: Mutex<TextComponent>,
    pub display_name: std::sync::Mutex<Option<TextComponent>>,
    pub tab_list_name: Mutex<Option<TextComponent>>,
    pub tab_list_order: AtomicI32,
    pub tab_list_latency: AtomicI32,
    pub tab_list_listed: AtomicBool,
    pub per_player_time: AtomicCell<Option<(u64, bool)>>,
    pub per_player_weather: AtomicCell<Option<PlayerWeather>>,
    pub custom_scoreboard: std::sync::Mutex<Option<CustomScoreboard>>,
    pub compass_target: AtomicCell<Option<pumpkin_util::math::position::BlockPos>>,
    pub respawn_location: AtomicCell<Option<pumpkin_util::math::position::BlockPos>>,
    pub hidden_players: Mutex<std::collections::HashSet<uuid::Uuid>>,
    pub advancements: Arc<Mutex<PlayerAdvancement>>,
    pub enchantment_seed: AtomicI32,
    pub fishing_bobber: AtomicI32,
    pub bedrock_skin: arc_swap::ArcSwap<pumpkin_protocol::bedrock::client::Skin>,
    pub end_credits: health::EndCredits,
    pub warden_spawn_tracker: std::sync::Mutex<WardenSpawnTracker>,
    pub score: AtomicI32,
    pub spawn_extra_particles_on_fall: AtomicBool,
    pub post_effects: std::sync::Mutex<Vec<String>>,
    /// Inbound packets waiting to be processed during player tick.
    pub inbound_packets: SegQueue<RawPacket>,
}

use base64::prelude::*;
use pumpkin_protocol::Property;
use serde::Deserialize;

// Bit masks for the Java skin pixels that Bedrock requires to be opaque.
// Adapted from Geyser's SkinProvider under the MIT License.
const SKIN_OPAQUE_MASK: &str = "AP//AAAAAAAA//8AAAAAAAD//wAAAAAAAP//AAAAAAAA//8AAAAAAAD//wAAAAAAAP//AAAAAAAA//8AAAAAAP////8AAAAA/////wAAAAD/////AAAAAP////8AAAAA/////wAAAAD/////AAAAAP////8AAAAA/////wAAAADwD/D/D/8AAPAP8P8P/wAA8A/w/w//AADwD/D/D/8AAP///////w8A////////DwD///////8PAP///////w8A////////DwD///////8PAP///////w8A////////DwD///////8PAP///////w8A////////DwD///////8PAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADwD/APAAAAAPAP8A8AAAAA8A/wDwAAAADwD/APAAAAAP////8AAAAA/////wAAAAD/////AAAAAP////8AAAAA/////wAAAAD/////AAAAAP////8AAAAA/////wAAAAD/////AAAAAP////8AAAAA/////wAAAAD/////AAA=";
const LEGACY_SKIN_OPAQUE_MASK: &str = "AP//AAAAAAAA//8AAAAAAAD//wAAAAAAAP//AAAAAAAA//8AAAAAAAD//wAAAAAAAP//AAAAAAAA//8AAAAAAP////8AAAAA/////wAAAAD/////AAAAAP////8AAAAA/////wAAAAD/////AAAAAP////8AAAAA/////wAAAADwD/D/D/APAPAP8P8P8A8A8A/w/w/wDwDwD/D/D/APAP////////8A/////////wD/////////AP////////8A/////////wD/////////AP////////8A/////////wD/////////AP////////8A/////////wD/////////AA==";

#[derive(Deserialize)]
struct TexturesProperty {
    textures: Textures,
}

#[derive(Deserialize)]
struct Textures {
    #[serde(rename = "SKIN")]
    skin: Option<SkinTexture>,
}

#[derive(Deserialize)]
struct SkinTexture {
    url: String,
    #[serde(default)]
    metadata: Option<SkinMetadata>,
}

#[derive(Deserialize)]
struct SkinMetadata {
    #[serde(default)]
    model: Option<String>,
}

fn decode_skin_png(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let width = reader.info().width;
    let height = reader.info().height;
    if width != 64 || (height != 32 && height != 64) {
        return None;
    }
    let buf_size = reader.output_buffer_size()?;
    let mut buf = vec![0u8; buf_size];
    let output_info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(output_info.buffer_size());

    let rgba = match output_info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => {
            let mut rgba = Vec::with_capacity((width * height * 4) as usize);
            for rgb in buf.as_chunks::<3>().0 {
                rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], u8::MAX]);
            }
            rgba
        }
        png::ColorType::Grayscale => {
            let mut rgba = Vec::with_capacity((width * height * 4) as usize);
            for &gray in &buf {
                rgba.extend_from_slice(&[gray, gray, gray, u8::MAX]);
            }
            rgba
        }
        png::ColorType::GrayscaleAlpha => {
            let mut rgba = Vec::with_capacity((width * height * 4) as usize);
            for ga in buf.as_chunks::<2>().0 {
                rgba.extend_from_slice(&[ga[0], ga[0], ga[0], ga[1]]);
            }
            rgba
        }
        png::ColorType::Indexed => return None,
    };

    if rgba.len() != (width * height * 4) as usize {
        return None;
    }

    Some((width, height, rgba))
}

impl Player {
    pub async fn fetch_skin(
        properties: &[Property],
    ) -> Option<pumpkin_protocol::bedrock::client::Skin> {
        let textures_prop = properties.iter().find(|p| &*p.name == "textures")?;
        let decoded = BASE64_STANDARD
            .decode(textures_prop.value.as_bytes())
            .ok()?;
        let textures: TexturesProperty = serde_json::from_slice(&decoded).ok()?;
        let skin_texture = textures.textures.skin?;
        let url = skin_texture.url;
        let is_slim = skin_texture
            .metadata
            .as_ref()
            .and_then(|m| m.model.as_deref())
            .is_some_and(|model| model == "slim");

        let client = pumpkin_auth::client();
        let bytes = client.get(&url).send().await.ok()?.bytes().await.ok()?;
        let (width, height, mut rgba) = decode_skin_png(&bytes)?;

        let opaque_mask = BASE64_STANDARD
            .decode(if height == 32 {
                LEGACY_SKIN_OPAQUE_MASK
            } else {
                SKIN_OPAQUE_MASK
            })
            .ok()?;
        for pixel_index in 0..(width * height) as usize {
            if opaque_mask[pixel_index >> 3] & (1 << (pixel_index & 7)) != 0 {
                rgba[pixel_index * 4 + 3] = u8::MAX;
            }
        }

        let mut skin = pumpkin_protocol::bedrock::client::Skin::steve();
        skin.set_slim(is_slim);
        skin.image_width = width;
        skin.image_height = height;
        skin.skin_data = rgba;
        skin.skin_id.clone_from(&url);
        skin.full_id = url;
        Some(skin)
    }

    #[expect(clippy::too_many_lines, clippy::items_after_statements)]
    pub fn new(
        client: Arc<ClientPlatform>,
        gameprofile: GameProfile,
        config: PlayerConfig,
        world: &Arc<World>,
        gamemode: GameMode,
    ) -> Self {
        let inventory_changed = Arc::new(AtomicBool::new(true));

        struct ScreenListener(Arc<AtomicBool>);

        impl ScreenHandlerListener for ScreenListener {
            fn on_slot_update(
                &self,
                _screen_handler: &ScreenHandlerBehaviour,
                _slot: u8,
                _stack: ItemStack,
            ) {
                self.0.store(true, Ordering::Relaxed);
            }
        }

        let server = world.server.upgrade().unwrap_or_else(|| {
            tracing::error!("server inactive");
            std::process::exit(1);
        });

        let player_uuid = gameprofile.id;

        let living_entity = LivingEntity::new(Entity::from_uuid(
            player_uuid,
            world.clone(),
            Vector3::new(0.0, 100.0, 0.0),
            &EntityType::PLAYER,
        ));
        living_entity.entity.invulnerable.store(
            matches!(gamemode, GameMode::Creative | GameMode::Spectator),
            Ordering::Relaxed,
        );
        living_entity
            .entity
            .no_physics
            .store(gamemode == GameMode::Spectator, Ordering::Relaxed);
        if gamemode == GameMode::Spectator {
            living_entity
                .entity
                .on_ground
                .store(false, Ordering::Relaxed);
        }

        let inventory = Arc::new(PlayerInventory::new(
            living_entity.entity_equipment.clone(),
            living_entity.equipment_slots.clone(),
        ));

        let ender_chest_inventory = Arc::new(EnderChestInventory::new());

        let player_screen_handler = Arc::new(std::sync::Mutex::new(PlayerScreenHandler::new(
            &inventory,
            None,
            0,
            Some(server.recipe_manager.clone()),
        )));

        // Initialize abilities based on gamemode (like vanilla's GameMode.setAbilities())
        let mut abilities = Abilities::default();
        abilities.set_for_gamemode(gamemode);

        let mut bedrock_skin = pumpkin_protocol::bedrock::client::Skin::steve();

        // Standard_Custom is a shared placeholder. Give fallback skins a stable,
        // per-player identity so Bedrock never sees duplicate skin IDs.
        if bedrock_skin.skin_id == "Standard_Custom" {
            let skin_id = format!("pumpkin:{player_uuid}");
            bedrock_skin.skin_id.clone_from(&skin_id);
            bedrock_skin.full_id = skin_id;
        }

        let supports_player_loaded = match client.as_ref() {
            ClientPlatform::Java(client) => client.version.load() >= JavaMinecraftVersion::V_1_21_4,
            ClientPlatform::Bedrock(_) => true,
        };
        let initially_loaded = !supports_player_loaded;

        Self {
            living_entity,
            config: ArcSwap::new(Arc::new(config)),
            advancements: Arc::new(Mutex::new(
                server
                    .advancement_manager
                    .clone()
                    .new_player_advancement(gameprofile.id),
            )),
            gameprofile,
            client,
            awaiting_teleport: Mutex::new(None),
            breath_manager: BreathManager::default(),
            // TODO: Load this from previous instance
            hunger_manager: HungerManager::default(),
            current_block_destroy_stage: AtomicI32::new(-1),
            current_block_breaking_speed: AtomicU32::new(0),
            synced_mining_efficiency_level: AtomicI32::new(-1),
            enchantment_seed: AtomicI32::new(rand::random()),
            open_container: AtomicCell::new(None),
            open_container_pos: AtomicCell::new(None),
            raid_omen_position: AtomicCell::new(None),
            entered_lava_pos: AtomicCell::new(None),
            tick_counter: AtomicI32::new(0),
            start_mining_time: AtomicI32::new(0),
            last_input: AtomicI8::new(0),
            carried_item: Mutex::new(None),
            experience_pick_up_delay: Mutex::new(0),
            teleport_id_count: AtomicI32::new(0),
            mining: AtomicBool::new(false),
            mining_pos: Mutex::new(BlockPos::ZERO),
            abilities: std::sync::Mutex::new(abilities),
            stats: std::sync::Mutex::new(statistics::Statistics::default()),
            gamemode: AtomicCell::new(gamemode),
            previous_gamemode: AtomicCell::new(None),
            camera_target_id: AtomicCell::new(None),
            is_movement_locked: AtomicBool::new(false),
            // TODO: Send the CPlayerSpawnPosition packet when the client connects with proper values
            respawn_point: std::sync::Mutex::new(None),
            sleeping_since: AtomicCell::new(None),
            sleeping_bed_pos: AtomicCell::new(None),
            sleeping_ignored: AtomicBool::new(false),
            // We want this to be an impossible watched section so that `chunker::update_position`
            // will mark chunks as watched for a new join rather than a respawn.
            // (We left shift by one so we can search around that chunk)
            watched_section: AtomicCell::new(Cylindrical::new(
                Vector2::new(0, 0),
                // Since 1 is not possible in vanilla it is used as uninit
                NonZero::new(1).unwrap_or(NonZero::<u8>::MIN),
            )),
            last_action_time: AtomicCell::new(std::time::Instant::now()),
            ping: AtomicU32::new(0),
            last_attacked_ticks: AtomicU32::new(0),
            client_loaded: AtomicBool::new(initially_loaded),
            bedrock_spawned: AtomicBool::new(false),
            client_loaded_timeout: AtomicU32::new(if initially_loaded { 0 } else { 60 }),
            chat_spam_tick_count: AtomicU32::new(0),
            // Item usage tracking
            using_item: AtomicBool::new(false),
            item_use_start_time: AtomicI32::new(0),
            using_hand: AtomicCell::new(None),
            // Minecraft has no way to change the default permission level of new players.
            // Minecraft's default permission level is 0.
            permission_lvl: server
                .data
                .operator_config
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get_entry(&player_uuid)
                .map_or(
                    AtomicCell::new(server.advanced_config.commands.default_op_level),
                    |op| AtomicCell::new(op.level),
                ),
            inventory,
            ender_chest_inventory,
            experience_level: AtomicI32::new(0),
            experience_progress: AtomicCell::new(0.0),
            experience_points: AtomicI32::new(0),
            item_cooldowns: std::sync::Mutex::new(HashMap::new()),
            chunk_sender: Mutex::new(crate::net::ChunkSender::new()),
            chunk_listener: Mutex::new(world.level.chunk_listener.add_global_chunk_listener()),
            held_chunk_tickets: Mutex::new(None),
            chunk_send_epoch: AtomicU32::new(0),
            xp_state: xp_levels::XpState::new(),
            last_sent_health: AtomicI32::new(-1),
            last_sent_food: AtomicU8::new(0),
            last_food_saturation: AtomicBool::new(true),
            subscribed_debug_sample: AtomicBool::new(false),
            has_played_before: AtomicBool::new(false),
            root_vehicle_uuid: AtomicCell::new(None),
            chat_session: Arc::new(Mutex::new(ChatSession::default())), // Placeholder value until the player actually sets their session id
            signature_cache: Mutex::new(MessageCache::default()),
            player_screen_handler: player_screen_handler.clone(),
            current_screen_handler: std::sync::Mutex::new(player_screen_handler),
            screen_handler_sync_id: AtomicU8::new(0),
            screen_handler_listener: Arc::new(ScreenListener(inventory_changed.clone())),
            inventory_changed,
            screen_handler_sync_handler: Arc::new(SyncHandler::new()),
            tab_list_header: Mutex::new(TextComponent::text("")),
            tab_list_footer: Mutex::new(TextComponent::text("")),
            display_name: std::sync::Mutex::new(None),
            tab_list_name: Mutex::new(None),
            tab_list_order: AtomicI32::new(0),
            tab_list_latency: AtomicI32::new(0),
            tab_list_listed: AtomicBool::new(true),
            per_player_time: AtomicCell::new(None),
            per_player_weather: AtomicCell::new(None),
            custom_scoreboard: std::sync::Mutex::new(None),
            compass_target: AtomicCell::new(None),
            respawn_location: AtomicCell::new(None),
            hidden_players: Mutex::new(std::collections::HashSet::new()),
            fishing_bobber: AtomicI32::new(-1),
            bedrock_skin: ArcSwap::new(Arc::new(bedrock_skin)),
            end_credits: health::EndCredits::default(),
            warden_spawn_tracker: std::sync::Mutex::new(WardenSpawnTracker::default()),
            score: AtomicI32::new(0),
            spawn_extra_particles_on_fall: AtomicBool::new(false),
            post_effects: std::sync::Mutex::new(Vec::new()),
            inbound_packets: SegQueue::new(),
        }
    }

    /// Spawns a task associated with this player-client. All tasks spawned with this method are awaited
    /// when the client. This means tasks should complete in a reasonable amount of time or select
    /// on `Self::await_close_interrupt` to cancel the task when the client is closed
    ///
    /// Returns an `Option<JoinHandle<F::Output>>`. If the client is closed, this returns `None`.
    pub fn spawn_task<F>(&self, task: F) -> Option<JoinHandle<F::Output>>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.client.spawn_task(task)
    }

    /// Bedrock remote-player spawn -> the `PlayerList` entry must precede `AddPlayer`.
    #[must_use]
    pub fn bedrock_spawn_packets(&self) -> (CPlayerList, CAddPlayer) {
        let entity = self.get_entity();
        let entity_id = i64::from(self.entity_id());
        let profile = &self.gameprofile;
        let mut entity_data = entity.bedrock_metadata();
        // name tag only shows on the crosshair without this.
        entity_data.set(
            pumpkin_protocol::bedrock::client::set_actor_data::entity_data_key::ALWAYS_SHOW_NAME_TAG,
            pumpkin_protocol::bedrock::client::set_actor_data::MetadataValue::Byte(1),
        );
        let add_player = CAddPlayer {
            uuid: profile.id,
            player_name: profile.name.clone(),
            target_runtime_id: VarULong(entity_id as u64),
            platform_chat_id: String::new(),
            position: entity.pos.load().to_f32_lossy(),
            velocity: entity.velocity.load().to_f32_lossy(),
            rotation: Vector2::new(entity.pitch.load(), entity.yaw.load()),
            y_head_rotation: entity.head_yaw.load(),
            carried_item:
                pumpkin_protocol::bedrock::network_item::NetworkItemStackDescriptor::default(),
            player_game_type: self.gamemode.load().into(),
            entity_data,
            synced_properties: PropertySyncData::default(),
            abilities_data: SerializedAbilitiesData {
                target_player_raw_id: entity_id,
                player_permissions: PlayerPermissionLevel::Visitor,
                command_permissions: CommandPermissionLevel::Any,
                layers: vec![SerializedAbilitiesDataSerializedLayer {
                    serialized_layer: 0,
                    abilities_set: 0,
                    ability_value: 0,
                    fly_speed: 0.05,
                    vertical_fly_speed: 0.05,
                    walk_speed: 0.1,
                }],
            },
            actor_links: Vec::new(),
            passenger_of_block: None,
            device_id: String::new(),
            build_platform: BuildPlatform::Unknown,
        };
        (self.bedrock_player_list(), add_player)
    }

    /// Removes the [`Player`] out of the current [`World`].
    pub async fn remove(self: &Arc<Self>) {
        if !self
            .current_screen_handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_any()
            .is::<PlayerScreenHandler>()
        {
            self.on_handled_screen_closed();
        }

        let vehicle = self
            .living_entity
            .entity
            .vehicle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(vehicle) = vehicle {
            self.root_vehicle_uuid
                .store(Some(vehicle.get_entity().entity_uuid));
            vehicle
                .get_entity()
                .remove_passenger_on_disconnect(self.entity_id());
        }

        self.stats
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .increment_custom(statistics::CustomStatistic::LeaveGame, 1);
        let world = self.world();
        world.remove_player(self, true).await;

        let cylindrical = self.watched_section.load();
        self.clean_up_chunk_tickets(&world.level);
        if let Ok(mut sender) = self.chunk_sender.lock() {
            sender.reset();
        }

        // Radial chunks are all of the chunks the player is theoretically viewing.
        // Given enough time, all of these chunks will be in memory.
        let radial_chunks = cylindrical.all_chunks_within();

        debug!(
            "Removing player {}, unwatching {} chunks",
            self.gameprofile.name,
            radial_chunks.len()
        );

        let level = &world.level;

        // Decrement the value of watched chunks
        let chunks_to_clean = level.mark_chunks_as_not_watched(radial_chunks);
        // Remove chunks with no watchers from the cache
        if !chunks_to_clean.is_empty() {
            world.remove_entities_in_chunks(&chunks_to_clean).await;
            level.clean_entity_chunks(&chunks_to_clean);
        }
        // Remove left over entries from all possiblily loaded chunks
        let cleaned_chunks = level.clean_memory();
        if !cleaned_chunks.is_empty() {
            world.remove_entities_in_chunks(&cleaned_chunks).await;
            level.clean_entity_chunks(&cleaned_chunks);
        }

        debug!(
            "Removed player id {} from world {} ({} chunks remain cached)",
            self.gameprofile.name,
            self.world().get_world_name(),
            level.loaded_chunk_count(),
        );

        //self.world().level.list_cached();
    }

    pub(crate) fn try_restore_vehicle(self: &Arc<Self>, vehicle: &Arc<dyn EntityBase>) {
        // Claim the UUID atomically, otherwise blank it
        // between a non-matching swap and restore.
        if self
            .root_vehicle_uuid
            .compare_exchange(Some(vehicle.get_entity().entity_uuid), None)
            .is_err()
        {
            return;
        }

        vehicle
            .get_entity()
            .add_passenger(vehicle.clone(), self.clone());
    }

    pub fn tick<'a>(&'a self, server: &'a Server) {
        let t_inbound = std::time::Instant::now();
        self.process_inbound_packets();
        server
            .tickprof
            .record(kvasir_tickprof::Phase::PlayerInbound, t_inbound.elapsed());

        if self.is_spectator() {
            self.living_entity
                .entity
                .on_ground
                .store(false, Ordering::Relaxed);
        }

        self.tick_camera();
        self.tick_screen_handler();

        // Statistics updates
        self.tick_statistics();
        self.tick_timers();

        let t_chunks = std::time::Instant::now();
        let total_sent_chunks = self.send_chunks();
        server
            .tickprof
            .record(kvasir_tickprof::Phase::PlayerChunks, t_chunks.elapsed());
        self.finish_bedrock_spawn(total_sent_chunks);
        self.advance_tick_counters();

        self.tick_mining();
        self.last_attacked_ticks.fetch_add(1, Ordering::Relaxed);

        self.living_entity.tick(self, server);

        self.breath_manager.tick(self);

        self.tick_peaceful_regeneration();

        self.hunger_manager.tick(self);

        // Vanilla updates pose in PlayerEntity#tick after super.tick().
        self.tick_advancements();

        // experience handling
        self.tick_experience();
        self.tick_health();
        self.tick_raid_omen();
        self.tick_maps(server);

        // Anti-spam counter decay
        self.decay_chat_spam(server);

        // Timeout/keep alive handling
        self.tick_client_load_timeout();
        // Idle timeout handling
        self.kick_when_idle(server);
    }

    pub const fn entity_id(&self) -> i32 {
        self.living_entity.entity.entity_id
    }

    pub fn world(&self) -> Arc<World> {
        self.living_entity.entity.world.load_full()
    }

    pub fn position(&self) -> Vector3<f64> {
        self.living_entity.entity.pos.load()
    }

    pub fn eye_position(&self) -> Vector3<f64> {
        let eye_height = self.living_entity.entity.get_eye_height();
        Vector3::new(
            self.living_entity.entity.pos.load().x,
            self.living_entity.entity.pos.load().y + eye_height,
            self.living_entity.entity.pos.load().z,
        )
    }

    /// Returns the player's rotation.
    /// Yaw then Pitch
    pub fn rotation(&self) -> (f32, f32) {
        (
            self.living_entity.entity.yaw.load(),
            self.living_entity.entity.pitch.load(),
        )
    }

    /// The id under which Pumpkin will store it's `mining_efficiency` modifier value
    const EFFICIENCY_ATTRIBUTE_MODIFIER_ID: &'static str = "minecraft:enchantment.efficiency";
}

impl PartialEq for Player {
    fn eq(&self, other: &Self) -> bool {
        self.gameprofile.id == other.gameprofile.id
    }
}

impl NBTStorage for PlayerInventory {
    fn write_nbt(&self, nbt: &mut NbtCompound) {
        // Save the selected slot (hotbar)
        nbt.put_int("SelectedItemSlot", i32::from(self.get_selected_slot()));

        // Create inventory list with the correct capacity (inventory size)
        let mut items: Vec<NbtTag> = Vec::with_capacity(41);
        {
            let main_inv = self
                .main_inventory
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for (i, stack) in main_inv.iter().enumerate() {
                if !stack.is_empty() {
                    let mut item_compound = NbtCompound::new();
                    item_compound.put_byte("Slot", i as i8);
                    stack.write_item_stack(&mut item_compound);
                    items.push(NbtTag::Compound(item_compound));
                }
            }
        }

        let mut equipment_compound = NbtCompound::new();
        {
            let equipment_guard = self
                .entity_equipment
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for (slot, stack) in &equipment_guard.equipment {
                if !stack.is_empty() {
                    let mut item_compound = NbtCompound::new();
                    stack.write_item_stack(&mut item_compound);
                    let vanilla_slot = match slot {
                        EquipmentSlot::Feet(_) => {
                            equipment_compound.put_compound("feet", item_compound.clone());
                            Some(100i8)
                        }
                        EquipmentSlot::Legs(_) => {
                            equipment_compound.put_compound("legs", item_compound.clone());
                            Some(101i8)
                        }
                        EquipmentSlot::Chest(_) => {
                            equipment_compound.put_compound("chest", item_compound.clone());
                            Some(102i8)
                        }
                        EquipmentSlot::Head(_) => {
                            equipment_compound.put_compound("head", item_compound.clone());
                            Some(103i8)
                        }
                        EquipmentSlot::OffHand(_) => {
                            equipment_compound.put_compound("offhand", item_compound.clone());
                            Some(-106i8)
                        }
                        _ => None,
                    };
                    if let Some(slot_byte) = vanilla_slot {
                        let mut inv_item_compound = NbtCompound::new();
                        inv_item_compound.put_byte("Slot", slot_byte);
                        stack.write_item_stack(&mut inv_item_compound);
                        items.push(NbtTag::Compound(inv_item_compound));
                    }
                }
            }
        }
        nbt.put_compound("equipment", equipment_compound);
        nbt.put("Inventory", NbtTag::List(items));
    }

    fn read_nbt_non_mut(&self, nbt: &NbtCompound) {
        // Read selected hotbar slot
        self.set_selected_slot(nbt.get_int("SelectedItemSlot").unwrap_or(0) as u8);
        // Process inventory list
        let set_stack_sync = |slot: usize, stack: ItemStack| {
            if slot < Self::MAIN_SIZE {
                let mut inv = self
                    .main_inventory
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                inv[slot] = stack;
            } else if let Some(slot) = self.equipment_slots.get(&slot) {
                self.entity_equipment
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .put(slot, stack);
            }
        };

        if let Some(inventory_list) = nbt.get_list("Inventory") {
            for tag in inventory_list {
                if let Some(item_compound) = tag.extract_compound()
                    && let Some(slot_byte) = item_compound.get_byte("Slot")
                {
                    let slot = match slot_byte {
                        100 => 36,  // feet
                        101 => 37,  // legs
                        102 => 38,  // chest
                        103 => 39,  // head
                        -106 => 40, // offhand
                        s if (0..=40).contains(&s) => s as usize,
                        _ => continue,
                    };
                    if let Some(item_stack) = ItemStack::read_item_stack(item_compound) {
                        set_stack_sync(slot, item_stack);
                    }
                }
            }
        }

        if let Some(equipment) = nbt.get_compound("equipment") {
            if let Some(offhand) = equipment.get_compound("offhand")
                && let Some(item_stack) = ItemStack::read_item_stack(offhand)
            {
                set_stack_sync(40, item_stack);
            }

            if let Some(head) = equipment.get_compound("head")
                && let Some(item_stack) = ItemStack::read_item_stack(head)
            {
                set_stack_sync(39, item_stack);
            }

            if let Some(chest) = equipment.get_compound("chest")
                && let Some(item_stack) = ItemStack::read_item_stack(chest)
            {
                set_stack_sync(38, item_stack);
            }

            if let Some(legs) = equipment.get_compound("legs")
                && let Some(item_stack) = ItemStack::read_item_stack(legs)
            {
                set_stack_sync(37, item_stack);
            }

            if let Some(feet) = equipment.get_compound("feet")
                && let Some(item_stack) = ItemStack::read_item_stack(feet)
            {
                set_stack_sync(36, item_stack);
            }
        }
    }
}

impl NBTStorageInit for PlayerInventory {}

impl NBTStorage for EnderChestInventory {
    fn write_nbt(&self, nbt: &mut NbtCompound) {
        // Create item list with the correct capacity (inventory size)
        let mut items: Vec<NbtTag> = Vec::with_capacity(Self::INVENTORY_SIZE);
        let ec_items = self
            .items
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (i, stack) in ec_items.iter().enumerate() {
            if !stack.is_empty() {
                let mut item_compound = NbtCompound::new();
                item_compound.put_byte("Slot", i as i8);
                stack.write_item_stack(&mut item_compound);
                items.push(NbtTag::Compound(item_compound));
            }
        }

        nbt.put("EnderItems", NbtTag::List(items));
    }

    fn read_nbt_non_mut(&self, nbt: &NbtCompound) {
        // Process item list
        if let Some(item_list) = nbt.get_list("EnderItems") {
            let mut items = self
                .items
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for tag in item_list {
                if let Some(item_compound) = tag.extract_compound()
                    && let Some(slot_byte) = item_compound.get_byte("Slot")
                    && (0..Self::INVENTORY_SIZE as i8).contains(&slot_byte)
                {
                    let slot = slot_byte as usize;
                    if let Some(item_stack) = ItemStack::read_item_stack(item_compound) {
                        items[slot] = item_stack;
                    }
                }
            }
        }
    }
}

impl NBTStorageInit for EnderChestInventory {}

impl EntityBase for Player {
    fn sound_source(&self) -> SoundCategory {
        SoundCategory::Players
    }

    /// Vanilla `Player.playSound`: everyone but this player, whose client plays it already.
    fn play_entity_sound(&self, sound: Sound, volume: f32, pitch: f32) {
        let entity = self.get_entity();
        if !entity.is_silent() {
            entity.world.load().play_sound_raw_expect(
                self,
                sound as u16,
                self.sound_source(),
                &entity.pos.load(),
                volume,
                pitch,
            );
        }
    }

    fn movement_emission(&self) -> MovementEmission {
        let entity = self.get_entity();
        let flying = self
            .abilities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .flying;
        if flying || entity.on_ground.load(Ordering::Relaxed) && entity.is_sneaking() {
            MovementEmission::None
        } else {
            MovementEmission::All
        }
    }

    fn get_swim_sound(&self) -> Option<Sound> {
        Some(Sound::EntityPlayerSwim)
    }

    fn get_swim_splash_sound(&self) -> Option<Sound> {
        Some(Sound::EntityPlayerSplash)
    }

    fn get_swim_high_speed_splash_sound(&self) -> Option<Sound> {
        Some(Sound::EntityPlayerSplashHighSpeed)
    }

    fn do_water_splash_effect(&self, caller: &dyn EntityBase) {
        if !self.is_spectator() {
            self.get_entity().water_splash_effect(caller);
        }
    }

    fn get_fall_sounds(&self) -> (Sound, Sound) {
        (Sound::EntityPlayerSmallFall, Sound::EntityPlayerBigFall)
    }

    fn play_step_sound(&self, pos: &BlockPos, state: &BlockState) {
        let entity = self.get_entity();
        if entity.is_in_water() {
            entity.water_swim_sound(self);
            self.play_muffled_step_sound(state);
            return;
        }
        let primary_pos = entity.get_primary_step_sound_block_pos(pos);
        if primary_pos == *pos {
            self.play_block_step_sound(state);
            return;
        }
        let primary_state = entity.world.load().get_block_state(&primary_pos);
        self.play_block_step_sound(primary_state);
        // Vanilla `playCombinationStepSounds` adds the muffled block underneath.
        if primary_state
            .id
            .to_block()
            .has_tag(&tag::Block::MINECRAFT_COMBINATION_STEP_SOUND_BLOCKS)
        {
            self.play_muffled_step_sound(state);
        }
    }

    fn damage_with_context(
        &self,
        caller: &dyn EntityBase,
        amount: f32,
        damage_type: DamageType,
        position: Option<Vector3<f64>>,
        source: Option<&dyn EntityBase>,
        cause: Option<&dyn EntityBase>,
    ) -> bool {
        self.damage_with_context(caller, amount, damage_type, position, source, cause)
    }

    fn teleport(
        &self,
        position: Vector3<f64>,
        yaw: Option<f32>,
        pitch: Option<f32>,
        world: Arc<World>,
    ) {
        if Arc::ptr_eq(&world, &self.world()) {
            // Same world
            let yaw = yaw.unwrap_or_else(|| self.living_entity.entity.yaw.load());
            let pitch = pitch.unwrap_or_else(|| self.living_entity.entity.pitch.load());
            self.request_teleport(position, yaw, pitch);
            let entity = self.get_entity();
            let chunk_pos = entity.chunk_pos.load();
            entity.world.load().broadcast_to_chunk_except(
                chunk_pos,
                &[self.living_entity.entity.entity_uuid],
                &CEntityPositionSync::new(
                    self.living_entity.entity.entity_id.into(),
                    position,
                    Vector3::new(0.0, 0.0, 0.0),
                    yaw,
                    pitch,
                    entity.on_ground.load(Ordering::SeqCst),
                ),
            );
        } else if let Some(player_arc) = self.world().get_player_by_uuid(self.gameprofile.id) {
            self.spawn_task(async move {
                player_arc.teleport_world(world, position, yaw, pitch).await;
            });
        }
    }

    fn get_entity(&self) -> &Entity {
        &self.living_entity.entity
    }

    fn get_living_entity(&self) -> Option<&LivingEntity> {
        Some(&self.living_entity)
    }

    fn get_player(&self) -> Option<&Player> {
        Some(self)
    }

    /// Bedrock renders remote players only from `AddPlayer`, never `AddActor`.
    fn send_bedrock_spawn_packet(&self, client: &crate::net::bedrock::BedrockClient) {
        let (player_list, add_player) = self.bedrock_spawn_packets();
        let equipment = pumpkin_protocol::bedrock::client::CMobEquipment {
            target_runtime_id: (self.entity_id() as u64).into(),
            item: (&self.inventory.held_item()).into(),
            slot: 0,
            selected_slot: 0,
            container_id: 0,
        };
        for data in [
            client.serialize_packet(&player_list),
            client.serialize_packet(&add_player),
            client.serialize_packet(&equipment),
        ]
        .into_iter()
        .flatten()
        {
            client.try_enqueue_packet(data);
        }
    }

    fn is_spectator(&self) -> bool {
        self.gamemode.load() == GameMode::Spectator
    }

    fn can_be_seen_as_enemy(&self) -> bool {
        !self
            .abilities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .invulnerable
            && !self.get_entity().invulnerable.load(Ordering::Relaxed)
            && self.can_be_seen_by_anyone()
    }

    fn set_on_fire_for_ticks(&self, ticks: u32) {
        let entity = self.get_entity();
        let ticks = if entity.invulnerable.load(Ordering::Relaxed) {
            1
        } else {
            ticks
        };
        if entity.fire_ticks.load(Ordering::Relaxed) < ticks as i32 {
            entity.fire_ticks.store(ticks as i32, Ordering::Relaxed);
        }
    }

    fn is_pushable(&self) -> bool {
        self.gamemode.load() != GameMode::Spectator && self.gamemode.load() != GameMode::Creative
    }

    fn get_name(&self) -> TextComponent {
        //TODO: team color
        TextComponent::text(self.gameprofile.name.clone())
    }

    fn get_display_name(&self) -> TextComponent {
        if let Some(display_name) = self
            .display_name
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            return display_name.clone();
        }
        let name = self.get_name();
        let name_clone = name.clone();
        let mut name = name.click_event(ClickEvent::SuggestCommand {
            command: format!("/tell {} ", self.gameprofile.name.clone()).into(),
        });
        name = name.hover_event(HoverEvent::show_entity(
            self.living_entity.entity.entity_uuid.to_string(),
            self.living_entity.entity.entity_type.resource_name.into(),
            Some(name_clone),
        ));
        name.insertion(self.gameprofile.name.clone())
    }

    fn cast_any(&self) -> &dyn std::any::Any {
        self
    }

    fn write_custom_nbt(&self, nbt: &mut NbtCompound) {
        nbt.put_int("DataVersion", DATA_VERSION);
        self.inventory.write_nbt(nbt);
        self.ender_chest_inventory.write_nbt(nbt);

        self.abilities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .write_nbt(nbt);

        self.write_experience_nbt(nbt);
        self.write_status_nbt(nbt);

        // Store food level, saturation, exhaustion, and tick timer
        self.hunger_manager.write_nbt(nbt);

        self.write_breath_nbt(nbt);

        nbt.put_string(
            "Dimension",
            self.world().dimension.minecraft_name.to_string(),
        );

        self.write_respawn_nbt(nbt);
        self.write_vehicle_nbt(nbt);
        self.stats
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .write_nbt(nbt);
    }

    fn read_custom_nbt(&self, nbt: &NbtCompound) {
        self.inventory.read_nbt_non_mut(nbt);
        self.ender_chest_inventory.read_nbt_non_mut(nbt);
        self.living_entity
            .apply_current_equipment_attribute_modifiers();

        self.read_experience_nbt(nbt);
        self.read_status_nbt(nbt);
        let gamemode = self.read_game_mode_nbt(nbt);
        self.apply_game_mode_flags(gamemode);

        self.has_played_before.store(
            nbt.get_bool("HasPlayedBefore").unwrap_or(false),
            Ordering::Relaxed,
        );

        self.hunger_manager.read_nbt_non_mut(nbt);

        self.read_breath_nbt(nbt);
        self.read_respawn_nbt(nbt);
        self.root_vehicle_uuid.store(read_root_vehicle(nbt));
        self.stats
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .read_nbt(nbt);
    }

    fn get_experience_reward(&self, _killer: Option<&dyn EntityBase>) -> u32 {
        // vanilla: min(level * 7, 100)
        let level = self.experience_level.load(Ordering::Relaxed);
        (level * 7).min(100) as u32
    }

    fn tick_in_void(&self, dyn_self: &dyn EntityBase) {
        self.living_entity.tick_in_void(dyn_self);
    }
}

#[derive(Clone, Copy, Debug)]
pub enum TitleMode {
    Title,
    SubTitle,
    ActionBar,
}

/// Represents a player's abilities and special powers.
///
/// This struct contains information about the player's current abilities, such as flight, invulnerability, and creative mode.
#[derive(Clone, Copy, Debug)]
pub struct Abilities {
    /// Indicates whether the player is invulnerable to damage.
    pub invulnerable: bool,
    /// Indicates whether the player is currently flying.
    pub flying: bool,
    /// Indicates whether the player is allowed to fly (if enabled).
    pub allow_flying: bool,
    /// Indicates whether the player is in creative mode.
    pub creative: bool,
    /// Indicates whether the player is allowed to modify the world.
    pub allow_modify_world: bool,
    /// The player's flying speed.
    pub fly_speed: f32,
    /// The field of view adjustment when the player is walking or sprinting.
    pub walk_speed: f32,
}

impl NBTStorage for Abilities {
    fn write_nbt(&self, nbt: &mut NbtCompound) {
        let mut component = NbtCompound::new();
        component.put_bool("invulnerable", self.invulnerable);
        component.put_bool("flying", self.flying);
        component.put_bool("mayfly", self.allow_flying);
        component.put_bool("instabuild", self.creative);
        component.put_bool("mayBuild", self.allow_modify_world);
        component.put_float("flySpeed", self.fly_speed);
        component.put_float("walkSpeed", self.walk_speed);
        nbt.put_compound("abilities", component);
    }

    fn read_nbt(&mut self, nbt: &mut NbtCompound) {
        Self::read_nbt(self, nbt);
    }
}

impl NBTStorageInit for Abilities {}

impl Default for Abilities {
    fn default() -> Self {
        Self {
            invulnerable: false,
            flying: false,
            allow_flying: false,
            creative: false,
            allow_modify_world: true,
            fly_speed: 0.05,
            walk_speed: 0.1,
        }
    }
}

impl Abilities {
    pub fn read_nbt(&mut self, nbt: &NbtCompound) {
        if let Some(component) = nbt.get_compound("abilities") {
            self.invulnerable = component.get_bool("invulnerable").unwrap_or(false);
            self.flying = component.get_bool("flying").unwrap_or(false);
            self.allow_flying = component.get_bool("mayfly").unwrap_or(false);
            self.creative = component.get_bool("instabuild").unwrap_or(false);
            self.allow_modify_world = component.get_bool("mayBuild").unwrap_or(true);
            self.fly_speed = component.get_float("flySpeed").unwrap_or(0.05);
            self.walk_speed = component.get_float("walkSpeed").unwrap_or(0.1);
        }
    }

    pub const fn set_for_gamemode(&mut self, gamemode: GameMode) {
        match gamemode {
            GameMode::Creative => {
                // self.flying = false; // Start not flying
                self.allow_flying = true;
                self.creative = true;
                self.invulnerable = true;
                self.allow_modify_world = true;
            }
            GameMode::Spectator => {
                self.flying = true;
                self.allow_flying = true;
                self.creative = false;
                self.invulnerable = true;
                self.allow_modify_world = false;
            }
            GameMode::Adventure => {
                self.flying = false;
                self.allow_flying = false;
                self.creative = false;
                self.invulnerable = false;
                self.allow_modify_world = false;
            }
            GameMode::Survival => {
                self.flying = false;
                self.allow_flying = false;
                self.creative = false;
                self.invulnerable = false;
                self.allow_modify_world = true;
            }
        }
    }
}

/// Represents the player's stored respawn point (bed/anchor/forced).
#[derive(Debug, Clone, PartialEq)]
pub struct RespawnPoint {
    pub dimension: Dimension,
    pub position: BlockPos,
    pub yaw: f32,
    pub force: bool,
}

pub struct CalculatedRespawnPoint {
    /// The exact position to spawn at (centered in block).
    pub position: Vector3<f64>,
    /// The yaw rotation.
    pub yaw: f32,
    /// The pitch rotation.
    pub pitch: f32,
    /// The dimension to spawn in.
    pub dimension: Dimension,
}

/// Represents the player's chat mode settings.
#[derive(Debug, Clone)]
pub enum ChatMode {
    /// Chat is enabled for the player.
    Enabled,
    /// The player should only see chat messages from commands.
    CommandsOnly,
    /// All messages should be hidden.
    Hidden,
}

pub struct InvalidChatMode;

impl TryFrom<i32> for ChatMode {
    type Error = InvalidChatMode;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Enabled),
            1 => Ok(Self::CommandsOnly),
            2 => Ok(Self::Hidden),
            _ => Err(InvalidChatMode),
        }
    }
}

/// Player's current chat session
pub struct ChatSession {
    pub session_id: uuid::Uuid,
    pub expires_at: i64,
    pub public_key: Box<[u8]>,
    pub signature: Box<[u8]>,
    pub messages_sent: i32,
    pub messages_received: i32,
    pub signature_cache: Vec<Box<[u8]>>,
}

impl Default for ChatSession {
    fn default() -> Self {
        Self::new(Uuid::nil(), 0, Box::new([]), Box::new([]))
    }
}

impl ChatSession {
    #[must_use]
    pub const fn new(
        session_id: Uuid,
        expires_at: i64,
        public_key: Box<[u8]>,
        key_signature: Box<[u8]>,
    ) -> Self {
        Self {
            session_id,
            expires_at,
            public_key,
            signature: key_signature,
            messages_sent: 0,
            messages_received: 0,
            signature_cache: Vec::new(),
        }
    }
}

#[derive(Clone, Default)]
pub struct LastSeen(Vec<Box<[u8]>>);

impl From<LastSeen> for Vec<Box<[u8]>> {
    fn from(seen: LastSeen) -> Self {
        seen.0
    }
}

impl AsRef<[Box<[u8]>]> for LastSeen {
    fn as_ref(&self) -> &[Box<[u8]>] {
        &self.0
    }
}

impl LastSeen {
    /// The sender's `last_seen` signatures are sent as ID's if the recipient has them in their cache.
    /// Otherwise, the full signature is sent. (ID:0 indicates full signature is being sent)
    pub fn indexed_for(&self, recipient: &Arc<Player>) -> Box<[PreviousMessage]> {
        let mut indexed = Vec::new();
        let cache = recipient
            .signature_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for signature in &self.0 {
            let index = cache.full_cache.iter().position(|s| s == signature);
            if let Some(index) = index {
                indexed.push(PreviousMessage {
                    // Send ID reference to recipient's cache (index + 1 because 0 is reserved for full signature)
                    id: VarInt(1 + index as i32),
                    signature: None,
                });
            } else {
                indexed.push(PreviousMessage {
                    // Send ID as 0 for full signature
                    id: VarInt(0),
                    signature: Some(signature.clone()),
                });
            }
        }
        indexed.into_boxed_slice()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LastSeenTrackedEntry {
    pub signature: Box<[u8]>,
    pub pending: bool,
}

impl LastSeenTrackedEntry {
    #[must_use]
    pub fn acknowledge(&self) -> Self {
        Self {
            signature: self.signature.clone(),
            pending: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LastSeenMessagesValidator {
    pub last_seen_count: usize,
    pub tracked_messages: VecDeque<Option<LastSeenTrackedEntry>>,
    pub last_pending_message: Option<Box<[u8]>>,
}

impl Default for LastSeenMessagesValidator {
    fn default() -> Self {
        Self::new(MAX_PREVIOUS_MESSAGES as usize)
    }
}

impl LastSeenMessagesValidator {
    #[must_use]
    pub fn new(last_seen_count: usize) -> Self {
        let mut tracked = VecDeque::with_capacity(last_seen_count);
        for _ in 0..last_seen_count {
            tracked.push_back(None);
        }
        Self {
            last_seen_count,
            tracked_messages: tracked,
            last_pending_message: None,
        }
    }

    pub fn add_pending(&mut self, signature: &[u8]) {
        if self.last_pending_message.as_deref() != Some(signature) {
            let sig_box: Box<[u8]> = signature.into();
            self.tracked_messages.push_back(Some(LastSeenTrackedEntry {
                signature: sig_box.clone(),
                pending: true,
            }));
            self.last_pending_message = Some(sig_box);
        }
    }

    #[must_use]
    pub fn tracked_messages_count(&self) -> usize {
        self.tracked_messages.len()
    }

    pub fn apply_offset(&mut self, offset: usize) -> Result<(), &'static str> {
        let max_offset = self
            .tracked_messages
            .len()
            .saturating_sub(self.last_seen_count);
        if offset <= max_offset {
            self.tracked_messages.drain(0..offset);
            Ok(())
        } else {
            Err("Advanced last seen window by more messages than expected")
        }
    }

    pub fn apply_update(
        &mut self,
        offset: usize,
        acknowledged: &[u8],
    ) -> Result<Vec<Box<[u8]>>, &'static str> {
        self.apply_offset(offset)?;
        let mut last_seen_entries = Vec::new();

        for i in 0..self.last_seen_count {
            let is_acknowledged = if i / 8 < acknowledged.len() {
                (acknowledged[i / 8] & (1 << (i % 8))) != 0
            } else {
                false
            };

            let message = self.tracked_messages.get(i).cloned().flatten();
            if is_acknowledged {
                let Some(entry) = message else {
                    return Err(
                        "Last seen update acknowledged unknown or previously ignored message",
                    );
                };
                self.tracked_messages[i] = Some(entry.acknowledge());
                last_seen_entries.push(entry.signature);
            } else {
                if let Some(entry) = message
                    && !entry.pending
                {
                    return Err("Last seen update ignored previously acknowledged message");
                }
                self.tracked_messages[i] = None;
            }
        }

        Ok(last_seen_entries)
    }
}

pub struct MessageCache {
    /// max 128 cached message signatures. Most recent FIRST.
    /// Server should (when possible) reference indexes in this (recipient's) cache instead of sending full signatures in last seen.
    /// Must be 1:1 with client's signature cache.
    full_cache: VecDeque<Box<[u8]>>,
    /// max 20 last seen messages by the sender. Most Recent LAST
    pub last_seen: LastSeen,
    pub last_seen_validator: LastSeenMessagesValidator,
}

impl Default for MessageCache {
    fn default() -> Self {
        Self {
            full_cache: VecDeque::with_capacity(MAX_CACHED_SIGNATURES as usize),
            last_seen: LastSeen::default(),
            last_seen_validator: LastSeenMessagesValidator::default(),
        }
    }
}

impl MessageCache {
    /// Not used for caching seen messages. Only for non-indexed signatures from senders.
    pub fn cache_signatures(&mut self, signatures: &[Box<[u8]>]) {
        for sig in signatures.iter().rev() {
            if self.full_cache.contains(sig) {
                continue;
            }
            // If the cache is maxed, and someone sends a signature older than the oldest in cache, ignore it
            if self.full_cache.len() < MAX_CACHED_SIGNATURES as usize {
                self.full_cache.push_back(sig.clone()); // Recipient never saw this message so it must be older than the oldest in cache
            }
        }
    }

    /// Adds a seen signature to `last_seen` and `full_cache`.
    pub fn add_seen_signature(&mut self, signature: &[u8]) {
        if self.last_seen.0.len() >= MAX_PREVIOUS_MESSAGES as usize {
            self.last_seen.0.remove(0);
        }
        self.last_seen.0.push(signature.into());
        // This probably doesn't need to be a loop, but better safe than sorry
        while self.full_cache.len() >= MAX_CACHED_SIGNATURES as usize {
            self.full_cache.pop_back();
        }
        self.full_cache.push_front(signature.into()); // Since recipient saw this message it will be most recent in cache
    }
}

impl InventoryPlayer for Player {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn drop_item(&self, item: ItemStack, _retain_ownership: bool) {
        self.drop_item(item);
    }

    fn has_infinite_materials(&self) -> bool {
        self.gamemode.load() == GameMode::Creative
    }

    fn is_creative(&self) -> bool {
        self.gamemode.load() == GameMode::Creative
    }

    fn is_spectator(&self) -> bool {
        self.gamemode.load() == GameMode::Spectator
    }

    fn experience_level(&self) -> i32 {
        self.experience_level
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn add_experience_levels(&self, levels: i32) {
        self.add_experience_levels(levels);
    }

    fn enchantment_seed(&self) -> i32 {
        self.enchantment_seed.load(Ordering::Relaxed)
    }

    fn set_enchantment_seed(&self, seed: i32) {
        self.enchantment_seed.store(seed, Ordering::Relaxed);
    }

    fn get_inventory(&self) -> Arc<PlayerInventory> {
        self.inventory.clone()
    }

    fn enqueue_inventory_packet(
        &self,
        packet: &CSetContainerContent,
        window_type: Option<WindowType>,
    ) {
        match self.client.as_ref() {
            ClientPlatform::Java(java) => {
                if let Ok(data) = java.serialize_packet(packet) {
                    java.try_enqueue_packet(data);
                }
            }
            ClientPlatform::Bedrock(bedrock) => {
                use pumpkin_protocol::bedrock::{
                    client::inventory_content::CInventoryContent,
                    network_item::{ContainerName, FullContainerName, NetworkItemStackDescriptor},
                };
                use pumpkin_protocol::codec::var_uint::VarUInt;

                let window_id = packet.window_id.0 as u32;
                if window_id == 0 {
                    // Java's player screen also contains crafting, armor, and off-hand
                    // slots. Bedrock container 0 contains only the 36-slot player
                    // inventory in hotbar-first order.
                    let slots = self
                        .inventory
                        .main_inventory
                        .read()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .iter()
                        .map(NetworkItemStackDescriptor::from)
                        .collect();
                    let bedrock_packet = CInventoryContent {
                        container_id: VarUInt(0),
                        slots,
                        full_container_name: FullContainerName {
                            container_name: ContainerName::Inventory,
                            dynamic_id: None,
                        },
                        storage_item: NetworkItemStackDescriptor::default(),
                    };
                    if let Ok(data) = bedrock.serialize_packet(&bedrock_packet) {
                        bedrock.try_enqueue_packet(data);
                    }
                } else if matches!(
                    window_type,
                    Some(
                        WindowType::Generic9x1
                            | WindowType::Generic9x2
                            | WindowType::Generic9x3
                            | WindowType::Generic9x4
                            | WindowType::Generic9x5
                            | WindowType::Generic9x6
                            | WindowType::Generic3x3
                    )
                ) {
                    // Java container screens append the player's 36 inventory slots to
                    // the container slots. Bedrock synchronizes those two inventories
                    // separately and addresses generic block containers as LevelEntity.
                    let container_slot_count = packet
                        .slot_data
                        .len()
                        .saturating_sub(PlayerInventory::MAIN_SIZE);
                    let slots = packet.slot_data[..container_slot_count]
                        .iter()
                        .map(|stack| NetworkItemStackDescriptor::from(&*stack.0))
                        .collect();
                    let bedrock_packet = CInventoryContent {
                        container_id: VarUInt(window_id),
                        slots,
                        full_container_name: FullContainerName {
                            container_name: ContainerName::LevelEntity,
                            dynamic_id: None,
                        },
                        storage_item: NetworkItemStackDescriptor::default(),
                    };
                    if let Ok(data) = bedrock.serialize_packet(&bedrock_packet) {
                        bedrock.try_enqueue_packet(data);
                    }
                }
            }
        }
    }

    fn enqueue_slot_packet(
        &self,
        packet: &CSetContainerSlot,
        window_type: Option<WindowType>,
        total_slots: usize,
    ) {
        match self.client.as_ref() {
            ClientPlatform::Java(java) => {
                if let Ok(data) = java.serialize_packet(packet) {
                    java.try_enqueue_packet(data);
                }
            }
            ClientPlatform::Bedrock(bedrock) => {
                use pumpkin_protocol::bedrock::{
                    client::inventory_slot::CInventorySlot,
                    network_item::{ContainerName, FullContainerName, NetworkItemStackDescriptor},
                };
                use pumpkin_protocol::codec::var_uint::VarUInt;

                let window_id = packet.window_id;
                if window_id == 0 {
                    if let Some(slot_idx) = bedrock_inventory_slot(packet.slot) {
                        let item_desc = NetworkItemStackDescriptor::from(&*packet.slot_data.0);
                        let bedrock_packet = CInventorySlot {
                            container_id: VarUInt(0),
                            slot: VarUInt(slot_idx),
                            full_container_name: Some(FullContainerName {
                                container_name: ContainerName::Inventory,
                                dynamic_id: None,
                            }),
                            storage_item: None,
                            item: item_desc,
                        };
                        if let Ok(data) = bedrock.serialize_packet(&bedrock_packet) {
                            bedrock.try_enqueue_packet(data);
                        }
                    }
                } else {
                    let slot_idx = packet.slot as usize;
                    let item_desc = NetworkItemStackDescriptor::from(&*packet.slot_data.0);

                    let bedrock_info = if total_slots >= 36 {
                        let container_slots = total_slots - 36;
                        if slot_idx < container_slots {
                            if window_type == Some(WindowType::Crafting) {
                                if slot_idx == 0 {
                                    Some((ContainerName::CreatedOutput, 0))
                                } else {
                                    Some((ContainerName::CraftingInput, (32 + slot_idx - 1) as u8))
                                }
                            } else {
                                Some((ContainerName::LevelEntity, slot_idx as u8))
                            }
                        } else {
                            let inv_slot = slot_idx - container_slots;
                            if inv_slot < 27 {
                                Some((ContainerName::Inventory, (inv_slot + 9) as u8))
                            } else {
                                Some((ContainerName::Inventory, (inv_slot - 27) as u8))
                            }
                        }
                    } else {
                        None
                    };

                    if let Some((container_name, slot_id)) = bedrock_info {
                        let bedrock_packet = CInventorySlot {
                            container_id: VarUInt(window_id as u32),
                            slot: VarUInt(slot_id as u32),
                            full_container_name: Some(FullContainerName {
                                container_name,
                                dynamic_id: None,
                            }),
                            storage_item: None,
                            item: item_desc,
                        };
                        if let Ok(data) = bedrock.serialize_packet(&bedrock_packet) {
                            bedrock.try_enqueue_packet(data);
                        }
                    }
                }
            }
        }
    }

    fn enqueue_cursor_packet(&self, packet: &CSetCursorItem) {
        match self.client.as_ref() {
            ClientPlatform::Java(java) => {
                if let Ok(data) = java.serialize_packet(packet) {
                    java.try_enqueue_packet(data);
                }
            }
            ClientPlatform::Bedrock(bedrock) => {
                use pumpkin_protocol::bedrock::{
                    client::inventory_content::CInventoryContent,
                    network_item::{ContainerName, FullContainerName, NetworkItemStackDescriptor},
                };
                use pumpkin_protocol::codec::var_uint::VarUInt;

                let item_desc = NetworkItemStackDescriptor::from(&*packet.stack.0);
                let bedrock_packet = CInventoryContent {
                    container_id: VarUInt(59),
                    slots: vec![item_desc],
                    full_container_name: FullContainerName {
                        container_name: ContainerName::Cursor,
                        dynamic_id: None,
                    },
                    storage_item: NetworkItemStackDescriptor::default(),
                };
                if let Ok(data) = bedrock.serialize_packet(&bedrock_packet) {
                    bedrock.try_enqueue_packet(data);
                }
            }
        }
    }

    fn enqueue_property_packet(&self, packet: &CSetContainerProperty) {
        self.try_send_client_packet(packet);
    }

    fn enqueue_slot_set_packet(&self, packet: &CSetPlayerInventory) {
        self.try_send_slot_set_packet(packet);
        self.sync_inventory_to_client();
    }

    fn enqueue_set_held_item_packet(&self, packet: &CSetSelectedSlot) {
        self.try_enqueue_packet_editioned(
            packet,
            &pumpkin_protocol::bedrock::client::CPlayerHotbar {
                selected_slot: pumpkin_protocol::codec::var_uint::VarUInt(packet.slot as u32),
                container_id: 0,
                should_select_slot: true,
            },
        );
    }

    fn enqueue_equipment_change(&self, slot: &EquipmentSlot, stack: &ItemStack) {
        self.living_entity
            .send_equipment_changes(&[(slot.clone(), stack.clone())]);

        if let Some(equippable) = stack.get_data_component::<EquippableImpl>() {
            self.world().play_sound_event(
                &equippable.equip_sound,
                SoundCategory::Players,
                &self.position(),
            );
        }
    }

    fn award_experience(&self, amount: i32) {
        debug!("Player::award_experience called with amount={amount}");
        if amount > 0 {
            debug!("Player: adding {amount} experience points");
            let player = self.world().get_player_by_uuid(self.gameprofile.id);
            if let Some(player) = player {
                player.add_experience_points(amount);
            }
        }
    }

    fn increment_stat(&self, category: StatisticCategory, stat_id: i32, amount: i32) {
        self.increment_stat(category, stat_id, amount);
    }

    fn play_block_sound(&self, sound: Sound, pitch: f32) {
        if let Some(pos) = self.open_container_pos.load() {
            self.world().play_sound_fine(
                sound,
                SoundCategory::Blocks,
                &pos.to_centered_f64(),
                1.0,
                pitch,
            );
        }
    }

    fn fire_prepare_item_enchant_event(
        &self,
        item: &ItemStack,
        level_requirements: &mut [i32; 3],
        enchantment_id: &mut [i32; 3],
        enchantment_level: &mut [i32; 3],
        bookshelf_count: i32,
    ) -> bool {
        let Some(player_arc) = self.world().get_player_by_uuid(self.gameprofile.id) else {
            return false;
        };
        let Some(server) = self.world().server.upgrade() else {
            return false;
        };
        let mut event = PrepareItemEnchantEvent::new(
            player_arc,
            item.clone(),
            *level_requirements,
            *enchantment_id,
            *enchantment_level,
            bookshelf_count,
        );
        server.plugin_manager.fire_blocking(&server, &mut event);
        if event.cancelled {
            return true;
        }
        *level_requirements = event.level_requirements;
        *enchantment_id = event.enchantment_id;
        *enchantment_level = event.enchantment_level;
        false
    }

    fn fire_enchant_item_event(
        &self,
        item: &ItemStack,
        option: i32,
        exp_level_cost: i32,
        enchantments_to_add: &mut Vec<(&'static pumpkin_data::Enchantment, i32)>,
    ) -> bool {
        let Some(player_arc) = self.world().get_player_by_uuid(self.gameprofile.id) else {
            return false;
        };
        let Some(server) = self.world().server.upgrade() else {
            return false;
        };
        let mut event = EnchantItemEvent::new(
            player_arc,
            item.clone(),
            option,
            exp_level_cost,
            enchantments_to_add.clone(),
        );
        server.plugin_manager.fire_blocking(&server, &mut event);
        if event.cancelled {
            return true;
        }
        *enchantments_to_add = event.enchantments_to_add;
        false
    }

    fn close_screen_handler(&self) {
        self.close_handled_screen();
    }

    fn use_anvil(&self) {
        if let Some(pos) = self.open_container_pos.load() {
            let world = self.world();
            let state = world.get_block_state(&pos);
            let block = pumpkin_data::Block::from_state_id(state.id);
            if block.has_tag(&pumpkin_data::tag::Block::MINECRAFT_ANVIL) {
                if !self.has_infinite_materials() && rand::random::<f32>() < 0.12 {
                    if let Some(new_state) =
                        crate::block::blocks::anvil::AnvilBlock::damage(state.id)
                    {
                        world.set_block_state(
                            &pos,
                            new_state,
                            pumpkin_world::world::BlockFlags::NOTIFY_ALL,
                        );
                        world.sync_world_event(
                            pumpkin_data::world::WorldEvent::SoundAnvilUsed,
                            pos,
                            0,
                        );
                    } else {
                        world.set_block_state(
                            &pos,
                            pumpkin_data::BlockStateId::AIR,
                            pumpkin_world::world::BlockFlags::NOTIFY_ALL,
                        );
                        world.sync_world_event(
                            pumpkin_data::world::WorldEvent::SoundAnvilBroken,
                            pos,
                            0,
                        );
                    }
                } else {
                    world.sync_world_event(pumpkin_data::world::WorldEvent::SoundAnvilUsed, pos, 0);
                }
            } else {
                world.sync_world_event(pumpkin_data::world::WorldEvent::SoundAnvilUsed, pos, 0);
            }
        }
    }

    fn use_grindstone(&self, xp_amount: i32) {
        if let Some(pos) = self.open_container_pos.load() {
            let world = self.world();
            if xp_amount > 0 {
                crate::entity::experience_orb::ExperienceOrbEntity::spawn(
                    &world,
                    pos.to_centered_f64(),
                    xp_amount as u32,
                );
            }
            world.sync_world_event(pumpkin_data::world::WorldEvent::SoundGrindstoneUsed, pos, 0);
        }
    }

    fn on_enchanted_item(&self, levels: i32) {
        self.trigger_advancement(
            crate::entity::player::advancement::trigger::AdvancementTrigger::EnchantedItem {
                levels,
            },
        );
    }

    fn on_recipe_crafted(&self, item: &pumpkin_data::item_stack::ItemStack) {
        let key = item.item.registry_key.to_string();
        self.trigger_advancement(
            crate::entity::player::advancement::trigger::AdvancementTrigger::RecipeCrafted {
                recipe_id: format!("minecraft:{key}"),
            },
        );
        self.trigger_advancement(
            crate::entity::player::advancement::trigger::AdvancementTrigger::RecipeUnlocked {
                recipe_id: format!("minecraft:{key}"),
            },
        );
        self.trigger_advancement(
            crate::entity::player::advancement::trigger::AdvancementTrigger::InventoryChanged,
        );
    }

    fn on_brewed_potion(&self) {
        self.trigger_advancement(
            crate::entity::player::advancement::trigger::AdvancementTrigger::BrewedPotion,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{bedrock_inventory_slot, read_root_vehicle, write_root_vehicle};
    use pumpkin_nbt::{compound::NbtCompound, tag::NbtTag};
    use uuid::Uuid;

    #[test]
    fn player_screen_slots_map_to_bedrock_inventory() {
        assert_eq!(bedrock_inventory_slot(9), Some(9));
        assert_eq!(bedrock_inventory_slot(35), Some(35));
        assert_eq!(bedrock_inventory_slot(36), Some(0));
        assert_eq!(bedrock_inventory_slot(44), Some(8));
        assert_eq!(bedrock_inventory_slot(8), None);
        assert_eq!(bedrock_inventory_slot(45), None);
    }

    #[test]
    fn root_vehicle_uuid_round_trips_with_vanilla_shape() {
        let expected = Uuid::from_u128(0xFEDC_BA98_7654_3210_89AB_CDEF_0123_4567);
        let mut nbt = NbtCompound::new();

        write_root_vehicle(&mut nbt, expected);

        assert_eq!(read_root_vehicle(&nbt), Some(expected));
        assert!(
            nbt.get_compound("RootVehicle")
                .and_then(|root| root.get_int_array("Attach"))
                .is_some()
        );
    }

    #[test]
    fn root_vehicle_uuid_accepts_integer_lists() {
        let expected = Uuid::from_u128(0xFEDC_BA98_7654_3210_89AB_CDEF_0123_4567);
        let value = expected.as_u128();
        let mut root_vehicle = NbtCompound::new();
        root_vehicle.put(
            "Attach",
            NbtTag::List(vec![
                NbtTag::Int((value >> 96) as i32),
                NbtTag::Int((value >> 64) as i32),
                NbtTag::Int((value >> 32) as i32),
                NbtTag::Int(value as i32),
            ]),
        );
        let mut nbt = NbtCompound::new();
        nbt.put("RootVehicle", NbtTag::Compound(root_vehicle));

        assert_eq!(read_root_vehicle(&nbt), Some(expected));
    }

    #[test]
    fn anti_spam_counter_decay_and_threshold() {
        use std::sync::atomic::{AtomicU32, Ordering};
        let counter = AtomicU32::new(0);
        let message_cost = 20u32;
        let spam_threshold = 200u32;
        let decay_per_tick = 1u32;

        // 10 messages -> count = 200 (within threshold)
        for _ in 0..10 {
            counter.fetch_add(message_cost, Ordering::SeqCst);
        }
        assert_eq!(counter.load(Ordering::SeqCst), 200);
        assert!(counter.load(Ordering::SeqCst) <= spam_threshold);

        // 11th message -> count = 220 (exceeds threshold)
        counter.fetch_add(message_cost, Ordering::SeqCst);
        assert_eq!(counter.load(Ordering::SeqCst), 220);
        assert!(counter.load(Ordering::SeqCst) > spam_threshold);

        // Simulate 25 ticks of decay
        for _ in 0..25 {
            let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                Some(count.saturating_sub(decay_per_tick))
            });
        }
        assert_eq!(counter.load(Ordering::SeqCst), 195);
        assert!(counter.load(Ordering::SeqCst) <= spam_threshold);

        // Simulate decay down past 0 (saturating at 0)
        for _ in 0..250 {
            let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                Some(count.saturating_sub(decay_per_tick))
            });
        }
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn reach_distances_adhere_to_platform_defaults() {
        // Standard entity interaction range: ~3.0 for Java, ~3.5 for Bedrock
        let java_default = 3.0f64;
        let bedrock_default = 3.5f64;
        assert!(bedrock_default > java_default);

        // Standard block interaction range: 4.5 for Java, 5.0 for Bedrock
        let java_block = 4.5f64;
        let bedrock_block = 5.0f64;
        assert!(bedrock_block > java_block);
    }

    #[test]
    fn decode_skin_png_works() {
        let mut png_bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png_bytes, 64, 64);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            let data = vec![128u8; 64 * 64 * 4];
            writer.write_image_data(&data).unwrap();
        };

        let decoded = super::decode_skin_png(&png_bytes);
        assert!(decoded.is_some());
        let (width, height, rgba) = decoded.unwrap();
        assert_eq!(width, 64);
        assert_eq!(height, 64);
        assert_eq!(rgba.len(), 64 * 64 * 4);
        assert_eq!(rgba[0], 128);

        // Test invalid dimensions
        let mut invalid_bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut invalid_bytes, 32, 32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            let data = vec![0u8; 32 * 32 * 4];
            writer.write_image_data(&data).unwrap();
        };
        assert!(super::decode_skin_png(&invalid_bytes).is_none());
        assert!(super::decode_skin_png(&[]).is_none());
    }
}

#[cfg(test)]
mod nbt_golden;
