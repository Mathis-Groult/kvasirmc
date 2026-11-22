//! The stages of `ThrownItemEntity::process_tick`, after vanilla `ThrowableProjectile.tick` and
//! `Projectile` hit detection: movement, collision search, hit dispatch.

use super::{ProjectileHit, ThrownItemEntity, calculate_ray_intersection, get_hit_face};
use crate::entity::EntityBase;
use crate::world::World;
use pumpkin_protocol::java::client::play::CEntityVelocity;
use pumpkin_util::math::boundingbox::BoundingBox;
use pumpkin_util::math::vector3::Vector3;
use std::sync::{Arc, atomic::Ordering};

/// One tick's move: where it started, the step, and the box searched for collisions.
pub struct Sweep {
    start: Vector3<f64>,
    delta: Vector3<f64>,
    search_box: BoundingBox,
}

impl Sweep {
    fn new(start: Vector3<f64>, delta: Vector3<f64>) -> Self {
        let end = start.add(&delta);
        let search_box = BoundingBox::new(
            Vector3::new(start.x.min(end.x), start.y.min(end.y), start.z.min(end.z)),
            Vector3::new(start.x.max(end.x), start.y.max(end.y), start.z.max(end.z)),
        )
        .expand(0.3, 0.3, 0.3);
        Self {
            start,
            delta,
            search_box,
        }
    }

    /// The point `t` of the way along the step.
    fn at(&self, t: f64) -> Vector3<f64> {
        self.start.add(&self.delta.multiply(t, t, t))
    }

    fn normal(&self) -> Vector3<f64> {
        self.delta.normalize().multiply(-1.0, -1.0, -1.0)
    }
}

impl ThrownItemEntity {
    /// Gravity and drag, the move, and the velocity packet.
    pub(super) fn advance(&self, world: &World) -> Sweep {
        let entity = self.get_entity();
        let mut velocity = entity.velocity.load();
        velocity.y -= self.get_gravity();

        let inertia = if entity.touching_water.load(Ordering::Relaxed) {
            0.8
        } else {
            0.99
        };
        velocity = velocity.multiply(inertia, inertia, inertia);
        entity.velocity.store(velocity);

        let sweep = Sweep::new(entity.pos.load(), velocity);
        entity.set_pos(sweep.start.add(&sweep.delta));

        let packet = CEntityVelocity::new(entity.entity_id.into(), velocity);
        world.broadcast_to_chunk(entity.chunk_pos.load(), &packet);
        sweep
    }

    /// The nearest block or entity on the step, if any.
    pub(super) fn find_hit(
        &self,
        world: &Arc<World>,
        caller: &dyn EntityBase,
        sweep: &Sweep,
    ) -> Option<ProjectileHit> {
        let (closest_t, block_hit) = Self::block_hit(world, caller, sweep);
        self.entity_hit(world, sweep, closest_t, block_hit)
    }

    /// The nearest block on the step, with its distance (1.0 when none).
    fn block_hit(
        world: &Arc<World>,
        caller: &dyn EntityBase,
        sweep: &Sweep,
    ) -> (f64, Option<ProjectileHit>) {
        let mut closest_t = 1.0f64;
        let mut hit = None;
        let (block_cols, block_positions) = world.get_block_collisions(sweep.search_box, caller);
        for (idx, bb) in block_cols.iter().enumerate() {
            if let Some(t) = calculate_ray_intersection(&sweep.start, &sweep.delta, bb)
                && t < closest_t
            {
                closest_t = t;
                // Map back to block pos
                for (len, pos) in &block_positions {
                    if idx < *len {
                        let hit_pos = sweep.at(t);
                        hit = Some(ProjectileHit::Block {
                            pos: *pos,
                            face: get_hit_face(hit_pos, *pos),
                            hit_pos,
                            normal: sweep.normal(),
                        });
                        break;
                    }
                }
            }
        }
        (closest_t, hit)
    }

    /// An entity nearer than `closest_t` replaces the block hit.
    fn entity_hit(
        &self,
        world: &Arc<World>,
        sweep: &Sweep,
        mut closest_t: f64,
        mut hit: Option<ProjectileHit>,
    ) -> Option<ProjectileHit> {
        let entity = self.get_entity();
        for cand in world.get_entities_at_box(&sweep.search_box) {
            if self.should_skip_collision(world, entity, &cand) {
                continue;
            }

            let ebb = cand.get_entity().bounding_box.load().expand(0.3, 0.3, 0.3);
            if let Some(t) = calculate_ray_intersection(&sweep.start, &sweep.delta, &ebb)
                && t < closest_t
            {
                closest_t = t;
                hit = Some(ProjectileHit::Entity {
                    entity: cand.clone(),
                    hit_pos: sweep.at(t),
                    normal: sweep.normal(),
                });
            }
        }
        hit
    }

    /// Runs the hit once: block reaction, the projectile's own `on_hit`, then removal.
    pub(super) fn dispatch_hit(
        &self,
        world: &Arc<World>,
        caller: &dyn EntityBase,
        hit: ProjectileHit,
    ) {
        // Ensure hit is only processed once per projectile
        if self.has_hit.swap(true, Ordering::SeqCst) {
            return;
        }

        if let ProjectileHit::Block { pos, hit_pos, .. } = &hit {
            let block = world.get_block(pos);
            let state = world.get_block_state(pos);
            if let Some(server) = world.server.upgrade() {
                world
                    .block_registry
                    .on_projectile_hit(block, world, caller, pos, state, hit_pos, &server);
            }
        }

        // Just trigger hit effects and remove
        caller.on_hit(hit);
        self.get_entity().remove();
    }
}
