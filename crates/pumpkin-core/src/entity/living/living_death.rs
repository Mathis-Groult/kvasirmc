//! Dying: messages, statistics, loot and experience.

use super::super::EntityBase;
use super::super::experience_orb::ExperienceOrbEntity;
use super::LivingEntity;
use crate::entity::player::statistics::{CustomStatistic, StatisticCategory};
use crate::world::loot::LootContextParameters;
use pumpkin_data::Enchantment;
use pumpkin_data::damage::DamageType;
use pumpkin_data::data_component_impl::{DeathProtectionImpl, EquipmentSlot};
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::entity::{EntityPose, EntityStatus, EntityType};
use pumpkin_data::potion::Effect;
use pumpkin_protocol::bedrock::server::actor_event::ActorEventID;
use pumpkin_util::Hand;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::text::TextComponent;
use std::sync::atomic::Ordering::Relaxed;

impl LivingEntity {
    #[must_use]
    pub fn should_drop_loot(&self) -> bool {
        !self.is_baby()
            && self
                .entity
                .world
                .load()
                .level_info
                .load()
                .game_rules
                .mob_drops
    }

    #[must_use]
    pub fn should_drop_experience(&self) -> bool {
        !self.is_baby()
    }

    #[allow(clippy::redundant_closure_for_method_calls)]
    /// Builds the chat death message for this entity: picks the
    /// `death.attack.<id>` (or the `.player` variant when a killer entity is
    /// known, or the kill-credit name when the killer is offline/removed)
    /// translation and fills in the victim and killer display names.
    pub fn get_death_message(
        dyn_self: &dyn EntityBase,
        damage_type: DamageType,
        source: Option<&dyn EntityBase>,
        cause: Option<&dyn EntityBase>,
    ) -> TextComponent {
        let kill_credit = dyn_self.get_living_entity().and_then(Self::get_kill_credit);
        let kill_credit_name = kill_credit.as_ref().map(|c| c.get_display_name());

        if let Some(living) = dyn_self.get_living_entity() {
            let tracker = living
                .combat_tracker
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            return tracker.get_death_message(dyn_self.get_display_name(), kill_credit_name);
        }

        if let Some(cause) = cause
            && source.is_some()
        {
            TextComponent::translate_cross(
                format!("death.attack.{}", damage_type.message_id),
                format!("death.attack.{}", damage_type.message_id),
                [dyn_self.get_display_name(), cause.get_display_name()],
            )
        } else if let Some(killer) = cause
            .or(source)
            .map(|c| c.get_display_name())
            .or(kill_credit_name)
        {
            TextComponent::translate_cross(
                format!("death.attack.{}.player", damage_type.message_id),
                format!("death.attack.{}.player", damage_type.message_id),
                [dyn_self.get_display_name(), killer],
            )
        } else {
            TextComponent::translate_cross(
                format!("death.attack.{}", damage_type.message_id),
                format!("death.attack.{}", damage_type.message_id),
                [dyn_self.get_display_name()],
            )
        }
    }

