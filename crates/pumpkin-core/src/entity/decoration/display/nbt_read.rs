//! Vanilla 26.3 `Display.readAdditionalSaveData`: every key is read with its default, so a key
//! missing from the NBT resets the field (a `/data merge` of `{}` returns it to the default).

use std::sync::atomic::Ordering;
use std::sync::{Mutex, PoisonError};

use pumpkin_nbt::compound::NbtCompound;
use pumpkin_util::math::vector3::Vector3;

use super::{DisplayEntity, light};

fn store<T>(slot: &Mutex<T>, value: T) {
    *slot.lock().unwrap_or_else(PoisonError::into_inner) = value;
}

/// `ExtraCodecs.VECTOR3F` / `QUATERNIONF_COMPONENTS`: a list of exactly `N` floats.
fn floats<const N: usize>(compound: &NbtCompound, key: &str) -> Option<[f32; N]> {
    let list = compound.get_list(key)?;
    if list.len() != N {
        return None;
    }
    let mut out = [0.0; N];
    for (slot, tag) in out.iter_mut().zip(list) {
        *slot = tag.extract_float()?;
    }
    Some(out)
}

/// A rotation as `QUATERNIONF_COMPONENTS` decodes it: normalised (a zero quaternion stays as is).
fn normalised(q: [f32; 4]) -> [f32; 4] {
    let length = q.iter().map(|c| c * c).sum::<f32>().sqrt();
    if length == 0.0 {
        q
    } else {
        q.map(|c| c / length)
    }
}

impl DisplayEntity {
    pub fn read_display_nbt(&self, nbt: &NbtCompound) {
        self.read_transformation(nbt.get_compound("transformation"));
        let int = |key: &str| nbt.get_int(key).unwrap_or(0);
        self.interpolation_duration
            .store(int("interpolation_duration"), Ordering::Relaxed);
        self.interpolation_start_delta_ticks
            .store(int("start_interpolation"), Ordering::Relaxed);
        self.teleport_duration
            .store(light::read_teleport_duration(nbt), Ordering::Relaxed);
        let mode = match nbt.get_string("billboard") {
            Some("vertical") => 1,
            Some("horizontal") => 2,
            Some("center") => 3,
            _ => 0,
        };
        self.billboard.store(mode, Ordering::Relaxed);
        let float = |key: &str, default: f32| nbt.get_float(key).unwrap_or(default);
        store(&self.view_range, float("view_range", 1.0));
        store(&self.shadow_radius, float("shadow_radius", 0.0));
        store(&self.shadow_strength, float("shadow_strength", 1.0));
        store(&self.width, float("width", 0.0));
        store(&self.height, float("height", 0.0));
        self.glow_color_override.store(
            nbt.get_int("glow_color_override").unwrap_or(-1),
            Ordering::Relaxed,
        );
        self.brightness
            .store(light::read_brightness(nbt), Ordering::Relaxed);
    }

    /// `Transformation.EXTENDED_CODEC`, decomposed form: all four keys are required and each list
    /// has its exact length, else the whole transformation is `IDENTITY`. (The 16-float matrix
    /// and axis-angle forms are not read yet: card #69 (3).)
    fn read_transformation(&self, transform: Option<&NbtCompound>) {
        let decoded = transform.and_then(|t| {
            Some((
                floats::<3>(t, "translation")?,
                floats::<4>(t, "left_rotation")?,
                floats::<3>(t, "scale")?,
                floats::<4>(t, "right_rotation")?,
            ))
        });
        let (translation, left, scale, right) = decoded.unwrap_or((
            [0.0; 3],
            [0.0, 0.0, 0.0, 1.0],
            [1.0; 3],
            [0.0, 0.0, 0.0, 1.0],
        ));
        store(
            &self.translation,
            Vector3::new(translation[0], translation[1], translation[2]),
        );
        store(&self.scale, Vector3::new(scale[0], scale[1], scale[2]));
        store(&self.left_rotation, normalised(left));
        store(&self.right_rotation, normalised(right));
    }
}

#[cfg(test)]
mod tests {
    use super::normalised;

    #[test]
    fn a_zero_quaternion_stays_zero_and_others_get_unit_length() {
        assert_eq!(normalised([0.0; 4]), [0.0; 4]);
        assert_eq!(normalised([0.0, 0.0, 0.0, 2.0]), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(normalised([0.0, 0.0, 0.0, 1.0]), [0.0, 0.0, 0.0, 1.0]);
    }
}
