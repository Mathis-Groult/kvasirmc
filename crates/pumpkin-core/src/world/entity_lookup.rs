//! Finding players and entities by id, name, box or distance.

use super::{World, entity_islands};
use crate::entity::{Entity, EntityBase, player::Player};
use pumpkin_data::entity::EntityType;
use pumpkin_util::math::{boundingbox::BoundingBox, position::BlockPos, vector3::Vector3};
use std::collections::HashMap;
use std::sync::Arc;

impl World {
    #[must_use]
    pub fn is_tracked_by_any_player(&self, entity: &Entity) -> bool {
        self.entity_tracker
            .is_tracked_by_any_player(entity.entity_id)
    }

    /// Gets a `Player` by an entity id
    pub fn get_player_by_id(&self, id: i32) -> Option<Arc<Player>> {
        for player in self.players.load().iter() {
            if player.entity_id() == id {
                entity_islands::check_reached(id);
                return Some(player.clone());
            }
        }
        None
    }

    /// Gets an entity by an entity id
    pub fn get_entity_by_id(&self, id: i32) -> Option<Arc<dyn EntityBase>> {
        for entity in self.entities.load().iter() {
            if entity.get_entity().entity_id == id {
                entity_islands::check_reached(id);
                return Some(entity.clone());
            }
        }
        for player in self.players.load().iter() {
            if player.get_entity().entity_id == id {
                entity_islands::check_reached(id);
                return Some(player.clone() as Arc<dyn EntityBase>);
            }
        }
        None
    }

    /// Gets a `Player` by a username
    pub fn get_player_by_name(&self, name: &str) -> Option<Arc<Player>> {
        for player in self.players.load().iter() {
            if player.gameprofile.name.eq_ignore_ascii_case(name) {
                return Some(player.clone());
            }
        }
        None
    }

    // Gets all entities at a Box
    pub fn get_all_at_box(&self, aabb: &BoundingBox) -> Vec<Arc<dyn EntityBase>> {
        let players_guard = self.players.load();

        self.entity_index
            .query(self.entities.load_full(), *aabb)
            .chain(
                players_guard
                    .iter()
                    .filter(|player| player.get_entity().bounding_box.load().intersects(aabb))
                    .map(|p| p.clone() as Arc<dyn EntityBase>),
            )
            .inspect(|entity| entity_islands::check_reached(entity.get_entity().entity_id))
            .collect()
    }

    // Gets all non Player entities at a Box
    pub fn get_entities_at_box(&self, aabb: &BoundingBox) -> Vec<Arc<dyn EntityBase>> {
        self.entity_index
            .query(self.entities.load_full(), *aabb)
            .inspect(|entity| entity_islands::check_reached(entity.get_entity().entity_id))
            .collect()
    }

    // Gets all Player entities at a Box
    pub fn get_players_at_box(&self, aabb: &BoundingBox) -> Vec<Arc<Player>> {
        let players_guard = self.players.load();
        players_guard
            .iter()
            .filter(|player| player.get_entity().bounding_box.load().intersects(aabb))
            .inspect(|player| entity_islands::check_reached(player.entity_id()))
            .cloned()
            .collect()
    }

    /// Retrieves a player by their unique UUID.
    ///
    /// This function searches the world's active player list for a player with the specified UUID.
    /// If found, it returns an `Arc<Player>` reference to the player. Otherwise, it returns `None`.
    ///
    /// # Arguments
    ///
    /// * `id`: The UUID of the player to retrieve.
    ///
    /// # Returns
    ///
    /// An `Option<Arc<Player>>` containing the player if found, or `None` if not.
    pub fn get_player_by_uuid(&self, id: uuid::Uuid) -> Option<Arc<Player>> {
        self.players
            .load()
            .iter()
            .find(|p| p.gameprofile.id == id)
            .inspect(|player| entity_islands::check_reached(player.entity_id()))
            .cloned()
    }

