//! The double rooms that span two levels: double Y (one cell, two levels), double XY (two cells
//! side by side, two levels) and double YZ (two cells in a row, two levels).

use pumpkin_util::{BlockDirection, math::block_box::BlockBox};

use crate::ProtoChunk;

use super::{
    super::{
        BASE_BLACK, BASE_GRAY, BASE_LIGHT, LAMP, MonumentPiece,
        graph::{RoomDefinition, RoomGraph},
    },
    RoomPiece, opening,
};

impl RoomPiece {
    pub(super) fn place_double_xy(
        &self,
        chunk: &mut ProtoChunk,
        chunk_box: &BlockBox,
        sea_level: i32,
        graph: &RoomGraph,
    ) {
        let p = &self.piece;
        let west = self.room.unwrap_or(0);
        let east = graph.connection(west, BlockDirection::East);
        let west_up = graph.connection(west, BlockDirection::Up);
        let east_up = graph.connection(east, BlockDirection::Up);
        let west_room = &graph.rooms[west];
        let east_room = &graph.rooms[east];
        let west_up_room = &graph.rooms[west_up];
        let east_up_room = &graph.rooms[east_up];
        if west_room.index / 25 > 0 {
            p.default_floor(
                chunk,
                chunk_box,
                8,
                0,
                opening(east_room, BlockDirection::Down),
            );
            p.default_floor(
                chunk,
                chunk_box,
                0,
                0,
                opening(west_room, BlockDirection::Down),
            );
        }
        if west_up_room.connections[BlockDirection::Up as usize].is_none() {
            p.fill_on_water(chunk, chunk_box, 1, 8, 1, 7, 8, 6, BASE_GRAY);
        }
        if east_up_room.connections[BlockDirection::Up as usize].is_none() {
            p.fill_on_water(chunk, chunk_box, 8, 8, 1, 14, 8, 6, BASE_GRAY);
        }
        for y in 1..=7 {
            let block = if y == 2 || y == 6 {
                BASE_GRAY
            } else {
                BASE_LIGHT
            };
            p.fill(chunk, chunk_box, 0, y, 0, 0, y, 7, block);
            p.fill(chunk, chunk_box, 15, y, 0, 15, y, 7, block);
            p.fill(chunk, chunk_box, 1, y, 0, 15, y, 0, block);
            p.fill(chunk, chunk_box, 1, y, 7, 14, y, 7, block);
        }
        for (x, z0, z1) in [
            (2, 3, 4),
            (3, 2, 2),
            (3, 5, 5),
            (13, 3, 4),
            (11, 2, 2),
            (11, 5, 5),
        ] {
            let x1 = if matches!(x, 3 | 11) { x + 1 } else { x };
            p.fill(chunk, chunk_box, x, 1, z0, x1, 7, z1, BASE_LIGHT);
        }
        p.fill(chunk, chunk_box, 5, 1, 3, 5, 3, 4, BASE_LIGHT);
        p.fill(chunk, chunk_box, 10, 1, 3, 10, 3, 4, BASE_LIGHT);
        p.fill(chunk, chunk_box, 5, 7, 2, 10, 7, 5, BASE_LIGHT);
        for (x, z) in [(5, 2), (10, 2), (5, 5), (10, 5)] {
            p.fill(chunk, chunk_box, x, 5, z, x, 7, z, BASE_LIGHT);
            p.block(chunk, chunk_box, x, 4, z, LAMP);
        }
        for (x, z) in [(6, 2), (9, 2), (6, 5), (9, 5)] {
            p.block(chunk, chunk_box, x, 6, z, BASE_LIGHT);
        }
        p.fill(chunk, chunk_box, 5, 4, 3, 6, 4, 4, BASE_LIGHT);
        p.fill(chunk, chunk_box, 9, 4, 3, 10, 4, 4, BASE_LIGHT);
        xy_openings(
            p,
            chunk,
            chunk_box,
            sea_level,
            [
                (west_room, 0, BlockDirection::West),
                (east_room, 8, BlockDirection::East),
            ],
            1,
        );
        xy_openings(
            p,
            chunk,
            chunk_box,
            sea_level,
            [
                (west_up_room, 0, BlockDirection::West),
                (east_up_room, 8, BlockDirection::East),
            ],
            5,
        );
    }

