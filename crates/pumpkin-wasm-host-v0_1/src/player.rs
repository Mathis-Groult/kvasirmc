mod bedrock;
mod disconnect_reason;
mod java;
mod with_store;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use wasmtime::component::Resource;

use crate::{
    events::{from_wasm_position, to_wasm_game_mode, to_wasm_position},
    pumpkin::{
        self,
        plugin::{
            player::{Player, PlayerSkin, SkinParts},
            statistics::{
                CustomStatistic as WitCustomStatistic, StatisticCategory as WitStatisticCategory,
            },
            uuid::Uuid,
        },
    },
    uuid::UuidExt,
};
use pumpkin_core::entity::{EntityBase, player::TitleMode};
use pumpkin_inventory::player::player_inventory::PlayerInventory;
use pumpkin_protocol::Property;
use pumpkin_util::permission::PermissionLvl;
use pumpkin_wasm_host_common::state::PluginHostState;

pub(crate) fn from_wit_server_link(
    state: &mut PluginHostState,
    link: pumpkin::plugin::player::ServerLink,
) -> wasmtime::Result<(pumpkin_protocol::Label, String)> {
    use pumpkin::plugin::player::{KnownServerLink, ServerLinkLabel};
    let label = match link.label {
        ServerLinkLabel::Known(known) => {
            let link_type = match known {
                KnownServerLink::BugReport => pumpkin_protocol::LinkType::BugReport,
                KnownServerLink::CommunityGuidelines => {
                    pumpkin_protocol::LinkType::CommunityGuidelines
                }
                KnownServerLink::Support => pumpkin_protocol::LinkType::Support,
                KnownServerLink::Status => pumpkin_protocol::LinkType::Status,
                KnownServerLink::Feedback => pumpkin_protocol::LinkType::Feedback,
                KnownServerLink::Community => pumpkin_protocol::LinkType::Community,
                KnownServerLink::Website => pumpkin_protocol::LinkType::Website,
                KnownServerLink::Forums => pumpkin_protocol::LinkType::Forums,
                KnownServerLink::News => pumpkin_protocol::LinkType::News,
                KnownServerLink::Announcements => pumpkin_protocol::LinkType::Announcements,
            };
            pumpkin_protocol::Label::BuiltIn(link_type)
        }
        ServerLinkLabel::Custom(text) => {
            let text_res = state.take(text)?;
            pumpkin_protocol::Label::TextComponent(Box::new(text_res))
        }
    };
    Ok((label, link.url))
}

#[must_use]
pub const fn to_wit_statistic_category(
    category: pumpkin_data::statistic::StatisticCategory,
) -> WitStatisticCategory {
    match category {
        pumpkin_data::statistic::StatisticCategory::Mined => WitStatisticCategory::Mined,
        pumpkin_data::statistic::StatisticCategory::Crafted => WitStatisticCategory::Crafted,
        pumpkin_data::statistic::StatisticCategory::Used => WitStatisticCategory::Used,
        pumpkin_data::statistic::StatisticCategory::Broken => WitStatisticCategory::Broken,
        pumpkin_data::statistic::StatisticCategory::PickedUp => WitStatisticCategory::PickedUp,
        pumpkin_data::statistic::StatisticCategory::Dropped => WitStatisticCategory::Dropped,
        pumpkin_data::statistic::StatisticCategory::Killed => WitStatisticCategory::Killed,
        pumpkin_data::statistic::StatisticCategory::KilledBy => WitStatisticCategory::KilledBy,
        pumpkin_data::statistic::StatisticCategory::Custom => WitStatisticCategory::Custom,
    }
}

#[must_use]
pub const fn from_wit_statistic_category(
    wit: WitStatisticCategory,
) -> pumpkin_data::statistic::StatisticCategory {
    match wit {
        WitStatisticCategory::Mined => pumpkin_data::statistic::StatisticCategory::Mined,
        WitStatisticCategory::Crafted => pumpkin_data::statistic::StatisticCategory::Crafted,
        WitStatisticCategory::Used => pumpkin_data::statistic::StatisticCategory::Used,
        WitStatisticCategory::Broken => pumpkin_data::statistic::StatisticCategory::Broken,
        WitStatisticCategory::PickedUp => pumpkin_data::statistic::StatisticCategory::PickedUp,
        WitStatisticCategory::Dropped => pumpkin_data::statistic::StatisticCategory::Dropped,
        WitStatisticCategory::Killed => pumpkin_data::statistic::StatisticCategory::Killed,
        WitStatisticCategory::KilledBy => pumpkin_data::statistic::StatisticCategory::KilledBy,
        WitStatisticCategory::Custom => pumpkin_data::statistic::StatisticCategory::Custom,
    }
}

#[must_use]
pub const fn to_wit_custom_statistic(
    stat: pumpkin_data::statistic::CustomStatistic,
) -> WitCustomStatistic {
    // SAFETY: WitCustomStatistic is generated in the same numerical order as CustomStatistic
    unsafe { std::mem::transmute(stat as u8) }
}

#[must_use]
pub fn from_wit_custom_statistic(
    wit: WitCustomStatistic,
) -> pumpkin_data::statistic::CustomStatistic {
    pumpkin_data::statistic::CustomStatistic::from_i32(wit as i32)
        .unwrap_or(pumpkin_data::statistic::CustomStatistic::PlayTime)
}

pub(crate) fn text_component_from_resource(
    state: &PluginHostState,
    text: &Resource<pumpkin::plugin::text::TextComponent>,
) -> pumpkin_util::text::TextComponent {
    state
        .get(text)
        .expect("invalid text-component resource handle")
        .clone()
}

pub(crate) const fn to_wit_permission_level(
    level: PermissionLvl,
) -> pumpkin::plugin::permission::PermissionLevel {
    match level {
        PermissionLvl::Zero => pumpkin::plugin::permission::PermissionLevel::Zero,
        PermissionLvl::One => pumpkin::plugin::permission::PermissionLevel::One,
        PermissionLvl::Two => pumpkin::plugin::permission::PermissionLevel::Two,
        PermissionLvl::Three => pumpkin::plugin::permission::PermissionLevel::Three,
        PermissionLvl::Four => pumpkin::plugin::permission::PermissionLevel::Four,
    }
}

pub(crate) const fn from_wit_permission_level(
    level: pumpkin::plugin::permission::PermissionLevel,
) -> PermissionLvl {
    match level {
        pumpkin::plugin::permission::PermissionLevel::Zero => PermissionLvl::Zero,
        pumpkin::plugin::permission::PermissionLevel::One => PermissionLvl::One,
        pumpkin::plugin::permission::PermissionLevel::Two => PermissionLvl::Two,
        pumpkin::plugin::permission::PermissionLevel::Three => PermissionLvl::Three,
        pumpkin::plugin::permission::PermissionLevel::Four => PermissionLvl::Four,
    }
}

