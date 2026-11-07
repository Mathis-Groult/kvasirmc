//! Health, damage and death.

use super::super::EntityBase;
use super::{Player, statistics};
use crate::net::ClientPlatform;
use core::f32;
use pumpkin_data::attributes::Attributes;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_protocol::bedrock::client::update_attributes::{
    AttributeData as BedrockAttribute, CUpdateAttributes as CBedrockAttributes,
};
use pumpkin_protocol::bedrock::server::actor_event::{ActorEventID, SActorEvent};
use pumpkin_protocol::bedrock::server::respawn::{RespawnState, SRespawn as SBedrockRespawn};
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::codec::var_ulong::VarULong;
use pumpkin_protocol::java::client::play::{CCombatDeath, CHurtAnimation, CSetHealth};
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::text::TextComponent;
use std::sync::Arc;
use std::sync::atomic::Ordering;

impl Player {
    pub fn send_hurt_animation(&self, yaw: f32) {
        let je_packet = CHurtAnimation::new(self.entity_id().into(), yaw);
        let be_packet = SActorEvent {
            target_runtime_id: VarULong(self.entity_id() as u64),
            event_id: ActorEventID::Hurt,
            data: VarInt(0),
            fire_at_position: None,
        };
        self.try_enqueue_packet_editioned(&je_packet, &be_packet);
    }

    pub fn heal(&self, additional_health: f32) {
        self.living_entity.heal(additional_health);
        self.send_health();
    }

    pub fn damage(
        &self,
        caller: &dyn crate::entity::EntityBase,
        amount: f32,
        damage_type: pumpkin_data::damage::DamageType,
    ) -> bool {
        self.damage_with_context(caller, amount, damage_type, None, None, None)
    }

    pub fn damage_with_context(
        &self,
        caller: &dyn crate::entity::EntityBase,
        amount: f32,
        damage_type: pumpkin_data::damage::DamageType,
        position: Option<Vector3<f64>>,
        source: Option<&dyn crate::entity::EntityBase>,
        cause: Option<&dyn crate::entity::EntityBase>,
    ) -> bool {
        if self
            .abilities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .invulnerable
            && damage_type != pumpkin_data::damage::DamageType::GENERIC_KILL
            && damage_type != pumpkin_data::damage::DamageType::OUT_OF_WORLD
        {
            return false;
        }
        // TODO: Vanilla `Player.hurtServer` difficulty scaling when `scalesWithDifficulty`
        self.living_entity
            .damage_with_context(caller, amount, damage_type, position, source, cause)
    }

    pub fn damage_generic(&self, amount: f32) -> bool {
        use pumpkin_data::damage::DamageType;
        self.living_entity.damage(self, amount, DamageType::GENERIC)
    }

    pub fn kill(&self) {
        use pumpkin_data::damage::DamageType;
        let health = self.living_entity.health.load();
        self.living_entity
            .damage(self, health + 10.0, DamageType::OUT_OF_WORLD);
    }

    pub fn send_health(&self) {
        if !self.has_client_loaded() {
            return;
        }

        let max_health = self.living_entity.get_max_health();
        let attribute = |name: &str, current_value, max_value, default_value| BedrockAttribute {
            min_value: 0.0,
            max_value,
            current_value,
            default_min_value: 0.0,
            default_max_value: max_value,
            default_value,
            name: name.to_string(),
            modifiers: Vec::new(),
        };

        self.try_enqueue_packet_editioned(
            &CSetHealth::new(
                self.living_entity.health.load(),
                self.hunger_manager.level.load().into(),
                self.hunger_manager.saturation.load(),
            ),
            &CBedrockAttributes {
                target_runtime_id: VarULong(self.entity_id() as u64),
                attribute_list: vec![
                    attribute(
                        "minecraft:health",
                        self.living_entity.health.load(),
                        max_health,
                        max_health,
                    ),
                    attribute(
                        "minecraft:player.hunger",
                        self.hunger_manager.level.load().into(),
                        20.0,
                        20.0,
                    ),
                    attribute(
                        "minecraft:player.saturation",
                        self.hunger_manager.saturation.load(),
                        20.0,
                        5.0,
                    ),
                ],
                // This is a client input tick, not our independent server tick counter.
                // Zero applies the authoritative values without prediction-history matching.
                tick: VarULong(0),
            },
        );
    }