    pub(super) fn place_double_y(
        &self,
        chunk: &mut ProtoChunk,
        chunk_box: &BlockBox,
        _sea_level: i32,
        graph: &RoomGraph,
    ) {
        let p = &self.piece;
        let lower = self.room.unwrap_or(0);
        let upper = graph.connection(lower, BlockDirection::Up);
        let lower_room = &graph.rooms[lower];
        let upper_room = &graph.rooms[upper];
        if lower_room.index / 25 > 0 {
            p.default_floor(
                chunk,
                chunk_box,
                0,
                0,
                opening(lower_room, BlockDirection::Down),
            );
        }
        if upper_room.connections[BlockDirection::Up as usize].is_none() {
            p.fill_on_water(chunk, chunk_box, 1, 8, 1, 6, 8, 6, BASE_GRAY);
        }
        p.fill(chunk, chunk_box, 0, 4, 0, 0, 4, 7, BASE_LIGHT);
        p.fill(chunk, chunk_box, 7, 4, 0, 7, 4, 7, BASE_LIGHT);
        p.fill(chunk, chunk_box, 1, 4, 0, 6, 4, 0, BASE_LIGHT);
        p.fill(chunk, chunk_box, 1, 4, 7, 6, 4, 7, BASE_LIGHT);
        for (x, z0, z1) in [(2, 1, 2), (5, 1, 2), (2, 5, 6), (5, 5, 6)] {
            p.fill(chunk, chunk_box, x, 4, z0, x, 4, z1, BASE_LIGHT);
        }
        for (x, z) in [(1, 2), (6, 2), (1, 5), (6, 5)] {
            p.block(chunk, chunk_box, x, 4, z, BASE_LIGHT);
        }
        for (level, room) in [(1, lower_room), (5, upper_room)] {
            south_north_walls(p, chunk, chunk_box, level, room);
            west_east_walls(p, chunk, chunk_box, level, room);
        }
    }

    pub(super) fn place_double_yz(
        &self,
        chunk: &mut ProtoChunk,
        chunk_box: &BlockBox,
        sea_level: i32,
        graph: &RoomGraph,
    ) {
        let p = &self.piece;
        let south = self.room.unwrap_or(0);
        let north = graph.connection(south, BlockDirection::North);
        let south_up = graph.connection(south, BlockDirection::Up);
        let north_up = graph.connection(north, BlockDirection::Up);
        let south_room = &graph.rooms[south];
        let north_room = &graph.rooms[north];
        let south_up_room = &graph.rooms[south_up];
        let north_up_room = &graph.rooms[north_up];
        if south_room.index / 25 > 0 {
            p.default_floor(
                chunk,
                chunk_box,
                0,
                8,
                opening(north_room, BlockDirection::Down),
            );
            p.default_floor(
                chunk,
                chunk_box,
                0,
                0,
                opening(south_room, BlockDirection::Down),
            );
        }
        if south_up_room.connections[BlockDirection::Up as usize].is_none() {
            p.fill_on_water(chunk, chunk_box, 1, 8, 1, 6, 8, 7, BASE_GRAY);
        }
        if north_up_room.connections[BlockDirection::Up as usize].is_none() {
            p.fill_on_water(chunk, chunk_box, 1, 8, 8, 6, 8, 14, BASE_GRAY);
        }
        for y in 1..=7 {
            let block = if y == 2 || y == 6 {
                BASE_GRAY
            } else {
                BASE_LIGHT
            };
            p.fill(chunk, chunk_box, 0, y, 0, 0, y, 15, block);
            p.fill(chunk, chunk_box, 7, y, 0, 7, y, 15, block);
            p.fill(chunk, chunk_box, 1, y, 0, 6, y, 0, block);
            p.fill(chunk, chunk_box, 1, y, 15, 6, y, 15, block);
        }
        for y in 1..=7 {
            let block = if y == 2 || y == 6 { LAMP } else { BASE_BLACK };
            p.fill(chunk, chunk_box, 3, y, 7, 4, y, 8, block);
        }
        for (room, z, cross) in [(south_room, 0, 3), (north_room, 15, 11)] {
            let longitudinal = if z == 0 {
                BlockDirection::South
            } else {
                BlockDirection::North
            };
            if opening(room, longitudinal) {
                p.water(chunk, chunk_box, sea_level, 3, 1, z, 4, 2, z);
            }
            if opening(room, BlockDirection::West) {
                p.water(chunk, chunk_box, sea_level, 0, 1, cross, 0, 2, cross + 1);
            }
            if opening(room, BlockDirection::East) {
                p.water(chunk, chunk_box, sea_level, 7, 1, cross, 7, 2, cross + 1);
            }
        }
        for (room, z, cross) in [(south_up_room, 0, 3), (north_up_room, 15, 11)] {
            yz_upper_openings(p, chunk, chunk_box, sea_level, room, z, cross);
        }
    }
}

/// The water openings of one level of a double XY room: south and north of both halves and the
/// outer side (west or east) of each, on rows `y` and `y + 1`.
fn xy_openings(
    p: &MonumentPiece,
    chunk: &mut ProtoChunk,
    chunk_box: &BlockBox,
    sea_level: i32,
    halves: [(&RoomDefinition, i32, BlockDirection); 2],
    y: i32,
) {
    for (room, offset_x, side) in halves {
        if opening(room, BlockDirection::South) {
            p.water(
                chunk,
                chunk_box,
                sea_level,
                offset_x + 3,
                y,
                0,
                offset_x + 4,
                y + 1,
                0,
            );
        }
        if opening(room, BlockDirection::North) {
            p.water(
                chunk,
                chunk_box,
                sea_level,
                offset_x + 3,
                y,
                7,
                offset_x + 4,
                y + 1,
                7,
            );
        }
        if opening(room, side) {
            let x = if side == BlockDirection::West { 0 } else { 15 };
            p.water(chunk, chunk_box, sea_level, x, y, 3, x, y + 1, 4);
        }
    }
}

