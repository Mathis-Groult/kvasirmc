//! Explosions and what they destroy.

use super::World;
use super::explosion::{
    BlockInteraction, Explosion, ExplosionDamageCalculator, ExplosionInteraction,
};
use pumpkin_data::{particle::Particle, sound::Sound};
use pumpkin_protocol::java::client::play::CExplosion;
use pumpkin_protocol::{IdOr, SoundEvent, codec::var_int::VarInt};
use pumpkin_util::math::vector3::Vector3;
use std::sync::Arc;

impl World {
    pub fn explode(
        self: &Arc<Self>,
        position: Vector3<f64>,
        power: f32,
        interaction: ExplosionInteraction,
    ) {
        self.explode_with_calculator(position, power, interaction, None);
    }

    pub fn explode_with_calculator(
        self: &Arc<Self>,
        position: Vector3<f64>,
        power: f32,
        interaction: ExplosionInteraction,
        damage_calculator: Option<Arc<dyn ExplosionDamageCalculator>>,
    ) {
        let block_interaction = self.get_block_interaction(interaction);
        let mut explosion = Explosion::new(power, position, block_interaction);
        if let Some(calc) = damage_calculator {
            explosion = explosion.with_damage_calculator(calc);
        }
        self.run_explosion(&explosion, position, power);
    }

    pub fn explode_tnt_minecart(self: &Arc<Self>, position: Vector3<f64>, power: f32) {
        let block_interaction = self.get_block_interaction(ExplosionInteraction::Tnt);
        let explosion = Explosion::new(power, position, block_interaction).preserving_rails();
        self.run_explosion(&explosion, position, power);
    }

    #[must_use]
    pub fn get_block_interaction(&self, interaction: ExplosionInteraction) -> BlockInteraction {
        let game_rules = &self.level_info.load().game_rules;
        match interaction {
            ExplosionInteraction::None => BlockInteraction::Keep,
            ExplosionInteraction::Block => {
                Self::get_destroy_type(game_rules.block_explosion_drop_decay)
            }
            ExplosionInteraction::Mob => {
                if game_rules.mob_griefing {
                    Self::get_destroy_type(game_rules.mob_explosion_drop_decay)
                } else {
                    BlockInteraction::Keep
                }
            }
            ExplosionInteraction::Tnt => {
                Self::get_destroy_type(game_rules.tnt_explosion_drop_decay)
            }
            ExplosionInteraction::Trigger => BlockInteraction::TriggerBlock,
        }
    }

    #[must_use]
    pub const fn get_destroy_type(drop_decay: bool) -> BlockInteraction {
        if drop_decay {
            BlockInteraction::DestroyWithDecay
        } else {
            BlockInteraction::Destroy
        }
    }

    pub(super) fn run_explosion(
        self: &Arc<Self>,
        explosion: &Explosion,
        position: Vector3<f64>,
        power: f32,
    ) {
        let mut event = crate::plugin::api::events::entity::entity_explode::EntityExplodeEvent::new(
            0, position, power,
        );
        if let Some(server) = self.server.upgrade() {
            server.plugin_manager.fire_blocking(&server, &mut event);
        }
        if event.cancelled {
            return;
        }

        let block_count = explosion.explode(self);
        let particle = if power < 2.0 {
            Particle::Explosion
        } else {
            Particle::ExplosionEmitter
        };
        let sound = IdOr::<SoundEvent>::Id(Sound::EntityGenericExplode as u16);
        for player in self.players.load().iter() {
            if player.position().squared_distance_to_vec(&position) > 4096.0 {
                continue;
            }
            player.try_send_client_packet(&CExplosion::new(
                position,
                power,
                block_count as i32,
                None,
                VarInt(particle as i32),
                sound.clone(),
            ));
        }
    }
}
