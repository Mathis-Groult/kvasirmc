//! What a flying arrow does each tick, and what happens when it hits.

use super::{ArrowEntity, get_hit_face};
use crate::entity::projectile::{ProjectileHit, calculate_ray_intersection};
use crate::entity::{Entity, EntityBase, living::LivingEntity};
use crate::world::World;
use pumpkin_data::damage::DamageType;
use pumpkin_data::data_component_impl::PotionDurationScaleImpl;
use pumpkin_data::entity::EntityType;
use pumpkin_data::particle::Particle;
use pumpkin_data::sound::{Sound, SoundCategory};
use pumpkin_protocol::IdOr;
use pumpkin_protocol::java::client::play::CSoundEffect;
use pumpkin_util::math::boundingbox::BoundingBox;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector3::Vector3;
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// Where an arrow was and is after one flying tick.
pub(super) struct Flight {
    pub(super) start_pos: Vector3<f64>,
    pub(super) new_pos: Vector3<f64>,
    pub(super) velocity: Vector3<f64>,
}

/// What a flying arrow does each tick, and what happens when it hits.
impl ArrowEntity {
    /// Puts the arrow out in water or rain, burns it down, lights it in lava or fire.
    /// Returns whether it is in water and whether it burns.
    pub(super) fn tick_fire(&self, entity: &Entity, world: &World) -> (bool, bool) {
        let mut fire_ticks = entity.fire_ticks.load(Ordering::Relaxed);
        let touching_water = entity.touching_water.load(Ordering::Relaxed);
        let in_water = touching_water || entity.is_in_water();
        let block_pos = entity.block_pos.load();
        let in_rain = world.is_raining_at(&block_pos);

        if in_water || in_rain {
            if entity.is_on_fire() || self.is_flame.load(Ordering::Relaxed) {
                entity.extinguish();
                self.is_flame.store(false, Ordering::Relaxed);
            }
        } else if fire_ticks > 0 {
            fire_ticks -= 1;
            entity.fire_ticks.store(fire_ticks, Ordering::Relaxed);
            if fire_ticks <= 0 && self.is_flame.load(Ordering::Relaxed) {
                self.is_flame.store(false, Ordering::Relaxed);
                entity.set_on_fire(false);
            }
        }

        // Check if arrow enters lava or fire block
        let current_block = world.get_block(&block_pos);
        if current_block == &pumpkin_data::Block::LAVA {
            entity.set_on_fire_for(15.0);
            self.is_flame.store(true, Ordering::Relaxed);
        } else if current_block == &pumpkin_data::Block::FIRE
            || current_block == &pumpkin_data::Block::SOUL_FIRE
        {
            entity.set_on_fire_for(8.0);
            self.is_flame.store(true, Ordering::Relaxed);
        }

        let is_on_fire = entity.is_on_fire() || self.is_flame.load(Ordering::Relaxed);
        entity.set_on_fire(is_on_fire);
        (in_water, is_on_fire)
    }

    /// A stuck arrow: falls again when its block is gone, and despawns after a while.
    pub(super) fn tick_stuck(&self, entity: &Entity, world: &World) {
        // Check if the block we are stuck into was broken / turned to air
        let last_pos = *self
            .last_block_pos
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(pos) = last_pos {
            let block = world.get_block(&pos);
            if block.is_air() {
                self.in_ground.store(false, Ordering::Relaxed);
                entity
                    .set_synced_data(pumpkin_data::tracked_data::abstract_arrow::IN_GROUND, false);
                let mut vel = entity.velocity.load();
                vel.x *= rand::random::<f64>() * 0.2;
                vel.y *= rand::random::<f64>() * 0.2;
                vel.z *= rand::random::<f64>() * 0.2;
                entity.velocity.store(vel);
            }
        }

        // Increment in-ground time and life
        let _in_ground_time = self.in_ground_time.fetch_add(1, Ordering::Relaxed);
        let life = self.life.fetch_add(1, Ordering::Relaxed);

        // Despawn after enough time
        if life >= Self::DESPAWN_TIME {
            entity.remove();
        }
    }

