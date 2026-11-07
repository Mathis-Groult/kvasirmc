//! Potion effects, post-processing effects and the raid omen.

use super::Player;
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::potion::Effect;
use pumpkin_data::sound::{Sound, SoundCategory};
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::java::client::play::CUpdateMobEffect;
use pumpkin_util::math::position::BlockPos;

impl Player {
    #[must_use]
    pub fn get_post_effects(&self) -> Vec<String> {
        self.post_effects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    #[must_use]
    pub fn has_post_effect(&self, effect: &str) -> bool {
        self.post_effects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .any(|e| e == effect)
    }

    pub fn add_post_effect(&self, effect: String) -> bool {
        let mut effects = self
            .post_effects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if effects.iter().any(|e| e == &effect) {
            return false;
        }
        effects.push(effect);
        self.try_send_client_packet(&pumpkin_protocol::java::client::play::CPostEffects::new(
            &effects,
        ));
        true
    }

    pub fn remove_post_effect(&self, effect: &str) -> bool {
        let mut effects = self
            .post_effects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        effects.iter().position(|e| e == effect).is_some_and(|pos| {
            effects.remove(pos);
            self.try_send_client_packet(&pumpkin_protocol::java::client::play::CPostEffects::new(
                &effects,
            ));
            true
        })
    }

    pub fn clear_post_effects(&self) -> usize {
        let mut effects = self
            .post_effects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if effects.is_empty() {
            return 0;
        }
        let count = effects.len();
        effects.clear();
        self.try_send_client_packet(&pumpkin_protocol::java::client::play::CPostEffects::new(
            &effects,
        ));
        count
    }

    pub fn send_post_effects(&self) {
        let effects = self
            .post_effects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.try_send_client_packet(&pumpkin_protocol::java::client::play::CPostEffects::new(
            &effects,
        ));
    }

    pub fn tick_raid_omen(&self) {
        if self.is_spectator() {
            return;
        }

        if let Some(bad_omen) = self.get_effect(&StatusEffect::BAD_OMEN)
            && !self.has_effect(&StatusEffect::RAID_OMEN)
        {
            let world = self.world();
            if !world.dimension.can_start_raid {
                return;
            }
            let player_pos = self.living_entity.entity.block_pos.load();
            let pos_f64 = self.living_entity.entity.pos.load();

            let village_pos = world
                .villager_poi
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get_nearest_job_site(player_pos, 64)
                .or_else(|| {
                    world.raids.try_lock().ok().and_then(|raids| {
                        raids
                            .get_nearby_raid(&player_pos, 64.0 * 64.0)
                            .map(|r| r.center)
                    })
                });

            if let Some(pos) = village_pos {
                if let Some(p) = self.world().get_player_by_uuid(self.gameprofile.id) {
                    let bad_omen_amplifier = bad_omen.amplifier;
                    p.living_entity.remove_effect(&StatusEffect::BAD_OMEN);
                    p.set_raid_omen_position(pos);
                    let effect = Effect {
                        effect_type: &StatusEffect::RAID_OMEN,
                        duration: 600,
                        amplifier: bad_omen_amplifier,
                        ambient: false,
                        show_particles: true,
                        show_icon: true,
                        blend: true,
                    };
                    p.add_effect(effect);
                }
                world.play_sound(Sound::BlockBellResonate, SoundCategory::Neutral, &pos_f64);
            }
        }
    }

    pub fn add_effect(&self, effect: Effect) {
        self.living_entity.add_effect(effect);
        self.trigger_advancement(
            crate::entity::player::advancement::trigger::AdvancementTrigger::EffectsChanged,
        );
    }

    pub fn has_effect(&self, effect_type: &'static StatusEffect) -> bool {
        self.living_entity.has_effect(effect_type)
    }

    pub fn get_effect(&self, effect_type: &'static StatusEffect) -> Option<Effect> {
        self.living_entity.get_effect(effect_type)
    }

    pub fn get_active_effects(&self) -> Vec<Effect> {
        let effects = self
            .living_entity
            .active_effects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        effects.values().cloned().collect()
    }

    #[must_use]
    pub fn get_raid_omen_position(&self) -> Option<BlockPos> {
        self.raid_omen_position.load()
    }

    pub fn set_raid_omen_position(&self, pos: BlockPos) {
        self.raid_omen_position.store(Some(pos));
    }

    pub fn clear_raid_omen_position(&self) {
        self.raid_omen_position.store(None);
    }

    pub fn send_active_effects(&self) {
        let effects: Vec<_> = self
            .living_entity
            .active_effects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .cloned()
            .collect();
        for effect in &effects {
            self.send_effect(effect);
        }
    }

    /**
     * Send a clientside only effect to the player.
     * It won't be tracked on the server.
     */
    pub fn send_effect(&self, effect: &Effect) {
        let mut flag: i8 = 0;

        if effect.ambient {
            flag |= 1;
        }
        if effect.show_particles {
            flag |= 2;
        }
        if effect.show_icon {
            flag |= 4;
        }
        if effect.blend {
            flag |= 8;
        }

        let effect_id = VarInt(i32::from(effect.effect_type.id));
        self.try_send_client_packet(&CUpdateMobEffect::new(
            self.entity_id().into(),
            effect_id,
            effect.amplifier.into(),
            effect.duration.into(),
            flag,
        ));
    }

    pub fn remove_effect(&self, effect_type: &'static StatusEffect) -> bool {
        let effect_id = VarInt(i32::from(effect_type.id));
        self.try_send_client_packet(
            &pumpkin_protocol::java::client::play::CRemoveMobEffect::new(
                self.entity_id().into(),
                effect_id,
            ),
        );

        self.living_entity.remove_effect(effect_type)

        // TODO broadcast metadata
    }

    pub fn remove_all_effects(&self) -> bool {
        let mut succeeded = false;
        let mut effect_list = vec![];
        let effects: Vec<_> = self
            .living_entity
            .active_effects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .copied()
            .collect();
        for effect in effects {
            effect_list.push(effect);
            let effect_id = VarInt(i32::from(effect.id));
            self.try_send_client_packet(
                &pumpkin_protocol::java::client::play::CRemoveMobEffect::new(
                    self.entity_id().into(),
                    effect_id,
                ),
            );
            succeeded = true;
        }

        // Need to remove effects afterward here because there would be a deadlock if this is done in the for loop.
        for effect in effect_list {
            self.living_entity.remove_effect(effect);
        }

        succeeded
    }
}
