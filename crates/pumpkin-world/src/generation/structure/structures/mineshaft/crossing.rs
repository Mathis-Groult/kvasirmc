//! Mineshaft crossings: one- or two-floor junctions of four corridors.

use pumpkin_data::{Block, BlockState};
use pumpkin_util::{
    BlockDirection,
    math::{block_box::BlockBox, vector3::Vector3},
    random::{RandomGenerator, RandomImpl},
};

use crate::{
    ProtoChunk,
    generation::structure::{
        piece::StructurePieceType,
        structures::{
            StructurePiece, StructurePieceBase, StructurePiecesCollector, WorldPortalExt,
        },
    },
};

use super::{Children, MineshaftType, is_in_invalid_location};

pub struct MineShaftCrossing {
    pub piece: StructurePiece,
    pub shaft_type: MineshaftType,
    pub direction: Option<BlockDirection>,
    pub is_two_floored: bool,
}

impl MineShaftCrossing {
    #[must_use]
    pub const fn new(
        gen_depth: u32,
        bounding_box: BlockBox,
        direction: Option<BlockDirection>,
        shaft_type: MineshaftType,
    ) -> Self {
        let is_two_floored = bounding_box.max.y - bounding_box.min.y + 1 > 3;
        let mut piece = StructurePiece::new(
            StructurePieceType::MineshaftCrossing,
            bounding_box,
            gen_depth,
        );
        piece.set_facing(direction);
        Self {
            piece,
            shaft_type,
            direction,
            is_two_floored,
        }
    }

    #[must_use]
    pub fn find_crossing(
        collector: &StructurePiecesCollector,
        random: &mut RandomGenerator,
        foot_x: i32,
        foot_y: i32,
        foot_z: i32,
        direction: BlockDirection,
    ) -> Option<BlockBox> {
        let y1 = if random.next_bounded_i32(4) == 0 {
            6
        } else {
            2
        };
        let mut box_cand = match direction {
            BlockDirection::South => BlockBox::new(-1, 0, 0, 3, y1, 4),
            BlockDirection::West => BlockBox::new(-4, 0, -1, 0, y1, 3),
            BlockDirection::East => BlockBox::new(0, 0, -1, 4, y1, 3),
            _ => BlockBox::new(-1, 0, -4, 3, y1, 0),
        };
        box_cand.move_pos(foot_x, foot_y, foot_z);
        collector
            .get_intersecting(&box_cand)
            .is_none()
            .then_some(box_cand)
    }

    #[expect(clippy::too_many_arguments)]
    pub(super) fn add_crossing_children(
        bb: &BlockBox,
        depth: u32,
        direction: Option<BlockDirection>,
        is_two_floored: bool,
        start_piece_box: &BlockBox,
        shaft_type: MineshaftType,
        collector: &mut StructurePiecesCollector,
        random: &mut RandomGenerator,
    ) {
        let dir = direction.unwrap_or(BlockDirection::North);
        let mut children = Children {
            start_piece_box,
            shaft_type,
            depth,
            collector,
            random,
        };

        match dir {
            BlockDirection::North => {
                children.add(bb.min.x + 1, bb.min.y, bb.min.z - 1, BlockDirection::North);
                children.add(bb.min.x - 1, bb.min.y, bb.min.z + 1, BlockDirection::West);
                children.add(bb.max.x + 1, bb.min.y, bb.min.z + 1, BlockDirection::East);
            }
            BlockDirection::South => {
                children.add(bb.min.x + 1, bb.min.y, bb.max.z + 1, BlockDirection::South);
                children.add(bb.min.x - 1, bb.min.y, bb.min.z + 1, BlockDirection::West);
                children.add(bb.max.x + 1, bb.min.y, bb.min.z + 1, BlockDirection::East);
            }
            BlockDirection::West => {
                children.add(bb.min.x + 1, bb.min.y, bb.min.z - 1, BlockDirection::North);
                children.add(bb.min.x + 1, bb.min.y, bb.max.z + 1, BlockDirection::South);
                children.add(bb.min.x - 1, bb.min.y, bb.min.z + 1, BlockDirection::West);
            }
            BlockDirection::East => {
                children.add(bb.min.x + 1, bb.min.y, bb.min.z - 1, BlockDirection::North);
                children.add(bb.min.x + 1, bb.min.y, bb.max.z + 1, BlockDirection::South);
                children.add(bb.max.x + 1, bb.min.y, bb.min.z + 1, BlockDirection::East);
            }
            _ => {}
        }

        if is_two_floored {
            if children.random.next_bool() {
                children.add(
                    bb.min.x + 1,
                    bb.min.y + 4,
                    bb.min.z - 1,
                    BlockDirection::North,
                );
            }
            if children.random.next_bool() {
                children.add(
                    bb.min.x - 1,
                    bb.min.y + 4,
                    bb.min.z + 1,
                    BlockDirection::West,
                );
            }
            if children.random.next_bool() {
                children.add(
                    bb.max.x + 1,
                    bb.min.y + 4,
                    bb.min.z + 1,
                    BlockDirection::East,
                );
            }
            if children.random.next_bool() {
                children.add(
                    bb.min.x + 1,
                    bb.min.y + 4,
                    bb.max.z + 1,
                    BlockDirection::South,
                );
            }
        }
    }
}