/// The south and north walls of one level of a double Y room: a window frame where the room opens,
/// a striped wall where it does not.
fn south_north_walls(
    p: &MonumentPiece,
    chunk: &mut ProtoChunk,
    chunk_box: &BlockBox,
    level: i32,
    room: &RoomDefinition,
) {
    for (direction, fixed, a, b) in [
        (BlockDirection::South, 0, 2, 5),
        (BlockDirection::North, 7, 2, 5),
    ] {
        if opening(room, direction) {
            p.fill(
                chunk,
                chunk_box,
                a,
                level,
                fixed,
                a,
                level + 2,
                fixed,
                BASE_LIGHT,
            );
            p.fill(
                chunk,
                chunk_box,
                b,
                level,
                fixed,
                b,
                level + 2,
                fixed,
                BASE_LIGHT,
            );
            p.fill(
                chunk,
                chunk_box,
                a + 1,
                level + 2,
                fixed,
                b - 1,
                level + 2,
                fixed,
                BASE_LIGHT,
            );
        } else {
            p.fill(
                chunk,
                chunk_box,
                0,
                level,
                fixed,
                7,
                level + 2,
                fixed,
                BASE_LIGHT,
            );
            p.fill(
                chunk,
                chunk_box,
                0,
                level + 1,
                fixed,
                7,
                level + 1,
                fixed,
                BASE_GRAY,
            );
        }
    }
}

/// The west and east walls of one level of a double Y room, like [`south_north_walls`].
fn west_east_walls(
    p: &MonumentPiece,
    chunk: &mut ProtoChunk,
    chunk_box: &BlockBox,
    level: i32,
    room: &RoomDefinition,
) {
    for (direction, fixed, a, b) in [
        (BlockDirection::West, 0, 2, 5),
        (BlockDirection::East, 7, 2, 5),
    ] {
        if opening(room, direction) {
            p.fill(
                chunk,
                chunk_box,
                fixed,
                level,
                a,
                fixed,
                level + 2,
                a,
                BASE_LIGHT,
            );
            p.fill(
                chunk,
                chunk_box,
                fixed,
                level,
                b,
                fixed,
                level + 2,
                b,
                BASE_LIGHT,
            );
            p.fill(
                chunk,
                chunk_box,
                fixed,
                level + 2,
                a + 1,
                fixed,
                level + 2,
                b - 1,
                BASE_LIGHT,
            );
        } else {
            p.fill(
                chunk,
                chunk_box,
                fixed,
                level,
                0,
                fixed,
                level + 2,
                7,
                BASE_LIGHT,
            );
            p.fill(
                chunk,
                chunk_box,
                fixed,
                level + 1,
                0,
                fixed,
                level + 1,
                7,
                BASE_GRAY,
            );
        }
    }
}

/// The openings of the upper level of one half of a double YZ room, with the frames around its
/// side openings.
fn yz_upper_openings(
    p: &MonumentPiece,
    chunk: &mut ProtoChunk,
    chunk_box: &BlockBox,
    sea_level: i32,
    room: &RoomDefinition,
    z: i32,
    cross: i32,
) {
    let longitudinal = if z == 0 {
        BlockDirection::South
    } else {
        BlockDirection::North
    };
    if opening(room, longitudinal) {
        p.water(chunk, chunk_box, sea_level, 3, 5, z, 4, 6, z);
    }
    if opening(room, BlockDirection::West) {
        p.water(chunk, chunk_box, sea_level, 0, 5, cross, 0, 6, cross + 1);
        p.fill(
            chunk,
            chunk_box,
            1,
            4,
            cross - 1,
            2,
            4,
            cross + 2,
            BASE_LIGHT,
        );
        p.fill(
            chunk,
            chunk_box,
            1,
            1,
            cross - 1,
            1,
            3,
            cross - 1,
            BASE_LIGHT,
        );
        p.fill(
            chunk,
            chunk_box,
            1,
            1,
            cross + 2,
            1,
            3,
            cross + 2,
            BASE_LIGHT,
        );
    }
    if opening(room, BlockDirection::East) {
        p.water(chunk, chunk_box, sea_level, 7, 5, cross, 7, 6, cross + 1);
        p.fill(
            chunk,
            chunk_box,
            5,
            4,
            cross - 1,
            6,
            4,
            cross + 2,
            BASE_LIGHT,
        );
        p.fill(
            chunk,
            chunk_box,
            6,
            1,
            cross - 1,
            6,
            3,
            cross - 1,
            BASE_LIGHT,
        );
        p.fill(
            chunk,
            chunk_box,
            6,
            1,
            cross + 2,
            6,
            3,
            cross + 2,
            BASE_LIGHT,
        );
    }
}
