//! Water, fluids, fire and freezing.

use super::{Entity, EntityBase, Flag, WATER_PUSH_SPEED, lava_push_speed};
use pumpkin_data::data_component_impl::EquipmentSlot;
use pumpkin_data::fluid::Fluid;
use pumpkin_data::tag::{self, Taggable};
use pumpkin_data::tracked_data;
use pumpkin_data::{damage::DamageType, entity::EntityType};
use pumpkin_protocol::{
    bedrock::client::set_actor_data::{MetadataValue, SyncedActorDataList, entity_data_key},
    codec::var_int::VarInt,
};
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};
use std::collections::BTreeMap;
use std::sync::atomic::Ordering::{self, Relaxed};

impl Entity {
    pub fn set_air_supply(&self, mut air: i32) {
        if air == self.air_supply.load(Relaxed) {
            return;
        }
        if let Some(server) = self.world.load().server.upgrade() {
            let mut event =
                crate::plugin::api::events::entity::entity_air_change::EntityAirChangeEvent::new(
                    self.entity_id,
                    air,
                );
            server.plugin_manager.fire_blocking(&server, &mut event);
            if event.cancelled {
                return;
            }
            air = event.amount;
        }
        self.air_supply.store(air, Relaxed);
        self.set_synced_data(tracked_data::entity::DATA_AIR_SUPPLY_ID, VarInt(air));
    }

    pub(super) fn update_fluid_state(&self, caller: &dyn EntityBase) {
        // A mob whose AI runs slowly (far from players, unseen, or one of a crowd) looks for
        // fluid at that rate too, but only while it was dry: a mob in fluid is pushed by it
        // every tick, as in vanilla.
        if let Some(mob) = caller
            .get_mob()
            .map(super::super::entity::mob::Mob::get_mob_entity)
            && mob.is_throttled()
            && !mob.is_decision_tick()
            && !self.first_tick.load(Ordering::Relaxed)
            && !self.touching_water.load(Ordering::SeqCst)
            && !self.touching_lava.load(Ordering::SeqCst)
        {
            return;
        }
        let world = self.world.load();
        for fluid in self.update_fluid_interaction(caller) {
            world
                .block_registry
                .on_entity_collision_fluid(fluid, caller);
        }
    }

    /// Vanilla `Entity.updateFluidInteraction`: fluid flags and current push only, without the
    /// fluid's `entityInside` effects. Returns the fluids the entity is in.
    pub(super) fn update_fluid_interaction(&self, caller: &dyn EntityBase) -> Vec<&'static Fluid> {
        let is_pushed = caller.is_pushed_by_fluids();
        let mut fluids = BTreeMap::new();

        let water_push = Vector3::default();

        let water_n = 0;

        let lava_push = Vector3::default();

        let lava_n = 0;

        let mut fluid_push = [water_push, lava_push];

        let mut fluid_n = [water_n, lava_n];

        let mut in_fluid = [false, false];

        // The maximum fluid height found

        let mut fluid_height: [f64; 2] = [0.0, 0.0];

        let entity_box = self.bounding_box.load();
        let bounding_box = entity_box.expand(-0.001, -0.001, -0.001);

        let min = bounding_box.min_block_pos();

        let max = bounding_box.max_block_pos();

        let world = self.world.load();

        for x in min.0.x..=max.0.x {
            for y in min.0.y..=max.0.y {
                for z in min.0.z..=max.0.z {
                    let pos = BlockPos::new(x, y, z);

                    let (fluid, state) = world.get_fluid_and_fluid_state(&pos);

                    if fluid.id != Fluid::EMPTY.id {
                        let surface_y =
                            f64::from(world.get_fluid_height(&pos, fluid, &state)) + f64::from(y);

                        if surface_y >= bounding_box.min.y {
                            let marginal_height = surface_y - entity_box.min.y;
                            let i = usize::from(
                                fluid.id == Fluid::FLOWING_LAVA.id || fluid.id == Fluid::LAVA.id,
                            );

                            fluid_height[i] = fluid_height[i].max(marginal_height);

                            in_fluid[i] = true;

                            if !is_pushed {
                                fluids.insert(fluid.id, fluid);

                                continue;
                            }

                            let mut fluid_velo = world.get_fluid_velocity(pos, fluid, &state);

                            if fluid_height[i] < 0.4 {
                                fluid_velo = fluid_velo * fluid_height[i];
                            }

                            fluid_push[i] += fluid_velo;

                            fluid_n[i] += 1;

                            fluids.insert(fluid.id, fluid);
                        }
                    }
                }
            }
        }