pub(crate) fn parse_ban_expiry(
    expires_at_utc: Option<String>,
    duration_seconds: Option<u64>,
) -> Option<time::OffsetDateTime> {
    if let Some(dur) = duration_seconds {
        let seconds = i64::try_from(dur).unwrap_or(i64::MAX);
        return Some(time::OffsetDateTime::now_utc() + time::Duration::seconds(seconds));
    }
    if let Some(s) = expires_at_utc {
        if s.eq_ignore_ascii_case("forever") || s.is_empty() {
            return None;
        }
        if let Ok(parsed) =
            time::OffsetDateTime::parse(&s, &time::format_description::well_known::Rfc3339)
        {
            return Some(parsed);
        }
        if let Ok(parsed) = time::OffsetDateTime::parse(
            &s,
            time::macros::format_description!(
                "[year]-[month]-[day] [hour]:[minute]:[second][offset_hour sign:mandatory]:[offset_minute]"
            ),
        ) {
            return Some(parsed);
        }
    }
    None
}

impl pumpkin::plugin::player::Host for PluginHostState {
    fn get_world_players(
        &mut self,
        world: Resource<pumpkin::plugin::world::World>,
    ) -> wasmtime::Result<Vec<Resource<pumpkin::plugin::player::Player>>> {
        let world = self.take(world)?;

        let mut players = Vec::new();
        for player in world.players.load().iter() {
            players.push(self.add(player.clone())?);
        }

        Ok(players)
    }
}
use crate::events::from_wasm_hand;
use pumpkin_inventory::generic_container_screen_handler::GenericContainerScreenHandler;
use pumpkin_inventory::player::ender_chest_inventory::EnderChestInventory;
use pumpkin_inventory::{Clearable, Inventory};
use pumpkin_protocol::codec::item_stack_seralizer::ItemStackSerializer;
use pumpkin_protocol::java::client::play::CSetContainerSlot;

use crate::pumpkin::plugin::item_stack::ItemStack as WitHostItemStack;

impl pumpkin::plugin::player::HostPlayer for PluginHostState {
    async fn set_item_in_hand(
        &mut self,
        player: Resource<Player>,
        hand: pumpkin::plugin::common::Hand,
        stack: Option<Resource<WitHostItemStack>>,
    ) -> wasmtime::Result<()> {
        let stack = if let Some(stack_res) = stack {
            self.take(stack_res)?.lock().await.clone()
        } else {
            pumpkin_data::item_stack::ItemStack::EMPTY.clone()
        };
        let player = self.get(&player)?;

        let hand = from_wasm_hand(hand);
        let slot = match hand {
            pumpkin_util::Hand::Right => player.inventory().get_selected_slot() as usize,
            pumpkin_util::Hand::Left => PlayerInventory::OFF_HAND_SLOT,
        };

        player.inventory().set_stack(slot, stack.clone());
        player.sync_hand_slot(slot, stack);

        Ok(())
    }

    async fn set_inventory_item(
        &mut self,
        player: Resource<Player>,
        slot: u8,
        stack: Option<Resource<WitHostItemStack>>,
    ) -> wasmtime::Result<()> {
        let stack = if let Some(stack_res) = stack {
            self.take(stack_res)?.lock().await.clone()
        } else {
            pumpkin_data::item_stack::ItemStack::EMPTY.clone()
        };
        let player = self.get(&player)?;

        player.inventory().set_stack(slot as usize, stack.clone());

        player.sync_hand_slot(slot as usize, stack);

        Ok(())
    }

    fn get_inventory(
        &mut self,
        player: Resource<Player>,
    ) -> wasmtime::Result<Resource<crate::pumpkin::plugin::inventory::PlayerInventory>> {
        self.add(self.get(&player)?.clone())
    }

    fn get_ender_chest(
        &mut self,
        player: Resource<Player>,
    ) -> wasmtime::Result<Resource<crate::pumpkin::plugin::inventory::Inventory>> {
        self.add(
            pumpkin_wasm_host_common::state::InventoryProvider::PlayerEnderChest(
                self.get(&player)?.clone(),
            ),
        )
    }

    fn get_inventory_item(
        &mut self,
        player: Resource<Player>,
        slot: u8,
    ) -> wasmtime::Result<Option<Resource<WitHostItemStack>>> {
        let player = self.get(&player)?;
        let stack = player.inventory().get_stack(slot as usize);
        if stack.is_empty() {
            Ok(None)
        } else {
            Ok(Some(self.add(Arc::new(tokio::sync::Mutex::new(stack)))?))
        }
    }

    fn get_ender_chest_item(
        &mut self,
        player: Resource<Player>,
        slot: u8,
    ) -> wasmtime::Result<Option<Resource<WitHostItemStack>>> {
        let player = self.get(&player)?;
        let ec = player.ender_chest_inventory();
        let stack = ec.get_stack(slot as usize);
        if stack.is_empty() {
            Ok(None)
        } else {
            Ok(Some(self.add(Arc::new(tokio::sync::Mutex::new(stack)))?))
        }
    }

    async fn set_ender_chest_item(
        &mut self,
        player: Resource<Player>,
        slot: u8,
        stack: Option<Resource<WitHostItemStack>>,
    ) -> wasmtime::Result<()> {
        let stack = if let Some(stack_res) = stack {
            self.take(stack_res)?.lock().await.clone()
        } else {
            pumpkin_data::item_stack::ItemStack::EMPTY.clone()
        };
        let player = self.get(&player)?;

        let ec = player.ender_chest_inventory();
        ec.set_stack(slot as usize, stack.clone());

        // If the player currently has their ender chest screen open, sync slot
        let sync_id = {
            let screen_handler_arc = player
                .current_screen_handler
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            let handler = screen_handler_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(generic) = handler
                .as_any()
                .downcast_ref::<GenericContainerScreenHandler>()
                && generic.inventory.as_any().is::<EnderChestInventory>()
            {
                Some(handler.sync_id())
            } else {
                None
            }
        };
        if let Some(sync_id) = sync_id {
            let stack_serializer = ItemStackSerializer::from(stack);
            let packet = CSetContainerSlot::new(sync_id as i8, 0, slot as i16, &stack_serializer);
            player.send_client_packet(&packet).await;
        }

        Ok(())
    }

    async fn clear_ender_chest(&mut self, player: Resource<Player>) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        let ec = player.ender_chest_inventory();
        ec.clear();

        // If the player currently has their ender chest screen open, sync all slots
        let sync_id = {
            let screen_handler_arc = player
                .current_screen_handler
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            let handler = screen_handler_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(generic) = handler
                .as_any()
                .downcast_ref::<GenericContainerScreenHandler>()
                && generic.inventory.as_any().is::<EnderChestInventory>()
            {
                Some(handler.sync_id())
            } else {
                None
            }
        };
        if let Some(sync_id) = sync_id {
            let empty_serializer =
                ItemStackSerializer::from(pumpkin_data::item_stack::ItemStack::EMPTY.clone());
            for slot in 0..27 {
                let packet =
                    CSetContainerSlot::new(sync_id as i8, 0, slot as i16, &empty_serializer);
                player.send_client_packet(&packet).await;
            }
        }

