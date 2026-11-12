//! One tick of a living entity, phase by phase (`LivingEntity::tick` calls them in order).

use std::sync::atomic::Ordering;
use std::sync::atomic::Ordering::Relaxed;

use kvasir_tickprof::parts::Part;
use pumpkin_data::damage::DamageType;
use pumpkin_data::data_component_impl::{FoodImpl, UseRemainderImpl};
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::entity::{EntityStatus, EntityType};
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::potion::Effect;
use pumpkin_protocol::bedrock::server::actor_event::ActorEventID;
use pumpkin_util::GameMode;

use super::LivingEntity;
use crate::entity::EntityBase;
use crate::server::Server;

impl LivingEntity {
    /// Movement and physics: simulated for a living or dying non-player; a living player
    /// (client-authoritative) only gets friction, suffocation, pushing and freezing.
    pub(super) fn tick_motion(&self, caller: &dyn EntityBase, is_alive: bool) {
        let in_death_animation = self.health.load() <= 0.0 && self.death_time.load(Relaxed) < 20;
        let is_player = self.entity.entity_type == &EntityType::PLAYER;
        if (is_alive || in_death_animation) && !is_player {
            Part::Movement.time(|| self.tick_movement(caller));
            // Vanilla-like order: freeze logic runs after movement/collisions.
            Part::Frozen.time(|| self.entity.tick_frozen(caller));
        } else if is_alive {
            // Client-authoritative players skip `travel`, so decay pushed velocity like
            // vanilla to prevent it accumulating and launching the player.
            self.apply_travel_friction();

            let suffocating = self.entity.tick_block_collisions(caller);
            // Vanilla `LivingEntity.isInWall`: sleeping entities are never in a wall.
            if suffocating
                && !caller
                    .get_player()
                    .is_some_and(crate::entity::player::Player::is_sleeping)
            {
                caller.damage(caller, 1.0, DamageType::IN_WALL);
            }

            // Players push other entities like any living entity.
            self.push_entities(caller);

            self.entity.tick_frozen(caller);
        }
    }

    /// Steps on the block under the entity (and the one below it under a non-solid block).
    pub(super) fn step_on_supporting_blocks(&self, caller: &dyn EntityBase) {
        // Fetch supporting blocks for players or other entities
        let supporting_pos = caller.get_player().map_or_else(
            || self.entity.get_supporting_block_pos(),
            crate::entity::player::Player::get_supporting_block_pos,
        );

        // Notify the block under the entity each tick if a supporting block position is found
        if self.entity.is_affected_by_blocks()
            && let Some(supporting) = supporting_pos
        {
            let world = self.entity.world.load_full();
            let (block, state) = world.get_block_and_state(&supporting);

            world
                .block_registry
                .on_entity_step(block, &world, caller, &supporting, state, false);

            // Check slightly below supporting_pos for additional supporting blocks (blocks under carpets and the like)
            if !block.is_solid() {
                let below_supporting = supporting.down();
                let (below_block, below_state) = world.get_block_and_state(&below_supporting);

                // If block is not air, notify it as well
                world.block_registry.on_entity_step(
                    below_block,
                    &world,
                    caller,
                    &below_supporting,
                    below_state,
                    true, // below supporting block
                );
            }
        }
    }

    /// A player's item in use gets its per-tick use callback.
    pub(super) fn tick_item_use(&self, caller: &dyn EntityBase, server: &Server) {
        if let Some(player) = caller.get_player() {
            let remaining_use_ticks = self.item_use_time.load(Ordering::Relaxed);
            if remaining_use_ticks > 0 {
                let item_in_use = self
                    .item_in_use
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone();
                if let Some(item) = item_in_use.as_ref() {
                    server
                        .item_registry
                        .on_use_tick(item, player, remaining_use_ticks);
                }
            }
        }
    }

    /// Counts the item in use down and finishes it at zero: applied and consumed if it is still
    /// in the used hand, released otherwise.
    pub(super) fn finish_item_in_use(&self, caller: &dyn EntityBase) {
        // Current active item
        if self.item_use_time.load(Ordering::Relaxed) > 0
            && self.item_use_time.fetch_sub(1, Ordering::Relaxed) <= 1
        {
            let item_in_use = self
                .item_in_use
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            if let Some(item) = item_in_use
                .as_ref()
                .filter(|item| self.is_in_used_hand(caller, item))
            {
                self.apply_finished_item_effects(caller, item);
                self.consume_finished_item(caller, item);
            }
            // Finished or released (the item left the used hand): the use ends either way.
            self.clear_active_hand();
        }
    }

