//! Held items, durability, drops and projectiles.

use super::super::item::ItemEntity;
use super::super::{Entity, EntityBase};
use super::{Player, statistics};
use crate::net::ClientPlatform;
use crate::server::Server;
use pumpkin_data::BlockState;
use pumpkin_data::data_component_impl::{EquipmentSlot, ToolImpl};
use pumpkin_data::entity::EntityType;
use pumpkin_data::item_stack::{DamageResult, ItemStack};
use pumpkin_inventory::Inventory;
use pumpkin_inventory::player::ender_chest_inventory::EnderChestInventory;
use pumpkin_inventory::player::player_inventory::PlayerInventory;
use pumpkin_inventory::screen_handler::ScreenHandler;
use pumpkin_protocol::bedrock::server::actor_event::ActorEventID;
use pumpkin_protocol::codec::item_stack_seralizer::ItemStackSerializer;
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::java::client::play::{CMapItemData, CSetPlayerInventory, MapIcon, MapPatch};
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::text::TextComponent;
use pumpkin_util::{GameMode, Hand};
use std::f64::consts::TAU;
use std::sync::Arc;
use std::sync::atomic::Ordering;

impl Player {
    pub const fn inventory(&self) -> &Arc<PlayerInventory> {
        &self.inventory
    }

    pub const fn ender_chest_inventory(&self) -> &Arc<EnderChestInventory> {
        &self.ender_chest_inventory
    }

    /// Opens the player's ender chest screen.
    pub fn open_ender_chest(self: &Arc<Self>) -> Option<u8> {
        self.increment_stat(
            pumpkin_data::statistic::StatisticCategory::Custom,
            pumpkin_data::statistic::CustomStatistic::OpenEnderchest as i32,
            1,
        );
        let inventory = self.ender_chest_inventory();
        self.open_handled_screen(
            &crate::block::blocks::ender_chest::EnderChestScreenFactory {
                inventory: inventory.clone(),
                tracker: None,
            },
            None,
        )
    }

    /// Pushes current inventory contents to the client via `CONTAINER_SET_SLOT`.
    ///
    /// `minecraft:set_player_inventory` is missing on 1.21.0/1.21.1 and is not
    /// applied to the local hotbar on later 1.21.x clients. Screen-handler slot
    /// updates are the vanilla path and (should) work on every supported version.
    pub fn sync_inventory_to_client(&self) {
        if let Ok(mut handler) = self.player_screen_handler.try_lock() {
            handler.send_content_updates();
        }

        let Ok(current_guard) = self.current_screen_handler.try_lock() else {
            return;
        };
        let current = current_guard.clone();
        drop(current_guard);

        let player_screen_ptr = Arc::as_ptr(&self.player_screen_handler).cast::<()>();
        let current_ptr = Arc::as_ptr(&current).cast::<()>();
        if player_screen_ptr == current_ptr {
            return;
        }

        if let Ok(mut handler) = current.try_lock() {
            handler.send_content_updates();
        }
    }

    pub fn try_send_slot_set_packet(&self, packet: &CSetPlayerInventory) {
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

                let item_stack = &*packet.item.0;
                let item_desc = NetworkItemStackDescriptor::from(item_stack);
                let bedrock_packet = CInventorySlot {
                    container_id: VarUInt(0),
                    slot: VarUInt(packet.slot.0 as u32),
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
        }
    }

    pub fn sync_hand_slot(&self, slot_index: usize, stack: ItemStack) {
        self.try_send_slot_set_packet(&CSetPlayerInventory::new(
            (slot_index as i32).into(),
            &ItemStackSerializer::from(stack.clone()),
        ));
        self.sync_inventory_to_client();

        if slot_index == self.inventory.get_selected_slot() as usize {
            self.living_entity
                .send_equipment_changes(&[(EquipmentSlot::MAIN_HAND, stack)]);
        } else if slot_index == PlayerInventory::OFF_HAND_SLOT {
            self.living_entity
                .send_equipment_changes(&[(EquipmentSlot::OFF_HAND, stack)]);
        }
    }