    /// Marks the entity as dead exactly once and runs the server-side death
    /// flow: stop movement input, attribute the kill, drop loot, broadcast the
    /// `Death` (3) entity event, and hand out XP. Safe to call on every lethal
    /// damage event; only the first call has an effect.
    #[allow(clippy::too_many_lines)]
    pub fn on_death(
        &self,
        damage_type: DamageType,
        source: Option<&dyn EntityBase>,
        cause: Option<&dyn EntityBase>,
    ) {
        let world = self.entity.world.load();
        let Some(dyn_self) = world.get_entity_by_id(self.entity.entity_id) else {
            return;
        };
        if self
            .dead
            .compare_exchange(false, true, Relaxed, Relaxed)
            .is_ok()
        {
            self.movement_input.store(Vector3::default());
            self.jumping.store(false, Relaxed);

            // Like vanilla, stats go to the kill credit, loot and XP to the killing blow.
            let kill_credit = self.get_kill_credit();
            let killer = cause.or(source);

            self.update_death_stats(&*dyn_self, kill_credit.as_deref());

            self.make_voice_sound(&*dyn_self, self.death_sound(&*dyn_self));
            world.send_entity_status(&self.entity, EntityStatus::Death, ActorEventID::Death);
            let looting_level;
            let tool = if let Some(cause_ent) = cause {
                if let Some(player) = cause_ent
                    .cast_any()
                    .downcast_ref::<crate::entity::player::Player>()
                {
                    let hand_stack = player
                        .inventory()
                        .get_stack_in_hand(pumpkin_util::Hand::Right);
                    looting_level = hand_stack
                        .get_enchantment_level(&Enchantment::LOOTING)
                        .max(0) as u32;
                    (!hand_stack.is_empty()).then(|| hand_stack.clone())
                } else {
                    looting_level = 0;
                    None
                }
            } else {
                looting_level = 0;
                None
            };

            let is_raining = world.is_raining();
            let is_thundering = world.is_thundering();

            let has_player_kill = self.last_hurt_by_player_memory_time.load(Relaxed) > 0;

            let params = LootContextParameters {
                killed_by_player: Some(has_player_kill),
                world: Some(&world),
                this_entity: Some(dyn_self.as_ref()),
                killer_entity: killer,
                direct_killer_entity: source,
                position: Some(self.entity.pos.load()),
                world_time: world.level_info.load().day_time as u64,
                damage_type: Some(damage_type),
                tool,
                is_raining: Some(is_raining),
                is_thundering: Some(is_thundering),
                is_on_fire: Some(
                    self.entity
                        .fire_ticks
                        .load(std::sync::atomic::Ordering::Relaxed)
                        > 0,
                ),
                direct_attacker_weapon: source.and_then(|direct| {
                    direct
                        .get_living_entity()
                        .map(|living| living.held_item(direct))
                }),
                ..Default::default()
            };

            let should_drop_loot = dyn_self.should_drop_loot();
            if should_drop_loot {
                self.drop_loot(&*dyn_self, &params);
            }

            // Award experience
            if params.killed_by_player.unwrap_or(false)
                && world.level_info.load().game_rules.mob_drops
                && dyn_self.should_drop_experience()
            {
                let amount = dyn_self.get_experience_reward(killer);
                if amount > 0 {
                    ExperienceOrbEntity::spawn(&world, self.entity.pos.load(), amount);
                }
            }
            self.entity.pose.store(EntityPose::Dying);

            if should_drop_loot {
                self.drop_equipment(looting_level);
            }
            if let Some(mob) = dyn_self.get_mob() {
                mob.drop_inventory();
            }

            // Broadcast death message if it's a player and the gamerule is enabled
            self.broadcast_death_message(&*dyn_self, damage_type, source, cause);

            // Trigger on_mob_death for active status effects
            let active_effects_vec: Vec<_> = {
                let effects = self
                    .active_effects
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                effects
                    .values()
                    .map(|e| (e.effect_type, e.amplifier))
                    .collect()
            };
            for (effect_type, amplifier) in active_effects_vec {
                if let Some(mob_effect) = crate::entity::effect::get_mob_effect(effect_type) {
                    mob_effect.on_mob_death(self, amplifier, &damage_type);
                }
            }

            self.reset_effects_and_attributes();
        }
    }

    pub(super) fn broadcast_death_message(
        &self,
        dyn_self: &dyn EntityBase,
        damage_type: DamageType,
        source: Option<&dyn EntityBase>,
        cause: Option<&dyn EntityBase>,
    ) {
        let world = self.entity.world.load();
        let show_death_messages = { world.level_info.load().game_rules.show_death_messages };
        if self.entity.entity_type == &EntityType::PLAYER {
            let death_message = Self::get_death_message(dyn_self, damage_type, source, cause);
            let mut final_death_message = death_message;
            if let Some(player) = dyn_self.get_player() {
                if let Some(player_arc) = world.get_player_by_uuid(player.gameprofile.id)
                    && let Some(server) = world.server.upgrade()
                {
                    let mut event =
                        crate::plugin::api::events::entity::entity_death::PlayerDeathEvent::new(
                            player_arc,
                            final_death_message.clone(),
                            0,
                        );
                    server.plugin_manager.fire_blocking(&server, &mut event);
                    if event.cancelled {
                        return;
                    }
                    final_death_message = event.death_message;
                }

                player.handle_killed(&final_death_message);
            }

            if show_death_messages && let Some(server) = world.server.upgrade() {
                for player in server.get_all_players() {
                    player.send_system_message(&final_death_message);
                }
            }
        } else if self.entity.custom_name.load().is_some() {
            let death_message = Self::get_death_message(dyn_self, damage_type, source, cause);
            tracing::info!(
                "Named entity {} died: {}",
                dyn_self.get_display_name().to_pretty_console(),
                death_message.to_pretty_console()
            );
        }
    }