    /// Retrieves an entity by their unique UUID.
    ///
    /// This function searches the world's entities for one with the specified UUID.
    /// If found, it returns an `Arc<dyn EntityBase>` reference to that entity. Otherwise, it returns `None`.
    ///
    /// # Arguments
    ///
    /// * `id`: The UUID of the entity to retrieve.
    ///
    /// # Returns
    ///
    /// An `Option<Arc<dyn EntityBase>>` containing the player if found, or `None` if not.
    pub fn get_entity_by_uuid(&self, id: uuid::Uuid) -> Option<Arc<dyn EntityBase>> {
        self.entity_index
            .by_uuid(&self.entities.load_full(), id)
            .inspect(|entity| entity_islands::check_reached(entity.get_entity().entity_id))
    }

    /// Gets a list of players whose location equals the given position in the world.
    ///
    /// It iterates through the players in the world and checks their location. If the player's location matches the
    /// given position, it will add this to a `Vec` which it later returns. If no
    /// player was found in that position, it will just return an empty `Vec`.
    ///
    /// # Arguments
    ///
    /// * `position`: The position the function will check.
    pub fn get_players_by_pos(&self, position: BlockPos) -> Vec<Arc<Player>> {
        self.players
            .load()
            .iter()
            .filter_map(|player| {
                let player_block_pos = player.get_entity().block_pos.load().0;
                (position.0.x == player_block_pos.x
                    && position.0.y == player_block_pos.y
                    && position.0.z == player_block_pos.z)
                    .then(|| Arc::clone(player))
            })
            .collect::<_>()
    }

    /// Gets the nearby players around a given world position.
    /// It "creates" a sphere and checks if whether players are inside
    /// and returns a `HashMap` where the UUID is the key and the `Player`
    /// object is the value.
    ///
    /// # Arguments
    /// * `pos`: The center of the sphere.
    /// * `radius`: The radius of the sphere. The higher the radius, the more area will be checked (in every direction).
    pub fn get_nearby_players(&self, pos: Vector3<f64>, radius: f64) -> Vec<Arc<Player>> {
        let radius_squared = radius.powi(2);

        self.players
            .load()
            .iter()
            .filter_map(|player| {
                let player_pos = player.get_entity().pos.load();
                (player_pos.squared_distance_to_vec(&pos) <= radius_squared).then(|| {
                    entity_islands::check_reached(player.entity_id());
                    player.clone()
                })
            })
            .collect()
    }

    pub fn get_nearby_entities(
        &self,
        pos: Vector3<f64>,
        radius: f64,
    ) -> HashMap<uuid::Uuid, Arc<dyn EntityBase>> {
        let radius_squared = radius.powi(2);

        self.entities
            .load()
            .iter()
            .filter_map(|entity| {
                let entity_pos = entity.get_entity().pos.load();
                (entity_pos.squared_distance_to_vec(&pos) <= radius_squared).then(|| {
                    entity_islands::check_reached(entity.get_entity().entity_id);
                    (entity.get_entity().entity_uuid, entity.clone())
                })
            })
            .collect()
    }

    /// Closest player that satisfies `predicate`. Unlike [`Self::get_closest_player`], a nearer
    /// player failing the predicate does not hide a farther one that passes it.
    pub fn get_nearest_player(
        &self,
        pos: Vector3<f64>,
        radius: f64,
        predicate: impl Fn(&Arc<Player>) -> bool,
    ) -> Option<Arc<Player>> {
        self.get_nearby_players(pos, radius)
            .into_iter()
            .filter(|player| predicate(player))
            .min_by(|a, b| {
                a.get_entity()
                    .pos
                    .load()
                    .squared_distance_to_vec(&pos)
                    .total_cmp(&b.get_entity().pos.load().squared_distance_to_vec(&pos))
            })
    }