    /// Damages a stack through the player's item events; the caller handles inventory and break updates.
    pub fn damage_item_stack(&self, stack: &mut ItemStack, amount: i32) -> DamageResult {
        if matches!(
            self.gamemode.load(),
            GameMode::Creative | GameMode::Spectator
        ) || amount <= 0
            || !stack.is_damageable()
            || stack.is_unbreakable()
        {
            return DamageResult::Untouched;
        }

        let original_item = stack.item;
        let world = self.world();
        let player_context = world
            .server
            .upgrade()
            .zip(world.get_player_by_uuid(self.gameprofile.id));
        let amount = if let Some((server, player)) = &player_context {
            let mut event =
                crate::plugin::api::events::player::player_item_damage::PlayerItemDamageEvent::new(
                    player.clone(),
                    original_item.registry_key.to_string(),
                    amount,
                );
            server.plugin_manager.fire_blocking(server, &mut event);
            if event.cancelled {
                return DamageResult::Untouched;
            }
            event.damage
        } else {
            amount
        };

        let result = stack.damage_item(amount);
        if result != DamageResult::Untouched {
            let vehicle_type = self
                .living_entity
                .entity
                .get_vehicle()
                .map(|v| format!("minecraft:{}", v.get_entity().entity_type.resource_name));
            self.trigger_advancement(
                crate::entity::player::advancement::trigger::AdvancementTrigger::ItemDurabilityChanged {
                    item_id: original_item.registry_key.to_string(),
                    vehicle_type,
                },
            );
        }
        if result == DamageResult::Broken
            && let Some((server, player)) = player_context
        {
            let mut event =
                crate::plugin::api::events::player::player_item_break::PlayerItemBreakEvent::new(
                    player,
                    original_item.registry_key.to_string(),
                );
            server.plugin_manager.fire_blocking(&server, &mut event);
        }
        result
    }

    /// Applies `amount` durability damage to the item in `slot`.
    /// Broadcasts an [`EntityStatus`] break event and syncs the slot if the item is destroyed.
    pub fn damage_item_in_slot(&self, slot: &EquipmentSlot, amount: i32) -> bool {
        // Direct PlayerInventory slot indices (matches build_equipment_slots).
        let slot_index: usize = match slot {
            EquipmentSlot::MainHand(_) => self.inventory.get_selected_slot() as usize,
            EquipmentSlot::OffHand(_) => PlayerInventory::OFF_HAND_SLOT, // 40
            EquipmentSlot::Feet(_) => 36,
            EquipmentSlot::Legs(_) => 37,
            EquipmentSlot::Chest(_) => 38,
            EquipmentSlot::Head(_) => 39,
            // Players do not have Body or Saddle equipment slots;
            // these are only used by non-player entities (e.g. horses).
            EquipmentSlot::Body(_) | EquipmentSlot::Saddle(_) => return false,
        };

        let mut stack = self.inventory.get_slot(slot_index);
        let original_item = stack.item;
        let break_sound = stack
            .get_data_component::<pumpkin_data::data_component_impl::BreakSoundImpl>()
            .and_then(|bs| match bs.sound {
                pumpkin_data::data_component_impl::IdOr::Id(s) => Some(s),
                pumpkin_data::data_component_impl::IdOr::Value(_) => None,
            });
        let result = self.damage_item_stack(&mut stack, amount);
        let updated = (result != DamageResult::Untouched).then_some((result, stack.clone()));

        if let Some((result, updated_stack)) = updated {
            self.inventory.set_slot(slot_index, updated_stack.clone());
            if result == DamageResult::Broken {
                self.increment_stat(
                    statistics::StatisticCategory::Broken,
                    original_item.id as i32,
                    1,
                );
                if let Some(sound) = break_sound {
                    self.world().play_sound(
                        sound,
                        pumpkin_data::sound::SoundCategory::Players,
                        &self.living_entity.entity.pos.load(),
                    );
                }
                self.world().send_entity_status(
                    &self.living_entity.entity,
                    super::super::equipment_break_status(slot),
                    ActorEventID::None,
                );
            }

            self.try_send_slot_set_packet(&CSetPlayerInventory::new(
                (slot_index as i32).into(),
                &ItemStackSerializer::from(updated_stack.clone()),
            ));
            self.sync_inventory_to_client();

            self.living_entity
                .send_equipment_changes(&[(slot.clone(), updated_stack)]);

            return true;
        }

        false
    }

