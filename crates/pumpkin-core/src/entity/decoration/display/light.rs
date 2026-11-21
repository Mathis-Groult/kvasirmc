//! Vanilla `Display` brightness override and teleport duration as stored in NBT.

use pumpkin_nbt::compound::NbtCompound;

/// Vanilla `Display.NO_BRIGHTNESS_OVERRIDE`.
pub(super) const NO_BRIGHTNESS_OVERRIDE: i32 = -1;

/// `Brightness.CODEC`: both `block` and `sky` are required and within 0..=15, else the whole
/// compound is rejected. Packed as `LightCoordsUtil.pack`: `block << 4 | sky << 20`.
pub(super) fn brightness_from_nbt(compound: &NbtCompound) -> Option<i32> {
    let light = |key: &str| compound.get_int(key).filter(|v| (0..=15).contains(v));
    Some(light("block")? << 4 | light("sky")? << 20)
}

/// The `brightness` key of a display entity; absent or rejected = no override.
pub(super) fn read_brightness(nbt: &NbtCompound) -> i32 {
    nbt.get_compound("brightness")
        .and_then(brightness_from_nbt)
        .unwrap_or(NO_BRIGHTNESS_OVERRIDE)
}

/// `LightCoordsUtil.block` and `.sky` of a packed brightness.
pub(super) const fn unpack(packed: i32) -> (i32, i32) {
    ((packed >> 4) & 0xF, (packed >> 20) & 0xF)
}

/// `teleport_duration`: `getIntOr(.., 0)` clamped to 0..=59.
pub(super) fn read_teleport_duration(nbt: &NbtCompound) -> i32 {
    nbt.get_int("teleport_duration").unwrap_or(0).clamp(0, 59)
}
