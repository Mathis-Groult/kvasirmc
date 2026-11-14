//! The parts the two nether fortress tower rooms share (the corridor exit and the nether warts
//! room): the brick shell, the battlements along the top and the bridge support legs.

use pumpkin_data::{Block, BlockState};
use pumpkin_util::math::block_box::BlockBox;

use crate::{ProtoChunk, generation::structure::structures::StructurePiece};

/// The floor, the cleared inside and the walls of a 13 x 14 x 13 tower room.
pub(super) fn place_shell(p: &StructurePiece, chunk: &mut ProtoChunk, bb: BlockBox) {
    let nb = Block::NETHER_BRICKS.default_state;
    let air = Block::AIR.default_state;

    // Main shell
    p.fill_with_outline(chunk, &bb, false, 0, 3, 0, 12, 4, 12, nb, nb);
    p.fill_with_outline(chunk, &bb, false, 0, 5, 0, 12, 13, 12, air, air);

    // Outer walls
    p.fill_with_outline(chunk, &bb, false, 0, 5, 0, 1, 12, 12, nb, nb);
    p.fill_with_outline(chunk, &bb, false, 11, 5, 0, 12, 12, 12, nb, nb);
    p.fill_with_outline(chunk, &bb, false, 2, 5, 11, 4, 12, 12, nb, nb);
    p.fill_with_outline(chunk, &bb, false, 8, 5, 11, 10, 12, 12, nb, nb);
    p.fill_with_outline(chunk, &bb, false, 5, 9, 11, 7, 12, 12, nb, nb);
    p.fill_with_outline(chunk, &bb, false, 2, 5, 0, 4, 12, 1, nb, nb);
    p.fill_with_outline(chunk, &bb, false, 8, 5, 0, 10, 12, 1, nb, nb);
    p.fill_with_outline(chunk, &bb, false, 5, 9, 0, 7, 12, 1, nb, nb);
    p.fill_with_outline(chunk, &bb, false, 2, 11, 2, 10, 12, 10, nb, nb);
}

/// The fences and brick merlons along the top of the walls, with the four corner fences.
pub(super) fn place_battlements(
    p: &StructurePiece,
    chunk: &mut ProtoChunk,
    bb: BlockBox,
    fence: impl Fn(bool, bool, bool, bool) -> &'static BlockState,
) {
    let nb = Block::NETHER_BRICKS.default_state;
    let f_ew = fence(true, true, false, false);
    let f_ns = fence(false, false, true, true);

    // Battlement fences and corner blocks along top
    let mut i = 1i32;
    while i <= 11 {
        p.fill_with_outline(chunk, &bb, false, i, 10, 0, i, 11, 0, f_ew, f_ew);
        p.fill_with_outline(chunk, &bb, false, i, 10, 12, i, 11, 12, f_ew, f_ew);
        p.fill_with_outline(chunk, &bb, false, 0, 10, i, 0, 11, i, f_ns, f_ns);
        p.fill_with_outline(chunk, &bb, false, 12, 10, i, 12, 11, i, f_ns, f_ns);
        p.add_block(chunk, nb, i, 13, 0, &bb);
        p.add_block(chunk, nb, i, 13, 12, &bb);
        p.add_block(chunk, nb, 0, 13, i, &bb);
        p.add_block(chunk, nb, 12, 13, i, &bb);
        if i != 11 {
            p.add_block(chunk, f_ew, i + 1, 13, 0, &bb);
            p.add_block(chunk, f_ew, i + 1, 13, 12, &bb);
            p.add_block(chunk, f_ns, 0, 13, i + 1, &bb);
            p.add_block(chunk, f_ns, 12, 13, i + 1, &bb);
        }
        i += 2;
    }

    // Corner fence pieces
    p.add_block(chunk, fence(false, true, true, false), 0, 13, 0, &bb);
    p.add_block(chunk, fence(false, true, false, true), 0, 13, 12, &bb);
    p.add_block(chunk, fence(true, false, false, true), 12, 13, 12, &bb);
    p.add_block(chunk, fence(true, false, true, false), 12, 13, 0, &bb);
}

/// The bridge floor crossing under the room and the legs that reach down to the ground.
pub(super) fn place_support_legs(p: &StructurePiece, chunk: &mut ProtoChunk, bb: BlockBox) {
    let nb = Block::NETHER_BRICKS.default_state;

    // Exterior bridge supports
    p.fill_with_outline(chunk, &bb, false, 4, 2, 0, 8, 2, 12, nb, nb);
    p.fill_with_outline(chunk, &bb, false, 0, 2, 4, 12, 2, 8, nb, nb);
    p.fill_with_outline(chunk, &bb, false, 4, 0, 0, 8, 1, 3, nb, nb);
    p.fill_with_outline(chunk, &bb, false, 4, 0, 9, 8, 1, 12, nb, nb);
    p.fill_with_outline(chunk, &bb, false, 0, 0, 4, 3, 1, 8, nb, nb);
    p.fill_with_outline(chunk, &bb, false, 9, 0, 4, 12, 1, 8, nb, nb);

    for i in 4..=8i32 {
        for j in 0..=2i32 {
            p.fill_downwards(chunk, nb, i, -1, j, &bb);
            p.fill_downwards(chunk, nb, i, -1, 12 - j, &bb);
        }
    }
    for i in 0..=2i32 {
        for j in 4..=8i32 {
            p.fill_downwards(chunk, nb, i, -1, j, &bb);
            p.fill_downwards(chunk, nb, 12 - i, -1, j, &bb);
        }
    }
}