    /// Checks and triggers location-based enchantments (e.g. Frost Walker) on the player's equipped armor.
    pub fn check_location_enchantments(&self, pos: Vector3<f64>, on_ground: bool) {
        if on_ground {
            let boots = self.inventory.get_slot(36);
            if !boots.is_empty() {
                crate::enchantment::EnchantmentHelper::on_location_changed(
                    self.get_entity(),
                    &boots,
                    pos,
                );
            }
        }
    }

    /// Convenience wrapper – damages the currently held (main-hand) item.
    pub fn damage_held_item(&self, amount: i32) -> bool {
        self.damage_item_in_slot(&EquipmentSlot::MAIN_HAND, amount)
    }

    pub fn apply_tool_damage_for_block_break(&self, state: &BlockState) {
        if matches!(
            self.gamemode.load(),
            GameMode::Creative | GameMode::Spectator
        ) {
            return;
        }

        if state.hardness <= 0.0 {
            return;
        }

        let damage = self
            .inventory()
            .held_item()
            .get_data_component::<ToolImpl>()
            .map_or(0, |tool| tool.damage_per_block as i32);

        if damage > 0 {
            self.damage_held_item(damage);
        }
    }

    pub fn drop_item(&self, item_stack: ItemStack) {
        self.increment_stat(
            statistics::StatisticCategory::Dropped,
            item_stack.item.id as i32,
            item_stack.item_count as i32,
        );
        self.increment_custom_stat(
            statistics::CustomStatistic::Drop,
            item_stack.item_count as i32,
        );
        let item_pos = self.living_entity.entity.pos.load()
            + Vector3::new(0.0, self.living_entity.entity.get_eye_height() - 0.3, 0.0);
        let entity = Entity::new(self.world(), item_pos, &EntityType::ITEM);

        let pitch = f64::from(self.living_entity.entity.pitch.load()).to_radians();
        let yaw = f64::from(self.living_entity.entity.yaw.load()).to_radians();
        let pitch_sin = pitch.sin();
        let pitch_cos = pitch.cos();
        let yaw_sin = yaw.sin();
        let yaw_cos = yaw.cos();
        let horizontal_offset = rand::random::<f64>() * TAU;
        let l = 0.02 * rand::random::<f64>();

        let velocity = Vector3::new(
            (-yaw_sin * pitch_cos).mul_add(0.3, horizontal_offset.cos() * l),
            (rand::random::<f64>() - rand::random::<f64>())
                .mul_add(0.1, (-pitch_sin).mul_add(0.3, 0.1)),
            (yaw_cos * pitch_cos).mul_add(0.3, horizontal_offset.sin() * l),
        );

        // TODO: Merge stacks together
        let item_entity = Arc::new(ItemEntity::new_with_velocity(
            entity, item_stack, velocity, 40,
        ));
        self.world().spawn_entity(item_entity);
    }

    pub fn drop_held_item(&self, drop_stack: bool) {
        let mut item_stack = self.inventory().held_item();

        if item_stack.is_empty() {
            return;
        }

        let drop_amount = if drop_stack { item_stack.item_count } else { 1 };
        let dropped_stack = item_stack.copy_with_count(drop_amount);

        if let Some(server) = self.world().server.upgrade()
            && let Some(player_arc) = self.world().get_player_by_uuid(self.gameprofile.id)
        {
            let mut event =
                crate::plugin::api::events::player::player_drop_item::PlayerDropItemEvent::new(
                    player_arc,
                    dropped_stack.item.registry_key.to_string(),
                    dropped_stack.item_count,
                );
            server.plugin_manager.fire_blocking(&server, &mut event);
            if event.cancelled {
                return;
            }
        }

        item_stack.decrement(drop_amount);
        let updated_stack = item_stack.clone();
        self.inventory().set_held_item(updated_stack.clone());

        self.drop_item(dropped_stack);

        let inv: Arc<dyn Inventory> = self.inventory.clone();
        let screen_binding = self
            .current_screen_handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut screen_handler = screen_binding
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let selected_slot = self.inventory.get_selected_slot();
        if let Some(slot_index) = screen_handler.get_slot_index(&inv, selected_slot as usize) {
            screen_handler.set_received_stack(slot_index, updated_stack);
            screen_handler.send_content_updates();
        }
    }

