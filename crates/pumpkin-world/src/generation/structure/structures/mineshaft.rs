use std::sync::Arc;

use pumpkin_data::{Block, BlockState};
use pumpkin_util::{
    BlockDirection,
    math::{block_box::BlockBox, position::BlockPos, vector3::Vector3},
    random::{RandomGenerator, RandomImpl},
};

use crate::{
    ProtoChunk,
    generation::structure::structures::{
        StructureGenerator, StructureGeneratorContext, StructurePiecesCollector, StructurePosition,
    },
};

mod corridor;
mod crossing;
mod room;
mod stairs;

pub use corridor::MineShaftCorridor;
pub use crossing::MineShaftCrossing;
pub use room::MineShaftRoom;
pub use stairs::MineShaftStairs;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MineshaftType {
    Normal,
    Mesa,
}

impl MineshaftType {
    #[must_use]
    pub const fn wood(self) -> &'static BlockState {
        match self {
            Self::Normal => Block::OAK_LOG.default_state,
            Self::Mesa => Block::DARK_OAK_LOG.default_state,
        }
    }

    #[must_use]
    pub const fn planks(self) -> &'static BlockState {
        match self {
            Self::Normal => Block::OAK_PLANKS.default_state,
            Self::Mesa => Block::DARK_OAK_PLANKS.default_state,
        }
    }

    #[must_use]
    pub const fn fence(self) -> &'static BlockState {
        match self {
            Self::Normal => Block::OAK_FENCE.default_state,
            Self::Mesa => Block::DARK_OAK_FENCE.default_state,
        }
    }

    #[must_use]
    pub const fn fence_block(self) -> &'static Block {
        match self {
            Self::Normal => &Block::OAK_FENCE,
            Self::Mesa => &Block::DARK_OAK_FENCE,
        }
    }
}

pub struct MineshaftGenerator {
    pub is_mesa: bool,
}

impl StructureGenerator for MineshaftGenerator {
    fn get_structure_position(
        &self,
        mut context: StructureGeneratorContext<'_>,
    ) -> Option<StructurePosition> {
        context.random.next_f64();
        let start_x = context.chunk_x << 4;
        let start_z = context.chunk_z << 4;
        let room_x = start_x + 2;
        let room_z = start_z + 2;

        let shaft_type = if self.is_mesa {
            MineshaftType::Mesa
        } else {
            MineshaftType::Normal
        };

        let start_room = MineShaftRoom::new(0, &mut context.random, room_x, room_z, shaft_type);
        let entrances = start_room.child_entrance_boxes.clone();
        let start_piece_box = start_room.piece.bounding_box;

        let mut collector = StructurePiecesCollector::default();
        collector.add_piece(Box::new(start_room));

        MineShaftRoom::add_room_children(
            &start_piece_box,
            shaft_type,
            &mut collector,
            &mut context.random,
            &entrances,
        );

        let dy = if self.is_mesa {
            let bbox = collector.get_bounding_box();
            let center_x = i32::midpoint(bbox.min.x, bbox.max.x);
            let center_z = i32::midpoint(bbox.min.z, bbox.max.z);
            let center_y = i32::midpoint(bbox.min.y, bbox.max.y);

            let surface_height = context
                .height_sampler
                .as_deref_mut()
                .map_or(context.sea_level, |s| s.estimate_height(center_x, center_z));

            let target_y = if surface_height <= context.sea_level {
                context.sea_level
            } else {
                let range = surface_height - context.sea_level + 1;
                context.sea_level + context.random.next_bounded_i32(range)
            };

            let dy = target_y - center_y;
            collector.shift(dy);
            dy
        } else {
            collector.shift_into(context.sea_level, context.min_y, &mut context.random, 10)
        };

        Some(StructurePosition {
            start_pos: BlockPos::new(start_x + 8, 50 + dy, start_z),
            collector: Arc::new(collector.into()),
        })
    }
}

enum GeneratedPiece {
    Corridor(MineShaftCorridor),
    Crossing(MineShaftCrossing),
    Stairs(MineShaftStairs),
}

impl GeneratedPiece {
    const fn bounding_box(&self) -> BlockBox {
        match self {
            Self::Corridor(c) => c.piece.bounding_box,
            Self::Crossing(cr) => cr.piece.bounding_box,
            Self::Stairs(s) => s.piece.bounding_box,
        }
    }
}

