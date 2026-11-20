use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::{
    entity::{Entity, EntityBase},
    server::Server,
};
use pumpkin_data::effect::StatusEffect;
use pumpkin_util::math::boundingbox::BoundingBox;
use pumpkin_util::math::vector3::Vector3;

type EffectEntry = (&'static StatusEffect, i32, u8, bool, bool, bool);
use pumpkin_data::item_stack::ItemStack;

#[derive(Clone)]
struct ParticleMeta {
    particle_id: pumpkin_protocol::codec::var_int::VarInt,
    data: Box<[u8]>,
}

impl pumpkin_protocol::java::client::play::MetadataSerializer for ParticleMeta {
    fn write_metadata(
        &self,
        writer: &mut impl std::io::Write,
        _version: &pumpkin_util::version::JavaMinecraftVersion,
    ) -> Result<(), pumpkin_protocol::ser::WritingError> {
        use pumpkin_protocol::ser::NetworkWriteExt;
        writer.write_var_int(&self.particle_id)?;
        writer.write_slice(&self.data)
    }
}

/// The effect cloud entity that is spawned where a lingering potion lands.
pub struct AreaEffectCloudEntity {
    pub entity: Entity,
    /// Stored potion item stack (may be empty) to read effects from.
    pub item_stack: Mutex<ItemStack>,
    /// Active potion effects as tuples: (`StatusEffect`, `duration_ticks`, `amplifier`, `ambient`, `show_particles`, `show_icon`)
    pub effects: Mutex<Vec<EffectEntry>>,
    pub radius: Mutex<f32>,
    pub duration: Mutex<i32>,
    pub age: Mutex<i32>,
    /// ticks between reapplications to the same entity
    pub reapplication_delay: Mutex<i32>,
    /// map of `entity_id` -> ticks remaining until that entity can be affected again
    pub reapplication_map: Mutex<HashMap<i32, i32>>,
    /// linear radius change per tick
    pub radius_on_tick: Mutex<f32>,
    /// radius change when an entity is affected
    pub radius_on_use: Mutex<f32>,
    /// duration change (ticks) when an entity is affected
    pub duration_on_use: Mutex<i32>,
    /// ticks to wait before the cloud becomes active and applies effects (grace period)
    pub wait_time: Mutex<i32>,
}

impl AreaEffectCloudEntity {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(entity: Entity) -> Arc<dyn EntityBase> {
        entity
            .no_physics
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let cloud = Self {
            entity,
            item_stack: Mutex::new(ItemStack::new(0, &pumpkin_data::item::Item::GLASS_BOTTLE)),
            effects: Mutex::new(Vec::new()),
            radius: Mutex::new(3.0),
            duration: Mutex::new(600), // default for lingering potions
            age: Mutex::new(0),
            reapplication_delay: Mutex::new(20),
            reapplication_map: Mutex::new(HashMap::new()),
            radius_on_tick: Mutex::new(-3.0 / 600.0), // default for lingering potions
            radius_on_use: Mutex::new(0.0),
            duration_on_use: Mutex::new(0),
            wait_time: Mutex::new(20),
        };

        Arc::new(cloud)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create(
        entity: Entity,
        item_stack: ItemStack,
        effects_in: Vec<EffectEntry>,
        duration_in: i32,
        radius_in: f32,
        reapplication_delay_in: i32,
        wait_time_in: i32,
        radius_on_use_in: f32,
        duration_on_use_in: i32,
    ) -> Arc<dyn EntityBase> {
        entity
            .no_physics
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let cloud = Self {
            entity,
            item_stack: Mutex::new(item_stack),
            effects: Mutex::new(effects_in),
            radius: Mutex::new(radius_in),
            duration: Mutex::new(duration_in),
            age: Mutex::new(0),
            reapplication_delay: Mutex::new(reapplication_delay_in),
            reapplication_map: Mutex::new(HashMap::new()),
            radius_on_tick: Mutex::new(-radius_in / (duration_in as f32).max(1.0)),
            radius_on_use: Mutex::new(radius_on_use_in),
            duration_on_use: Mutex::new(duration_on_use_in),
            wait_time: Mutex::new(wait_time_in),
        };

        Arc::new(cloud)
    }
}

/// The value behind a mutex, even if another thread panicked holding it.
fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl AreaEffectCloudEntity {
    /// Counts one more tick of age; removes the cloud and returns `false` when it is too old.
    fn grow_older(&self) -> bool {
        let mut age = locked(&self.age);
        *age += 1;
        let duration = *locked(&self.duration);
        if *age > duration {
            self.entity.remove();
            return false;
        }
        true
    }

    /// Changes the radius by its per-tick amount and tells clients; removes the cloud and
    /// returns `false` when nothing is left of it.
    fn shrink_over_time(&self) -> bool {
        let mut radius = locked(&self.radius);
        *radius += *locked(&self.radius_on_tick);
        let current_radius = *radius;
        if current_radius <= 0.0 {
            self.entity.remove();
            return false;
        }

        drop(radius);
        self.entity.set_synced_data(
            pumpkin_data::tracked_data::area_effect_cloud::RADIUS,
            current_radius,
        );
        true
    }

    /// Applies the effects to every creature in reach that is not waiting out its delay.
    fn affect_nearby(&self) {
        let pos = self.entity.pos.load();
        let r = f64::from(*locked(&self.radius));
        let aabb = BoundingBox::new(
            Vector3::new(pos.x - r, pos.y - r, pos.z - r),
            Vector3::new(pos.x + r, pos.y + r, pos.z + r),
        );
        let world = self.entity.world.load();

        let mut candidates = world.get_entities_at_box(&aabb);
        for p in world.get_players_at_box(&aabb) {
            candidates.push(p.clone() as Arc<dyn EntityBase>);
        }

        for cand in candidates {
            // Skip self and other `AreaEffectCloud` entities
            if cand.get_entity().entity_id == self.get_entity().entity_id
                || *cand.get_entity().entity_type
                    == pumpkin_data::entity::EntityType::AREA_EFFECT_CLOUD
            {
                continue;
            }
            if !self.affect(&cand, pos) {
                return;
            }
        }
    }

    /// Affects one creature, then pays the cloud's price for it. Returns `false` when that
    /// price used the cloud up.
    fn affect(&self, cand: &Arc<dyn EntityBase>, pos: Vector3<f64>) -> bool {
        let ent_id = cand.get_entity().entity_id;
        if locked(&self.reapplication_map).contains_key(&ent_id) {
            return true;
        }

        let radius_f = f64::from(*locked(&self.radius));
        let pos_e = cand.get_entity().pos.load();
        let dx = pos_e.x - pos.x;
        let dy = pos_e.y - pos.y;
        let dz = pos_e.z - pos.z;
        let dist = (dx * dx + dy * dy + dz * dz).sqrt();
        if dist > radius_f {
            return true;
        }
        let scale = 1.0f32 - (dist as f32 / radius_f as f32);

        let effs_clone = locked(&self.effects).clone();
        // Only living entities can receive effects, and only if something would change
        let Some(living) = cand.get_living_entity() else {
            return true;
        };
        let will_apply = effs_clone.iter().any(|(eff, _, _, _, _, _)| {
            // Instant effects always apply
            eff.id == StatusEffect::INSTANT_DAMAGE.id
                || eff.id == StatusEffect::INSTANT_HEALTH.id
                || !living.has_effect(eff)
        });
        if !will_apply {
            return true;
        }

        crate::item::potion::PotionContents::apply_effects_to(
            living,
            effs_clone,
            scale,
            crate::item::potion::PotionApplicationSource::AreaEffectCloud,
            Some(crate::entity::living::EffectSource {
                direct: self,
                owner: None,
            }),
        );

        let delay = *locked(&self.reapplication_delay);
        locked(&self.reapplication_map).insert(ent_id, delay);
        self.pay_for_use()
    }

    /// The radius and the duration change by their per-use amounts. Returns `false` when
    /// that removed the cloud.
    fn pay_for_use(&self) -> bool {
        let radius_on_use = *locked(&self.radius_on_use);
        if radius_on_use != 0.0 {
            let mut radius_lock = locked(&self.radius);
            *radius_lock += radius_on_use;
            let current_radius = *radius_lock;
            drop(radius_lock);
            if current_radius < 0.5 {
                self.entity.remove();
                return false;
            }
            self.entity.set_synced_data(
                pumpkin_data::tracked_data::area_effect_cloud::RADIUS,
                current_radius,
            );
        }

        let duration_on_use = *locked(&self.duration_on_use);
        if duration_on_use != 0 {
            let mut duration_lock = locked(&self.duration);
            if *duration_lock != -1 {
                *duration_lock += duration_on_use;
                if *duration_lock <= 0 {
                    drop(duration_lock);
                    self.entity.remove();
                    return false;
                }
            }
        }
        true
    }
}

impl EntityBase for AreaEffectCloudEntity {
    fn init_data_tracker(&self) {
        // Send initial radius and particle (color) so clients render correctly
        let radius = *self
            .radius
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        // Compute particle color
        let stack = self
            .item_stack
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let effects = self
            .effects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();

        // Use ARGB format
        let mut color: i32 = (0xFFi32 << 24) | 0x385dc6; // default water-like color

        if let Some(pc) =
            stack.get_data_component::<pumpkin_data::data_component_impl::PotionContentsImpl>()
        {
            if let Some(c) = pc.custom_color {
                color = c | (0xFFi32 << 24);
            } else if !effects.is_empty() {
                let mut r_sum = 0.0f32;
                let mut g_sum = 0.0f32;
                let mut b_sum = 0.0f32;
                let count = effects.len() as f32;
                for (eff, _, _, _, _, _) in &effects {
                    let c = eff.color;
                    r_sum += ((c >> 16) & 0xFF) as f32;
                    g_sum += ((c >> 8) & 0xFF) as f32;
                    b_sum += (c & 0xFF) as f32;
                }
                let r = (r_sum / count) as i32;
                let g = (g_sum / count) as i32;
                let b = (b_sum / count) as i32;
                color = (0xFFi32 << 24) | (r << 16) | (g << 8) | b;
            }
        } else if !effects.is_empty() {
            let mut r_sum = 0.0f32;
            let mut g_sum = 0.0f32;
            let mut b_sum = 0.0f32;
            let count = effects.len() as f32;
            for (eff, _, _, _, _, _) in &effects {
                let c = eff.color;
                r_sum += ((c >> 16) & 0xFF) as f32;
                g_sum += ((c >> 8) & 0xFF) as f32;
                b_sum += (c & 0xFF) as f32;
            }
            let r = (r_sum / count) as i32;
            let g = (g_sum / count) as i32;
            let b = (b_sum / count) as i32;
            color = (0xFFi32 << 24) | (r << 16) | (g << 8) | b;
        }

        // Build raw particle option bytes for ENTITY_EFFECT
        let data_bytes = color.to_be_bytes();

        let meta = ParticleMeta {
            particle_id: pumpkin_protocol::codec::var_int::VarInt(
                pumpkin_data::particle::Particle::EntityEffect as i32,
            ),
            data: Box::new(data_bytes),
        };

        self.entity.set_synced_data(
            pumpkin_data::tracked_data::area_effect_cloud::PARTICLE,
            meta,
        );

        self.entity.set_synced_data(
            pumpkin_data::tracked_data::area_effect_cloud::RADIUS,
            radius,
        );

        // Initial waiting flag
        let wait_time = *self
            .wait_time
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let is_waiting = 0 < wait_time;
        self.entity.set_synced_data(
            pumpkin_data::tracked_data::area_effect_cloud::WAITING,
            is_waiting,
        );
    }

    fn tick(&self, _caller: &dyn EntityBase, _server: &Server) {
        if !self.grow_older() {
            return;
        }

        let age = *locked(&self.age);
        let wait_time = *locked(&self.wait_time);

        // When the waiting period ends, notify clients so they render full particles
        if age == wait_time && wait_time > 0 {
            self.entity.set_synced_data(
                pumpkin_data::tracked_data::area_effect_cloud::WAITING,
                false,
            );
        }

        // Respect waiting/grace period
        if age < wait_time {
            return;
        }

        if !self.shrink_over_time() {
            return;
        }

        // Tick down reapplication map
        locked(&self.reapplication_map).retain(|_, v| {
            *v -= 1;
            *v > 0
        });

        self.affect_nearby();
    }

    fn get_entity(&self) -> &Entity {
        &self.entity
    }

    fn get_living_entity(&self) -> Option<&crate::entity::living::LivingEntity> {
        None
    }

    fn cast_any(&self) -> &dyn std::any::Any {
        self
    }
}