    /// Gravity, drag and the move of one flying tick.
    pub(super) fn fly(entity: &Entity, in_water: bool) -> Flight {
        let start_pos = entity.pos.load();
        let mut velocity = entity.velocity.load();

        // Apply gravity
        velocity.y -= Self::GRAVITY;

        // Apply inertia (air resistance or water drag)
        let inertia = if in_water {
            Self::WATER_INERTIA
        } else {
            Self::AIR_INERTIA
        };
        velocity = velocity.multiply(inertia, inertia, inertia);

        entity.velocity.store(velocity);

        // Update rotation based on velocity
        let len = velocity.horizontal_length();
        entity.set_rotation(
            velocity.x.atan2(velocity.z) as f32 * 57.295_776,
            velocity.y.atan2(len) as f32 * 57.295_776,
        );

        // Move arrow
        let new_pos = start_pos.add(&velocity);
        entity.set_pos(new_pos);
        Flight {
            start_pos,
            new_pos,
            velocity,
        }
    }

    /// Bubbles in water, flames when burning, sparkles when critical.
    pub(super) fn spawn_flight_particles(
        &self,
        world: &World,
        in_water: bool,
        is_on_fire: bool,
        flight: &Flight,
    ) {
        let Flight {
            start_pos,
            new_pos,
            velocity,
        } = *flight;
        let entity = self.get_entity();
        // Spawn particles while arrow is flying
        if in_water {
            for i in 0..4 {
                let factor = 0.25 * f64::from(i);
                world.spawn_particle(
                    Vector3::new(
                        new_pos.x - velocity.x * factor,
                        new_pos.y - velocity.y * factor,
                        new_pos.z - velocity.z * factor,
                    ),
                    Vector3::new(velocity.x as f32, velocity.y as f32, velocity.z as f32),
                    0.0,
                    1,
                    Particle::Bubble,
                );
            }
        }

        if is_on_fire {
            world.spawn_particle(
                entity.pos.load(),
                Vector3::new(0.0f32, 0.0f32, 0.0f32),
                0.0,
                1,
                Particle::Flame,
            );
        }

        if self.is_critical.load(Ordering::Relaxed) {
            for i in 0..4 {
                let factor = f64::from(i) / 4.0;
                world.spawn_particle(
                    Vector3::new(
                        start_pos.x + velocity.x * factor,
                        start_pos.y + velocity.y * factor,
                        start_pos.z + velocity.z * factor,
                    ),
                    Vector3::new(
                        -velocity.x as f32,
                        (-velocity.y + 0.2) as f32,
                        -velocity.z as f32,
                    ),
                    0.0,
                    1,
                    Particle::Crit,
                );
            }
        }
    }

