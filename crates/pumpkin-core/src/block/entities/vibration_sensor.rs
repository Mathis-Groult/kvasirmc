//! The vibration listener inside a (calibrated) sculk sensor: vanilla 26.3
//! `VibrationSystem.Listener`/`Ticker` with `SculkSensorBlockEntity$VibrationUser`.

use std::sync::{Arc, Mutex, PoisonError};

use pumpkin_data::{game_event::GameEvent, tag};
use pumpkin_nbt::{compound::NbtCompound, tag::NbtTag};
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};

use crate::block::blocks::redstone::sculk_sensor::SculkSensorBlock;
use crate::world::World;
use crate::world::game_event::{GameEventContext, GameEventListener};
use crate::world::vibration::{
    block_destination_payload, frequency, is_occluded, is_valid_vibration, send_vibration_particle,
    strength_for_distance,
};

/// A vibration heard by the sensor and not yet delivered.
#[derive(Clone, Copy)]
struct Vibration {
    event: GameEvent,
    distance: f32,
    origin: Vector3<f64>,
}

impl Vibration {
    /// `VibrationInfo.CODEC` (source and projectile owner are not kept).
    fn read_nbt(nbt: &NbtCompound) -> Option<Self> {
        let event = GameEvent::from_name(nbt.get_string("game_event")?)?;
        let distance = nbt.get_float("distance")?;
        let pos = nbt.get_list("pos")?;
        if pos.len() != 3 || !distance.is_finite() || distance < 0.0 {
            return None;
        }
        let origin = Vector3::new(
            pos[0].extract_double()?,
            pos[1].extract_double()?,
            pos[2].extract_double()?,
        );
        (origin.x.is_finite() && origin.y.is_finite() && origin.z.is_finite()).then_some(Self {
            event,
            distance,
            origin,
        })
    }

    fn write_nbt(&self) -> NbtCompound {
        let mut nbt = NbtCompound::new();
        nbt.put_string("game_event", format!("minecraft:{}", self.event.name()));
        nbt.put_float("distance", self.distance);
        nbt.put_list(
            "pos",
            vec![
                NbtTag::Double(self.origin.x),
                NbtTag::Double(self.origin.y),
                NbtTag::Double(self.origin.z),
            ],
        );
        nbt
    }
}

#[derive(Default)]
struct State {
    /// Best vibration of the tick it arrived in, and that tick (`VibrationSelector`).
    candidate: Option<(Vibration, i64)>,
    /// The vibration travelling to the sensor and its remaining ticks.
    current: Option<(Vibration, i32)>,
}

pub struct VibrationSensor {
    position: BlockPos,
    calibrated: bool,
    state: Mutex<State>,
}

impl VibrationSensor {
    #[must_use]
    pub const fn new(position: BlockPos, calibrated: bool) -> Self {
        Self {
            position,
            calibrated,
            state: Mutex::new(State {
                candidate: None,
                current: None,
            }),
        }
    }

    /// A sensor holding the vibrations of a saved `listener` compound (`VibrationSystem.Data`).
    #[must_use]
    pub fn from_nbt(position: BlockPos, calibrated: bool, nbt: Option<&NbtCompound>) -> Self {
        let sensor = Self::new(position, calibrated);
        if let Some(nbt) = nbt
            && let Some(tick) = nbt
                .get_compound("selector")
                .and_then(|s| s.get_long("tick"))
            && let delay = nbt.get_int("event_delay").unwrap_or(0)
            && delay >= 0
        {
            let mut state = sensor.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.current = nbt
                .get_compound("event")
                .and_then(Vibration::read_nbt)
                .map(|vibration| (vibration, delay));
            state.candidate = nbt
                .get_compound("selector")
                .and_then(|s| s.get_compound("event"))
                .and_then(Vibration::read_nbt)
                .map(|vibration| (vibration, tick));
        }
        sensor
    }

    /// The `listener` compound (`VibrationSystem.Data.CODEC`).
    #[must_use]
    pub fn write_nbt(&self) -> NbtCompound {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let mut nbt = NbtCompound::new();
        if let Some((vibration, _)) = &state.current {
            nbt.put_compound("event", vibration.write_nbt());
        }
        let mut selector = NbtCompound::new();
        selector.put_long(
            "tick",
            state.candidate.as_ref().map_or(-1, |(_, tick)| *tick),
        );
        if let Some((vibration, _)) = &state.candidate {
            selector.put_compound("event", vibration.write_nbt());
        }
        nbt.put_compound("selector", selector);
        nbt.put_int("event_delay", state.current.map_or(0, |(_, delay)| delay));
        nbt
    }

    const fn range(&self) -> i32 {
        if self.calibrated { 16 } else { 8 }
    }

    /// `VibrationUser.canReceiveVibration`.
    fn can_receive(&self, world: &World, source: BlockPos, event: GameEvent) -> bool {
        if source == self.position && matches!(event.name(), "block_destroy" | "block_place") {
            return false;
        }
        let frequency = frequency(event);
        if frequency == 0 || !SculkSensorBlock::can_activate(world, &self.position) {
            return false;
        }
        if self.calibrated {
            let back = SculkSensorBlock::calibrated_input(world, &self.position);
            if back != 0 && back != frequency {
                return false;
            }
        }
        true
    }

    /// `VibrationSystem.Ticker.tick`.
    pub fn tick(&self, world: &Arc<World>) {
        let now = world.get_world_age();
        let arrived = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.current.is_none()
                && let Some((vibration, tick)) = state.candidate
                && tick < now
            {
                state.candidate = None;
                let travel = vibration.distance.floor() as i32;
                state.current = Some((vibration, travel));
                if let Ok(payload) = block_destination_payload(self.position, travel) {
                    send_vibration_particle(world, vibration.origin, &payload);
                }
            }
            let Some((vibration, travel)) = state.current else {
                return;
            };
            let travel = travel - 1;
            if travel > 0 {
                state.current = Some((vibration, travel));
                return;
            }
            state.current = None;
            vibration
        };
        if !SculkSensorBlock::can_activate(world, &self.position) {
            return;
        }
        let block = world.get_block(&self.position);
        SculkSensorBlock::activate(
            world,
            &self.position,
            block,
            strength_for_distance(arrived.distance, self.range()),
            frequency(arrived.event),
        );
    }
}

impl GameEventListener for VibrationSensor {
    fn listener_position(&self) -> Vector3<f64> {
        self.position.to_centered_f64()
    }

    fn listener_radius(&self) -> i32 {
        self.range()
    }

    fn handle_game_event(
        &self,
        world: &World,
        event: GameEvent,
        context: &GameEventContext<'_>,
        source_position: Vector3<f64>,
    ) -> bool {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.current.is_some()
            || !is_valid_vibration(&tag::GameEvent::MINECRAFT_VIBRATIONS, event, context)
            || !self.can_receive(world, BlockPos::floored_v(source_position), event)
            || is_occluded(world, source_position, self.listener_position())
        {
            return false;
        }
        let now = world.get_world_age();
        let distance = source_position.squared_distance_to_vec(&self.listener_position());
        let vibration = Vibration {
            event,
            distance: distance.sqrt() as f32,
            origin: source_position,
        };
        let replaces = state.candidate.is_none_or(|(held, tick)| {
            tick == now
                && (vibration.distance < held.distance
                    || (vibration.distance == held.distance
                        && frequency(event) > frequency(held.event)))
        });
        if replaces {
            state.candidate = Some((vibration, now));
        }
        true
    }
}
