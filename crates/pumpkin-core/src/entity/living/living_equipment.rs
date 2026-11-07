//! Equipment, held items and the attribute changes they cause.

use super::super::{Entity, EntityBase};
use super::{
    LivingEntity, attribute_modifier_slot_matches, attributes_by_id, prevents_equipment_drop,
    push_unique_attribute,
};
use crate::entity::attributes::AttributeInstance;
use crate::entity::attributes::Modifier;
use crate::entity::attributes::ModifierOperation;
use crate::entity::mob::equipment::DEFAULT_EQUIPMENT_DROP_CHANCE;
use pumpkin_data::attributes::Attributes;
use pumpkin_data::data_component_impl::Operation;
use pumpkin_data::data_component_impl::{
    AttributeModifiersImpl, BlocksAttacksImpl, EquipmentSlot, EquippableImpl,
};
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::tracked_data;
use pumpkin_inventory::player::player_inventory::PlayerInventory;
use pumpkin_inventory::screen_handler::InventoryPlayer;
use pumpkin_protocol::bedrock::client::take_item_actor::CTakeItemActor;
use pumpkin_protocol::bedrock::server::actor_event::ActorEventID;
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::codec::var_ulong::VarULong;
use pumpkin_protocol::java::client::play::{CSetPlayerInventory, CTakeItemEntity};
use pumpkin_protocol::{
    codec::item_stack_seralizer::ItemStackSerializer, java::client::play::CSetEquipment,
};
use pumpkin_util::Hand;
use pumpkin_util::math::position::BlockPos;
use rand::RngExt;
use std::sync::atomic::Ordering;

impl LivingEntity {
    /// Triggers location-based enchantment effects (e.g. Frost Walker) when the entity's block position changes.
    pub fn on_changed_block(&self, caller: &dyn EntityBase, _pos: BlockPos) {
        let pos_f64 = self.entity.pos.load();
        if let Some(player) = caller.get_player() {
            let boots = player.inventory.get_slot(36);
            if !boots.is_empty() {
                crate::enchantment::EnchantmentHelper::on_location_changed(
                    &self.entity,
                    &boots,
                    pos_f64,
                );
            }
        } else {
            let boots = self
                .entity_equipment
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(&EquipmentSlot::FEET);
            if !boots.is_empty() {
                crate::enchantment::EnchantmentHelper::on_location_changed(
                    &self.entity,
                    &boots,
                    pos_f64,
                );
            }
        }
    }

    pub fn send_equipment_changes(&self, equipment: &[(EquipmentSlot, ItemStack)]) {
        if equipment.is_empty() {
            return;
        }
        self.apply_and_send_equipment_attribute_modifiers(equipment);

        if equipment
            .iter()
            .any(|(slot, _)| *slot == EquipmentSlot::FEET)
        {
            let pos_f64 = self.entity.pos.load();
            for (slot, stack) in equipment {
                if *slot == EquipmentSlot::FEET && !stack.is_empty() {
                    crate::enchantment::EnchantmentHelper::on_location_changed(
                        &self.entity,
                        stack,
                        pos_f64,
                    );
                }
            }
        }

        let equipment_java: Vec<(i8, ItemStackSerializer)> = equipment
            .iter()
            .map(|(slot, stack)| {
                (
                    slot.discriminant(),
                    ItemStackSerializer::from(stack.clone()),
                )
            })
            .collect();
        let je_packet = CSetEquipment::new(self.entity_id().into(), equipment_java);

        let mut sent_editioned = false;
        for (slot, stack) in equipment {
            if *slot == EquipmentSlot::MAIN_HAND {
                self.update_weapon_attributes(stack);
            }
            if *slot == EquipmentSlot::MAIN_HAND || *slot == EquipmentSlot::OFF_HAND {
                let window_id = if *slot == EquipmentSlot::OFF_HAND {
                    120
                } else {
                    0
                };

                let be_packet = pumpkin_protocol::bedrock::client::CMobEquipment {
                    target_runtime_id: (self.entity_id() as u64).into(),
                    item: pumpkin_protocol::bedrock::network_item::NetworkItemStackDescriptor::from(
                        stack,
                    ),
                    slot: 0,
                    selected_slot: 0,
                    container_id: window_id,
                };
                self.entity.world.load().send_to_tracking_players_editioned(
                    &self.entity,
                    &je_packet,
                    &be_packet,
                );
                sent_editioned = true;
            }
        }

        if !sent_editioned {
            self.entity
                .world
                .load()
                .send_to_tracking_players(&self.entity, &je_packet);
        }
    }

