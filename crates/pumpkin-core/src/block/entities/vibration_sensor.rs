//! The vibration listener inside a (calibrated) sculk sensor: vanilla 26.3
//! `VibrationSystem.Listener`/`Ticker` with `SculkSensorBlockEntity$VibrationUser`.

use std::sync::{Arc, Mutex, PoisonError};

use pumpkin_data::{game_event::GameEvent, tag};
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};

use crate::block::blocks::redstone::sculk_sensor::SculkSensorBlock;
use crate::world::World;
use crate::world::game_event::{GameEventContext, GameEventListener};
use crate::world::vibration::{frequency, is_occluded, is_valid_vibration, strength_for_distance};

/// A vibration heard by the sensor and not yet delivered.
#[derive(Clone, Copy)]
struct Vibration {
    event: GameEvent,
    distance: f32,
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
            state: Mutex::new(State { candidate: None, current: None }),
        }
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
                state.current = Some((vibration, vibration.distance.floor() as i32));
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