        Ok(())
    }

    fn get_item_in_hand(
        &mut self,
        player: Resource<Player>,
        hand: pumpkin::plugin::common::Hand,
    ) -> wasmtime::Result<Option<Resource<WitHostItemStack>>> {
        let player = self.get(&player)?;
        let hand = from_wasm_hand(hand);
        let stack = player.inventory().get_stack_in_hand(hand);
        if stack.is_empty() {
            Ok(None)
        } else {
            Ok(Some(self.add(Arc::new(tokio::sync::Mutex::new(stack)))?))
        }
    }

    fn as_entity(
        &mut self,
        player: Resource<Player>,
    ) -> wasmtime::Result<Resource<pumpkin::plugin::world::Entity>> {
        self.add(self.get(&player)?.clone() as _)
    }

    fn get_id(&mut self, player: Resource<Player>) -> wasmtime::Result<Uuid> {
        let player = self.get(&player)?;
        Ok(Uuid::to_wit(&player.gameprofile.id))
    }

    fn get_name(&mut self, player: Resource<Player>) -> wasmtime::Result<String> {
        let player = self.get(&player)?;
        Ok(player.gameprofile.name.clone())
    }

    fn get_position(
        &mut self,
        player: Resource<Player>,
    ) -> wasmtime::Result<pumpkin::plugin::common::Position> {
        let player = self.get(&player)?;
        Ok(to_wasm_position(player.position()))
    }

    fn get_yaw(&mut self, player: Resource<Player>) -> wasmtime::Result<f32> {
        let player = self.get(&player)?;
        Ok(player.get_entity().yaw.load())
    }

    fn get_pitch(&mut self, player: Resource<Player>) -> wasmtime::Result<f32> {
        let player = self.get(&player)?;
        Ok(player.get_entity().pitch.load())
    }

    fn get_world(
        &mut self,
        player: Resource<Player>,
    ) -> wasmtime::Result<Resource<pumpkin::plugin::world::World>> {
        let player = self.get(&player)?;
        let world = player.world();
        self.add(world)
            .map_err(|_| wasmtime::Error::msg("failed to add world resource"))
    }

    fn get_gamemode(
        &mut self,
        player: Resource<Player>,
    ) -> wasmtime::Result<pumpkin::plugin::common::GameMode> {
        let player = self.get(&player)?;
        Ok(to_wasm_game_mode(player.gamemode.load()))
    }

    fn get_locale(&mut self, player: Resource<Player>) -> wasmtime::Result<String> {
        let player = self.get(&player)?;
        Ok(player.config.load().locale.clone())
    }

    fn get_ping(&mut self, player: Resource<Player>) -> wasmtime::Result<u32> {
        let player = self.get(&player)?;
        Ok(player.ping.load(Ordering::Relaxed))
    }

    fn get_permission_level(
        &mut self,
        player: Resource<Player>,
    ) -> wasmtime::Result<pumpkin::plugin::permission::PermissionLevel> {
        let player = self.get(&player)?;
        Ok(to_wit_permission_level(player.permission_lvl.load()))
    }

    fn set_permission(
        &mut self,
        player: Resource<Player>,
        node: String,
        value: bool,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        let server = self.server.as_ref().expect("server not available");

        server
            .permission_manager
            .set_permission(player.gameprofile.id, node, value);

        Ok(())
    }

    fn unset_permission(&mut self, player: Resource<Player>, node: String) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        let server = self.server.as_ref().expect("server not available");

        server
            .permission_manager
            .unset_permission(&player.gameprofile.id, &node);

        Ok(())
    }

    fn has_permission_set(
        &mut self,
        player: Resource<Player>,
        node: String,
    ) -> wasmtime::Result<Option<bool>> {
        let player = self.get(&player)?;
        let server = self.server.as_ref().expect("server not available");

        Ok(server
            .permission_manager
            .has_permission_set(&player.gameprofile.id, &node))
    }

    fn get_display_name(
        &mut self,
        player: Resource<Player>,
    ) -> wasmtime::Result<Resource<pumpkin::plugin::text::TextComponent>> {
        let player = self.get(&player)?;
        let display_name = player.get_display_name();
        self.add(display_name)
            .map_err(|_| wasmtime::Error::msg("failed to add text-component resource"))
    }

    fn set_display_name(
        &mut self,
        player: Resource<Player>,
        display_name: Resource<pumpkin::plugin::text::TextComponent>,
    ) -> wasmtime::Result<()> {
        let display_name = self.take(display_name)?;
        let player = self.get(&player)?;
        player.set_display_name(Some(display_name));
        Ok(())
    }

    fn get_tab_list_name(
        &mut self,
        player: Resource<Player>,
    ) -> wasmtime::Result<Option<Resource<pumpkin::plugin::text::TextComponent>>> {
        let player = self.get(&player)?;
        let tab_list_name = player.get_tab_list_name();
        tab_list_name.map_or_else(
            || Ok(None),
            |name| {
                self.add(name)
                    .map(Some)
                    .map_err(|_| wasmtime::Error::msg("failed to add text-component resource"))
            },
        )
    }

    fn set_tab_list_name(
        &mut self,
        player: Resource<Player>,
        name: Option<Resource<pumpkin::plugin::text::TextComponent>>,
    ) -> wasmtime::Result<()> {
        let name = name.map(|n| self.take(n)).transpose()?;
        let player = self.get(&player)?;
        player.set_tab_list_name(name);
        Ok(())
    }

    fn send_system_message(
        &mut self,
        player: Resource<Player>,
        text: Resource<pumpkin::plugin::text::TextComponent>,
        overlay: bool,
    ) -> wasmtime::Result<()> {
        let component = self.take(text)?;
        let player = self.get(&player)?;
        player.send_system_message_raw(&component, overlay);
        Ok(())
    }

    async fn delete_message_by_signature(
        &mut self,
        player: Resource<Player>,
        signature: Vec<u8>,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        if let Some(client) = player.client.java() {
            let packet =
                pumpkin_protocol::java::client::play::CDeleteChat::from_signature(&signature);
            client.send_packet(&packet).await;
        }
        Ok(())
    }

    async fn delete_message_by_id(
        &mut self,
        player: Resource<Player>,
        signature_id: i32,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        if let Some(client) = player.client.java() {
            let packet =
                pumpkin_protocol::java::client::play::CDeleteChat::from_cache_id(signature_id);
            client.send_packet(&packet).await;
        }
        Ok(())
    }

    fn set_camera(
        &mut self,
        player: Resource<Player>,
        entity: Option<Resource<crate::pumpkin::plugin::world::Entity>>,
    ) -> wasmtime::Result<()> {
        let entity = entity.map(|r| self.take(r)).transpose()?;
        let player = self.get(&player)?;
        if let Some(target) = entity {
            player.set_camera_entity_id(target.get_entity().entity_id);
        } else {
            player.reset_camera();
        }
        Ok(())
    }

    fn set_camera_entity_id(
        &mut self,
        player: Resource<Player>,
        entity_id: u32,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.set_camera_entity_id(entity_id as i32);
        Ok(())
    }

    fn get_camera_entity_id(&mut self, player: Resource<Player>) -> wasmtime::Result<u32> {
        let player = self.get(&player)?;
        Ok(player.get_camera_entity_id() as u32)
    }

    fn reset_camera(&mut self, player: Resource<Player>) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.reset_camera();
        Ok(())
    }

    fn play_sound(
        &mut self,
        player: Resource<Player>,
        sound: pumpkin::plugin::sounds::Sound,
        category: pumpkin::plugin::sounds::SoundCategory,
        volume: f32,
        pitch: f32,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        let sound_name = format!("{sound:?}").to_lowercase().replace('_', ".");
        let sound_data = pumpkin_data::sound::Sound::from_name(&sound_name)
            .ok_or_else(|| wasmtime::Error::msg(format!("Unknown sound: {sound_name}")))?;
        let internal_category = super::world::from_wit_sound_category(category);
        let pos = player.position();
        player.play_sound(
            sound_data as u16,
            internal_category,
            &pos,
            volume,
            pitch,
            rand::random::<i64>(),
        );
        Ok(())
    }

    fn play_sound_at(
        &mut self,
        player: Resource<Player>,
        pos: pumpkin::plugin::common::Position,
        sound: pumpkin::plugin::sounds::Sound,
        category: pumpkin::plugin::sounds::SoundCategory,
        volume: f32,
        pitch: f32,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        let sound_name = format!("{sound:?}").to_lowercase().replace('_', ".");
        let sound_data = pumpkin_data::sound::Sound::from_name(&sound_name)
            .ok_or_else(|| wasmtime::Error::msg(format!("Unknown sound: {sound_name}")))?;
        let internal_category = super::world::from_wit_sound_category(category);
        player.play_sound(
            sound_data as u16,
            internal_category,
            &pumpkin_util::math::vector3::Vector3::new(pos.0, pos.1, pos.2),
            volume,
            pitch,
            rand::random::<i64>(),
        );
        Ok(())
    }

    fn stop_sound(
        &mut self,
        player: Resource<Player>,
        sound: Option<pumpkin::plugin::sounds::Sound>,
        category: Option<pumpkin::plugin::sounds::SoundCategory>,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        let sound_rl = sound.and_then(|s| {
            let sound_name = format!("{s:?}").to_lowercase().replace('_', ".");
            pumpkin_data::sound::Sound::from_name(&sound_name).map(|s| s.to_name().into())
        });
        let cat = category.map(super::world::from_wit_sound_category);
        player.stop_sound(sound_rl, cat);
        Ok(())
    }

    fn play_custom_sound(
        &mut self,
        player: Resource<Player>,
        sound_name: String,
        category: pumpkin::plugin::sounds::SoundCategory,
        volume: f32,
        pitch: f32,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        let internal_category = super::world::from_wit_sound_category(category);
        let pos = player.position();
        player.play_custom_sound(&sound_name, internal_category, &pos, volume, pitch);
        Ok(())
    }

    fn play_custom_sound_at(
        &mut self,
        player: Resource<Player>,
        pos: pumpkin::plugin::common::Position,
        sound_name: String,
        category: pumpkin::plugin::sounds::SoundCategory,
        volume: f32,
        pitch: f32,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        let internal_category = super::world::from_wit_sound_category(category);
        player.play_custom_sound(
            &sound_name,
            internal_category,
            &pumpkin_util::math::vector3::Vector3::new(pos.0, pos.1, pos.2),
            volume,
            pitch,
        );
        Ok(())
    }

    fn stop_custom_sound(
        &mut self,
        player: Resource<Player>,
        sound_name: Option<String>,
        category: Option<pumpkin::plugin::sounds::SoundCategory>,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        let sound_rl = sound_name
            .as_deref()
            .map(pumpkin_util::resource_location::ResourceLocation::from);
        let cat = category.map(super::world::from_wit_sound_category);
        player.stop_sound(sound_rl, cat);
        Ok(())
    }

    fn spawn_particles(
        &mut self,
        player: Resource<Player>,
        particle: pumpkin::plugin::particles::Particle,
        pos: pumpkin::plugin::common::Position,
        count: u32,
        offset: pumpkin::plugin::common::Position,
        max_speed: f32,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        let particle_data =
            pumpkin_data::particle::Particle::from_id(particle as u16).ok_or_else(|| {
                wasmtime::Error::msg(format!("Unknown particle ID: {}", particle as u16))
            })?;
        player.spawn_particles(
            particle_data,
            pumpkin_util::math::vector3::Vector3::new(pos.0, pos.1, pos.2),
            count,
            pumpkin_util::math::vector3::Vector3::new(
                offset.0 as f32,
                offset.1 as f32,
                offset.2 as f32,
            ),
            max_speed,
        );
        Ok(())
    }

    fn send_block_change(
        &mut self,
        player: Resource<Player>,
        pos: pumpkin::plugin::common::BlockPos,
        block_id: u16,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.send_block_change(
            pumpkin_util::math::position::BlockPos(pumpkin_util::math::vector3::Vector3::new(
                pos.x, pos.y, pos.z,
            )),
            block_id,
        );
        Ok(())
    }

    fn reset_block_change(
        &mut self,
        player: Resource<Player>,
        pos: pumpkin::plugin::common::BlockPos,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.reset_block_change(pumpkin_util::math::position::BlockPos(
            pumpkin_util::math::vector3::Vector3::new(pos.x, pos.y, pos.z),
        ));
        Ok(())
    }

    fn send_hurt_animation(&mut self, player: Resource<Player>, yaw: f32) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.send_hurt_animation(yaw);
        Ok(())
    }

    fn open_book(
        &mut self,
        player: Resource<Player>,
        hand: pumpkin::plugin::common::Hand,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        let hand = match hand {
            pumpkin::plugin::common::Hand::Right => pumpkin_util::Hand::Right,
            pumpkin::plugin::common::Hand::Left => pumpkin_util::Hand::Left,
        };
        player.open_book(hand);
        Ok(())
    }

    fn open_sign_editor(
        &mut self,
        player: Resource<Player>,
        pos: pumpkin::plugin::common::BlockPos,
        is_front_text: bool,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.open_sign_editor(
            pumpkin_util::math::position::BlockPos(pumpkin_util::math::vector3::Vector3::new(
                pos.x, pos.y, pos.z,
            )),
            is_front_text,
        );
        Ok(())
    }

    fn set_velocity(
        &mut self,
        player: Resource<Player>,
        velocity: pumpkin::plugin::common::Position,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.set_velocity(pumpkin_util::math::vector3::Vector3::new(
            velocity.0, velocity.1, velocity.2,
        ));
        Ok(())
    }

    fn apply_knockback(
        &mut self,
        player: Resource<Player>,
        strength: f64,
        x: f64,
        z: f64,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.apply_knockback(strength, x, z);
        Ok(())
    }

    fn set_movement_locked(
        &mut self,
        player: Resource<Player>,
        locked: bool,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.set_movement_locked(locked);
        Ok(())
    }

    fn is_movement_locked(&mut self, player: Resource<Player>) -> wasmtime::Result<bool> {
        let player = self.get(&player)?;
        Ok(player.is_movement_locked())
    }

    fn set_freeze_ticks(&mut self, player: Resource<Player>, ticks: i32) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.set_freeze_ticks(ticks);
        Ok(())
    }

    fn get_freeze_ticks(&mut self, player: Resource<Player>) -> wasmtime::Result<i32> {
        let player = self.get(&player)?;
        Ok(player.get_freeze_ticks())
    }

    fn set_server_links(
        &mut self,
        player: Resource<Player>,
        links: Vec<pumpkin::plugin::player::ServerLink>,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?.clone();
        let mut converted = Vec::new();
        for link in links {
            converted.push(from_wit_server_link(self, link)?);
        }
        let protocol_links: Vec<pumpkin_protocol::Link<'_>> = converted
            .iter()
            .map(|(label, url)| pumpkin_protocol::Link::new(label.clone(), url))
            .collect();
        player.set_server_links(&protocol_links);
        Ok(())
    }

    fn remove_effect(
        &mut self,
        player: Resource<Player>,
        effect: pumpkin::plugin::status_effect::StatusEffectType,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        let effect_type = super::status_effect::from_wasm_status_effect_type(effect);
        if let Some(status_effect) =
            pumpkin_data::effect::StatusEffect::from_name(effect_type.to_name())
        {
            player.remove_effect(status_effect);
        }
        Ok(())
    }

    fn clear_effects(&mut self, player: Resource<Player>) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.remove_all_effects();
        Ok(())
    }

    fn has_effect(
        &mut self,
        player: Resource<Player>,
        effect: pumpkin::plugin::status_effect::StatusEffectType,
    ) -> wasmtime::Result<bool> {
        let player = self.get(&player)?;
        let effect_type = super::status_effect::from_wasm_status_effect_type(effect);
        Ok(
            pumpkin_data::effect::StatusEffect::from_name(effect_type.to_name())
                .is_some_and(|status_effect| player.has_effect(status_effect)),
        )
    }

    fn get_effect(
        &mut self,
        player: Resource<Player>,
        effect: pumpkin::plugin::status_effect::StatusEffectType,
    ) -> wasmtime::Result<Option<pumpkin::plugin::status_effect::StatusEffectInstance>> {
        let player = self.get(&player)?;
        let effect_type = super::status_effect::from_wasm_status_effect_type(effect);
        if let Some(status_effect) =
            pumpkin_data::effect::StatusEffect::from_name(effect_type.to_name())
            && let Some(eff) = player.get_effect(status_effect)
        {
            return Ok(super::status_effect::to_wasm_status_effect_instance(&eff));
        }
        Ok(None)
    }

    fn get_active_effects(
        &mut self,
        player: Resource<Player>,
    ) -> wasmtime::Result<Vec<pumpkin::plugin::status_effect::StatusEffectInstance>> {
        let player = self.get(&player)?;
        let effects = player.get_active_effects();
        let mut list = Vec::with_capacity(effects.len());
        for eff in &effects {
            if let Some(instance) = super::status_effect::to_wasm_status_effect_instance(eff) {
                list.push(instance);
            }
        }
        Ok(list)
    }

    fn get_statistic(
        &mut self,
        player: Resource<Player>,
        category: WitStatisticCategory,
        stat_id: i32,
    ) -> wasmtime::Result<i32> {
        let player = self.get(&player)?;
        Ok(player.get_stat(from_wit_statistic_category(category), stat_id))
    }

    fn set_statistic(
        &mut self,
        player: Resource<Player>,
        category: WitStatisticCategory,
        stat_id: i32,
        value: i32,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.set_stat(from_wit_statistic_category(category), stat_id, value);
        Ok(())
    }

    fn increment_statistic(
        &mut self,
        player: Resource<Player>,
        category: WitStatisticCategory,
        stat_id: i32,
        amount: i32,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.increment_stat(from_wit_statistic_category(category), stat_id, amount);
        Ok(())
    }

    fn get_custom_statistic(
        &mut self,
        player: Resource<Player>,
        stat: WitCustomStatistic,
    ) -> wasmtime::Result<i32> {
        let player = self.get(&player)?;
        Ok(player.get_custom_stat(from_wit_custom_statistic(stat)))
    }

    fn set_custom_statistic(
        &mut self,
        player: Resource<Player>,
        stat: WitCustomStatistic,
        value: i32,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.set_custom_stat(from_wit_custom_statistic(stat), value);
        Ok(())
    }

    fn increment_custom_statistic(
        &mut self,
        player: Resource<Player>,
        stat: WitCustomStatistic,
        amount: i32,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.increment_custom_stat(from_wit_custom_statistic(stat), amount);
        Ok(())
    }

    fn send_stats(&mut self, player: Resource<Player>) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.send_stats();
        Ok(())
    }

    fn get_team(&mut self, player: Resource<Player>) -> wasmtime::Result<Option<String>> {
        let player = self.get(&player)?;
        let team = player.get_team();
        Ok(team.map(|t| t.name))
    }

    fn start_cooldown(
        &mut self,
        player: Resource<Player>,
        group: String,
        duration_ticks: i32,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.start_cooldown(group, duration_ticks);
        Ok(())
    }

    fn get_cooldown(&mut self, player: Resource<Player>, group: String) -> wasmtime::Result<f32> {
        let player = self.get(&player)?;
        Ok(player.get_cooldown(&group))
    }

    fn is_on_cooldown(
        &mut self,
        player: Resource<Player>,
        group: String,
    ) -> wasmtime::Result<bool> {
        let player = self.get(&player)?;
        Ok(player.is_on_cooldown(&group))
    }

    fn set_allow_flight(
        &mut self,
        player: Resource<Player>,
        allowed: bool,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.set_allow_flight(allowed);
        Ok(())
    }

    fn set_fly_speed(&mut self, player: Resource<Player>, speed: f32) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.set_fly_speed(speed);
        Ok(())
    }

    fn set_walk_speed(&mut self, player: Resource<Player>, speed: f32) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.set_walk_speed(speed);
        Ok(())
    }

    fn set_invulnerable(
        &mut self,
        player: Resource<Player>,
        invulnerable: bool,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.set_invulnerable(invulnerable);
        Ok(())
    }

    fn set_player_time(
        &mut self,
        player: Resource<Player>,
        time: u64,
        relative: bool,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.set_player_time(time, relative);
        Ok(())
    }

    fn reset_player_time(&mut self, player: Resource<Player>) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.reset_player_time();
        Ok(())
    }

    fn get_player_time(&mut self, player: Resource<Player>) -> wasmtime::Result<Option<u64>> {
        let player = self.get(&player)?;
        Ok(player.get_player_time())
    }

    fn is_player_time_relative(&mut self, player: Resource<Player>) -> wasmtime::Result<bool> {
        let player = self.get(&player)?;
        Ok(player.is_player_time_relative())
    }

    fn set_player_weather(
        &mut self,
        player: Resource<Player>,
        weather: crate::pumpkin::plugin::player::PlayerWeather,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        let w = match weather {
            crate::pumpkin::plugin::player::PlayerWeather::Clear => {
                pumpkin_core::entity::player::PlayerWeather::Clear
            }
            crate::pumpkin::plugin::player::PlayerWeather::Downfall => {
                pumpkin_core::entity::player::PlayerWeather::Downfall
            }
        };
        player.set_player_weather(w);
        Ok(())
    }

    fn reset_player_weather(&mut self, player: Resource<Player>) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.reset_player_weather();
        Ok(())
    }

    fn get_player_weather(
        &mut self,
        player: Resource<Player>,
    ) -> wasmtime::Result<Option<crate::pumpkin::plugin::player::PlayerWeather>> {
        let player = self.get(&player)?;
        Ok(player.get_player_weather().map(|w| match w {
            pumpkin_core::entity::player::PlayerWeather::Clear => {
                crate::pumpkin::plugin::player::PlayerWeather::Clear
            }
            pumpkin_core::entity::player::PlayerWeather::Downfall => {
                crate::pumpkin::plugin::player::PlayerWeather::Downfall
            }
        }))
    }

    fn set_compass_target(
        &mut self,
        player: Resource<Player>,
        pos: crate::pumpkin::plugin::common::Position,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        let pos_vec = from_wasm_position(pos);
        let block_pos = pumpkin_util::math::position::BlockPos::new(
            pos_vec.x as i32,
            pos_vec.y as i32,
            pos_vec.z as i32,
        );
        player.set_compass_target(block_pos);
        Ok(())
    }

    fn get_compass_target(
        &mut self,
        player: Resource<Player>,
    ) -> wasmtime::Result<crate::pumpkin::plugin::common::Position> {
        let player = self.get(&player)?;
        let target = player
            .get_compass_target()
            .unwrap_or(pumpkin_util::math::position::BlockPos::new(0, 0, 0));
        let vec3 = pumpkin_util::math::vector3::Vector3::new(
            f64::from(target.0.x),
            f64::from(target.0.y),
            f64::from(target.0.z),
        );
        Ok(to_wasm_position(vec3))
    }

    fn set_respawn_location(
        &mut self,
        player: Resource<Player>,
        pos: crate::pumpkin::plugin::common::Position,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        let pos_vec = from_wasm_position(pos);
        let block_pos = pumpkin_util::math::position::BlockPos::new(
            pos_vec.x as i32,
            pos_vec.y as i32,
            pos_vec.z as i32,
        );
        player.set_respawn_location(block_pos);
        Ok(())
    }

    fn get_respawn_location(
        &mut self,
        player: Resource<Player>,
    ) -> wasmtime::Result<Option<crate::pumpkin::plugin::common::Position>> {
        let player = self.get(&player)?;
        Ok(player.get_respawn_location().map(|p| {
            let vec3 = pumpkin_util::math::vector3::Vector3::new(
                f64::from(p.0.x),
                f64::from(p.0.y),
                f64::from(p.0.z),
            );
            to_wasm_position(vec3)
        }))
    }

    fn hide_player(
        &mut self,
        player: Resource<Player>,
        other: Resource<Player>,
    ) -> wasmtime::Result<()> {
        let other = self.take(other)?;
        let player = self.get(&player)?;
        player.hide_player(other.gameprofile.id);
        Ok(())
    }

    fn show_player(
        &mut self,
        player: Resource<Player>,
        other: Resource<Player>,
    ) -> wasmtime::Result<()> {
        let other = self.take(other)?;
        let player = self.get(&player)?;
        player.show_player(other.gameprofile.id);
        Ok(())
    }

    fn can_see(
        &mut self,
        player: Resource<Player>,
        other: Resource<Player>,
    ) -> wasmtime::Result<bool> {
        let other = self.take(other)?;
        let player = self.get(&player)?;
        Ok(player.can_see(&other.gameprofile.id))
    }

    fn can_see_player(
        &mut self,
        player: Resource<Player>,
        other: Resource<Player>,
    ) -> wasmtime::Result<bool> {
        let other = self.take(other)?;
        let player = self.get(&player)?;
        Ok(player.can_see(&other.gameprofile.id))
    }

    fn set_tab_list_ping(
        &mut self,
        player: Resource<Player>,
        latency_ms: i32,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.set_tab_list_ping(latency_ms);
        Ok(())
    }

    fn set_item_cooldown(
        &mut self,
        player: Resource<Player>,
        item_id: String,
        ticks: i32,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.set_item_cooldown(&item_id, ticks);
        Ok(())
    }

    fn get_item_cooldown(
        &mut self,
        player: Resource<Player>,
        item_id: String,
    ) -> wasmtime::Result<Option<i32>> {
        let player = self.get(&player)?;
        Ok(player.get_item_cooldown(&item_id))
    }

    fn has_item_cooldown(
        &mut self,
        player: Resource<Player>,
        item_id: String,
    ) -> wasmtime::Result<bool> {
        let player = self.get(&player)?;
        Ok(player.has_item_cooldown(&item_id))
    }

    fn ray_trace_block(
        &mut self,
        player: Resource<Player>,
        max_distance: f64,
        include_fluids: bool,
    ) -> wasmtime::Result<Option<pumpkin::plugin::world::RayTraceBlockResult>> {
        let player = self.get(&player)?;
        let start = player.living_entity.entity.get_eye_pos();
        let direction = player.living_entity.entity.get_looking_vector();
        let end = start + direction * max_distance;
        let world = player.living_entity.entity.world.load_full();

        let hit = world.ray_trace_block(start, end, include_fluids);

        Ok(hit.map(
            |(pos, face, hit_pos)| pumpkin::plugin::world::RayTraceBlockResult {
                pos: pumpkin::plugin::world::BlockPos {
                    x: pos.0.x,
                    y: pos.0.y,
                    z: pos.0.z,
                },
                face: crate::world::to_wasm_block_direction(face),
                hit_pos: to_wasm_position(hit_pos),
            },
        ))
    }

    fn ray_trace_entity(
        &mut self,
        player: Resource<Player>,
        max_distance: f64,
    ) -> wasmtime::Result<Option<pumpkin::plugin::world::RayTraceEntityResult>> {
        let player = self.get(&player)?;
        let start = player.living_entity.entity.get_eye_pos();
        let direction = player.living_entity.entity.get_looking_vector();
        let end = start + direction * max_distance;
        let world = player.living_entity.entity.world.load_full();
        let self_id = player.living_entity.entity.entity_id;

        let hits = world.ray_trace_entities(start, end);
        for (hit_entity, hit_pos, distance) in hits {
            if hit_entity.get_entity().entity_id != self_id {
                let entity_res = self
                    .add(hit_entity)
                    .map_err(|_| wasmtime::Error::msg("failed to add entity resource"))?;
                return Ok(Some(pumpkin::plugin::world::RayTraceEntityResult {
                    entity: entity_res,
                    hit_pos: to_wasm_position(hit_pos),
                    distance,
                }));
            }
        }

        Ok(None)
    }

    fn get_target_entity(
        &mut self,
        player: Resource<Player>,
        max_distance: f64,
    ) -> wasmtime::Result<Option<Resource<pumpkin::plugin::world::Entity>>> {
        let res = self.ray_trace_entity(player, max_distance)?;
        Ok(res.map(|r| r.entity))
    }

    fn get_target_block(
        &mut self,
        player: Resource<Player>,
        max_distance: u32,
    ) -> wasmtime::Result<Option<crate::pumpkin::plugin::common::Position>> {
        let player = self.get(&player)?;
        let res = player.get_target_block(
            &player.living_entity.entity.world.load_full(),
            f64::from(max_distance),
        );
        Ok(res.map(|p| {
            let vec3 = pumpkin_util::math::vector3::Vector3::new(
                f64::from(p.0.x),
                f64::from(p.0.y),
                f64::from(p.0.z),
            );
            to_wasm_position(vec3)
        }))
    }

    fn get_target_block_exact(
        &mut self,
        player: Resource<Player>,
        max_distance: f64,
        include_fluids: bool,
    ) -> wasmtime::Result<Option<pumpkin::plugin::world::RayTraceBlockResult>> {
        self.ray_trace_block(player, max_distance, include_fluids)
    }

    fn launch_projectile(
        &mut self,
        _player: Resource<Player>,
        _type_: crate::pumpkin::plugin::player::ProjectileType,
    ) -> wasmtime::Result<Option<Resource<crate::pumpkin::plugin::world::Entity>>> {
        Ok(None)
    }

    fn set_tab_list_header_footer(
        &mut self,
        player: Resource<Player>,
        header: Resource<pumpkin::plugin::text::TextComponent>,
        footer: Resource<pumpkin::plugin::text::TextComponent>,
    ) -> wasmtime::Result<()> {
        let header = self.take(header)?;
        let footer = self.take(footer)?;
        let player = self.get(&player)?;
        player.set_tab_list_header_footer(&header, &footer);
        Ok(())
    }

    fn set_tab_list_order(&mut self, player: Resource<Player>, order: i32) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.set_tab_list_order(order);
        Ok(())
    }

    fn set_tab_list_latency(
        &mut self,
        player: Resource<Player>,
        latency: i32,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.set_tab_list_latency(latency);
        Ok(())
    }

    fn set_tab_list_listed(
        &mut self,
        player: Resource<Player>,
        listed: bool,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.set_tab_list_listed(listed);
        Ok(())
    }

    fn show_title(
        &mut self,
        player: Resource<Player>,
        text: Resource<pumpkin::plugin::text::TextComponent>,
    ) -> wasmtime::Result<()> {
        let component = self.take(text)?;
        let player = self.get(&player)?;
        player.show_title(&component, &TitleMode::Title);
        Ok(())
    }

    fn show_subtitle(
        &mut self,
        player: Resource<Player>,
        text: Resource<pumpkin::plugin::text::TextComponent>,
    ) -> wasmtime::Result<()> {
        let component = self.take(text)?;
        let player = self.get(&player)?;
        player.show_title(&component, &TitleMode::SubTitle);
        Ok(())
    }

    fn show_actionbar(
        &mut self,
        player: Resource<Player>,
        text: Resource<pumpkin::plugin::text::TextComponent>,
    ) -> wasmtime::Result<()> {
        let component = self.take(text)?;
        let player = self.get(&player)?;
        player.show_title(&component, &TitleMode::ActionBar);
        Ok(())
    }

    fn send_title_animation(
        &mut self,
        player: Resource<Player>,
        fade_in: i32,
        stay: i32,
        fade_out: i32,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.send_title_animation(fade_in, stay, fade_out);
        Ok(())
    }

    async fn transfer(
        &mut self,
        player: Resource<Player>,
        host: String,
        port: u16,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        if let pumpkin_core::net::ClientPlatform::Java(client) = player.client.as_ref() {
            client
                .send_packet(&pumpkin_protocol::java::client::play::CTransfer::new(
                    &host,
                    pumpkin_protocol::codec::var_int::VarInt(i32::from(port)),
                ))
                .await;
        }
        Ok(())
    }

    fn get_selected_slot(&mut self, player: Resource<Player>) -> wasmtime::Result<u8> {
        let player = self.get(&player)?;
        Ok(player.inventory.get_selected_slot())
    }

    fn get_health(&mut self, player: Resource<Player>) -> wasmtime::Result<f32> {
        let player = self.get(&player)?;
        Ok(player.living_entity.health.load())
    }

    fn set_health(&mut self, player: Resource<Player>, health: f32) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.set_health(health);
        Ok(())
    }

    fn get_max_health(&mut self, player: Resource<Player>) -> wasmtime::Result<f32> {
        let player = self.get(&player)?;
        Ok(player.living_entity.get_max_health())
    }

    fn set_max_health(
        &mut self,
        player: Resource<Player>,
        max_health: f32,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.set_max_health(max_health);
        Ok(())
    }

    fn get_food_level(&mut self, player: Resource<Player>) -> wasmtime::Result<u8> {
        let player = self.get(&player)?;
        Ok(player.hunger_manager.level.load())
    }

    fn get_saturation(&mut self, player: Resource<Player>) -> wasmtime::Result<f32> {
        let player = self.get(&player)?;
        Ok(player.hunger_manager.saturation.load())
    }

    fn set_saturation(
        &mut self,
        player: Resource<Player>,
        saturation: f32,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.set_saturation(saturation);
        Ok(())
    }

    fn get_exhaustion(&mut self, player: Resource<Player>) -> wasmtime::Result<f32> {
        let player = self.get(&player)?;
        Ok(player.get_exhaustion())
    }

    fn set_exhaustion(
        &mut self,
        player: Resource<Player>,
        exhaustion: f32,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.set_exhaustion(exhaustion);
        Ok(())
    }

    fn get_absorption(&mut self, player: Resource<Player>) -> wasmtime::Result<f32> {
        let player = self.get(&player)?;
        Ok(player.get_absorption())
    }

    fn set_absorption(
        &mut self,
        player: Resource<Player>,
        absorption: f32,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        player.set_absorption(absorption);
        Ok(())
    }

    fn get_experience_level(&mut self, player: Resource<Player>) -> wasmtime::Result<i32> {
        let player = self.get(&player)?;
        Ok(player.experience_level.load(Ordering::Relaxed))
    }

    fn get_experience_progress(&mut self, player: Resource<Player>) -> wasmtime::Result<f32> {
        let player = self.get(&player)?;
        Ok(player.experience_progress.load())
    }

    fn get_experience_points(&mut self, player: Resource<Player>) -> wasmtime::Result<i32> {
        let player = self.get(&player)?;
        Ok(player.experience_points.load(Ordering::Relaxed))
    }

    fn is_flying(&mut self, player: Resource<Player>) -> wasmtime::Result<bool> {
        let player = self.get(&player)?;
        Ok(player.is_flying())
    }

    fn set_flying(&mut self, player: Resource<Player>, flying: bool) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        {
            let mut abilities = player
                .abilities
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            abilities.flying = flying;
        };
        player.send_abilities_update();
        Ok(())
    }

    fn get_abilities(
        &mut self,
        player: Resource<Player>,
    ) -> wasmtime::Result<pumpkin::plugin::player::PlayerAbilities> {
        let player = self.get(&player)?;
        let abilities = player
            .abilities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(pumpkin::plugin::player::PlayerAbilities {
            invulnerable: abilities.invulnerable,
            flying: abilities.flying,
            allow_flying: abilities.allow_flying,
            creative: abilities.creative,
            allow_modify_world: abilities.allow_modify_world,
            fly_speed: abilities.fly_speed,
            walk_speed: abilities.walk_speed,
        })
    }

    fn set_abilities(
        &mut self,
        player: Resource<Player>,
        abilities: pumpkin::plugin::player::PlayerAbilities,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        {
            let mut a = player
                .abilities
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            a.invulnerable = abilities.invulnerable;
            a.flying = abilities.flying;
            a.allow_flying = abilities.allow_flying;
            a.creative = abilities.creative;
            a.allow_modify_world = abilities.allow_modify_world;
            a.fly_speed = abilities.fly_speed;
            a.walk_speed = abilities.walk_speed;
        };
        player.send_abilities_update();
        Ok(())
    }

    fn get_ip(&mut self, player: Resource<Player>) -> wasmtime::Result<String> {
        let player = self.get(&player)?;
        Ok(player.get_ip())
    }

    fn get_skin(&mut self, player: Resource<Player>) -> wasmtime::Result<Option<PlayerSkin>> {
        let player = self.get(&player)?;
        Ok(player
            .gameprofile
            .properties
            .load()
            .iter()
            .find(|p| p.name.as_ref() == "textures")
            .map(|p| PlayerSkin {
                value: p.value.to_string(),
                signature: p.signature.as_ref().map(std::string::ToString::to_string),
            }))
    }

    fn set_skin(&mut self, player: Resource<Player>, skin: PlayerSkin) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        let mut properties = (**player.gameprofile.properties.load()).clone();

        properties.retain(|p| p.name.as_ref() != "textures");
        properties.push(Property {
            name: "textures".into(),
            value: skin.value.into(),
            signature: skin.signature.map(std::convert::Into::into),
        });

        player.gameprofile.properties.store(Arc::new(properties));

        Ok(())
    }

    fn get_skin_parts(&mut self, player: Resource<Player>) -> wasmtime::Result<SkinParts> {
        let player = self.get(&player)?;
        let mask = player.config.load().skin_parts;
        let mut parts = SkinParts::empty();
        if mask & 0x01 != 0 {
            parts |= SkinParts::CAPE;
        }
        if mask & 0x02 != 0 {
            parts |= SkinParts::JACKET;
        }
        if mask & 0x04 != 0 {
            parts |= SkinParts::LEFT_SLEEVE;
        }
        if mask & 0x08 != 0 {
            parts |= SkinParts::RIGHT_SLEEVE;
        }
        if mask & 0x10 != 0 {
            parts |= SkinParts::LEFT_PANTS_LEG;
        }
        if mask & 0x20 != 0 {
            parts |= SkinParts::RIGHT_PANTS_LEG;
        }
        if mask & 0x40 != 0 {
            parts |= SkinParts::HAT;
        }
        Ok(parts)
    }

    fn set_skin_parts(
        &mut self,
        player: Resource<Player>,
        parts: SkinParts,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        let mut mask = 0u8;
        if parts.contains(SkinParts::CAPE) {
            mask |= 0x01;
        }
        if parts.contains(SkinParts::JACKET) {
            mask |= 0x02;
        }
        if parts.contains(SkinParts::LEFT_SLEEVE) {
            mask |= 0x04;
        }
        if parts.contains(SkinParts::RIGHT_SLEEVE) {
            mask |= 0x08;
        }
        if parts.contains(SkinParts::LEFT_PANTS_LEG) {
            mask |= 0x10;
        }
        if parts.contains(SkinParts::RIGHT_PANTS_LEG) {
            mask |= 0x20;
        }
        if parts.contains(SkinParts::HAT) {
            mask |= 0x40;
        }

        {
            let mut config = (**player.config.load()).clone();
            config.skin_parts = mask;
            player.config.store(Arc::new(config));
        };
        player.send_client_information();
        Ok(())
    }

    fn get_advancement_progress(
        &mut self,
        player: Resource<Player>,
        advancement_id: String,
    ) -> wasmtime::Result<Option<pumpkin::plugin::advancement::AdvancementProgress>> {
        let player = self.get(&player)?;
        let Some(advancement) = crate::advancement::find_advancement(&advancement_id) else {
            return Ok(None);
        };
        let guard = player
            .advancements
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let progress = guard.progress.map.get(advancement).map_or_else(
            || pumpkin::plugin::advancement::AdvancementProgress {
                advancement_id: advancement.id.to_string(),
                done: false,
                awarded_criteria: Vec::new(),
                remaining_criteria: advancement
                    .criteria
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            },
            |progress| pumpkin::plugin::advancement::AdvancementProgress {
                advancement_id: advancement.id.to_string(),
                done: progress.is_done(),
                awarded_criteria: progress
                    .get_completed_criteria()
                    .map(|s| s.to_string())
                    .collect(),
                remaining_criteria: progress
                    .get_remaining_criteria()
                    .map(|s| s.to_string())
                    .collect(),
            },
        );
        Ok(Some(progress))
    }

    fn revoke_advancement_criterion(
        &mut self,
        player: Resource<Player>,
        advancement_id: String,
        criterion: String,
    ) -> wasmtime::Result<bool> {
        let player = self.get(&player)?;
        let Some(advancement) = crate::advancement::find_advancement(&advancement_id) else {
            return Ok(false);
        };
        let mut guard = player
            .advancements
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let revoked = guard.revoke(advancement, &criterion);
        if revoked {
            guard.flush_dirty(player, true);
        }
        Ok(revoked)
    }

    fn revoke_advancement(
        &mut self,
        player: Resource<Player>,
        advancement_id: String,
    ) -> wasmtime::Result<bool> {
        let player = self.get(&player)?;
        let Some(advancement) = crate::advancement::find_advancement(&advancement_id) else {
            return Ok(false);
        };
        let mut guard = player
            .advancements
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let progress = guard.progress.get_mut_or_start_progress(advancement);
        if !progress.has_progress() {
            return Ok(false);
        }
        let completed: Vec<Arc<str>> = progress.get_completed_criteria().collect();
        let mut any_revoked = false;
        for criterion in completed {
            if guard.revoke(advancement, &criterion) {
                any_revoked = true;
            }
        }
        if any_revoked {
            guard.flush_dirty(player, true);
        }
        Ok(any_revoked)
    }

    fn has_advancement(
        &mut self,
        player: Resource<Player>,
        advancement_id: String,
    ) -> wasmtime::Result<bool> {
        let player = self.get(&player)?;
        let Some(advancement) = crate::advancement::find_advancement(&advancement_id) else {
            return Ok(false);
        };
        let guard = player
            .advancements
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let done =
            guard.progress.map.get(advancement).is_some_and(
                pumpkin_core::entity::player::advancement::AdvancementProgress::is_done,
            );
        Ok(done)
    }

    fn get_completed_advancements(
        &mut self,
        player: Resource<Player>,
    ) -> wasmtime::Result<Vec<String>> {
        let player = self.get(&player)?;
        let guard = player
            .advancements
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let list = guard
            .progress
            .map
            .iter()
            .filter(|(_, p)| p.is_done())
            .map(|(adv, _)| adv.id.to_string())
            .collect();
        Ok(list)
    }

    fn get_selected_advancement_tab(
        &mut self,
        player: Resource<Player>,
    ) -> wasmtime::Result<Option<String>> {
        let player = self.get(&player)?;
        let guard = player
            .advancements
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(guard.last_selected_tab.map(|adv| adv.id.to_string()))
    }

    fn set_selected_advancement_tab(
        &mut self,
        player: Resource<Player>,
        tab_id: Option<String>,
    ) -> wasmtime::Result<()> {
        let player = self.get(&player)?;
        let target_adv = tab_id
            .as_deref()
            .and_then(crate::advancement::find_advancement);
        let mut guard = player
            .advancements
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.set_selected_tab(target_adv);
        Ok(())
    }

    fn as_java(
        &mut self,
        player: Resource<Player>,
    ) -> wasmtime::Result<Option<Resource<pumpkin::plugin::player::JavaPlayer>>> {
        let player = self.get(&player)?.clone();
        if let pumpkin_core::net::ClientPlatform::Java(_) = player.client.as_ref() {
            Ok(Some(self.add(player)?))
        } else {
            Ok(None)
        }
    }

    fn as_bedrock(
        &mut self,
        player: Resource<Player>,
    ) -> wasmtime::Result<Option<Resource<pumpkin::plugin::player::BedrockPlayer>>> {
        let player = self.get(&player)?.clone();
        if let pumpkin_core::net::ClientPlatform::Bedrock(_) = player.client.as_ref() {
            Ok(Some(self.add(player)?))
        } else {
            Ok(None)
        }
    }

    fn drop(&mut self, rep: Resource<Player>) -> wasmtime::Result<()> {
        self.drop(rep)
    }
}
