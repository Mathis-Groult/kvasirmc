//! The two fixed decorations of a simple monument room (design 0: four lamp corners, design 1:
//! lamp pillars); design 2 stays in `place_simple`.

use pumpkin_util::{BlockDirection, math::block_box::BlockBox};

use crate::ProtoChunk;

use super::{
    super::{BASE_GRAY, BASE_LIGHT, LAMP, MonumentPiece, graph::RoomDefinition},
    opening,
};

/// Design 0: a lamp in each corner block and a frame or a closed wall on each side.
pub(super) fn design_0(
    p: &MonumentPiece,
    chunk: &mut ProtoChunk,
    chunk_box: &BlockBox,
    room: &RoomDefinition,
) {
    for (x0, z0, lamp_x, lamp_z) in [(0, 0, 1, 1), (5, 0, 6, 1), (0, 5, 1, 6), (5, 5, 6, 6)] {
        p.fill(chunk, chunk_box, x0, 1, z0, x0 + 2, 1, z0 + 2, BASE_LIGHT);
        p.fill(chunk, chunk_box, x0, 3, z0, x0 + 2, 3, z0 + 2, BASE_LIGHT);
        let outer_x = if x0 == 0 { x0 } else { x0 + 2 };
        let outer_z = if z0 == 0 { z0 } else { z0 + 2 };
        p.fill(
            chunk,
            chunk_box,
            outer_x,
            2,
            z0,
            outer_x,
            2,
            z0 + 2,
            BASE_GRAY,
        );
        p.fill(
            chunk,
            chunk_box,
            x0 + 1 - i32::from(x0 != 0),
            2,
            outer_z,
            x0 + 2 - i32::from(x0 != 0),
            2,
            outer_z,
            BASE_GRAY,
        );
        p.block(chunk, chunk_box, lamp_x, 2, lamp_z, LAMP);
    }
    if opening(room, BlockDirection::South) {
        p.fill(chunk, chunk_box, 3, 3, 0, 4, 3, 0, BASE_LIGHT);
    } else {
        p.fill(chunk, chunk_box, 3, 3, 0, 4, 3, 1, BASE_LIGHT);
        p.fill(chunk, chunk_box, 3, 2, 0, 4, 2, 0, BASE_GRAY);
        p.fill(chunk, chunk_box, 3, 1, 0, 4, 1, 1, BASE_LIGHT);
    }
    if opening(room, BlockDirection::North) {
        p.fill(chunk, chunk_box, 3, 3, 7, 4, 3, 7, BASE_LIGHT);
    } else {
        p.fill(chunk, chunk_box, 3, 3, 6, 4, 3, 7, BASE_LIGHT);
        p.fill(chunk, chunk_box, 3, 2, 7, 4, 2, 7, BASE_GRAY);
        p.fill(chunk, chunk_box, 3, 1, 6, 4, 1, 7, BASE_LIGHT);
    }
    if opening(room, BlockDirection::West) {
        p.fill(chunk, chunk_box, 0, 3, 3, 0, 3, 4, BASE_LIGHT);
    } else {
        p.fill(chunk, chunk_box, 0, 3, 3, 1, 3, 4, BASE_LIGHT);
        p.fill(chunk, chunk_box, 0, 2, 3, 0, 2, 4, BASE_GRAY);
        p.fill(chunk, chunk_box, 0, 1, 3, 1, 1, 4, BASE_LIGHT);
    }
    if opening(room, BlockDirection::East) {
        p.fill(chunk, chunk_box, 7, 3, 3, 7, 3, 4, BASE_LIGHT);
    } else {
        p.fill(chunk, chunk_box, 6, 3, 3, 7, 3, 4, BASE_LIGHT);
        p.fill(chunk, chunk_box, 7, 2, 3, 7, 2, 4, BASE_GRAY);
        p.fill(chunk, chunk_box, 6, 1, 3, 7, 1, 4, BASE_LIGHT);
    }
}

/// Design 1: four lamp pillars, corner posts and a wall on each side that does not open.
pub(super) fn design_1(
    p: &MonumentPiece,
    chunk: &mut ProtoChunk,
    chunk_box: &BlockBox,
    room: &RoomDefinition,
) {
    for (x, z) in [(2, 2), (2, 5), (5, 5), (5, 2)] {
        p.fill(chunk, chunk_box, x, 1, z, x, 3, z, BASE_LIGHT);
        p.block(chunk, chunk_box, x, 2, z, LAMP);
    }
    for (x0, z0, x1, z1) in [
        (0, 0, 1, 0),
        (0, 1, 0, 1),
        (0, 7, 1, 7),
        (0, 6, 0, 6),
        (6, 7, 7, 7),
        (7, 6, 7, 6),
        (6, 0, 7, 0),
        (7, 1, 7, 1),
    ] {
        p.fill(chunk, chunk_box, x0, 1, z0, x1, 3, z1, BASE_LIGHT);
    }
    for (x, z) in [
        (1, 0),
        (0, 1),
        (1, 7),
        (0, 6),
        (6, 7),
        (7, 6),
        (6, 0),
        (7, 1),
    ] {
        p.block(chunk, chunk_box, x, 2, z, BASE_GRAY);
    }
    for (direction, x0, z0, x1, z1) in [
        (BlockDirection::South, 1, 0, 6, 0),
        (BlockDirection::North, 1, 7, 6, 7),
        (BlockDirection::West, 0, 1, 0, 6),
        (BlockDirection::East, 7, 1, 7, 6),
    ] {
        if !opening(room, direction) {
            p.fill(chunk, chunk_box, x0, 1, z0, x1, 1, z1, BASE_LIGHT);
            p.fill(chunk, chunk_box, x0, 2, z0, x1, 2, z1, BASE_GRAY);
            p.fill(chunk, chunk_box, x0, 3, z0, x1, 3, z1, BASE_LIGHT);
        }
    }
}
