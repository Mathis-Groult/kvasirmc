//! Mineshaft stairs: a sloping tunnel between two corridor levels.

use pumpkin_data::Block;
use pumpkin_util::{BlockDirection, math::block_box::BlockBox, random::RandomGenerator};

use crate::{
    ProtoChunk,
    generation::structure::{
        piece::StructurePieceType,
        structures::{
            StructurePiece, StructurePieceBase, StructurePiecesCollector, WorldPortalExt,
        },
    },
};

use super::{MineshaftType, generate_and_add_piece, is_in_invalid_location};

pub struct MineShaftStairs {
    pub piece: StructurePiece,
    pub shaft_type: MineshaftType,
}

impl MineShaftStairs {
    #[must_use]
    pub const fn new(
        gen_depth: u32,
        bounding_box: BlockBox,
        direction: BlockDirection,
        shaft_type: MineshaftType,
    ) -> Self {
        let mut piece =
            StructurePiece::new(StructurePieceType::MineshaftStairs, bounding_box, gen_depth);
        piece.set_facing(Some(direction));
        Self { piece, shaft_type }
    }

    #[must_use]
    pub fn find_stairs(
        collector: &StructurePiecesCollector,
        _random: &mut RandomGenerator,
        foot_x: i32,
        foot_y: i32,
        foot_z: i32,
        direction: BlockDirection,
    ) -> Option<BlockBox> {
        let mut box_cand = match direction {
            BlockDirection::South => BlockBox::new(0, -5, 0, 2, 2, 8),
            BlockDirection::West => BlockBox::new(-8, -5, 0, 0, 2, 2),
            BlockDirection::East => BlockBox::new(0, -5, 0, 8, 2, 2),
            _ => BlockBox::new(0, -5, -8, 2, 2, 0),
        };
        box_cand.move_pos(foot_x, foot_y, foot_z);
        collector
            .get_intersecting(&box_cand)
            .is_none()
            .then_some(box_cand)
    }

    pub(super) fn add_stairs_children(
        bb: &BlockBox,
        depth: u32,
        direction: Option<BlockDirection>,
        start_piece_box: &BlockBox,
        shaft_type: MineshaftType,
        collector: &mut StructurePiecesCollector,
        random: &mut RandomGenerator,
    ) {
        let dir = direction.unwrap_or(BlockDirection::North);

        match dir {
            BlockDirection::North => {
                generate_and_add_piece(
                    start_piece_box,
                    collector,
                    random,
                    bb.min.x,
                    bb.min.y,
                    bb.min.z - 1,
                    BlockDirection::North,
                    depth,
                    shaft_type,
                );
            }
            BlockDirection::South => {
                generate_and_add_piece(
                    start_piece_box,
                    collector,
                    random,
                    bb.min.x,
                    bb.min.y,
                    bb.max.z + 1,
                    BlockDirection::South,
                    depth,
                    shaft_type,
                );
            }
            BlockDirection::West => {
                generate_and_add_piece(
                    start_piece_box,
                    collector,
                    random,
                    bb.min.x - 1,
                    bb.min.y,
                    bb.min.z,
                    BlockDirection::West,
                    depth,
                    shaft_type,
                );
            }
            BlockDirection::East => {
                generate_and_add_piece(
                    start_piece_box,
                    collector,
                    random,
                    bb.max.x + 1,
                    bb.min.y,
                    bb.min.z,
                    BlockDirection::East,
                    depth,
                    shaft_type,
                );
            }
            _ => {}
        }
    }
}

impl StructurePieceBase for MineShaftStairs {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn get_structure_piece(&self) -> &StructurePiece {
        &self.piece
    }

    fn get_structure_piece_mut(&mut self) -> &mut StructurePiece {
        &mut self.piece
    }

    fn place(
        &mut self,
        chunk: &mut ProtoChunk,
        _block_registry: &dyn WorldPortalExt,
        _random: &mut RandomGenerator,
        _seed: i64,
        chunk_box: &BlockBox,
    ) {
        if is_in_invalid_location(&self.piece.bounding_box, chunk, chunk_box) {
            return;
        }

        let air = Block::CAVE_AIR.default_state;
        self.piece.fill(chunk, chunk_box, 0, 5, 0, 2, 7, 1, air);
        self.piece.fill(chunk, chunk_box, 0, 0, 7, 2, 2, 8, air);

        for i in 0..5 {
            let y_min = 5 - i - i32::from(i < 4);
            let y_max = 7 - i;
            let z = 2 + i;
            self.piece
                .fill(chunk, chunk_box, 0, y_min, z, 2, y_max, z, air);
        }
    }
}