    pub fn swap_item(&self) {
        if let Some(server) = self.world().server.upgrade()
            && let Some(player_arc) = self.world().get_player_by_uuid(self.gameprofile.id)
        {
            let mut event = crate::plugin::api::events::player::player_swap_hands::PlayerSwapHandItemsEvent::new(player_arc);
            server.plugin_manager.fire_blocking(&server, &mut event);
            if event.cancelled {
                return;
            }
        }
        let (main_hand_item, off_hand_item) = self.inventory.swap_item();
        let equipment = &[
            (EquipmentSlot::MAIN_HAND, main_hand_item),
            (EquipmentSlot::OFF_HAND, off_hand_item),
        ];
        self.living_entity.send_equipment_changes(equipment);
        // todo this.player.stopUsingItem();
    }

    pub fn tick_maps(&self, server: &Server) {
        use pumpkin_data::data_component_impl::MapIdImpl;
        use pumpkin_data::item::Item;

        for hand in Hand::all() {
            let stack = self.inventory().get_stack_in_hand(hand);

            if stack.item.id == Item::FILLED_MAP.id
                && let Some(map_id_comp) = stack.get_data_component::<MapIdImpl>()
            {
                let map_id = map_id_comp.id;
                if let Some(map_data_arc) = server.map_manager.get_map(map_id)
                    && let Ok(mut map_data) = map_data_arc.try_lock()
                {
                    map_data.update(self);

                    let tick_count = self.tick_counter.load(Ordering::Relaxed);
                    if map_data.dirty || tick_count % 10 == 0 {
                        let scale = 1 << map_data.scale;
                        let pos = self.position();
                        let dx = pos.x - map_data.center_x as f64;
                        let dz = pos.z - map_data.center_z as f64;

                        let raw_x = dx / scale as f64 * 2.0;
                        let raw_z = dz / scale as f64 * 2.0;
                        let is_off_map = !(-127.0..=127.0).contains(&raw_x)
                            || !(-127.0..=127.0).contains(&raw_z);

                        let icon_x = raw_x.clamp(-128.0, 127.0) as i8;
                        let icon_z = raw_z.clamp(-128.0, 127.0) as i8;

                        let yaw = self.living_entity.entity.yaw.load();
                        let icon_direction =
                            ((((yaw * 16.0 / 360.0).round() as i32 + 8) % 16 + 16) % 16) as i8;

                        let decoration_type = if is_off_map {
                            &pumpkin_data::map_decoration::MapDecorationType::PLAYER_OFF_MAP
                        } else {
                            &pumpkin_data::map_decoration::MapDecorationType::PLAYER
                        };

                        let mut icons = vec![MapIcon {
                            icon_type: VarInt(decoration_type.id as i32),
                            x: icon_x,
                            z: icon_z,
                            direction: icon_direction,
                            display_name: None,
                        }];
                        icons.extend(map_data.decorations.iter().map(|decoration| {
                            MapIcon {
                                icon_type: VarInt(decoration.icon_type),
                                x: decoration.x,
                                z: decoration.z,
                                direction: decoration.direction,
                                display_name: decoration
                                    .display_name
                                    .as_ref()
                                    .map(|name| TextComponent::text(name.clone())),
                            }
                        }));

                        let data = map_data.dirty.then(|| MapPatch {
                            columns: 128,
                            rows: 128,
                            x: 0,
                            z: 0,
                            data: &*map_data.colors,
                        });

                        self.try_send_client_packet(&CMapItemData {
                            map_id: VarInt(map_id),
                            scale: map_data.scale,
                            tracking_position: true,
                            locked: map_data.locked,
                            icons: Some(&icons),
                            data,
                        });
                        map_data.dirty = false;
                    }
                }
            }
        }
    }