    /// What a finished item does to the user: food, consume effects, potion, ominous bottle,
    /// suspicious stew.
    fn apply_finished_item_effects(&self, caller: &dyn EntityBase, item: &ItemStack) {
        // Consume item
        if let Some(food) = item.get_data_component::<FoodImpl>()
            && let Some(player) = caller.get_player()
        {
            player
                .hunger_manager
                .eat(player, food.nutrition as u8, food.saturation);
            self.entity
                .world
                .load()
                .play_bedrock_level_sound("burp", &self.entity.pos.load(), -1);
        }

        self.apply_consumable_effects(caller, item);

        // Handle potion consumption
        if item
            .get_data_component::<pumpkin_data::data_component_impl::PotionContentsImpl>()
            .is_some()
        {
            let effects = crate::item::potion::PotionContents::read_potion_effects(item);
            crate::item::potion::PotionContents::apply_effects_to(
                self,
                effects,
                1.0,
                crate::item::potion::PotionApplicationSource::Normal,
                None,
            );
        }

        if let Some(ominous) = item
            .get_data_component::<pumpkin_data::data_component_impl::OminousBottleAmplifierImpl>()
        {
            self.add_effect(Effect {
                effect_type: &StatusEffect::BAD_OMEN,
                duration: 120_000,
                amplifier: ominous.amplifier.clamp(0, 4) as u8,
                ambient: false,
                show_particles: false,
                show_icon: true,
                blend: false,
            });
        }

        if let Some(stew) = item
            .get_data_component::<pumpkin_data::data_component_impl::SuspiciousStewEffectsImpl>()
        {
            for entry in stew.effects.iter() {
                if let Some(effect_type) = StatusEffect::from_minecraft_name(&entry.effect) {
                    self.add_effect(Effect {
                        effect_type,
                        duration: entry.duration,
                        amplifier: 0,
                        ambient: false,
                        show_particles: true,
                        show_icon: true,
                        blend: false,
                    });
                }
            }
        }
    }

    /// A player who finished an item: the advancement, one consumed from the used hand (with
    /// the item's use remainder), the use cooldown.
    fn consume_finished_item(&self, caller: &dyn EntityBase, item: &ItemStack) {
        if let Some(player) = caller.get_player() {
            player.trigger_advancement(
                crate::entity::player::advancement::trigger::AdvancementTrigger::ConsumeItem {
                    item_id: format!("minecraft:{}", item.item.registry_key),
                },
            );

            // Vanilla `ItemStack.finishUsingItem`: consume one, then apply `use_remainder`.
            let finish_using = |mut stack: ItemStack| {
                let stack_count_before_using = stack.item_count;
                let gamemode = player.gamemode.load();
                stack.decrement_unless_creative(gamemode, 1);
                match item.get_data_component::<UseRemainderImpl>() {
                    Some(use_remainder) => use_remainder.convert_into_remainder(
                        stack,
                        stack_count_before_using,
                        gamemode == GameMode::Creative,
                        |mut extra| {
                            if !player.inventory.insert_stack_anywhere(&mut extra)
                                && !extra.is_empty()
                            {
                                player.drop_item(extra);
                            }
                        },
                    ),
                    None => stack,
                }
            };

            // The used hand holds the item (checked above), as in vanilla.
            let hand = self.used_hand();
            let stack = self.get_stack_in_hand(caller, hand);
            player
                .inventory
                .set_stack_in_hand(hand, finish_using(stack));

            if let Some(cooldown) = item.get_use_cooldown() {
                let group = cooldown
                    .cooldown_group
                    .clone()
                    .unwrap_or_else(|| item.item.registry_key.to_string());
                player.start_cooldown(group, (cooldown.seconds * 20.0) as i32);
            }
        }
    }

    /// Hurt cooldown and the last-hurt-by-player memory count down.
    pub(super) fn tick_hurt_counters(&self) {
        if self.hurt_cooldown.load(Relaxed) > 0 {
            self.hurt_cooldown.fetch_sub(1, Relaxed);
        }

        if self.last_hurt_by_player_memory_time.load(Relaxed) > 0 {
            self.last_hurt_by_player_memory_time.fetch_sub(1, Relaxed);
        } else {
            self.last_hurt_by_player.store(None);
        }
    }

    /// The death animation: a dead player stays (despawned for Bedrock viewers at 10), anything
    /// else poofs and is removed at 20.
    pub(super) fn tick_death(&self) {
        if self.health.load() <= 0.0 {
            let time = self
                .death_time
                .fetch_update(Relaxed, Relaxed, |time| Some(time.saturating_add(1)))
                .unwrap_or_else(|time| time)
                .saturating_add(1);
            if self.entity.entity_type == &EntityType::PLAYER {
                // Bedrock keeps a dead remote player actor in its death pose.
                // Remove Java players after the animation so respawn can
                // recreate a live, interactable actor with the same identity.
                if time == 10 {
                    self.entity
                        .world
                        .load()
                        .despawn_dead_java_player_for_bedrock(&self.entity);
                }
                // Players remain part of the world until their client requests a
                // respawn. Removing one here breaks reconnecting while dead.
                return;
            }
            // Vanilla `LivingEntity.tickDeath` sends the POOF (60) particle event
            // once the death animation finished; the death event (3) was already
            // broadcast in `on_death`. Sending it again here would restart the
            // client-side death animation.
            if time >= 20 && !self.entity.removed.swap(true, Ordering::Relaxed) {
                self.entity.world.load().send_entity_status(
                    &self.entity,
                    EntityStatus::Poof,
                    ActorEventID::GroundDust,
                );
                self.entity.remove();
            }
        }
    }
}
