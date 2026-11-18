//! Breaking blocks and looking at targets.

use super::super::EntityBase;
use super::Player;
use crate::block;
use crate::entity::attributes::{Modifier, ModifierOperation};
use crate::net::ClientPlatform;
use crate::server::Server;
use crate::world::{BlockBreakingProgress, World};
use core::f32;
use pumpkin_data::attributes::Attributes;
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::{Block, BlockState, Enchantment};
use pumpkin_util::GameMode;
use pumpkin_util::math::{boundingbox::BoundingBox, position::BlockPos, vector3::Vector3};
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[cfg(test)]
mod break_stats_tests;

impl Player {
    pub(super) fn continue_mining(
        &self,
        location: BlockPos,
        world: &World,
        state: &BlockState,
        starting_time: i32,
    ) -> bool {
        let time = self.tick_counter.load(Ordering::Relaxed) - starting_time;
        let speed = block::calc_block_breaking(self, state, Block::from_state_id(state.id));
        let total_progress = speed * (time + 1) as f32;
        let stage = (total_progress * 10.0) as i32;
        let stage = stage.min(9);
        let old_speed = self
            .current_block_breaking_speed
            .swap(speed.to_bits(), Ordering::Relaxed);
        let speed_changed = old_speed != speed.to_bits();
        if stage != self.current_block_destroy_stage.load(Ordering::Relaxed) || speed_changed {
            world.set_block_breaking(
                &self.living_entity.entity,
                location,
                BlockBreakingProgress::Update {
                    stage,
                    speed: speed_changed.then_some(speed),
                },
            );
            self.current_block_destroy_stage
                .store(stage, Ordering::Relaxed);
        }
        total_progress >= 1.0
    }

    pub(crate) fn stop_mining(&self) {
        let was_mining = self.mining.swap(false, Ordering::Relaxed);
        let stage = self.current_block_destroy_stage.swap(-1, Ordering::Relaxed);
        self.current_block_breaking_speed
            .store(0, Ordering::Relaxed);

        if was_mining || stage >= 0 {
            let pos = *self
                .mining_pos
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            self.world().set_block_breaking(
                &self.living_entity.entity,
                pos,
                BlockBreakingProgress::Stop,
            );
        }
    }

    pub fn get_target_block(
        &self,
        world: &Arc<World>,
        max_distance: f64,
    ) -> Option<pumpkin_util::math::position::BlockPos> {
        let (yaw, pitch) = (
            self.living_entity.entity.yaw.load(),
            self.living_entity.entity.pitch.load(),
        );
        let eye_pos = self.living_entity.entity.get_eye_pos();
        let yaw_rad = f64::from(yaw + 90.0).to_radians();
        let pitch_rad = f64::from(-pitch).to_radians();
        let dir = pumpkin_util::math::vector3::Vector3::new(
            pitch_rad.cos() * yaw_rad.cos(),
            pitch_rad.sin(),
            pitch_rad.cos() * yaw_rad.sin(),
        );
        let end_pos = eye_pos + dir * max_distance;
        let hit = world.clip(&pumpkin_world::clip::ClipContext::new(
            eye_pos,
            end_pos,
            pumpkin_world::clip::ClipBlock::Outline,
            pumpkin_world::clip::ClipFluid::None,
        ));
        if hit.is_miss {
            None
        } else {
            Some(hit.block_pos)
        }
    }

    pub fn get_target_entity(
        &self,
        world: &Arc<World>,
        max_distance: f64,
    ) -> Option<Arc<dyn EntityBase>> {
        let (yaw, pitch) = (
            self.living_entity.entity.yaw.load(),
            self.living_entity.entity.pitch.load(),
        );
        let eye_pos = self.living_entity.entity.get_eye_pos();
        let yaw_rad = f64::from(yaw + 90.0).to_radians();
        let pitch_rad = f64::from(-pitch).to_radians();
        let dir = pumpkin_util::math::vector3::Vector3::new(
            pitch_rad.cos() * yaw_rad.cos(),
            pitch_rad.sin(),
            pitch_rad.cos() * yaw_rad.sin(),
        );
        let ray = dir * max_distance;
        let end_pos = eye_pos + ray;
        let search_box = pumpkin_util::math::boundingbox::BoundingBox::new(
            pumpkin_util::math::vector3::Vector3::new(
                eye_pos.x.min(end_pos.x),
                eye_pos.y.min(end_pos.y),
                eye_pos.z.min(end_pos.z),
            ),
            pumpkin_util::math::vector3::Vector3::new(
                eye_pos.x.max(end_pos.x),
                eye_pos.y.max(end_pos.y),
                eye_pos.z.max(end_pos.z),
            ),
        )
        .expand(1.0, 1.0, 1.0);

        let entities = world.get_entities_at_box(&search_box);
        let mut closest_t = 1.0f64;
        let mut closest_entity = None;

        for entity in entities {
            if entity.get_entity().entity_id == self.entity_id() {
                continue;
            }
            let ebb = entity.get_entity().bounding_box.load();
            if let Some(t) = ebb.ray_intersection(eye_pos, ray)
                && t < closest_t
            {
                closest_t = t;
                closest_entity = Some(entity);
            }
        }
        closest_entity
    }

    pub fn block_interaction_range(&self) -> f64 {
        let is_bedrock = matches!(self.client.as_ref(), ClientPlatform::Bedrock(_));
        if self.gamemode.load() == GameMode::Creative {
            5.0
        } else if is_bedrock {
            let attr = self
                .living_entity
                .get_attribute_value(&Attributes::BLOCK_INTERACTION_RANGE);
            attr.max(5.0)
        } else {
            self.living_entity
                .get_attribute_value(&Attributes::BLOCK_INTERACTION_RANGE)
        }
    }

