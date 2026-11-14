//! The starting room of a mineshaft, with corridors leaving it on all four sides.

use std::sync::{Arc, Mutex};

use pumpkin_data::Block;
use pumpkin_util::{
    BlockDirection,
    math::block_box::BlockBox,
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

pub struct MineShaftRoom {
    pub piece: StructurePiece,
    pub shaft_type: MineshaftType,
    pub child_entrance_boxes: Arc<Mutex<Vec<BlockBox>>>,
}

impl MineShaftRoom {
    #[must_use]
    pub fn new(
        gen_depth: u32,
        random: &mut RandomGenerator,
        west: i32,
        north: i32,
        shaft_type: MineshaftType,
    ) -> Self {
        let max_x = west + 7 + random.next_bounded_i32(6);
        let max_y = 54 + random.next_bounded_i32(6);
        let max_z = north + 7 + random.next_bounded_i32(6);
        let bounding_box = BlockBox::new(west, 50, north, max_x, max_y, max_z);
        Self {
            piece: StructurePiece::new(StructurePieceType::MineshaftRoom, bounding_box, gen_depth),
            shaft_type,
            child_entrance_boxes: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub(super) fn add_room_children(
        start_piece_box: &BlockBox,
        shaft_type: MineshaftType,
        collector: &mut StructurePiecesCollector,
        random: &mut RandomGenerator,
        child_entrance_boxes: &Arc<Mutex<Vec<BlockBox>>>,
    ) {
        let depth = 0;
        let y_span = start_piece_box.max.y - start_piece_box.min.y + 1;
        let mut height_space = y_span - 4;
        if height_space <= 0 {
            height_space = 1;
        }

        let x_span = start_piece_box.max.x - start_piece_box.min.x + 1;
        let z_span = start_piece_box.max.z - start_piece_box.min.z + 1;

        let mut children = Children {
            start_piece_box,
            shaft_type,
            depth,
            collector,
            random,
        };
        add_north_south_exits(&mut children, height_space, x_span, child_entrance_boxes);
        add_west_east_exits(&mut children, height_space, z_span, child_entrance_boxes);
    }
}

impl StructurePieceBase for MineShaftRoom {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn get_structure_piece(&self) -> &StructurePiece {
        &self.piece
    }

    fn get_structure_piece_mut(&mut self) -> &mut StructurePiece {
        &mut self.piece
    }

    fn translate(&mut self, x: i32, y: i32, z: i32) {
        self.piece.translate(x, y, z);
        for bb in &mut *self
            .child_entrance_boxes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            bb.move_pos(x, y, z);
        }
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
        let bb = self.piece.bounding_box;

        let max_clear_y = (bb.min.y + 3).min(bb.max.y);
        for y in (bb.min.y + 1)..=max_clear_y {
            for x in bb.min.x..=bb.max.x {
                for z in bb.min.z..=bb.max.z {
                    if chunk_box.contains(x, y, z) {
                        chunk.set_block_state(x, y, z, air);
                    }
                }
            }
        }

        for entrance in &*self
            .child_entrance_boxes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            for y in (entrance.max.y - 2)..=entrance.max.y {
                for x in entrance.min.x..=entrance.max.x {
                    for z in entrance.min.z..=entrance.max.z {
                        if chunk_box.contains(x, y, z) {
                            chunk.set_block_state(x, y, z, air);
                        }
                    }
                }
            }
        }

        let diag_x = (bb.max.x - bb.min.x + 1) as f32;
        let diag_y = (bb.max.y - (bb.min.y + 4) + 1) as f32;
        let diag_z = (bb.max.z - bb.min.z + 1) as f32;
        let cx = bb.min.x as f32 + diag_x / 2.0;
        let cz = bb.min.z as f32 + diag_z / 2.0;
        let y0 = bb.min.y + 4;

        if diag_y > 0.0 {
            for y in y0..=bb.max.y {
                let ny = (y - y0) as f32 / diag_y;
                for x in bb.min.x..=bb.max.x {
                    let nx = (x as f32 - cx) / (diag_x * 0.5);
                    for z in bb.min.z..=bb.max.z {
                        let nz = (z as f32 - cz) / (diag_z * 0.5);
                        if nx * nx + ny * ny + nz * nz <= 1.05 && chunk_box.contains(x, y, z) {
                            chunk.set_block_state(x, y, z, air);
                        }
                    }
                }
            }
        }
    }
}

/// Corridors leaving the start room through its north and south walls, each with the opening it
/// cuts into the wall.
fn add_north_south_exits(
    children: &mut Children,
    height_space: i32,
    x_span: i32,
    child_entrance_boxes: &Arc<Mutex<Vec<BlockBox>>>,
) {
    let start_piece_box = children.start_piece_box;
    let mut pos = 0;
    while pos < x_span {
        pos += children.random.next_bounded_i32(x_span);
        if pos + 3 > x_span {
            break;
        }
        let child_x = start_piece_box.min.x + pos;
        let child_y = start_piece_box.min.y + children.random.next_bounded_i32(height_space) + 1;
        let child_z = start_piece_box.min.z - 1;

        if let Some(child_box) = children.add(child_x, child_y, child_z, BlockDirection::North) {
            child_entrance_boxes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(BlockBox::new(
                    child_box.min.x,
                    child_box.min.y,
                    start_piece_box.min.z,
                    child_box.max.x,
                    child_box.max.y,
                    start_piece_box.min.z + 1,
                ));
        }
        pos += 4;
    }

    pos = 0;
    while pos < x_span {
        pos += children.random.next_bounded_i32(x_span);
        if pos + 3 > x_span {
            break;
        }
        let child_x = start_piece_box.min.x + pos;
        let child_y = start_piece_box.min.y + children.random.next_bounded_i32(height_space) + 1;
        let child_z = start_piece_box.max.z + 1;

        if let Some(child_box) = children.add(child_x, child_y, child_z, BlockDirection::South) {
            child_entrance_boxes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(BlockBox::new(
                    child_box.min.x,
                    child_box.min.y,
                    start_piece_box.max.z - 1,
                    child_box.max.x,
                    child_box.max.y,
                    start_piece_box.max.z,
                ));
        }
        pos += 4;
    }
}

/// Corridors leaving the start room through its west and east walls, like
/// [`add_north_south_exits`].
fn add_west_east_exits(
    children: &mut Children,
    height_space: i32,
    z_span: i32,
    child_entrance_boxes: &Arc<Mutex<Vec<BlockBox>>>,
) {
    let start_piece_box = children.start_piece_box;
    let mut pos = 0;
    while pos < z_span {
        pos += children.random.next_bounded_i32(z_span);
        if pos + 3 > z_span {
            break;
        }
        let child_x = start_piece_box.min.x - 1;
        let child_y = start_piece_box.min.y + children.random.next_bounded_i32(height_space) + 1;
        let child_z = start_piece_box.min.z + pos;

        if let Some(child_box) = children.add(child_x, child_y, child_z, BlockDirection::West) {
            child_entrance_boxes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(BlockBox::new(
                    start_piece_box.min.x,
                    child_box.min.y,
                    child_box.min.z,
                    start_piece_box.min.x + 1,
                    child_box.max.y,
                    child_box.max.z,
                ));
        }
        pos += 4;
    }

    pos = 0;
    while pos < z_span {
        pos += children.random.next_bounded_i32(z_span);
        if pos + 3 > z_span {
            break;
        }
        let child_x = start_piece_box.max.x + 1;
        let child_y = start_piece_box.min.y + children.random.next_bounded_i32(height_space) + 1;
        let child_z = start_piece_box.min.z + pos;

        if let Some(child_box) = children.add(child_x, child_y, child_z, BlockDirection::East) {
            child_entrance_boxes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(BlockBox::new(
                    start_piece_box.max.x - 1,
                    child_box.min.y,
                    child_box.min.z,
                    start_piece_box.max.x,
                    child_box.max.y,
                    child_box.max.z,
                ));
        }
        pos += 4;
    }
}