    /// The first block or entity the arrow's path crosses this tick.
    pub(super) fn find_hit(
        &self,
        entity: &Entity,
        world: &World,
        flight: &Flight,
    ) -> Option<ProjectileHit> {
        let Flight {
            start_pos,
            new_pos,
            velocity,
        } = *flight;
        // Check for collisions using raycasting
        let search_box = BoundingBox::new(
            Vector3::new(
                start_pos.x.min(new_pos.x),
                start_pos.y.min(new_pos.y),
                start_pos.z.min(new_pos.z),
            ),
            Vector3::new(
                start_pos.x.max(new_pos.x),
                start_pos.y.max(new_pos.y),
                start_pos.z.max(new_pos.z),
            ),
        )
        .expand(0.3, 0.3, 0.3);

        let mut closest_t = 1.0f64;
        let mut hit = None;

        // Block collisions
        let (block_cols, block_positions) =
            world.get_block_collisions(search_box, self.get_entity());
        for (idx, bb) in block_cols.iter().enumerate() {
            if let Some(t) = calculate_ray_intersection(&start_pos, &velocity, bb)
                && t < closest_t
            {
                closest_t = t;

                // Map back to block pos
                for (len, pos) in &block_positions {
                    if idx < *len {
                        let hit_pos = start_pos.add(&velocity.multiply(t, t, t));
                        hit = Some(ProjectileHit::Block {
                            pos: *pos,
                            face: get_hit_face(hit_pos, *pos),
                            hit_pos,
                            normal: velocity.normalize().multiply(-1.0, -1.0, -1.0),
                        });
                        break;
                    }
                }
            }
        }

        // Entity collisions
        let candidates = world.get_entities_at_box(&search_box);
        for cand in candidates {
            if self.should_skip_collision(entity, &cand) {
                continue;
            }

            let ebb = cand.get_entity().bounding_box.load().expand(0.3, 0.3, 0.3);
            if let Some(t) = calculate_ray_intersection(&start_pos, &velocity, &ebb)
                && t < closest_t
            {
                closest_t = t;
                let hit_pos = start_pos.add(&velocity.multiply(t, t, t));
                hit = Some(ProjectileHit::Entity {
                    entity: cand.clone(),
                    hit_pos,
                    normal: velocity.normalize().multiply(-1.0, -1.0, -1.0),
                });
            }
        }

        hit
    }

    /// Hands a hit on to `on_hit`, unless the arrow has used up its hits.
    pub(super) fn resolve_hit(&self, caller: &dyn EntityBase, hit: ProjectileHit) {
        match hit {
            ProjectileHit::Block { .. } => {
                if self.has_hit.swap(true, Ordering::SeqCst) {
                    return;
                }
                caller.on_hit(hit);
            }
            ProjectileHit::Entity { .. } => {
                let pierce = self.pierce_level.load(Ordering::Relaxed);
                let pierced_len = self
                    .pierced_entities
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .len();
                if pierced_len >= pierce as usize && self.has_hit.swap(true, Ordering::SeqCst) {
                    return;
                }
                caller.on_hit(hit);
            }
        }
    }