impl StructurePieceBase for MineShaftCrossing {
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
        let planks = self.shaft_type.planks();
        let bb = self.piece.bounding_box;

        if self.is_two_floored {
            clear_cross(chunk, chunk_box, &bb, bb.min.y..=(bb.min.y + 2), air);
            clear_cross(chunk, chunk_box, &bb, (bb.max.y - 2)..=bb.max.y, air);
            let mid_y = bb.min.y + 3;
            for x in (bb.min.x + 1)..bb.max.x {
                for z in (bb.min.z + 1)..bb.max.z {
                    if chunk_box.contains(x, mid_y, z) {
                        chunk.set_block_state(x, mid_y, z, air);
                    }
                }
            }
        } else {
            clear_cross(chunk, chunk_box, &bb, bb.min.y..=bb.max.y, air);
        }

        for (cx, cz) in [
            (bb.min.x + 1, bb.min.z + 1),
            (bb.min.x + 1, bb.max.z - 1),
            (bb.max.x - 1, bb.min.z + 1),
            (bb.max.x - 1, bb.max.z - 1),
        ] {
            let above_pos = Vector3::new(cx, bb.max.y + 1, cz);
            if chunk_box.contains_pos(&above_pos)
                && !chunk.get_block_state(&above_pos).to_state().is_air()
            {
                for py in bb.min.y..=bb.max.y {
                    if chunk_box.contains(cx, py, cz) {
                        chunk.set_block_state(cx, py, cz, planks);
                    }
                }
            }
        }

        let floor_y = bb.min.y - 1;
        for x in bb.min.x..=bb.max.x {
            for z in bb.min.z..=bb.max.z {
                let inside_pos = Vector3::new(x, bb.min.y, z);
                if chunk_box.contains_pos(&inside_pos)
                    && inside_pos.y
                        < chunk.ocean_floor_wg_height_exclusive(inside_pos.x, inside_pos.z)
                {
                    let floor_pos = Vector3::new(x, floor_y, z);
                    let state = chunk.get_block_state(&floor_pos);
                    if !state.to_state().is_side_solid(BlockDirection::Up.into()) {
                        chunk.set_block_state(x, floor_y, z, planks);
                    }
                }
            }
        }
    }
}

/// Clears the plus-shaped floor plan of a crossing (the box without its four corner columns) on
/// the rows `ys`.
fn clear_cross(
    chunk: &mut ProtoChunk,
    chunk_box: &BlockBox,
    bb: &BlockBox,
    ys: std::ops::RangeInclusive<i32>,
    air: &'static BlockState,
) {
    for y in ys {
        for x in (bb.min.x + 1)..bb.max.x {
            for z in bb.min.z..=bb.max.z {
                if chunk_box.contains(x, y, z) {
                    chunk.set_block_state(x, y, z, air);
                }
            }
        }
        for x in bb.min.x..=bb.max.x {
            for z in (bb.min.z + 1)..bb.max.z {
                if chunk_box.contains(x, y, z) {
                    chunk.set_block_state(x, y, z, air);
                }
            }
        }
    }
}