#[expect(clippy::too_many_arguments)]
fn create_random_shaft_piece(
    collector: &StructurePiecesCollector,
    random: &mut RandomGenerator,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    direction: BlockDirection,
    gen_depth: u32,
    shaft_type: MineshaftType,
) -> Option<GeneratedPiece> {
    let random_selection = random.next_bounded_i32(100);
    if random_selection >= 80 {
        if let Some(crossing_box) =
            MineShaftCrossing::find_crossing(collector, random, foot_x, foot_y, foot_z, direction)
        {
            return Some(GeneratedPiece::Crossing(MineShaftCrossing::new(
                gen_depth,
                crossing_box,
                Some(direction),
                shaft_type,
            )));
        }
    } else if random_selection >= 70 {
        if let Some(stairs_box) =
            MineShaftStairs::find_stairs(collector, random, foot_x, foot_y, foot_z, direction)
        {
            return Some(GeneratedPiece::Stairs(MineShaftStairs::new(
                gen_depth, stairs_box, direction, shaft_type,
            )));
        }
    } else if let Some(corridor_box) =
        MineShaftCorridor::find_corridor_size(collector, random, foot_x, foot_y, foot_z, direction)
    {
        return Some(GeneratedPiece::Corridor(MineShaftCorridor::new(
            gen_depth,
            random,
            corridor_box,
            direction,
            shaft_type,
        )));
    }
    None
}

#[expect(clippy::too_many_arguments)]
fn generate_and_add_piece(
    start_piece_box: &BlockBox,
    collector: &mut StructurePiecesCollector,
    random: &mut RandomGenerator,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    direction: BlockDirection,
    depth: u32,
    shaft_type: MineshaftType,
) -> Option<BlockBox> {
    if depth > 8 {
        return None;
    }

    if (foot_x - start_piece_box.min.x).abs() <= 80
        && (foot_z - start_piece_box.min.z).abs() <= 80
        && let Some(new_piece) = create_random_shaft_piece(
            collector,
            random,
            foot_x,
            foot_y,
            foot_z,
            direction,
            depth + 1,
            shaft_type,
        )
    {
        let piece_box = new_piece.bounding_box();
        match new_piece {
            GeneratedPiece::Corridor(corridor) => {
                let bb = corridor.piece.bounding_box;
                let orientation = corridor.piece.facing.unwrap_or(BlockDirection::North);
                collector.add_piece(Box::new(corridor));
                MineShaftCorridor::add_corridor_children(
                    &bb,
                    depth + 1,
                    orientation,
                    start_piece_box,
                    shaft_type,
                    collector,
                    random,
                );
            }
            GeneratedPiece::Crossing(crossing) => {
                let bb = crossing.piece.bounding_box;
                let dir = crossing.direction;
                let is_two_floored = crossing.is_two_floored;
                collector.add_piece(Box::new(crossing));
                MineShaftCrossing::add_crossing_children(
                    &bb,
                    depth + 1,
                    dir,
                    is_two_floored,
                    start_piece_box,
                    shaft_type,
                    collector,
                    random,
                );
            }
            GeneratedPiece::Stairs(stairs) => {
                let bb = stairs.piece.bounding_box;
                let facing = stairs.piece.facing;
                collector.add_piece(Box::new(stairs));
                MineShaftStairs::add_stairs_children(
                    &bb,
                    depth + 1,
                    facing,
                    start_piece_box,
                    shaft_type,
                    collector,
                    random,
                );
            }
        }
        Some(piece_box)
    } else {
        None
    }
}

/// The children of one mineshaft piece: the arguments of [`generate_and_add_piece`] that stay the
/// same for all of them.
struct Children<'a> {
    start_piece_box: &'a BlockBox,
    shaft_type: MineshaftType,
    depth: u32,
    collector: &'a mut StructurePiecesCollector,
    random: &'a mut RandomGenerator,
}

impl Children<'_> {
    /// Adds the piece starting at the foot position facing `direction` (with all its own children);
    /// returns its box when one was added.
    fn add(
        &mut self,
        foot_x: i32,
        foot_y: i32,
        foot_z: i32,
        direction: BlockDirection,
    ) -> Option<BlockBox> {
        generate_and_add_piece(
            self.start_piece_box,
            self.collector,
            self.random,
            foot_x,
            foot_y,
            foot_z,
            direction,
            self.depth,
            self.shaft_type,
        )
    }
}

const fn is_falling_block(block: &Block) -> bool {
    use pumpkin_data::BlockId;
    matches!(
        block.id,
        BlockId::SAND
            | BlockId::RED_SAND
            | BlockId::GRAVEL
            | BlockId::SUSPICIOUS_SAND
            | BlockId::SUSPICIOUS_GRAVEL
            | BlockId::ANVIL
            | BlockId::CHIPPED_ANVIL
            | BlockId::DAMAGED_ANVIL
            | BlockId::DRAGON_EGG
            | BlockId::WHITE_CONCRETE_POWDER
            | BlockId::ORANGE_CONCRETE_POWDER
            | BlockId::MAGENTA_CONCRETE_POWDER
            | BlockId::LIGHT_BLUE_CONCRETE_POWDER
            | BlockId::YELLOW_CONCRETE_POWDER
            | BlockId::LIME_CONCRETE_POWDER
            | BlockId::PINK_CONCRETE_POWDER
            | BlockId::GRAY_CONCRETE_POWDER
            | BlockId::LIGHT_GRAY_CONCRETE_POWDER
            | BlockId::CYAN_CONCRETE_POWDER
            | BlockId::PURPLE_CONCRETE_POWDER
            | BlockId::BLUE_CONCRETE_POWDER
            | BlockId::BROWN_CONCRETE_POWDER
            | BlockId::GREEN_CONCRETE_POWDER
            | BlockId::RED_CONCRETE_POWDER
            | BlockId::BLACK_CONCRETE_POWDER
    )
}