    /// The arrow stops in a block, tells the client, and forgets what it pierced.
    pub(super) fn stick_into_block(
        &self,
        entity: &Entity,
        world: &Arc<World>,
        pos: BlockPos,
        hit_pos: Vector3<f64>,
    ) {
        // Arrow hit a block - stick into it
        self.in_ground.store(true, Ordering::Relaxed);
        self.shake_time.store(7, Ordering::Relaxed);
        *self
            .last_block_pos
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(pos);

        let block = world.get_block(&pos);
        let state = world.get_block_state(&pos);
        if let Some(server) = world.server.upgrade() {
            world
                .block_registry
                .on_projectile_hit(block, world, self, &pos, state, &hit_pos, &server);
        }

        if block == &pumpkin_data::Block::TARGET
            && let Some(player) = self.owner.get_player(world)
        {
            player.trigger_advancement(
                crate::entity::player::advancement::trigger::AdvancementTrigger::Bullseye,
            );
        }

        // Stop the arrow with slight position offset backwards
        let velocity = entity.velocity.load();
        let norm_dir = Vector3::new(
            velocity.x.signum(),
            velocity.y.signum(),
            velocity.z.signum(),
        );
        let offset = norm_dir.multiply(0.05, 0.05, 0.05);
        entity.set_pos(hit_pos.sub(&offset));
        entity.velocity.store(Vector3::new(0.0, 0.0, 0.0));

        // Notify client that arrow is in ground
        entity.set_synced_data(pumpkin_data::tracked_data::abstract_arrow::IN_GROUND, true);

        Self::play_hit_sound(entity, world, hit_pos);

        // Reset critical flag and pierce level
        self.set_critical(false);
        self.set_pierce_level(0);
        self.pierced_entities
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    /// Vanilla's pitch for the arrow-hit sound.
    pub(super) fn play_hit_sound(entity: &Entity, world: &World, hit_pos: Vector3<f64>) {
        let sound_pitch = 1.2 / (rand::random::<f32>() * 0.2 + 0.9);
        let sound_packet = CSoundEffect::new(
            IdOr::Id(Sound::EntityArrowHit as u16),
            SoundCategory::Neutral,
            &hit_pos,
            1.0,
            sound_pitch,
            0,
        );
        let chunk_pos = entity.chunk_pos.load();
        world.broadcast_to_chunk(chunk_pos, &sound_packet);
    }

    /// Speed times base damage, rounded up, plus a random bonus for a critical arrow.
    pub(super) fn roll_damage(&self, power: f64) -> i32 {
        let mut damage = (power * self.get_base_damage()).ceil() as i32;
        if self.is_critical.load(Ordering::Relaxed) {
            let bonus = (rand::random::<u32>() % (damage.max(0) / 2 + 2) as u32) as i32;
            damage = damage.saturating_add(bonus);
        }
        damage
    }

    /// Pushes the target along the arrow's horizontal heading, `punch` levels strong.
    pub(super) fn punch_target(target: &Entity, punch: u8, velocity: Vector3<f64>) {
        if punch > 0 {
            let norm = Vector3::new(velocity.x, 0.0, velocity.z).normalize();
            let push_scale = f64::from(punch) * 0.6;
            target
                .velocity
                .store(target.velocity.load().add(&Vector3::new(
                    norm.x * push_scale,
                    0.1,
                    norm.z * push_scale,
                )));
        }
    }

    /// The potion effects the arrow carries, and the glow of a spectral one.
    pub(super) fn apply_arrow_effects(&self, living: &LivingEntity, is_spectral: bool) {
        let item_stack = self
            .item_stack
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let scale = item_stack
            .get_data_component::<PotionDurationScaleImpl>()
            .map_or(1.0, |component| component.scale);
        crate::item::potion::PotionContents::apply_effects_to(
            living,
            crate::item::potion::PotionContents::read_potion_effects(&item_stack),
            scale,
            crate::item::potion::PotionApplicationSource::Arrow,
            None,
        );

        if is_spectral {
            living.add_effect(Self::spectral_glowing_effect());
        }
    }

    /// The arrow hits a creature: damage, fire, knock-back, sound, effects, piercing.
    pub(super) fn hit_entity(
        &self,
        entity: &Entity,
        world: &World,
        target: &Arc<dyn EntityBase>,
        hit_pos: Vector3<f64>,
    ) {
        let target_entity_id = target.get_entity().entity_id;
        self.pierced_entities
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(target_entity_id);

        let velocity = entity.velocity.load();
        let damage = self.roll_damage(velocity.length());

        let is_enderman =
            target.get_entity().entity_type == &pumpkin_data::entity::EntityType::ENDERMAN;
        let is_on_fire = entity.is_on_fire() || self.is_flame.load(Ordering::Relaxed);
        if is_on_fire && !is_enderman {
            target.get_entity().set_on_fire_for(5.0);
        }

        let punch = self.punch_level.load(Ordering::Relaxed);
        let is_spectral = entity.entity_type.id == EntityType::SPECTRAL_ARROW.id;
        let pierce = self.pierce_level.load(Ordering::Relaxed);

        let owner_entity = self.owner.get(world);

        let damage_succeeded = target.damage_with_context(
            target.as_ref(),
            damage as f32,
            DamageType::ARROW,
            Some(hit_pos),
            Some(self),
            // Like vanilla, an ownerless arrow is its own cause.
            Some(owner_entity.as_deref().unwrap_or(self)),
        );

        if let Some(living) = target.get_living_entity() {
            Self::punch_target(target.get_entity(), punch, velocity);

            Self::play_hit_sound(entity, world, hit_pos);

            if Self::should_apply_post_hurt_effects(damage_succeeded) {
                self.apply_arrow_effects(living, is_spectral);
            }
        }

        // Check pierce level
        let pierced_count = self
            .pierced_entities
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len();
        if pierced_count > pierce as usize {
            entity.remove();
        }
    }
}