    /// Closest entity that satisfies `predicate`. See [`Self::get_nearest_player`] for why this is
    /// not [`Self::get_closest_entity`] followed by a check.
    pub fn get_nearest_entity(
        &self,
        pos: Vector3<f64>,
        radius: f64,
        entity_types: Option<&[&'static EntityType]>,
        predicate: impl Fn(&Arc<dyn EntityBase>) -> bool,
    ) -> Option<Arc<dyn EntityBase>> {
        self.get_nearby_entities(pos, radius)
            .into_values()
            .filter(|entity| {
                entity_types.is_none_or(|types| types.contains(&entity.get_entity().entity_type))
                    && predicate(entity)
            })
            .min_by(|a, b| {
                a.get_entity()
                    .pos
                    .load()
                    .squared_distance_to_vec(&pos)
                    .total_cmp(&b.get_entity().pos.load().squared_distance_to_vec(&pos))
            })
    }

    pub fn get_closest_player(&self, pos: Vector3<f64>, radius: f64) -> Option<Arc<Player>> {
        self.get_nearest_player(pos, radius, |_| true)
    }

    /// Gets the closest entity to a position, with optional filtering by entity type.
    ///
    /// # Arguments
    ///
    /// * `pos` - The position to search around.
    /// * `radius` - The radius to search within.
    /// * `entity_types` - Optional array of entity types to filter by. If None, all entity types are included.
    ///
    /// # Returns
    ///
    /// The closest entity that matches the filter criteria, or None if no entities are found.
    pub fn get_closest_entity(
        &self,
        pos: Vector3<f64>,
        radius: f64,
        entity_types: Option<&[&'static EntityType]>,
    ) -> Option<Arc<dyn EntityBase>> {
        self.get_nearest_entity(pos, radius, entity_types, |_| true)
    }

    /// Adds entities to the provided [`Vec`] that satisfy a particular condition and are
    /// present in the provided [`BoundingBox`].
    ///
    /// # Arguments
    ///
    /// * `list`: The `Vec` to add to.
    /// * `max_list_capacity`: The maximum capacity of `list` for adding entities. If this limit is reached, no more
    ///   entities will be added to the list. If `list` already reaches this limit, nothing happens.
    /// * `bounding_box`: The bounding box to filter any added entities.
    /// * `predicate`: A predicate function, which has to be `true` for an entity to be added to the list.
    pub fn extend_entities_in_box_where(
        &self,
        list: &mut Vec<Arc<dyn EntityBase>>,
        max_list_capacity: usize,
        bounding_box: BoundingBox,
        predicate: impl Fn(&dyn EntityBase) -> bool,
    ) {
        if list.len() >= max_list_capacity {
            return;
        }
        for player in self.players.load().iter() {
            if bounding_box.intersects(&player.get_entity().bounding_box.load())
                && predicate(player.as_ref())
            {
                entity_islands::check_reached(player.entity_id());
                list.push(player.clone());
                if list.len() > max_list_capacity {
                    return;
                }
            }
        }
        for entity in self
            .entity_index
            .query(self.entities.load_full(), bounding_box)
        {
            if predicate(entity.as_ref()) {
                entity_islands::check_reached(entity.get_entity().entity_id);
                list.push(entity);
                if list.len() > max_list_capacity {
                    return;
                }
            }
        }
    }

    /// Adds entities to the provided [`Vec`] that satisfy a particular condition.
    ///
    /// # Arguments
    ///
    /// * `list`: The `Vec` to add to.
    /// * `max_list_capacity`: The maximum capacity of `list` for adding entities. If this limit is reached, no more
    ///   entities will be added to the list. If `list` already reaches this limit, nothing happens.
    /// * `predicate`: A predicate function, which has to be `true` for an entity to be added to the list.
    pub fn extend_entities_where(
        &self,
        list: &mut Vec<Arc<dyn EntityBase>>,
        max_list_capacity: usize,
        predicate: impl Fn(&dyn EntityBase) -> bool,
    ) {
        if list.len() >= max_list_capacity {
            return;
        }
        // Loop the players.
        for player in self.players.load().iter() {
            if !predicate(player.as_ref()) {
                continue;
            }
            entity_islands::check_reached(player.entity_id());
            // We add the player to the list.
            list.push(player.clone());
            // Check if the list is too big.
            if list.len() > max_list_capacity {
                return;
            }
        }
        // Same with entities.
        for entity in self.entities.load().iter() {
            if !predicate(entity.as_ref()) {
                continue;
            }
            entity_islands::check_reached(entity.get_entity().entity_id);
            list.push(entity.clone());
            if list.len() > max_list_capacity {
                return;
            }
            // TODO: Implement ender dragon handling
        }
    }
}