fn is_in_invalid_location(piece_bb: &BlockBox, chunk: &ProtoChunk, chunk_box: &BlockBox) -> bool {
    let x0 = (piece_bb.min.x - 1).max(chunk_box.min.x);
    let y0 = (piece_bb.min.y - 1).max(chunk_box.min.y);
    let z0 = (piece_bb.min.z - 1).max(chunk_box.min.z);
    let x1 = (piece_bb.max.x + 1).min(chunk_box.max.x);
    let y1 = (piece_bb.max.y + 1).min(chunk_box.max.y);
    let z1 = (piece_bb.max.z + 1).min(chunk_box.max.z);

    if x0 > x1 || y0 > y1 || z0 > z1 {
        return false;
    }

    let mid_x = i32::midpoint(x0, x1);
    let mid_y = i32::midpoint(y0, y1);
    let mid_z = i32::midpoint(z0, z1);
    if chunk
        .get_biome(mid_x >> 2, mid_y >> 2, mid_z >> 2)
        .registry_id
        == "minecraft:deep_dark"
    {
        return true;
    }

    for x in x0..=x1 {
        for z in z0..=z1 {
            if chunk
                .get_block_state(&Vector3::new(x, y0, z))
                .to_state()
                .is_liquid()
                || chunk
                    .get_block_state(&Vector3::new(x, y1, z))
                    .to_state()
                    .is_liquid()
            {
                return true;
            }
        }
    }
    for x in x0..=x1 {
        for y in y0..=y1 {
            if chunk
                .get_block_state(&Vector3::new(x, y, z0))
                .to_state()
                .is_liquid()
                || chunk
                    .get_block_state(&Vector3::new(x, y, z1))
                    .to_state()
                    .is_liquid()
            {
                return true;
            }
        }
    }
    for z in z0..=z1 {
        for y in y0..=y1 {
            if chunk
                .get_block_state(&Vector3::new(x0, y, z))
                .to_state()
                .is_liquid()
                || chunk
                    .get_block_state(&Vector3::new(x1, y, z))
                    .to_state()
                    .is_liquid()
            {
                return true;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generation::get_world_gen;
    use pumpkin_data::dimension::Dimension;
    use pumpkin_util::random::legacy_rand::LegacyRand;
    use pumpkin_util::world_seed::Seed;

    #[test]
    fn mineshaft_corridor_chest_minecart_spawns_with_loot_table() {
        let world_gen = get_world_gen(
            Seed(0),
            Dimension::OVERWORLD,
            false,
            Vec::new(),
            String::new(),
        );
        let mut chunk = ProtoChunk::new(0, 0, &world_gen);
        let chunk_box = BlockBox::new(0, -64, 0, 15, 319, 15);

        // Place floor block under position (2, 63, 2)
        chunk.set_block_state(2, 63, 2, Block::STONE.default_state);
        // Ensure air at (2, 64, 2)
        chunk.set_block_state(2, 64, 2, Block::AIR.default_state);

        let corridor_box = BlockBox::new(0, 64, 0, 2, 67, 10);
        let mut random = RandomGenerator::Legacy(LegacyRand::from_seed(42));
        let corridor = MineShaftCorridor::new(
            0,
            &mut random,
            corridor_box,
            BlockDirection::South,
            MineshaftType::Normal,
        );

        let created = corridor.create_chest(
            &mut chunk,
            &chunk_box,
            &mut random,
            2,
            0,
            2,
            "minecraft:chests/abandoned_mineshaft",
        );

        assert!(created);
        let entities = chunk.take_pending_structure_entities();
        assert_eq!(entities.len(), 1);
        let entity = &entities[0];
        assert_eq!(entity.get_string("id"), Some("minecraft:chest_minecart"));
        assert_eq!(
            entity.get_string("LootTable"),
            Some("minecraft:chests/abandoned_mineshaft")
        );
        assert!(entity.get_long("LootTableSeed").is_some());
        let pos_list = entity.get_list("Pos").expect("Pos list must exist");
        assert_eq!(pos_list.len(), 3);
        assert_eq!(pos_list[0].extract_double(), Some(2.5));
        assert_eq!(pos_list[1].extract_double(), Some(64.5));
        assert_eq!(pos_list[2].extract_double(), Some(2.5));
    }
}
