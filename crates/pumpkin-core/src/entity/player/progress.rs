//! Statistics and advancements.

use super::super::EntityBase;
use super::advancement::PlayerAdvancement;
use super::{Player, statistics};
use crate::net::ClientPlatform;
use pumpkin_data::entity::EntityType;
use pumpkin_data::tag::Taggable;
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::java::client::play::{CAwardStats, Statistic};
use std::sync::atomic::Ordering;

impl Player {
    pub fn send_stats(&self) {
        if let ClientPlatform::Java(java) = self.client.as_ref() {
            let packet_stats: Vec<Statistic> = {
                let stats_guard = self
                    .stats
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                stats_guard
                    .stats
                    .iter()
                    .map(|((category, stat), value)| Statistic {
                        category_id: VarInt(*category),
                        statistic_id: VarInt(*stat),
                        value: VarInt(*value),
                    })
                    .collect()
            };

            let packet = CAwardStats {
                stats: &packet_stats,
            };
            if let Ok(data) = java.serialize_packet(&packet) {
                java.try_enqueue_packet(data);
            }
        }
    }

    pub fn increment_stat(&self, category: statistics::StatisticCategory, stat: i32, amount: i32) {
        let final_amount = if let Some(player_arc) =
            self.world().get_player_by_uuid(self.gameprofile.id)
            && let Some(server) = self.world().server.upgrade()
        {
            let mut event = crate::plugin::api::events::player::player_statistic_increment::PlayerStatisticIncrementEvent {
                player: player_arc,
                statistic_id: format!("{category:?}:{stat}"),
                amount,
                cancelled: false,
            };
            server.plugin_manager.fire_blocking(&server, &mut event);
            if event.cancelled {
                return;
            }
            event.amount
        } else {
            amount
        };
        if let Ok(mut stats) = self.stats.try_lock() {
            stats.increment(category, stat, final_amount);
        }
    }

    pub fn set_stat(&self, category: statistics::StatisticCategory, stat: i32, value: i32) {
        if let Ok(mut stats) = self.stats.try_lock() {
            stats.set(category, stat, value);
        }
    }

    pub fn get_stat(&self, category: statistics::StatisticCategory, stat: i32) -> i32 {
        self.stats
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(category, stat)
    }

    pub fn get_custom_stat(&self, stat: statistics::CustomStatistic) -> i32 {
        self.get_stat(statistics::StatisticCategory::Custom, stat as i32)
    }

    pub fn set_custom_stat(&self, stat: statistics::CustomStatistic, value: i32) {
        self.set_stat(statistics::StatisticCategory::Custom, stat as i32, value);
    }

