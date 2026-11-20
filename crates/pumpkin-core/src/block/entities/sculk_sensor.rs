use super::{
    BlockEntity,
    vibration_sensor::{ListenerKind, VibrationSensor},
};
use crate::world::{World, game_event::GameEventListener};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_util::math::position::BlockPos;
use std::sync::Arc;
use std::sync::Mutex;

pub struct SculkSensorBlockEntity {
    pub position: BlockPos,
    pub last_vibration_frequency: Mutex<i32>,
    sensor: VibrationSensor,
}

impl BlockEntity for SculkSensorBlockEntity {
    fn resource_location(&self) -> &'static str {
        Self::ID
    }

    fn get_position(&self) -> BlockPos {
        self.position
    }

    fn from_nbt(nbt: &pumpkin_nbt::compound::NbtCompound, position: BlockPos) -> Self
    where
        Self: Sized,
    {
        let last_vibration_frequency = nbt.get_int("last_vibration_frequency").unwrap_or(0);
        Self {
            position,
            last_vibration_frequency: Mutex::new(last_vibration_frequency),
            sensor: VibrationSensor::from_nbt(
                position,
                ListenerKind::Sensor,
                nbt.get_compound("listener"),
            ),
        }
    }

    fn write_nbt(&self, nbt: &mut NbtCompound) {
        if let Ok(last_vibration_frequency) = self.last_vibration_frequency.lock() {
            nbt.put_int("last_vibration_frequency", *last_vibration_frequency);
        }
        nbt.put_compound("listener", self.sensor.write_nbt());
    }

    fn chunk_data_nbt(&self) -> Option<NbtCompound> {
        let mut nbt = NbtCompound::new();
        nbt.put_int(
            "last_vibration_frequency",
            *self.last_vibration_frequency.try_lock().ok()?,
        );
        Some(nbt)
    }

    fn tick(&self, world: &Arc<World>) {
        self.sensor.tick(world);
    }

    fn game_event_listener(&self) -> Option<&dyn GameEventListener> {
        Some(&self.sensor)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl SculkSensorBlockEntity {
    pub const ID: &'static str = "minecraft:sculk_sensor";
    #[must_use]
    pub const fn new(position: BlockPos) -> Self {
        Self {
            position,
            last_vibration_frequency: Mutex::new(0),
            sensor: VibrationSensor::new(position, ListenerKind::Sensor),
        }
    }
}