    pub(super) fn update_death_stats(
        &self,
        dyn_self: &dyn EntityBase,
        cause: Option<&dyn EntityBase>,
    ) {
        if let Some(victim_player) = dyn_self.get_player() {
            victim_player.increment_custom_stat(CustomStatistic::Deaths, 1);
            victim_player.set_stat(
                StatisticCategory::Custom,
                CustomStatistic::TimeSinceDeath as i32,
                0,
            );
            victim_player.set_stat(
                StatisticCategory::Custom,
                CustomStatistic::TimeSinceRest as i32,
                0,
            );
            if let Some(killer_entity) = cause.map(EntityBase::get_entity) {
                victim_player.increment_stat(
                    StatisticCategory::KilledBy,
                    killer_entity.entity_type.id as i32,
                    1,
                );
            }
        }

        if let Some(killer_player) = cause.and_then(|c| c.get_player()) {
            killer_player.increment_stat(
                StatisticCategory::Killed,
                self.entity.entity_type.id as i32,
                1,
            );
            if dyn_self.get_player().is_some() {
                killer_player.increment_stat(
                    StatisticCategory::Custom,
                    CustomStatistic::PlayerKills as i32,
                    1,
                );
            } else {
                killer_player.increment_stat(
                    StatisticCategory::Custom,
                    CustomStatistic::MobKills as i32,
                    1,
                );

                let resource_name = self.entity.entity_type.resource_name;
                let criterion_key = format!("minecraft:{resource_name}");
                killer_player.trigger_advancement(
                    crate::entity::player::advancement::trigger::AdvancementTrigger::PlayerKilledEntity {
                        entity_type_resource: criterion_key,
                    },
                );

                if resource_name == "skeleton" {
                    let distance_sq = killer_player
                        .position()
                        .squared_distance_to_vec(&self.entity.pos.load());
                    if distance_sq >= 2500.0 {
                        killer_player.trigger_advancement(crate::entity::player::advancement::trigger::AdvancementTrigger::SniperDuel);
                    }
                }

                if resource_name == "phantom" {
                    killer_player.trigger_advancement(crate::entity::player::advancement::trigger::AdvancementTrigger::TwoBirdsOneArrow);
                }

                let held_item = killer_player.inventory().held_item();
                let is_crossbow = held_item.item.registry_key == "crossbow";
                if is_crossbow {
                    killer_player.trigger_advancement(
                        crate::entity::player::advancement::trigger::AdvancementTrigger::Arbalistic,
                    );
                }

                killer_player.trigger_advancement(
                    crate::entity::player::advancement::trigger::AdvancementTrigger::KillMobNearSculkCatalyst,
                );
            }
        }
    }

    pub(super) fn drop_loot(&self, dyn_self: &dyn EntityBase, params: &LootContextParameters) {
        let mob = dyn_self
            .get_mob()
            .map(crate::entity::mob::Mob::get_mob_entity);
        let key = mob
            .and_then(|mob| {
                mob.death_loot_table
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()
            })
            .unwrap_or_else(|| {
                format!(
                    "minecraft:entities/{}",
                    self.get_entity().entity_type.resource_name
                )
            });
        let world = self.entity.world.load();
        if let Some(loot_table) = world.get_loot_table(&key) {
            let seed = mob
                .map(|mob| mob.death_loot_table_seed.load(Relaxed))
                .filter(|seed| *seed != 0)
                .unwrap_or_else(rand::random);
            let pos = self.entity.block_pos.load();
            for stack in crate::world::loot::generate_loot_from_handle(&loot_table, seed, params) {
                world.drop_stack(&pos, stack);
            }
        }
    }

    /// Tries to use a totem of undying from the entity's hands. If successful, applies the totem effects and returns true.
    #[allow(dead_code)]
    pub(super) async fn try_use_death_protector(&self, caller: &dyn EntityBase) -> bool {
        for hand in Hand::all() {
            let mut stack = self.get_stack_in_hand(caller, hand);

            // Clear the stack and use the totem of undying
            if stack.get_data_component::<DeathProtectionImpl>().is_some() {
                let mut resurrect_event =
                    crate::plugin::api::events::entity::entity_resurrect::EntityResurrectEvent::new(
                        self.entity.entity_id,
                    );
                if let Some(server) = self.entity.world.load().server.upgrade() {
                    server
                        .plugin_manager
                        .fire(&server, &mut resurrect_event)
                        .await;
                }
                if resurrect_event.cancelled {
                    return false;
                }

                let item_key = stack.item.registry_key.to_string();
                stack.clear();
                let slot = match hand {
                    Hand::Right => EquipmentSlot::MAIN_HAND,
                    Hand::Left => EquipmentSlot::OFF_HAND,
                };
                if let Some(player) = caller.get_player() {
                    player.trigger_advancement(
                        crate::entity::player::advancement::trigger::AdvancementTrigger::UsedTotem {
                            item_id: item_key,
                        },
                    );
                    player
                        .inventory()
                        .entity_equipment
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .equipment
                        .insert(slot, stack);
                } else {
                    self.entity_equipment
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .equipment
                        .insert(slot, stack);
                }
                self.set_health(1.0);
                self.entity.world.load().send_entity_status(
                    &self.entity,
                    EntityStatus::ProtectedFromDeath,
                    ActorEventID::InstantDeath,
                );

                // Set Absorption, Regeneration, and Fire Resistance effects
                self.add_effect(Effect {
                    effect_type: &StatusEffect::ABSORPTION,
                    duration: 100,
                    amplifier: 1,
                    ambient: false,
                    show_particles: true,
                    show_icon: true,
                    blend: false,
                });
                self.add_effect(Effect {
                    effect_type: &StatusEffect::REGENERATION,
                    duration: 900,
                    amplifier: 1,
                    ambient: false,
                    show_particles: true,
                    show_icon: true,
                    blend: false,
                });
                self.add_effect(Effect {
                    effect_type: &StatusEffect::FIRE_RESISTANCE,
                    duration: 800,
                    amplifier: 0,
                    ambient: false,
                    show_particles: true,
                    show_icon: true,
                    blend: false,
                });

                return true;
            }
        }

        false
    }
}
