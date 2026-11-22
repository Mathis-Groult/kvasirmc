mod stages;

use super::EnderDragonPhase;
use crate::entity::boss::ender_dragon::{DEATH_TIMER_MAX, EnderDragonEntity};
use crate::entity::experience_orb::ExperienceOrbEntity;
use pumpkin_util::math::vector3::Vector3;

pub struct DyingPhase;

impl super::Phase for DyingPhase {
    fn get_type(&self) -> EnderDragonPhase {
        EnderDragonPhase::Dying
    }

    fn begin(&self, dragon: &EnderDragonEntity) {
        *dragon
            .target_location
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        *dragon
            .dragon_death_time
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = 0;
    }

    fn get_fly_speed(&self) -> f32 {
        3.0
    }

    fn get_fly_target_location(&self) -> Option<Vector3<f64>> {
        None
    }

    fn tick(&self, dragon: &EnderDragonEntity) {
        let death_time = stages::count_up(dragon);

        let entity = &dragon.mob_entity.living_entity.entity;
        let world = entity.world.load();
        let pos = entity.pos.load();

        let target = stages::podium_target(dragon, &world);
        stages::steer_to_podium(dragon, pos, target);
        stages::play_effects(&world, &pos, death_time);

        let xp_count = stages::xp_total(&world);
        if death_time > 150 && death_time % 5 == 0 {
            ExperienceOrbEntity::spawn(&world, pos, (xp_count as f32 * 0.08) as u32);
        }

        entity.velocity.store(Vector3::new(0.0, 0.1, 0.0));

        if death_time >= DEATH_TIMER_MAX {
            stages::finish(dragon, &world, pos, xp_count);
        }
    }
}
