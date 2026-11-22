//! The state a new entity derives from where it is placed: block and chunk position, type
//! dimensions, the biome there and the bounding box (the first half of `Entity::from_uuid_with_id`).

use pumpkin_data::biome::Biome;
use pumpkin_data::entity::EntityType;
use pumpkin_util::math::{
    boundingbox::{BoundingBox, EntityDimensions},
    get_section_cord,
    position::BlockPos,
    vector2::Vector2,
    vector3::Vector3,
};

use super::{Entity, EntityBoundingBox};
use crate::world::World;

pub(super) struct Placement {
    pub block_pos: BlockPos,
    pub chunk_pos: Vector2<i32>,
    pub dimensions: EntityDimensions,
    pub biome: &'static Biome,
    pub bounding_box: EntityBoundingBox,
}

impl Placement {
    pub(super) fn new(
        world: &World,
        entity_id: i32,
        entity_type: &'static EntityType,
        position: Vector3<f64>,
    ) -> Self {
        let floor_x = position.x.floor() as i32;
        let floor_y = position.y.floor() as i32;
        let floor_z = position.z.floor() as i32;

        let dimensions = Entity::type_dimensions(entity_type);

        let biome = world
            .level
            .get_rough_biome(&BlockPos::new(floor_x, floor_y, floor_z));

        let bounding_box = EntityBoundingBox::new(
            world,
            entity_id,
            entity_type,
            BoundingBox::new_from_pos(position.x, position.y, position.z, &dimensions),
        );

        Self {
            block_pos: BlockPos(Vector3::new(floor_x, floor_y, floor_z)),
            chunk_pos: Vector2::new(get_section_cord(floor_x), get_section_cord(floor_z)),
            dimensions,
            biome,
            bounding_box,
        }
    }
}
