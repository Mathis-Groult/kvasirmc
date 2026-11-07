//! Sounds, particles and block changes shown to this player.

use super::Player;
use crate::net::ClientPlatform;
use core::f32;
use pumpkin_data::particle::Particle;
use pumpkin_data::sound::SoundCategory;
use pumpkin_protocol::IdOr;
use pumpkin_protocol::SoundEvent;
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::java::client::play::{CBlockUpdate, CParticle, CSoundEffect, CStopSound};
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};
use pumpkin_util::resource_location::ResourceLocation;

impl Player {
    pub fn spawn_particle(
        &self,
        position: Vector3<f64>,
        offset: Vector3<f32>,
        max_speed: f32,
        particle_count: i32,
        particle: Particle,
    ) {
        let packet = CParticle::new(
            false,
            false,
            position,
            offset,
            max_speed,
            particle_count,
            VarInt(particle as i32),
            &[],
        );
        if let ClientPlatform::Java(client) = self.client.as_ref()
            && let Ok(data) = client.serialize_packet(&packet)
        {
            client.try_enqueue_packet(data);
        }
    }

    pub fn play_sound(
        &self,
        sound_id: u16,
        category: SoundCategory,
        position: &Vector3<f64>,
        volume: f32,
        pitch: f32,
        seed: i64,
    ) {
        let packet = CSoundEffect::new(IdOr::Id(sound_id), category, position, volume, pitch, seed);
        self.try_send_client_packet(&packet);
    }

    pub fn play_sound_event(
        &self,
        sound: SoundEvent,
        category: SoundCategory,
        position: &Vector3<f64>,
        volume: f32,
        pitch: f32,
        seed: i64,
    ) {
        let packet = CSoundEffect::new(IdOr::Value(sound), category, position, volume, pitch, seed);
        self.try_send_client_packet(&packet);
    }

    /// Stops a sound playing on the client.
    ///
    /// # Arguments
    ///
    /// * `sound_id`: An optional [`ResourceLocation`] specifying the sound to stop. If [`None`], all sounds in the specified category (if any) will be stopped.
    /// * `category`: An optional [`SoundCategory`] specifying the sound category to stop. If [`None`], all sounds with the specified resource location (if any) will be stopped.
    pub fn stop_sound(&self, sound_id: Option<ResourceLocation>, category: Option<SoundCategory>) {
        let packet = CStopSound::new(sound_id, category);
        self.try_send_client_packet(&packet);
    }

    /// Plays a custom sound event by identifier for the player.
    pub fn play_custom_sound(
        &self,
        sound_name: &str,
        category: SoundCategory,
        position: &Vector3<f64>,
        volume: f32,
        pitch: f32,
    ) {
        self.play_sound_event(
            pumpkin_protocol::SoundEvent {
                sound_name: sound_name.into(),
                range: None,
            },
            category,
            position,
            volume,
            pitch,
            rand::random::<i64>(),
        );
    }

    pub fn spawn_particles(
        &self,
        particle: Particle,
        pos: Vector3<f64>,
        count: u32,
        offset: Vector3<f32>,
        max_speed: f32,
    ) {
        let packet = CParticle::new(
            false,
            false,
            pos,
            offset,
            max_speed,
            count as i32,
            (particle.to_id() as i32).into(),
            &[],
        );
        self.try_send_client_packet(&packet);
    }

    pub fn send_block_change(&self, location: BlockPos, state_id: u16) {
        let packet = CBlockUpdate::new(location, (state_id as i32).into());
        self.try_send_client_packet(&packet);
    }

    pub fn reset_block_change(&self, location: BlockPos) {
        let state_id = self.world().get_block_state_id(&location);
        let packet = CBlockUpdate::new(location, (state_id.as_u16() as i32).into());
        self.try_send_client_packet(&packet);
    }
}