    /// Applies the held item's attack attribute modifiers to this entity's
    /// attribute map and sends the changed attributes to clients. Without this
    /// the client never sees the reduced attack speed and does not show the
    /// crosshair attack indicator.
    pub(super) fn update_weapon_attributes(&self, stack: &ItemStack) {
        let component = stack.get_data_component::<AttributeModifiersImpl>();

        // Single pass over the item's modifiers, split by attribute.
        let mut speed_modifiers: Vec<Modifier> = Vec::new();
        let mut damage_modifiers: Vec<Modifier> = Vec::new();
        for modifier in component
            .into_iter()
            .flat_map(|c| c.attribute_modifiers.iter())
        {
            let target = if modifier.r#type == &Attributes::ATTACK_SPEED {
                &mut speed_modifiers
            } else if modifier.r#type == &Attributes::ATTACK_DAMAGE {
                &mut damage_modifiers
            } else {
                continue;
            };
            target.push(Modifier {
                id: modifier.id.to_string(),
                amount: modifier.amount,
                operation: match modifier.operation {
                    Operation::AddValue => ModifierOperation::Add,
                    Operation::AddMultipliedBase => ModifierOperation::MultiplyBase,
                    Operation::AddMultipliedTotal => ModifierOperation::MultiplyTotal,
                },
            });
        }

        let mut changed: Vec<Attributes> = Vec::new();
        {
            let mut attributes = self
                .attributes
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for (attribute, modifiers) in [
                (Attributes::ATTACK_SPEED, speed_modifiers),
                (Attributes::ATTACK_DAMAGE, damage_modifiers),
            ] {
                let instance = attributes
                    .entry(attribute.id)
                    .or_insert_with(|| AttributeInstance::new(attribute.default_value));
                if instance.modifiers == modifiers {
                    continue;
                }
                instance.modifiers = modifiers;
                instance.mark_dirty();
                changed.push(attribute);
            }
        }
        if !changed.is_empty() {
            crate::entity::attributes::send_attribute_updates_for_living(self, changed);
        }
    }

    /// Applies item `attribute_modifiers` for the given slots and notifies clients.
    ///
    /// The local HUD armor bar is driven by `minecraft:armor` / `minecraft:armor_toughness`
    /// on `UPDATE_ATTRIBUTES`, not by `SET_EQUIPMENT`.
    pub fn apply_and_send_equipment_attribute_modifiers(
        &self,
        equipment: &[(EquipmentSlot, ItemStack)],
    ) {
        let mut touched = Vec::new();
        for (slot, stack) in equipment {
            self.apply_equipment_slot_attribute_modifiers(slot, stack, &mut touched);
        }
        if !touched.is_empty() {
            crate::entity::attributes::send_attribute_updates_for_living(self, touched);
        }
    }

    /// Re-applies modifiers from every currently equipped stack without notifying clients.
    pub fn apply_current_equipment_attribute_modifiers(&self) {
        let equipment = self.snapshot_equipped_stacks();
        let mut touched = Vec::new();
        for (slot, stack) in &equipment {
            self.apply_equipment_slot_attribute_modifiers(slot, stack, &mut touched);
        }
    }

    /// Re-applies modifiers from every currently equipped stack and sends updates.
    pub fn send_current_equipment_attribute_modifiers(&self) {
        self.apply_and_send_equipment_attribute_modifiers(&self.snapshot_equipped_stacks());
    }

    pub(super) fn snapshot_equipped_stacks(&self) -> Vec<(EquipmentSlot, ItemStack)> {
        let guard = self
            .entity_equipment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard
            .equipment
            .iter()
            .map(|(slot, stack)| (slot.clone(), stack.clone()))
            .collect()
    }

    pub(super) fn apply_equipment_slot_attribute_modifiers(
        &self,
        slot: &EquipmentSlot,
        stack: &ItemStack,
        touched: &mut Vec<Attributes>,
    ) {
        let previous = {
            let mut map = self
                .equipment_attribute_modifier_ids
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            map.remove(slot).unwrap_or_default()
        };
        for (attr_id, modifier_id) in previous {
            if let Some(attr) = attributes_by_id(attr_id) {
                self.update_attribute(attr, |inst| inst.remove_modifier(&modifier_id));
                push_unique_attribute(touched, attr);
            }
        }

        if stack.is_empty() {
            return;
        }
        let Some(modifiers) = stack.get_data_component::<AttributeModifiersImpl>() else {
            return;
        };

        let mut applied = Vec::new();
        for item_mod in modifiers.attribute_modifiers.iter() {
            if !attribute_modifier_slot_matches(&item_mod.slot, slot) {
                continue;
            }
            let operation = match item_mod.operation {
                Operation::AddValue => ModifierOperation::Add,
                Operation::AddMultipliedBase => ModifierOperation::MultiplyBase,
                Operation::AddMultipliedTotal => ModifierOperation::MultiplyTotal,
            };
            self.update_attribute(item_mod.r#type, |inst| {
                inst.add_or_replace_modifier(Modifier {
                    id: item_mod.id.to_string(),
                    amount: item_mod.amount,
                    operation,
                });
            });
            applied.push((item_mod.r#type.id, item_mod.id.to_string()));
            push_unique_attribute(touched, item_mod.r#type);
        }

        if !applied.is_empty() {
            let mut map = self
                .equipment_attribute_modifier_ids
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            map.insert(slot.clone(), applied);
        }
    }

    /// Picks up an Item entity or XP Orb
    pub fn pickup(&self, item: &Entity, stack_amount: u32) -> bool {
        let mut pickup_event =
            crate::plugin::api::events::entity::entity_pickup_item::EntityPickupItemEvent::new(
                self.entity.entity_id,
                item.entity_type.id.to_string(),
                stack_amount as u8,
            );
        if let Some(server) = self.entity.world.load().server.upgrade() {
            server
                .plugin_manager
                .fire_blocking(&server, &mut pickup_event);
            if pickup_event.cancelled {
                return false;
            }
        }

        let chunk_pos = self.entity.chunk_pos.load();
        self.entity.world.load().broadcast_to_chunk_editioned(
            chunk_pos,
            &CTakeItemEntity::new(
                item.entity_id.into(),
                self.entity.entity_id.into(),
                VarInt(stack_amount as i32),
            ),
            &CTakeItemActor {
                item_runtime_id: VarULong(item.entity_id as u64),
                actor_runtime_id: VarULong(self.entity.entity_id as u64),
            },
        );
        true
    }

    /// Sends the Hand animation to all others, used when Eating for example
    pub fn set_active_hand(&self, hand: Hand, stack: ItemStack, duration: i32) {
        let is_blocking = stack.get_data_component::<BlocksAttacksImpl>().is_some();
        self.item_use_time.store(duration, Ordering::Relaxed);
        *self
            .item_in_use
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(stack);
        *self
            .active_hand
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(hand);
        self.recent_kinetic_enemies
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.set_living_flag(Self::USING_ITEM_FLAG, true);
        self.set_living_flag(Self::OFF_HAND_ACTIVE_FLAG, hand == Hand::Left);
        if is_blocking {
            self.set_bedrock_blocking(true);
        }
    }

    pub(super) fn set_living_flag(&self, flag: u8, value: bool) {
        let index = flag;
        let mut b = self.livings_flags.load(Ordering::Relaxed);
        if value {
            b |= index;
        } else {
            b &= !index;
        }
        self.livings_flags.store(b, Ordering::Relaxed);

        let bedrock_meta = (flag == Self::USING_ITEM_FLAG).then(|| {
            let index =
                pumpkin_protocol::bedrock::client::set_actor_data::entity_data_flag::USING_ITEM;
            let mask = 1i64 << index;
            if value {
                self.entity.bedrock_flags.fetch_or(mask, Ordering::Relaxed);
            } else {
                self.entity
                    .bedrock_flags
                    .fetch_and(!mask, Ordering::Relaxed);
            }

            let mut meta =
                pumpkin_protocol::bedrock::client::set_actor_data::SyncedActorDataList::new();
            meta.set(
                pumpkin_protocol::bedrock::client::set_actor_data::entity_data_key::FLAGS,
                pumpkin_protocol::bedrock::client::set_actor_data::MetadataValue::Int64(
                    self.entity.bedrock_flags.load(Ordering::Relaxed),
                ),
            );
            meta
        });

        self.entity
            .set_synced_data(tracked_data::living_entity::DATA_LIVING_ENTITY_FLAGS, b);
        if let Some(bedrock_meta) = &bedrock_meta {
            self.entity.send_bedrock_actor_data(bedrock_meta);
        }
    }

    pub fn set_bedrock_blocking(&self, blocking: bool) {
        let flag = pumpkin_protocol::bedrock::client::set_actor_data::entity_data_flag::BLOCKING;
        let index = (flag - 64) as u8;
        let mask = 1i64 << index;
        if blocking {
            self.entity
                .bedrock_flags_two
                .fetch_or(mask, Ordering::Relaxed);
        } else {
            self.entity
                .bedrock_flags_two
                .fetch_and(!mask, Ordering::Relaxed);
        }

        let mut meta =
            pumpkin_protocol::bedrock::client::set_actor_data::SyncedActorDataList::new();
        meta.set(
            pumpkin_protocol::bedrock::client::set_actor_data::entity_data_key::FLAGS_TWO,
            pumpkin_protocol::bedrock::client::set_actor_data::MetadataValue::Int64(
                self.entity.bedrock_flags_two.load(Ordering::Relaxed),
            ),
        );
        self.entity.send_bedrock_actor_data(&meta);
    }

    pub fn clear_active_hand(&self) {
        *self
            .item_in_use
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        *self
            .active_hand
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        self.recent_kinetic_enemies
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.item_use_time.store(0, Ordering::Relaxed);

        self.set_living_flag(Self::USING_ITEM_FLAG, false);
        self.set_bedrock_blocking(false);
    }

    pub fn is_blocking(&self) -> bool {
        let item_in_use = self
            .item_in_use
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(item) = item_in_use.as_ref()
            && item.get_data_component::<BlocksAttacksImpl>().is_some()
        {
            let use_time = self.item_use_time.load(Ordering::Relaxed);
            let required_time = if let Some(dyn_self) = self
                .entity
                .world
                .load()
                .get_entity_by_id(self.entity.entity_id)
                && let Some(player) = dyn_self
                    .cast_any()
                    .downcast_ref::<crate::entity::player::Player>()
                && matches!(
                    player.client.as_ref(),
                    crate::net::ClientPlatform::Bedrock(_)
                ) {
                0
            } else {
                5
            };
            return item.get_max_use_time() - use_time >= required_time;
        }
        false
    }

    pub(super) fn drop_equipment(&self, looting_level: u32) {
        let world = self.entity.world.load();
        let block_pos = self.entity.block_pos.load();

        let drop_chances = self
            .equipment_drop_chances
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let slots_to_drop: Vec<EquipmentSlot> = {
            let mut slots: Vec<_> = self.equipment_slots.values().cloned().collect();
            slots.push(EquipmentSlot::MAIN_HAND);
            slots
        };

        for slot in &slots_to_drop {
            let mut chance = drop_chances
                .get(slot)
                .copied()
                .unwrap_or(DEFAULT_EQUIPMENT_DROP_CHANCE);
            // A chance above 1.0 marks a guaranteed, undamaged drop.
            let preserved = chance > 1.0;
            // Vanilla approximation: EnchantmentHelper.processEquipmentDropChance
            // adds lootingLevel * 0.01 to the per-slot equipment drop chance.
            chance += looting_level as f32 * 0.01;
            chance = chance.min(1.0);
            if !preserved && rand::random::<f32>() >= chance {
                continue;
            }
            let mut item = self
                .entity_equipment
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .equipment
                .remove(slot)
                .unwrap_or_else(|| ItemStack::EMPTY.clone());
            if item.is_empty() {
                continue;
            }
            if prevents_equipment_drop(&item) {
                continue;
            }
            // Vanilla approximation: Mob.dropCustomDeathLoot applies random
            // damage to dropped equipment using two chained random calls:
            // setDamageValue(maxDamage - random.nextInt(1 + random.nextInt(max(maxDamage - 3, 1))))
            if !preserved && let Some(max_damage) = item.get_max_damage() {
                let mut rng = rand::rng();
                let inner = rng.random_range(0..(max_damage - 3).max(1));
                let outer = rng.random_range(0..=inner);
                item.set_damage((max_damage - outer).max(0));
            }
            world.drop_stack(&block_pos, item);
        }
    }

    #[allow(dead_code)]
    pub(super) fn damage_armor_items(&self, caller: &dyn EntityBase, damage_amount: f32) {
        // Formula: armor loses floor(incoming_damage / 4) durability, minimum 1.
        let armor_damage = (damage_amount / 4.0).floor().max(1.0) as i32;
        let mut equipment_updates = Vec::new();

        // TODO: Falling anvil/stalactite should only damage the helmet slot.
        // TODO: Implement DAMAGE_RESISTANT component checks (e.g. netherite vs fire).

        let armor_slots: Vec<(usize, ItemStack, EquipmentSlot)> = {
            let equipment_lock = self
                .entity_equipment
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            self.equipment_slots
                .iter()
                .filter(|(_, slot)| slot.is_armor_slot())
                .filter_map(|(index, slot)| {
                    equipment_lock
                        .equipment
                        .get(slot)
                        .cloned()
                        .map(|stack| (*index, stack, slot.clone()))
                })
                .collect()
        };

        for (slot_index, mut stack, slot) in armor_slots {
            if stack.is_empty() {
                continue;
            }

            let takes_damage = stack
                .get_data_component::<EquippableImpl>()
                .is_none_or(|equippable| equippable.damage_on_hurt);

            if takes_damage {
                let item_id = stack.item.id;
                let slot_result = stack.damage_item(armor_damage);
                if slot_result != pumpkin_data::item_stack::DamageResult::Untouched {
                    if slot_result == pumpkin_data::item_stack::DamageResult::Broken {
                        if let Some(player) = caller.get_player() {
                            player.increment_stat(
                                pumpkin_data::statistic::StatisticCategory::Broken,
                                item_id as i32,
                                1,
                            );
                        }
                        let world = self.entity.world.load();
                        world.send_entity_status(
                            &self.entity,
                            super::super::equipment_break_status(&slot),
                            ActorEventID::None,
                        );
                    }
                    equipment_updates.push((slot.clone(), stack.clone()));
                    if let Some(player) = caller.get_player() {
                        player.enqueue_slot_set_packet(&CSetPlayerInventory::new(
                            (slot_index as i32).into(),
                            &ItemStackSerializer::from(stack),
                        ));
                    }
                }
            }
        }

        if !equipment_updates.is_empty() {
            self.send_equipment_changes(&equipment_updates);
        }
    }

    pub fn held_item(&self, caller: &dyn EntityBase) -> ItemStack {
        if let Some(player) = caller.get_player() {
            return player.inventory.held_item();
        }
        let equipment = self
            .entity_equipment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        equipment
            .equipment
            .get(&EquipmentSlot::MAIN_HAND)
            .cloned()
            .unwrap_or_else(|| ItemStack::EMPTY.clone())
    }

    pub fn get_stack_in_hand(&self, caller: &dyn EntityBase, hand: Hand) -> ItemStack {
        match hand {
            Hand::Left => self.off_hand_item(caller),
            Hand::Right => self.held_item(caller),
        }
    }

    /// getOffHandStack in source
    pub fn off_hand_item(&self, caller: &dyn EntityBase) -> ItemStack {
        if let Some(player) = caller.get_player() {
            return player.inventory.off_hand_item();
        }
        let Some(slot) = self.equipment_slots.get(&PlayerInventory::OFF_HAND_SLOT) else {
            return ItemStack::EMPTY.clone();
        };
        let equipment = self
            .entity_equipment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        equipment
            .equipment
            .get(slot)
            .cloned()
            .unwrap_or_else(|| ItemStack::EMPTY.clone())
    }
}