        let lava_speed = lava_push_speed(world.dimension.fast_lava);

        self.push_by_fluid(WATER_PUSH_SPEED, fluid_push[0], fluid_n[0]);

        self.push_by_fluid(lava_speed, fluid_push[1], fluid_n[1]);

        let water_height = fluid_height[0];

        let in_water = in_fluid[0];

        if in_water {
            if let Some(living) = caller.get_living_entity() {
                living.fall_distance.store(0.0);
            }

            if !self.touching_water.load(Ordering::SeqCst)
                && !self.first_tick.load(Ordering::Relaxed)
            {
                caller.do_water_splash_effect(caller);
            }
        }

        self.water_height.store(water_height);

        self.touching_water.store(in_water, Ordering::SeqCst);

        let lava_height = fluid_height[1];

        let in_lava = in_fluid[1];

        if in_lava && let Some(living) = caller.get_living_entity() {
            let halved_fall = living.fall_distance.load() / 2.0;

            if halved_fall != 0.0 {
                living.fall_distance.store(halved_fall);
            }
        }

        self.lava_height.store(lava_height);

        self.touching_lava.store(in_lava, Ordering::SeqCst);

        // BTreeMap auto-sorts water before lava as in vanilla
        fluids.into_values().collect()
    }

    pub(super) fn push_by_fluid(&self, speed: f64, mut push: Vector3<f64>, n: usize) {
        if push.length_squared() != 0.0 {
            if n > 0 {
                push = push * (1.0 / (n as f64));
            }

            if self.entity_type != &EntityType::PLAYER {
                push = push.normalize();
            }

            push = push * speed;

            let velo = self.velocity.load();

            if velo.x.abs() < 0.003 && velo.z.abs() < 0.003 && velo.length_squared() < 0.000_020_25
            {
                push = push.normalize() * 0.0045;
            }

            self.velocity.store(velo + push);
        }
    }

    /// Extinguishes this entity.
    pub fn extinguish(&self) {
        self.fire_ticks.store(0, Ordering::Relaxed);
    }

    /// Check if the entity is currently in powder snow.
    ///
    /// The flag is reset at the start of each tick and set while processing
    /// block collisions for the current tick.
    pub fn is_in_powder_snow(&self) -> bool {
        self.is_in_powder_snow.load(Ordering::Relaxed)
    }

    /// Check if this entity type is immune to freezing
    pub fn is_freeze_immune(&self) -> bool {
        self.entity_type
            .has_tag(&tag::EntityType::MINECRAFT_FREEZE_IMMUNE_ENTITY_TYPES)
    }

    /// Mirrors vanilla `LivingEntity#canFreeze`: spectators and entities wearing
    /// freeze-immune wearables (e.g. leather armor) cannot freeze.
    pub(super) fn can_freeze(&self, caller: &dyn EntityBase) -> bool {
        if caller.is_spectator() || self.is_freeze_immune() {
            return false;
        }

        let Some(living) = caller.get_living_entity() else {
            return true;
        };

        if let Ok(equipment) = living.entity_equipment.try_lock() {
            for (slot, stack) in &equipment.equipment {
                if (*slot == EquipmentSlot::HEAD
                    || *slot == EquipmentSlot::CHEST
                    || *slot == EquipmentSlot::LEGS
                    || *slot == EquipmentSlot::FEET)
                    && stack
                        .get_item()
                        .has_tag(&tag::Item::MINECRAFT_FREEZE_IMMUNE_WEARABLES)
                {
                    return false;
                }
            }
        }

        true
    }

    /// Ticks the frozen state of the entity.
    /// In powder snow and freezeable: `frozen_ticks` increases by 1 (up to `MAX_FROZEN_TICKS`)
    /// Otherwise: `frozen_ticks` decreases by 2 (down to 0)
    /// When fully frozen, deals 1 damage every 40 ticks
    pub fn tick_frozen(&self, caller: &dyn EntityBase) {
        let can_freeze = self.can_freeze(caller);
        let in_powder_snow = self.is_in_powder_snow();
        let old_frozen_ticks = self.frozen_ticks.load(Ordering::Relaxed);

        let new_frozen_ticks = if in_powder_snow && can_freeze {
            // Increase frozen ticks when in powder snow
            (old_frozen_ticks + 1).min(Self::MAX_FROZEN_TICKS)
        } else {
            // Vanilla: thaw whenever not in powder snow OR when freezing is prevented
            (old_frozen_ticks - 2).max(0)
        };

        // Only update and send metadata if the value changed
        if new_frozen_ticks != old_frozen_ticks {
            self.frozen_ticks.store(new_frozen_ticks, Ordering::Relaxed);
            let mut bedrock_meta = SyncedActorDataList::new();
            bedrock_meta.set(
                entity_data_key::FREEZING_EFFECT_STRENGTH,
                MetadataValue::Float(new_frozen_ticks as f32),
            );
            self.set_synced_data(
                tracked_data::entity::DATA_TICKS_FROZEN,
                VarInt(new_frozen_ticks),
            );
            self.send_bedrock_actor_data(&bedrock_meta);
        }

        // Vanilla parity: full-freeze damage is tick-phase based.
        if can_freeze
            && new_frozen_ticks >= Self::MAX_FROZEN_TICKS
            && self.tick_count.load(Ordering::Relaxed) % Self::FREEZE_DAMAGE_INTERVAL == 0
        {
            let world = self.world.load_full();
            if world.level_info.load().game_rules.freeze_damage
                && let Some(entity) = world.get_entity_by_id(self.entity_id)
            {
                entity.damage(entity.as_ref(), 1.0, DamageType::FREEZE);
            }
        }
    }

    /// Sets the number of ticks the entity has been frozen.
    pub fn set_frozen_ticks(&self, ticks: i32) {
        let new_frozen_ticks = ticks.clamp(0, Self::MAX_FROZEN_TICKS);
        self.frozen_ticks.store(new_frozen_ticks, Ordering::Relaxed);
        let mut bedrock_meta = SyncedActorDataList::new();
        bedrock_meta.set(
            entity_data_key::FREEZING_EFFECT_STRENGTH,
            MetadataValue::Float(new_frozen_ticks as f32),
        );
        self.set_synced_data(
            tracked_data::entity::DATA_TICKS_FROZEN,
            VarInt(new_frozen_ticks),
        );
        self.send_bedrock_actor_data(&bedrock_meta);
    }

    /// Returns the number of ticks the entity has been frozen.
    pub fn get_frozen_ticks(&self) -> i32 {
        self.frozen_ticks.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn is_in_water(&self) -> bool {
        self.touching_water.load(Ordering::Relaxed)
    }

    pub fn is_in_water_or_rain(&self) -> bool {
        let world = self.world.load();
        let pos = self.block_pos.load();
        self.is_in_water()
            || world.is_raining_at(&pos)
            || world.is_raining_at(&BlockPos::new(
                pos.0.x,
                self.bounding_box.load().max.y.floor() as i32,
                pos.0.z,
            ))
    }

    #[must_use]
    pub fn is_submerged_in_water(&self) -> bool {
        let pos = self.pos.load();
        let eye_y = pos.y + self.get_eye_height();
        let eye_pos = BlockPos::floored(pos.x, eye_y, pos.z);
        let world = self.world.load();
        let (fluid, state) = world.get_fluid_and_fluid_state(&eye_pos);
        fluid.matches_type(&Fluid::WATER)
            && eye_y
                <= f64::from(eye_pos.0.y)
                    + f64::from(world.get_fluid_height(&eye_pos, fluid, &state))
    }

    #[must_use]
    pub fn is_under_water(&self) -> bool {
        self.is_in_water() && self.is_submerged_in_water()
    }

    /// Sets whether the entity is on fire for visual and damage purposes. This is separate from `fire_ticks` which tracks the damage aspect of being on fire.
    pub fn set_on_fire(&self, on_fire: bool) {
        if self.has_visual_fire.load(Ordering::Relaxed) != on_fire {
            self.has_visual_fire.store(on_fire, Ordering::Relaxed);
            self.set_flag(Flag::OnFire, on_fire);
        }
    }

    #[must_use]
    pub fn is_on_fire(&self) -> bool {
        self.fire_ticks.load(Ordering::Relaxed) > 0 || self.has_visual_fire.load(Ordering::Relaxed)
    }

    /// Vanilla `Entity.fireImmune`: the entity type.
    pub fn is_fire_immune(&self) -> bool {
        self.entity_type.fire_immune || self.fire_immune.load(Ordering::Relaxed)
    }
}
