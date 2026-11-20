//! Step sounds, vibrations and splash effects.

use super::{Entity, EntityBase};
use pumpkin_data::Block;
use pumpkin_data::BlockState;
use pumpkin_data::tag::{self, Taggable};
use pumpkin_data::{
    entity::EntityPose,
    game_event::GameEvent,
    sound::{Sound, SoundCategory},
};
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};
use std::sync::atomic::Ordering::{self};

impl Entity {
    /// Vanilla `isStateClimbable`.
    pub(super) fn is_state_climbable(block: &Block) -> bool {
        block.has_tag(&tag::Block::MINECRAFT_CLIMBABLE) || block == &Block::POWDER_SNOW
    }

    /// Vanilla `vibrationAndSoundEffectsFromBlock`.
    pub(super) fn vibration_and_sound_effects_from_block(
        &self,
        caller: &dyn EntityBase,
        pos: &BlockPos,
        state: &'static BlockState,
        should_sound: bool,
        should_vibrate: bool,
        movement: Vector3<f64>,
    ) -> bool {
        if state.is_air() {
            return false;
        }
        let climbable = Self::is_state_climbable(state.id.to_block());
        let crouching = self.pose.load() == EntityPose::Crouching;
        if !(self.on_ground.load(Ordering::Relaxed)
            || climbable
            || crouching && movement.y == 0.0
            || caller.is_on_rails())
            || self.is_swimming()
        {
            return false;
        }
        if should_sound {
            caller.play_step_sound(pos, state);
            if self.should_play_amethyst_step_sound(state) {
                self.play_amethyst_step_sound(caller);
            }
        }
        if should_vibrate {
            self.emit_own_game_event(caller, GameEvent::Step);
        }
        true
    }

    pub(super) fn should_play_amethyst_step_sound(&self, state: &BlockState) -> bool {
        state
            .id
            .to_block()
            .has_tag(&tag::Block::MINECRAFT_CRYSTAL_SOUND_BLOCKS)
            && self.tick_count.load(Ordering::Relaxed)
                >= self.last_crystal_sound_play_tick.load(Ordering::Relaxed) + 20
    }

    /// Vanilla `playAmethystStepSound`.
    pub(super) fn play_amethyst_step_sound(&self, caller: &dyn EntityBase) {
        let tick_count = self.tick_count.load(Ordering::Relaxed);
        let ticks_since = tick_count - self.last_crystal_sound_play_tick.load(Ordering::Relaxed);
        let intensity = (self.crystal_sound_intensity.load() * 0.997f64.powi(ticks_since) as f32
            + 0.07)
            .min(1.0);
        self.crystal_sound_intensity.store(intensity);
        let pitch = (intensity * rand::random::<f32>()).mul_add(1.2, 0.5);
        let volume = intensity.mul_add(1.2, 0.1);
        caller.play_entity_sound(Sound::BlockAmethystBlockChime, volume, pitch);
        self.last_crystal_sound_play_tick
            .store(tick_count, Ordering::Relaxed);
    }

    /// Vanilla `getPrimaryStepSoundBlockPos`: the block above when it sounds from inside, like snow.
    pub fn get_primary_step_sound_block_pos(&self, affecting_pos: &BlockPos) -> BlockPos {
        let above_pos = affecting_pos.up();
        let above = self.world.load().get_block_state(&above_pos).id.to_block();
        if above.has_tag(&tag::Block::MINECRAFT_INSIDE_STEP_SOUND_BLOCKS)
            || above.has_tag(&tag::Block::MINECRAFT_COMBINATION_STEP_SOUND_BLOCKS)
        {
            above_pos
        } else {
            *affecting_pos
        }
    }

    /// Vanilla `waterSwimSound`: louder the faster the entity, or its controlling rider, swims.
    pub fn water_swim_sound(&self, caller: &dyn EntityBase) {
        let rider = caller.get_controlling_passenger();
        let (velocity, volume_modifier) = rider.as_ref().map_or_else(
            || (self.velocity.load(), 0.35),
            |rider| (rider.get_entity().velocity.load(), 0.4),
        );
        let speed = (velocity.x * velocity.x * 0.2
            + velocity.y * velocity.y
            + velocity.z * velocity.z * 0.2)
            .sqrt() as f32;
        caller.play_swim_sound((speed * volume_modifier).min(1.0));
    }

    /// The base `Entity.doWaterSplashEffect`. Clients spawn the bubble and splash particles
    /// themselves.
    pub fn water_splash_effect(&self, caller: &dyn EntityBase) {
        let rider = caller.get_controlling_passenger();
        let (velocity, volume_modifier) = rider.as_ref().map_or_else(
            || (self.velocity.load(), 0.2),
            |rider| (rider.get_entity().velocity.load(), 0.9),
        );
        let speed = ((velocity.x * velocity.x * 0.2
            + velocity.y * velocity.y
            + velocity.z * velocity.z * 0.2)
            .sqrt() as f32
            * volume_modifier)
            .min(1.0);
        let sound = if speed < 0.25 {
            caller.get_swim_splash_sound()
        } else {
            caller.get_swim_high_speed_splash_sound()
        };
        if let Some(sound) = sound {
            let pitch = (rand::random::<f32>() - rand::random::<f32>()).mul_add(0.4, 1.0);
            caller.play_entity_sound(sound, speed, pitch);
        }
        self.emit_own_game_event(caller, GameEvent::Splash);
    }

    /// Vanilla `gameEvent(event)`: at the entity, with itself as the source.
    pub(super) fn emit_own_game_event(&self, caller: &dyn EntityBase, event: GameEvent) {
        self.world.load().emit_game_event_with_context(
            event.name(),
            self.pos.load(),
            &crate::world::game_event::GameEventContext {
                source_entity: Some(caller),
                affected_state: None,
            },
        );
    }

    /// Plays sound at this entity's position with the entity's sound category
    pub fn play_sound(&self, sound: Sound) {
        self.world
            .load()
            .play_sound(sound, SoundCategory::Neutral, &self.pos.load());
    }
}
