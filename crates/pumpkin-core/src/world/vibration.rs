//! What every vibration listener (sculk sensors, allays, ...) shares, from vanilla 26.3
//! `VibrationSystem` and `VibrationSystem$User`.

use pumpkin_data::{BlockDirection, game_event::GameEvent, tag, tag::Taggable};
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};

use super::{World, game_event::GameEventContext};

/// `VibrationSystem.VIBRATION_FREQUENCY_FOR_EVENT`; 0 means the event makes no vibration.
#[must_use]
pub fn frequency(event: GameEvent) -> u8 {
    match event.name() {
        "step" | "swim" | "flap" => 1,
        "projectile_land" | "hit_ground" | "splash" | "bounce" => 2,
        "item_interact_finish" | "projectile_shoot" | "instrument_play" => 3,
        "entity_action" | "elytra_glide" | "unequip" => 4,
        "entity_dismount" | "equip" => 5,
        "entity_interact" | "shear" | "entity_mount" => 6,
        "entity_damage" => 7,
        "drink" | "eat" => 8,
        "container_close" | "block_close" | "block_deactivate" | "block_detach" => 9,
        "container_open" | "block_open" | "block_activate" | "block_attach" | "prime_fuse"
        | "note_block_play" => 10,
        "block_change" => 11,
        "block_destroy" | "fluid_pickup" => 12,
        "block_place" | "fluid_place" => 13,
        "entity_place" | "lightning_strike" | "teleport" => 14,
        "entity_die" | "explode" => 15,
        _ => 0,
    }
}

/// `VibrationSystem.getRedstoneStrengthForDistance`.
#[must_use]
pub fn strength_for_distance(distance: f32, range: i32) -> u8 {
    let fraction = f64::from(distance) / f64::from(range);
    (15 - (fraction * 15.0).floor() as i32).max(1) as u8
}

/// `User.isValidVibration` without the affected-state part: the event is one the listener hears
/// and its source does not hide it.
#[must_use]
pub fn is_valid_vibration(
    listenable: &tag::Tag,
    event: GameEvent,
    context: &GameEventContext<'_>,
) -> bool {
    if !listenable.0.contains(&event.name()) {
        return false;
    }
    let Some(source) = context.source_entity else {
        return true;
    };
    let ignored_while_sneaking = tag::GameEvent::MINECRAFT_IGNORE_VIBRATIONS_SNEAKING
        .0
        .contains(&event.name());
    !source.is_spectator()
        && (!source.get_entity().is_sneaking() || !ignored_while_sneaking)
        && !source.dampens_vibrations()
}

/// `VibrationSystem.Listener.isOccluded`: wool-like blocks must close all six lines.
#[must_use]
pub fn is_occluded(world: &World, origin: Vector3<f64>, destination: Vector3<f64>) -> bool {
    let from = BlockPos::floored_v(origin).to_centered_f64();
    let to = BlockPos::floored_v(destination).to_centered_f64();
    BlockDirection::all().iter().all(|direction| {
        let nudged_source = from + direction.to_offset().to_f64() * f64::from(1.0e-5f32);
        pumpkin_world::clip::traverse_blocks(nudged_source, to, |pos| {
            world
                .get_block(pos)
                .has_tag(&tag::Block::MINECRAFT_OCCLUDES_VIBRATION_SIGNALS)
                .then_some(())
        })
        .is_some()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strength_falls_with_distance() {
        assert_eq!(strength_for_distance(0.0, 8), 15);
        assert_eq!(strength_for_distance(4.0, 8), 8);
        assert_eq!(strength_for_distance(8.0, 8), 1);
        assert_eq!(strength_for_distance(1.0, 16), 15);
    }

    #[test]
    fn frequencies_follow_the_vanilla_table() {
        assert_eq!(frequency(GameEvent::from_name("step").unwrap()), 1);
        assert_eq!(frequency(GameEvent::from_name("block_place").unwrap()), 13);
        assert_eq!(frequency(GameEvent::from_name("explode").unwrap()), 15);
    }
}
