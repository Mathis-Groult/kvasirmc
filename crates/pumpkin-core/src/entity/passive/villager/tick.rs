//! The steps of a villager's tick: timers, trade gossip, merchant updates, home, sleep and
//! golem spawning.

#[allow(clippy::wildcard_imports)]
use super::*;

impl VillagerEntity {
    /// Counts the unhappy and trade-sound timers down.
    pub(super) fn tick_counters(&self) {
        let unhappy_counter = self.unhappy_counter.load(Ordering::Relaxed);
        if unhappy_counter > 0 {
            let unhappy_counter = unhappy_counter - 1;
            self.unhappy_counter
                .store(unhappy_counter, Ordering::Relaxed);
            self.get_entity().set_synced_data(
                tracked_data::villager::UNHAPPY_COUNTER,
                VarInt(unhappy_counter),
            );
        }
        self.trade_sound_cooldown
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |cooldown| {
                (cooldown > 0).then_some(cooldown - 1)
            })
            .ok();
    }

    /// Rewards the last trading player with trading gossip and shows the villager happy.
    pub(super) fn reward_last_trade(&self, world: &World) {
        let last_traded_player = self
            .last_traded_player
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(player_uuid) = last_traded_player {
            let mut gossips = self
                .gossips
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let value = gossips
                .entry(player_uuid)
                .or_default()
                .entry(GossipType::Trading)
                .or_default();
            *value = (*value + 2).min(GossipType::Trading.max_value());
            drop(gossips);
            world.send_entity_status(
                self.get_entity(),
                pumpkin_data::entity::EntityStatus::VillagerHappy,
                ActorEventID::VillagerHappy,
            );
        }
    }

    /// When the merchant update timer runs out (not while trading): the pending level up with
    /// its new trades, and regeneration.
    pub(super) fn finish_merchant_update(&self) {
        if !self.is_trading.load(Ordering::Relaxed)
            && self.merchant_update_timer.load(Ordering::Relaxed) > 0
            && self.merchant_update_timer.fetch_sub(1, Ordering::Relaxed) == 1
        {
            if self
                .increase_profession_level_on_update
                .swap(false, Ordering::Relaxed)
            {
                let mut data = *self
                    .villager_data
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                data.level.0 += 1;
                self.set_villager_data(data);
                self.add_trades(data.profession_enum(), data.level.0);
            }
            self.mob_entity.living_entity.add_effect(Effect {
                effect_type: &StatusEffect::REGENERATION,
                duration: 200,
                amplifier: 0,
                ambient: false,
                show_particles: true,
                show_icon: true,
                blend: false,
            });
        }
    }

    /// Forgets a home that is no longer a bed's head, waking the villager if it slept there.
    pub(super) fn check_home(&self, world: &World) {
        // 1. Bed / Sleeping logic (for all villagers: babies, nitwits, adults)
        let is_sleeping = self.get_entity().pose.load() == EntityPose::Sleeping;

        // Check if current bed is still valid
        if let Some(current_home) = self.get_home_pos() {
            let (block, state) = world.get_block_and_state(&current_home);
            let valid = if block.has_tag(&pumpkin_data::tag::Block::MINECRAFT_BEDS) {
                let bed_props = BedProperties::from_state_id(state.id);
                bed_props.part == BedPart::Head
            } else {
                false
            };

            if !valid {
                *self
                    .home_pos
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
                if is_sleeping {
                    // Wake up if bed was broken
                    self.get_entity().set_pose(EntityPose::Standing);
                    self.get_entity().set_synced_data(
                        pumpkin_data::tracked_data::villager::SLEEPING_POS_ID,
                        None::<BlockPos>,
                    );
                }
            }
        }
    }

    /// Without a home: takes the nearest bed head within 16 blocks (4 up or down) that no other
    /// villager around claimed.
    pub(super) fn find_home(&self, world: &World) {
        // If no bed, search for one
        if self.get_home_pos().is_none() {
            let pos = self.get_entity().block_pos.load();
            let start = BlockPos::new(pos.0.x - 16, pos.0.y - 4, pos.0.z - 16);
            let end = BlockPos::new(pos.0.x + 16, pos.0.y + 4, pos.0.z + 16);

            let aabb = BoundingBox::new(
                Vector3::new(
                    pos.0.x as f64 - 32.0,
                    pos.0.y as f64 - 16.0,
                    pos.0.z as f64 - 32.0,
                ),
                Vector3::new(
                    pos.0.x as f64 + 32.0,
                    pos.0.y as f64 + 16.0,
                    pos.0.z as f64 + 32.0,
                ),
            );
            let nearby_entities = world.get_all_at_box(&aabb);

            let mut claimed_homes = Vec::new();
            for entity in nearby_entities {
                if entity.get_entity().entity_id != self.get_entity().entity_id
                    && entity.get_entity().entity_type
                        == &pumpkin_data::entity::EntityType::VILLAGER
                    && let Some(home) = entity.get_home_pos()
                {
                    claimed_homes.push(home);
                }
            }

            let mut best_home = None;
            let mut best_dist = f64::MAX;

            for p in BlockPos::iterate(start, end) {
                let (block, state) = world.get_block_and_state(&p);
                if block.has_tag(&pumpkin_data::tag::Block::MINECRAFT_BEDS) {
                    let bed_props = BedProperties::from_state_id(state.id);
                    let bed_head_pos = if bed_props.part == BedPart::Head {
                        p
                    } else {
                        p.offset(bed_props.facing.to_offset())
                    };

                    if claimed_homes.contains(&bed_head_pos) {
                        continue;
                    }

                    let dist = bed_head_pos
                        .to_f64()
                        .squared_distance_to_vec(&self.get_entity().pos.load());
                    if dist < best_dist {
                        best_dist = dist;
                        best_home = Some(bed_head_pos);
                    }
                }
            }

            if let Some(home) = best_home {
                *self
                    .home_pos
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(home);
            }
        }
    }

    /// At night beside its free bed the villager lies down; by day (or homeless) it wakes up.
    pub(super) fn sleep_or_wake(&self, world: &Arc<World>) {
        // Handle Sleeping/Waking up based on time
        let is_sleeping = self.get_entity().pose.load() == EntityPose::Sleeping;
        if let Some(home_pos) = self.get_home_pos() {
            let time = world.get_time_of_day();
            let is_night = (12000..=23000).contains(&time);

            if is_night {
                if !is_sleeping {
                    // Check distance to bed. If close enough, go to sleep
                    let dist = home_pos
                        .to_f64()
                        .squared_distance_to_vec(&self.get_entity().pos.load());
                    if dist <= 4.0 {
                        // Within 2 blocks (squared distance 4.0)
                        let (block, state) = world.get_block_and_state(&home_pos);
                        if block.has_tag(&pumpkin_data::tag::Block::MINECRAFT_BEDS) {
                            let bed_props = BedProperties::from_state_id(state.id);
                            if !bed_props.occupied {
                                // Make bed occupied
                                BedBlock::set_occupied(true, world, block, &home_pos, state.id);

                                self.get_entity().set_pose(EntityPose::Sleeping);
                                self.get_entity().set_synced_data(
                                    pumpkin_data::tracked_data::villager::SLEEPING_POS_ID,
                                    Some(home_pos),
                                );
                            }
                        }
                    }
                }
            } else if is_sleeping {
                // It is day, wake up!
                let (block, state) = world.get_block_and_state(&home_pos);
                if block.has_tag(&pumpkin_data::tag::Block::MINECRAFT_BEDS) {
                    let bed_props = BedProperties::from_state_id(state.id);
                    if bed_props.occupied {
                        BedBlock::set_occupied(false, world, block, &home_pos, state.id);
                    }
                }

                self.get_entity().set_pose(EntityPose::Standing);
                self.get_entity().set_synced_data(
                    pumpkin_data::tracked_data::villager::SLEEPING_POS_ID,
                    None::<BlockPos>,
                );
            }
        } else if is_sleeping {
            // Wake up during the day
            self.get_entity().set_pose(EntityPose::Standing);
            self.get_entity().set_synced_data(
                pumpkin_data::tracked_data::villager::SLEEPING_POS_ID,
                None::<BlockPos>,
            );
        }
    }

    /// Every 100 ticks: a villager that has a home and worked within a day, with three other
    /// villagers and no golem around, spawns an iron golem.
    pub(super) fn spawn_golem_if_needed(&self, world: &Arc<World>, game_time: i64, age: i32) {
        // 2. Iron Golem spawning logic (only for adults)
        let profession = self
            .villager_data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .profession_enum();
        if profession != VillagerProfession::Nitwit && age >= 0 {
            // Checked every 20 ticks, golem spawn check every ~100 ticks
            if age % 100 == 0 && self.get_home().is_some() {
                // Check if panicked or talked recently to spawn an Iron Golem
                let has_bed = self.get_home().is_some();
                let has_worked =
                    game_time - self.last_worked_at_poi.load(Ordering::Relaxed) < 24000;
                if has_bed && has_worked {
                    // Check nearby villagers
                    let my_pos = self.get_entity().pos.load();
                    let bb = BoundingBox::new(
                        my_pos.sub_raw(16.0, 8.0, 16.0),
                        my_pos.add_raw(16.0, 8.0, 16.0),
                    );
                    let nearby_villagers = world
                        .get_entities_at_box(&bb)
                        .iter()
                        .filter(|e| {
                            e.get_entity().entity_type
                                == &pumpkin_data::entity::EntityType::VILLAGER
                        })
                        .count();

                    if nearby_villagers >= 3 {
                        // Check if an iron golem is already nearby
                        let nearby_golems = world
                            .get_entities_at_box(&bb)
                            .iter()
                            .filter(|e| {
                                e.get_entity().entity_type
                                    == &pumpkin_data::entity::EntityType::IRON_GOLEM
                            })
                            .count();

                        if nearby_golems == 0 {
                            // Attempt to spawn an Iron Golem
                            let spawn_pos = my_pos.add_raw(0.0, 0.5, 0.0);
                            let golem_entity = Entity::new(
                                world.clone(),
                                spawn_pos,
                                &pumpkin_data::entity::EntityType::IRON_GOLEM,
                            );
                            let golem = crate::entity::passive::iron_golem::IronGolemEntity::new(
                                golem_entity,
                            );
                            world.spawn_entity_non_save(golem as Arc<dyn EntityBase>);
                            world.send_entity_status(
                                self.get_entity(),
                                pumpkin_data::entity::EntityStatus::VillagerHappy,
                                ActorEventID::VillagerHappy,
                            );
                        }
                    }
                }
            }
        }
    }
}
