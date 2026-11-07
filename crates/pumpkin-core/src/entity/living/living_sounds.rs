//! Hurt, death and fall sounds.

use super::super::EntityBase;
use super::LivingEntity;
use pumpkin_data::entity::EntityType;
use pumpkin_data::sound::Sound;
use pumpkin_data::sound::SoundCategory;
use pumpkin_util::math::position::BlockPos;
use rand::RngExt;

impl LivingEntity {
    pub(super) fn hurt_sound_for_entity(entity_type: &'static EntityType) -> Sound {
        entity_type.hurt_sound.unwrap_or(Sound::EntityGenericHurt)
    }

    pub(super) fn death_sound_for_entity(entity_type: &'static EntityType) -> Sound {
        entity_type.death_sound.unwrap_or(Sound::EntityGenericDeath)
    }

    pub fn make_sound(&self, sound: Sound, category: SoundCategory, volume: f32) {
        if self.entity.is_silent() {
            return;
        }
        self.entity.world.load().play_sound_fine(
            sound,
            category,
            &self.entity.pos.load(),
            volume,
            self.get_pitch(),
        );
    }

    pub(super) fn make_voice_sound(&self, caller: &dyn EntityBase, sound: Sound) {
        let custom_sound = caller.get_mob().and_then(|mob| mob.as_custom_sound());
        self.make_sound(
            sound,
            custom_sound.map_or(
                SoundCategory::Players,
                super::super::custom_sound::CustomSound::sound_category,
            ),
            custom_sound.map_or(1.0, super::super::custom_sound::CustomSound::sound_volume),
        );
    }

    pub(crate) fn get_pitch(&self) -> f32 {
        let mut rng = rand::rng();
        if self.is_baby() {
            (rng.random::<f32>() - rng.random::<f32>()) * 0.2 + 1.5
        } else {
            (rng.random::<f32>() - rng.random::<f32>()) * 0.2 + 1.0
        }
    }

    /// Vanilla `getFallDamageSound`.
    pub(super) fn get_fall_damage_sound(caller: &dyn EntityBase, damage: f32) -> Sound {
        let (small, big) = caller.get_fall_sounds();
        if damage as i32 > 4 { big } else { small }
    }

    /// Vanilla `playBlockFallSound`: the fall sound of the block landed on.
    pub(super) fn play_block_fall_sound(&self, caller: &dyn EntityBase) {
        if self.entity.is_silent() {
            return;
        }
        let pos = self.entity.pos.load();
        let block_pos = BlockPos::new(
            pos.x.floor() as i32,
            (pos.y - 0.2).floor() as i32,
            pos.z.floor() as i32,
        );
        let state = self.entity.world.load().get_block_state(&block_pos);
        if !state.is_air() {
            let sound_type = state.get_sound_type();
            caller.play_entity_sound(
                sound_type.fall_sound,
                sound_type.volume * 0.5,
                sound_type.pitch * 0.75,
            );
        }
    }

    pub(super) fn death_sound(&self, entity: &dyn EntityBase) -> Sound {
        if let Some(sound_source) = entity.get_mob().and_then(|x| x.as_custom_sound())
            && let Some(audio) = sound_source.death_sound()
        {
            return audio;
        }

        Self::death_sound_for_entity(self.entity.entity_type)
    }

    pub(super) fn hurt_sound(&self, entity: &dyn EntityBase) -> Sound {
        if let Some(sound_source) = entity.get_mob().and_then(|x| x.as_custom_sound())
            && let Some(audio) = sound_source.hurt_sound()
        {
            return audio;
        }

        Self::hurt_sound_for_entity(self.entity.entity_type)
    }
}