    pub fn increment_custom_stat(&self, stat: statistics::CustomStatistic, amount: i32) {
        self.increment_stat(statistics::StatisticCategory::Custom, stat as i32, amount);
    }

    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn get_movement_statistic(&self) -> statistics::CustomStatistic {
        let entity = self.get_entity();
        if entity.has_vehicle() {
            let vehicle = entity
                .vehicle
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(vehicle) = vehicle.as_ref() {
                let entity_type = vehicle.get_entity().entity_type;
                if entity_type.has_tag(&pumpkin_data::tag::EntityType::MINECRAFT_BOAT)
                    || entity_type.has_tag(&pumpkin_data::tag::EntityType::C_BOATS)
                    || entity_type == &EntityType::OAK_BOAT
                    || entity_type == &EntityType::SPRUCE_BOAT
                    || entity_type == &EntityType::BIRCH_BOAT
                    || entity_type == &EntityType::JUNGLE_BOAT
                    || entity_type == &EntityType::ACACIA_BOAT
                    || entity_type == &EntityType::DARK_OAK_BOAT
                    || entity_type == &EntityType::MANGROVE_BOAT
                    || entity_type == &EntityType::CHERRY_BOAT
                    || entity_type == &EntityType::PALE_OAK_BOAT
                    || entity_type == &EntityType::BAMBOO_RAFT
                    || entity_type == &EntityType::OAK_CHEST_BOAT
                    || entity_type == &EntityType::SPRUCE_CHEST_BOAT
                    || entity_type == &EntityType::BIRCH_CHEST_BOAT
                    || entity_type == &EntityType::JUNGLE_CHEST_BOAT
                    || entity_type == &EntityType::ACACIA_CHEST_BOAT
                    || entity_type == &EntityType::DARK_OAK_CHEST_BOAT
                    || entity_type == &EntityType::MANGROVE_CHEST_BOAT
                    || entity_type == &EntityType::CHERRY_CHEST_BOAT
                    || entity_type == &EntityType::PALE_OAK_CHEST_BOAT
                    || entity_type == &EntityType::BAMBOO_CHEST_RAFT
                {
                    return statistics::CustomStatistic::BoatOneCm;
                }
                if entity_type.has_tag(&pumpkin_data::tag::EntityType::C_MINECARTS)
                    || entity_type == &EntityType::MINECART
                    || entity_type == &EntityType::CHEST_MINECART
                    || entity_type == &EntityType::FURNACE_MINECART
                    || entity_type == &EntityType::TNT_MINECART
                    || entity_type == &EntityType::HOPPER_MINECART
                    || entity_type == &EntityType::COMMAND_BLOCK_MINECART
                    || entity_type == &EntityType::SPAWNER_MINECART
                {
                    return statistics::CustomStatistic::MinecartOneCm;
                }
                if entity_type == &EntityType::HORSE
                    || entity_type == &EntityType::DONKEY
                    || entity_type == &EntityType::MULE
                    || entity_type == &EntityType::SKELETON_HORSE
                    || entity_type == &EntityType::ZOMBIE_HORSE
                    || entity_type == &EntityType::CAMEL
                    || entity_type == &EntityType::LLAMA
                    || entity_type == &EntityType::TRADER_LLAMA
                {
                    return statistics::CustomStatistic::HorseOneCm;
                }
                if entity_type == &EntityType::PIG {
                    return statistics::CustomStatistic::PigOneCm;
                }
                if entity_type == &EntityType::STRIDER {
                    return statistics::CustomStatistic::StriderOneCm;
                }
                if entity_type == &EntityType::HAPPY_GHAST {
                    return statistics::CustomStatistic::HappyGhastOneCm;
                }
                if entity_type == &EntityType::NAUTILUS
                    || entity_type == &EntityType::ZOMBIE_NAUTILUS
                {
                    return statistics::CustomStatistic::NautilusOneCm;
                }
            }
        }

        if self.is_flying() {
            return statistics::CustomStatistic::FlyOneCm;
        }

        if entity.fall_flying.load(Ordering::Relaxed) {
            return statistics::CustomStatistic::AviateOneCm;
        }

        if entity.swimming.load(Ordering::Relaxed) {
            return statistics::CustomStatistic::SwimOneCm;
        }

        let pos = entity.block_pos.load();
        let world = entity.world.load_full();
        let block = world.get_block(&pos);
        if block.has_tag(&pumpkin_data::tag::Block::MINECRAFT_CLIMBABLE) {
            return statistics::CustomStatistic::ClimbOneCm;
        }

        if entity.touching_water.load(Ordering::Relaxed) {
            if entity.is_submerged_in_water() {
                return statistics::CustomStatistic::WalkUnderWaterOneCm;
            }
            return statistics::CustomStatistic::WalkOnWaterOneCm;
        }

        if entity.sneaking.load(Ordering::Relaxed) {
            return statistics::CustomStatistic::CrouchOneCm;
        }

        if entity.sprinting.load(Ordering::Relaxed) {
            return statistics::CustomStatistic::SprintOneCm;
        }

        if !entity.on_ground.load(Ordering::Relaxed) && entity.velocity.load().y < -0.005 {
            return statistics::CustomStatistic::FallOneCm;
        }

        statistics::CustomStatistic::WalkOneCm
    }

    pub fn has_advancement(
        &self,
        advancement: &'static pumpkin_data::advancement::Advancement,
    ) -> bool {
        self.advancements.try_lock().is_ok_and(|advancements| {
            advancements
                .progress
                .map
                .get(advancement)
                .is_some_and(crate::entity::player::advancement::AdvancementProgress::is_done)
        })
    }

    pub fn trigger_advancement_criterion(
        &self,
        advancement: &'static pumpkin_data::advancement::Advancement,
        criterion: &str,
    ) {
        let Some((player, result)) =
            self.advancements
                .try_lock()
                .ok()
                .and_then(|mut advancements| {
                    let player = advancements.player.upgrade()?;
                    let result = advancements.award(advancement, criterion);
                    Some((player, result))
                })
        else {
            return;
        };

        PlayerAdvancement::finish_award(&player, advancement, result);
    }

    pub fn check_inventory_advancements(&self) {
        if self.inventory_changed.swap(false, Ordering::Relaxed)
            && let Some(p) = self.world().get_player_by_uuid(self.gameprofile.id)
        {
            p.trigger_advancement(
                crate::entity::player::advancement::trigger::AdvancementTrigger::InventoryChanged,
            );
        }
    }
}