    pub fn entity_interaction_range(&self) -> f64 {
        let is_bedrock = matches!(self.client.as_ref(), ClientPlatform::Bedrock(_));
        if self.gamemode.load() == GameMode::Creative {
            5.0
        } else if is_bedrock {
            let attr = self
                .living_entity
                .get_attribute_value(&Attributes::ENTITY_INTERACTION_RANGE);
            // Bedrock players have a slightly longer interaction/attack reach (~3.0 in Java vs ~3.5 in Bedrock)
            attr.max(3.5)
        } else {
            self.living_entity
                .get_attribute_value(&Attributes::ENTITY_INTERACTION_RANGE)
        }
    }

    pub fn can_interact_with_entity(&self, target: &dyn EntityBase, additional_range: f64) -> bool {
        let d = self.entity_interaction_range() + additional_range;
        target
            .get_entity()
            .bounding_box
            .load()
            .squared_magnitude(self.eye_position())
            < d * d
    }

    pub fn can_interact_with_block_at(&self, position: &BlockPos, additional_range: f64) -> bool {
        let d = self.block_interaction_range() + additional_range;
        let box_pos = BoundingBox::from_block(position);
        let entity_pos = self.living_entity.entity.pos.load();
        let eye_height = self.living_entity.entity.get_eye_height();
        box_pos.squared_magnitude(Vector3 {
            x: entity_pos.x,
            y: entity_pos.y + eye_height,
            z: entity_pos.z,
        }) < d * d
    }

    /// Whether the held item, game mode or adventure rules forbid mining `block` (Java and
    /// Bedrock digging both ask).
    pub(crate) fn cannot_mine(&self, server: &Server, block: &'static Block) -> bool {
        let inventory = self.inventory();
        let held = inventory.held_item();
        let gamemode = self.gamemode.load();
        !server.item_registry.can_mine(held.item, self)
            || gamemode == GameMode::Spectator
            || (gamemode == GameMode::Creative
                && held
                    .get_data_component::<pumpkin_data::data_component_impl::ToolImpl>()
                    .is_some_and(|t| !t.can_destroy_blocks_in_creative))
            || (gamemode == GameMode::Adventure
                && !held
                    .get_data_component::<pumpkin_data::data_component_impl::CanBreakImpl>()
                    .is_some_and(|c| c.matches_block(block)))
    }

    pub fn can_harvest(&self, state: &BlockState, block: &'static Block) -> bool {
        !state.tool_required() || self.inventory().held_item().is_correct_for_drops(block)
    }

    pub(super) fn sync_mining_efficiency(&self) {
        let level = self
            .inventory()
            .held_item()
            .get_enchantment_level(&Enchantment::EFFICIENCY);
        if self
            .synced_mining_efficiency_level
            .swap(level, Ordering::Relaxed)
            == level
        {
            return;
        }
        self.living_entity
            .update_attribute(&Attributes::MINING_EFFICIENCY, |inst| {
                if level > 0 {
                    inst.add_or_replace_modifier(Modifier {
                        id: Self::EFFICIENCY_ATTRIBUTE_MODIFIER_ID.to_string(),
                        amount: f64::from(level * level + 1),
                        operation: ModifierOperation::Add,
                    });
                } else {
                    inst.remove_modifier(Self::EFFICIENCY_ATTRIBUTE_MODIFIER_ID);
                }
            });
        crate::entity::attributes::send_attribute_updates_for_living(
            &self.living_entity,
            vec![Attributes::MINING_EFFICIENCY],
        );
    }

    pub fn get_mining_speed(&self, block: &'static Block) -> f32 {
        self.sync_mining_efficiency();
        let held_item = self.inventory.held_item();
        let mut speed = held_item.get_speed(block);
        // Effi only gets applied if tool's break speed for block is alreadyabove 1 (meaning it's
        // the correct tool)
        if speed > 1.0 {
            speed += self
                .living_entity
                .get_attribute_value(&Attributes::MINING_EFFICIENCY) as f32;
        }
        // Haste
        if self.living_entity.has_effect(&StatusEffect::HASTE)
            || self.living_entity.has_effect(&StatusEffect::CONDUIT_POWER)
        {
            speed *= ((self.get_haste_amplifier() + 1) as f32).mul_add(0.2, 1.0);
        }
        // Fatigue
        if let Some(fatigue) = self.living_entity.get_effect(&StatusEffect::MINING_FATIGUE) {
            let fatigue_speed = match fatigue.amplifier {
                0 => 0.3,
                1 => 0.09,
                2 => 0.0027,
                _ => 8.1E-4,
            };
            speed *= fatigue_speed;
        }
        // TODO: Handle when in water
        if !self.living_entity.entity.on_ground.load(Ordering::Relaxed) {
            speed /= 5.0;
        }
        speed
    }

    pub(super) fn get_haste_amplifier(&self) -> u32 {
        let mut i = 0;
        let mut j = 0;
        if let Some(effect) = self.living_entity.get_effect(&StatusEffect::HASTE) {
            i = effect.amplifier;
        }
        if let Some(effect) = self.living_entity.get_effect(&StatusEffect::CONDUIT_POWER) {
            j = effect.amplifier;
        }
        u32::from(i.max(j))
    }
}
