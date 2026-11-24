//! One cluster of a natural spawn attempt: a random walk from the attempt's position that
//! picks a spawner for the biome, then spawns a group of that mob where the walk allows it.

use super::{
    SpawnState, get_random_spawn_mob_at, is_right_distance_to_player_and_spawn_point,
    is_valid_spawn_position_for_type,
};
use crate::entity::EntityBase;
use crate::entity::mob::spawn::{SpawnGroupData, finalize_spawn};
use crate::entity::r#type::from_type;
use crate::world::World;
use pumpkin_data::biome::Spawner;
use pumpkin_data::entity::{EntityType, MobCategory};
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use rand::{RngExt, rng};
use std::sync::Arc;
use uuid::Uuid;

/// What one attempt shares between its clusters.
pub(super) struct Attempt<'a> {
    pub category: &'static MobCategory,
    pub world: &'a Arc<World>,
    pub y: i32,
    pub chunk_pos: &'a Vector2<i32>,
    pub spawn_state: &'a SpawnState,
    pub is_thundering: bool,
}

/// How one step of the walk ends the cluster, or not.
enum Step {
    Continue,
    EndGroup,
    Capped,
}

/// The walk of one cluster: where it stands, the spawner it chose and its group so far.
struct Walk {
    x: i32,
    z: i32,
    spawner: Option<&'static Spawner>,
    tries: i32,
    group_size: i32,
    group_data: Option<SpawnGroupData>,
}

impl Attempt<'_> {
    /// Runs one cluster from `start`, adding its mobs to `batch`. Returns `true` when the
    /// chunk's cluster limit is reached, which ends the whole attempt.
    pub(super) fn spawn_cluster(
        &self,
        start: BlockPos,
        cluster_size: &mut i32,
        batch: &mut Vec<Arc<dyn EntityBase>>,
    ) -> bool {
        let mut walk = Walk {
            x: start.0.x,
            z: start.0.z,
            spawner: None,
            tries: (rng().random::<f32>() * 4.0).ceil() as i32,
            group_size: 0,
            group_data: None,
        };
        let mut done = 0;
        while done < walk.tries {
            match self.step(&mut walk, cluster_size, batch) {
                Step::Continue => {}
                Step::EndGroup => break,
                Step::Capped => return true,
            }
            done += 1;
        }
        false
    }

    fn step(
        &self,
        walk: &mut Walk,
        cluster_size: &mut i32,
        batch: &mut Vec<Arc<dyn EntityBase>>,
    ) -> Step {
        walk.x += rng().random_range(0..6) - rng().random_range(0..6);
        walk.z += rng().random_range(0..6) - rng().random_range(0..6);
        let check_pos = BlockPos::new(walk.x, self.y, walk.z);
        let at = Vector3::new(
            f64::from(walk.x) + 0.5,
            f64::from(self.y),
            f64::from(walk.z) + 0.5,
        );
        let Some(player_distance_sq) = self.nearest_player_distance_sq(&at) else {
            return Step::Continue;
        };
        if !is_right_distance_to_player_and_spawn_point(
            self.world,
            &check_pos,
            self.chunk_pos,
            player_distance_sq,
        ) {
            return Step::Continue;
        }

        let Some(spawner) = self.spawner(walk, &check_pos) else {
            return Step::EndGroup;
        };
        let Some(entity_type) = EntityType::from_name(spawner.r#type) else {
            return Step::EndGroup;
        };
        if !(is_valid_spawn_position_for_type(
            self.world,
            &check_pos,
            self.category,
            entity_type,
            spawner.r#type,
            player_distance_sq,
            self.is_thundering,
        ) && self
            .spawn_state
            .can_spawn(entity_type, &check_pos, self.world))
        {
            return Step::Continue;
        }
        let Some(entity) = self.candidate(entity_type, at, player_distance_sq) else {
            return Step::Continue;
        };

        walk.group_data = finalize_spawn(&entity, self.world, walk.group_data.take());
        *cluster_size += 1;
        walk.group_size += 1;
        let group_full = entity
            .get_mob()
            .is_some_and(|mob| mob.is_max_group_size_reached(walk.group_size));
        batch.push(entity);
        self.spawn_state
            .after_spawn(entity_type, &check_pos, self.world);
        if *cluster_size >= entity_type.limit_per_chunk {
            Step::Capped
        } else if group_full {
            Step::EndGroup
        } else {
            Step::Continue
        }
    }

    /// The distance to the nearest player who is not a spectator, squared.
    fn nearest_player_distance_sq(&self, at: &Vector3<f64>) -> Option<f64> {
        self.world
            .players
            .load()
            .iter()
            .filter(|player| player.gamemode.load() != GameMode::Spectator)
            .map(|player| {
                let player_pos = player.position();
                let dx = at.x - player_pos.x;
                let dy = at.y - player_pos.y;
                let dz = at.z - player_pos.z;
                dx * dx + dy * dy + dz * dz
            })
            .min_by(f64::total_cmp)
    }

    /// The cluster's spawner: picked at the first legal position, which also sets the tries
    /// the group gets.
    fn spawner(&self, walk: &mut Walk, pos: &BlockPos) -> Option<&'static Spawner> {
        if let Some(spawner) = walk.spawner {
            return Some(spawner);
        }
        let spawner = get_random_spawn_mob_at(self.world, self.category, pos)?;
        walk.spawner = Some(spawner);
        walk.tries = spawner.min_count
            + rng().random_range(0..=(spawner.max_count - spawner.min_count).max(0));
        Some(spawner)
    }

    /// A new mob at `at`, unless it would vanish at once for being too far from every player.
    fn candidate(
        &self,
        entity_type: &'static EntityType,
        at: Vector3<f64>,
        player_distance_sq: f64,
    ) -> Option<Arc<dyn EntityBase>> {
        let entity = from_type(entity_type, at, self.world, Uuid::new_v4());
        entity
            .get_entity()
            .set_rotation(rng().random::<f32>() * 360.0, 0.0);
        let stays = entity.get_mob().is_none_or(|mob| {
            let despawn_dist = f64::from(entity_type.category.despawn_distance);
            !(player_distance_sq > despawn_dist * despawn_dist
                && mob.remove_when_far_away(player_distance_sq))
        });
        stays.then_some(entity)
    }
}