    pub(super) fn send_bedrock_respawn_state(&self, state: RespawnState) {
        if let ClientPlatform::Bedrock(client) = self.client.as_ref() {
            let entity = self.get_entity();
            let position = entity.pos.load();
            if let Ok(data) = client.serialize_packet(&SBedrockRespawn {
                position: Vector3::new(
                    position.x as f32,
                    position.y as f32 + entity.entity_type.eye_height,
                    position.z as f32,
                ),
                state,
                player_runtime_id: VarULong(self.entity_id() as u64),
            }) {
                client.try_enqueue_packet(data);
            }
        }
    }

    pub fn tick_health(&self) {
        if !self.has_client_loaded() {
            return;
        }

        let health = self.living_entity.health.load() as i32;
        let food = self.hunger_manager.level.load();
        let saturation = self.hunger_manager.saturation.load();

        let last_health = self.last_sent_health.load(Ordering::Relaxed);
        let last_food = self.last_sent_food.load(Ordering::Relaxed);
        let last_saturation = self.last_food_saturation.load(Ordering::Relaxed);

        if health != last_health || food != last_food || (saturation == 0.0) != last_saturation {
            self.last_sent_health.store(health, Ordering::Relaxed);
            self.last_sent_food.store(food, Ordering::Relaxed);
            self.last_food_saturation
                .store(saturation == 0.0, Ordering::Relaxed);
            self.send_health();
        }
    }

    pub fn set_health(&self, health: f32) {
        self.living_entity.set_health(health);
        self.send_health();
    }

    pub fn set_max_health(&self, max_health: f32) {
        self.living_entity.set_max_health(max_health);
        self.send_health();
    }

    pub fn set_invulnerable(&self, invulnerable: bool) {
        self.abilities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .invulnerable = invulnerable;
        self.send_abilities_update();
    }

    pub fn get_absorption(&self) -> f32 {
        self.living_entity.get_absorption()
    }

    pub fn set_absorption(&self, absorption: f32) {
        self.living_entity.set_absorption(absorption);
    }

    pub async fn respawn(self: &Arc<Self>) {
        self.world().respawn_player(self, false).await;
        // The client rebuilt its attribute state on respawn, so send the held
        // weapon modifiers again.
        crate::entity::attributes::send_attribute_updates_for_living(
            &self.living_entity,
            vec![Attributes::ATTACK_SPEED, Attributes::ATTACK_DAMAGE],
        );
    }

    pub fn send_combat_death(&self, death_msg: &TextComponent) {
        self.try_enqueue_packet_editioned(
            &CCombatDeath::new(self.entity_id().into(), death_msg),
            &SActorEvent {
                target_runtime_id: VarULong(self.entity_id() as u64),
                event_id: ActorEventID::Death,
                data: VarInt(0),
                fire_at_position: None,
            },
        );
    }

    pub fn handle_killed(&self, death_msg: &TextComponent) {
        self.trigger_advancement(
            crate::entity::player::advancement::trigger::AdvancementTrigger::PlayerKilled,
        );
        crate::entity::mob::neutral::tell_neutral_mobs_player_died(self, &self.world());
        let block_pos = self.position().to_block_pos();

        let keep_inventory = { self.world().level_info.load().game_rules.keep_inventory };

        if !keep_inventory {
            let mut main_inv = self
                .inventory()
                .main_inventory
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for item in main_inv.iter_mut() {
                if !item.is_empty() {
                    let stack = std::mem::replace(item, ItemStack::EMPTY.clone());
                    self.increment_stat(
                        statistics::StatisticCategory::Dropped,
                        stack.item.id as i32,
                        stack.item_count as i32,
                    );
                    self.increment_custom_stat(
                        statistics::CustomStatistic::Drop,
                        stack.item_count as i32,
                    );
                    self.world().drop_stack(&block_pos, stack);
                }
            }
        }

        // Reset air supply & drowning ticks on death
        self.breath_manager.reset(self);

        if matches!(self.client.as_ref(), ClientPlatform::Java(_)) {
            self.set_client_loaded(false);
        }
        self.send_combat_death(death_msg);
        self.send_health();
        self.send_bedrock_respawn_state(RespawnState::SearchingForSpawn);
    }
}