    /// Start using an item (e.g. drawing a bow)
    pub fn start_using_item(&self, hand: Hand) {
        self.using_item.store(true, Ordering::Relaxed);
        self.item_use_start_time
            .store(self.tick_counter.load(Ordering::Relaxed), Ordering::Relaxed);
        self.using_hand.store(Some(hand));
    }

    /// Stop using an item
    pub fn stop_using_item(&self) {
        self.using_item.store(false, Ordering::Relaxed);
        self.using_hand.store(None);
    }

    /// Get the number of ticks the item has been in use
    pub fn get_item_use_ticks(&self) -> i32 {
        if !self.using_item.load(Ordering::Relaxed) {
            return 0;
        }
        self.tick_counter.load(Ordering::Relaxed) - self.item_use_start_time.load(Ordering::Relaxed)
    }

    /// Find arrow in inventory (main hand, offhand, or inventory slots)
    pub fn find_arrow(&self) -> Option<usize> {
        let inventory = &self.inventory;

        // Check offhand first
        let stack = inventory.get_slot(PlayerInventory::OFF_HAND_SLOT);
        if crate::item::items::projectile_weapon::ProjectileWeaponItem::is_arrow(stack.item)
            && stack.item_count > 0
        {
            return Some(PlayerInventory::OFF_HAND_SLOT);
        }

        // Check hotbar and main inventory
        for slot in 0..PlayerInventory::MAIN_SIZE {
            let stack = inventory.get_slot(slot);
            if crate::item::items::projectile_weapon::ProjectileWeaponItem::is_arrow(stack.item)
                && stack.item_count > 0
            {
                return Some(slot);
            }
        }

        None
    }

    /// Find crossbow projectile in inventory (offhand/mainhand arrow or firework rocket, then inventory arrows)
    pub fn find_crossbow_projectile(&self) -> Option<usize> {
        let inventory = &self.inventory;

        let off_hand = inventory.get_slot(PlayerInventory::OFF_HAND_SLOT);
        if crate::item::items::projectile_weapon::ProjectileWeaponItem::is_arrow_or_firework(
            off_hand.item,
        ) && off_hand.item_count > 0
        {
            return Some(PlayerInventory::OFF_HAND_SLOT);
        }

        let selected = inventory.get_selected_slot() as usize;
        let main_hand = inventory.get_slot(selected);
        if crate::item::items::projectile_weapon::ProjectileWeaponItem::is_arrow_or_firework(
            main_hand.item,
        ) && main_hand.item_count > 0
        {
            return Some(selected);
        }

        self.find_arrow()
    }

    /// Consume one arrow from the specified slot
    pub fn consume_arrow(&self, slot: usize) -> bool {
        let gamemode = self.gamemode.load();
        if gamemode == GameMode::Creative {
            return true; // Don't consume in creative
        }

        let inventory = &self.inventory;
        let mut stack = inventory.get_slot(slot);
        match stack.item_count {
            2.. => {
                stack.item_count -= 1;
                inventory.set_slot(slot, stack);
                true
            }
            1 => {
                inventory.set_slot(slot, ItemStack::EMPTY.clone());
                true
            }
            _ => false,
        }
    }

    pub fn has_item_in_inventory(&self, item: &pumpkin_data::item::Item) -> bool {
        let main_inv = self
            .inventory
            .main_inventory
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for stack in main_inv.iter() {
            if !stack.is_empty() && stack.item.id == item.id {
                return true;
            }
        }
        let equipment = self
            .inventory
            .entity_equipment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for stack in equipment.equipment.values() {
            if !stack.is_empty() && stack.item.id == item.id {
                return true;
            }
        }
        false
    }
}
