use std::sync::Arc;

use pumpkin_data::enchantment::EnchantmentEntityEffect;
use pumpkin_util::math::vector3::Vector3;

use super::{
    AllOf, ApplyEntityImpulse, ApplyExhaustion, ApplyMobEffect, ChangeItemDamage, DamageEntity,
    ExplodeEffect, Ignite, PlaySound, ReplaceBlock, ReplaceDisk, RunFunction, SetBlockProperties,
    SpawnParticlesEffect, SummonEntityEffect,
};
use crate::entity::Entity;
use crate::entity::player::Player;
use crate::world::World;

/// Extension trait for enchantment entity effects.
pub trait EnchantmentEntityEffectExt {
    /// Applies this entity effect at the given position in the world.
    fn apply(
        &self,
        world: &Arc<World>,
        enchantment_level: i32,
        owner: Option<&Arc<Player>>,
        entity: Option<&Entity>,
        position: Vector3<f64>,
    );
}

/// The arguments every effect is applied with.
struct Call<'a> {
    world: &'a Arc<World>,
    enchantment_level: i32,
    owner: Option<&'a Arc<Player>>,
    entity: Option<&'a Entity>,
    position: Vector3<f64>,
}

impl Call<'_> {
    fn run(&self, effect: &impl EnchantmentEntityEffectExt) {
        effect.apply(
            self.world,
            self.enchantment_level,
            self.owner,
            self.entity,
            self.position,
        );
    }
}

/// The effects that carry several settings of their own.
/// The effects that change blocks around the target.
fn apply_block_effect(effect: &EnchantmentEntityEffect, call: &Call<'_>) {
    match effect {
        EnchantmentEntityEffect::ReplaceBlock {
            offset_x,
            offset_y,
            offset_z,
            trigger_game_event,
        } => {
            let offset = Vector3::new(*offset_x, *offset_y, *offset_z);
            call.run(&ReplaceBlock::new(offset, *trigger_game_event));
        }
        EnchantmentEntityEffect::SetBlockProperties {
            properties,
            offset_x,
            offset_y,
            offset_z,
            trigger_game_event,
        } => {
            let offset = Vector3::new(*offset_x, *offset_y, *offset_z);
            let props = properties
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect();
            call.run(&SetBlockProperties::new(props, offset, *trigger_game_event));
        }
        EnchantmentEntityEffect::ReplaceDisk {
            radius,
            height,
            offset_x,
            offset_y,
            offset_z,
            predicate,
            block_state,
            trigger_game_event,
        } => {
            let offset = Vector3::new(*offset_x, *offset_y, *offset_z);
            call.run(&ReplaceDisk {
                radius: radius.clone(),
                height: height.clone(),
                offset,
                predicate: predicate.clone(),
                block_state,
                trigger_game_event: *trigger_game_event,
            });
        }
        _ => {}
    }
}

fn apply_configured(effect: &EnchantmentEntityEffect, call: &Call<'_>) {
    match effect {
        EnchantmentEntityEffect::SpawnParticles {
            particle,
            horizontal_position,
            vertical_position,
            horizontal_velocity,
            vertical_velocity,
            speed,
        } => {
            let spawn_particles = SpawnParticlesEffect::new(
                *particle,
                *horizontal_position,
                *vertical_position,
                horizontal_velocity.clone(),
                vertical_velocity.clone(),
                speed.clone(),
            );
            call.run(&spawn_particles);
        }
        EnchantmentEntityEffect::ApplyMobEffect {
            to_apply,
            min_duration,
            max_duration,
            min_amplifier,
            max_amplifier,
        } => {
            call.run(&ApplyMobEffect::new(
                to_apply.to_vec(),
                min_duration.clone(),
                max_duration.clone(),
                min_amplifier.clone(),
                max_amplifier.clone(),
            ));
        }
        EnchantmentEntityEffect::Explode {
            offset_x,
            offset_y,
            offset_z,
            radius,
            create_fire,
            ..
        } => {
            call.run(&ExplodeEffect::new(
                false,
                None,
                None,
                None,
                Vector3::new(*offset_x, *offset_y, *offset_z),
                radius.clone(),
                *create_fire,
                crate::world::ExplosionInteraction::None,
                None,
                None,
                None,
            ));
        }
        EnchantmentEntityEffect::ReplaceBlock { .. }
        | EnchantmentEntityEffect::SetBlockProperties { .. }
        | EnchantmentEntityEffect::ReplaceDisk { .. } => apply_block_effect(effect, call),
        _ => {}
    }
}

impl EnchantmentEntityEffectExt for EnchantmentEntityEffect {
    fn apply(
        &self,
        world: &Arc<World>,
        enchantment_level: i32,
        owner: Option<&Arc<Player>>,
        entity: Option<&Entity>,
        position: Vector3<f64>,
    ) {
        let call = Call {
            world,
            enchantment_level,
            owner,
            entity,
            position,
        };
        match self {
            Self::Ignite { duration } => {
                call.run(&Ignite::new(duration.clone()));
            }
            Self::DamageEntity {
                min_damage,
                max_damage,
                damage_type,
            } => {
                call.run(&DamageEntity::new(
                    min_damage.clone(),
                    max_damage.clone(),
                    *damage_type,
                ));
            }
            Self::ChangeItemDamage { amount } => {
                call.run(&ChangeItemDamage::new(amount.clone()));
            }
            Self::PlaySound { sound } => {
                call.run(&PlaySound::new(sound));
            }
            Self::ReplaceBlock { .. }
            | Self::SetBlockProperties { .. }
            | Self::ReplaceDisk { .. }
            | Self::SpawnParticles { .. }
            | Self::ApplyMobEffect { .. }
            | Self::Explode { .. } => apply_configured(self, &call),
            Self::SummonEntity {
                entity_types,
                join_team,
            } => {
                call.run(&SummonEntityEffect::new(entity_types.to_vec(), *join_team));
            }
            Self::RunFunction { function } => {
                call.run(&RunFunction::new(*function));
            }
            Self::ApplyExhaustion { amount } => {
                call.run(&ApplyExhaustion::new(amount.clone()));
            }
            Self::ApplyImpulse {
                direction,
                coordinate_scale,
                magnitude,
            } => {
                call.run(&ApplyEntityImpulse::new(
                    *direction,
                    *coordinate_scale,
                    magnitude.clone(),
                ));
            }
            Self::AllOf(effects) => {
                call.run(&AllOf::new(effects));
            }
            Self::Other => {}
        }
    }
}
