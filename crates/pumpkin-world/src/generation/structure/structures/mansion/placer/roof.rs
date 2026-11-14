//! The roof of each floor: roof tops and front pieces over uncovered cells, low walls where the
//! floor above covers a cell, and the roof corners.

use pumpkin_data::Rotation;
use pumpkin_util::math::vector3::Vector3;

use super::super::{
    Direction, RotateDirection,
    grid::{MansionGrid, SimpleGrid},
    offset, relative,
};
use super::MansionPiecePlacer;

impl MansionPiecePlacer<'_> {
    fn roof_position(
        &self,
        origin: Vector3<i32>,
        rotation: Rotation,
        x: i32,
        y: i32,
    ) -> Vector3<i32> {
        let position = relative(
            origin,
            rotation.rotate(Direction::South),
            8 + (y - self.start_y) * 8,
        );
        relative(
            position,
            rotation.rotate(Direction::East),
            (x - self.start_x) * 8,
        )
    }

    pub(super) fn create_roof(
        &mut self,
        origin: Vector3<i32>,
        rotation: Rotation,
        grid: &SimpleGrid,
        above: Option<&SimpleGrid>,
    ) {
        self.add_roof_tops(origin, rotation, grid, above);

        if let Some(above) = above {
            for y in 0..grid.height() {
                for x in 0..grid.width() {
                    let position = self.roof_position(origin, rotation, x, y);
                    if !MansionGrid::is_house(grid, x, y) || !MansionGrid::is_house(above, x, y) {
                        continue;
                    }
                    self.add_small_walls(position, rotation, grid, x, y);
                }
            }
        }

        self.add_roof_corners(origin, rotation, grid, above);
    }

    /// The roof over every house cell not covered by the floor above, with its front pieces at the
    /// edges.
    fn add_roof_tops(
        &mut self,
        origin: Vector3<i32>,
        rotation: Rotation,
        grid: &SimpleGrid,
        above: Option<&SimpleGrid>,
    ) {
        for y in 0..grid.height() {
            for x in 0..grid.width() {
                let position = self.roof_position(origin, rotation, x, y);
                let covered = above.is_some_and(|above| MansionGrid::is_house(above, x, y));
                if !MansionGrid::is_house(grid, x, y) || covered {
                    continue;
                }
                self.add("roof", offset(position, 0, 3, 0), rotation);
                if !MansionGrid::is_house(grid, x + 1, y) {
                    self.add(
                        "roof_front",
                        relative(position, rotation.rotate(Direction::East), 6),
                        rotation,
                    );
                }
                if !MansionGrid::is_house(grid, x - 1, y) {
                    let position = relative(
                        relative(position, rotation.rotate(Direction::East), 0),
                        rotation.rotate(Direction::South),
                        7,
                    );
                    self.add("roof_front", position, rotation.then(Rotation::Rotate180));
                }
                if !MansionGrid::is_house(grid, x, y - 1) {
                    self.add(
                        "roof_front",
                        relative(position, rotation.rotate(Direction::West), 1),
                        rotation.then(Rotation::CounterClockwise90),
                    );
                }
                if !MansionGrid::is_house(grid, x, y + 1) {
                    let position = relative(
                        relative(position, rotation.rotate(Direction::East), 6),
                        rotation.rotate(Direction::South),
                        6,
                    );
                    self.add("roof_front", position, rotation.then(Rotation::Clockwise90));
                }
            }
        }
    }

    /// The low walls and their corners around a cell of this floor that the floor above covers.
    fn add_small_walls(
        &mut self,
        position: Vector3<i32>,
        rotation: Rotation,
        grid: &SimpleGrid,
        x: i32,
        y: i32,
    ) {
        if !MansionGrid::is_house(grid, x + 1, y) {
            self.add(
                "small_wall",
                relative(position, rotation.rotate(Direction::East), 7),
                rotation,
            );
        }
        if !MansionGrid::is_house(grid, x - 1, y) {
            let wall = relative(
                relative(position, rotation.rotate(Direction::West), 1),
                rotation.rotate(Direction::South),
                6,
            );
            self.add("small_wall", wall, rotation.then(Rotation::Rotate180));
        }
        if !MansionGrid::is_house(grid, x, y - 1) {
            let wall = relative(
                relative(position, rotation.rotate(Direction::West), 0),
                rotation.rotate(Direction::North),
                1,
            );
            self.add(
                "small_wall",
                wall,
                rotation.then(Rotation::CounterClockwise90),
            );
        }
        if !MansionGrid::is_house(grid, x, y + 1) {
            let wall = relative(
                relative(position, rotation.rotate(Direction::East), 6),
                rotation.rotate(Direction::South),
                7,
            );
            self.add("small_wall", wall, rotation.then(Rotation::Clockwise90));
        }
        if !MansionGrid::is_house(grid, x + 1, y) {
            if !MansionGrid::is_house(grid, x, y - 1) {
                let corner = relative(
                    relative(position, rotation.rotate(Direction::East), 7),
                    rotation.rotate(Direction::North),
                    2,
                );
                self.add("small_wall_corner", corner, rotation);
            }
            if !MansionGrid::is_house(grid, x, y + 1) {
                let corner = relative(
                    relative(position, rotation.rotate(Direction::East), 8),
                    rotation.rotate(Direction::South),
                    7,
                );
                self.add(
                    "small_wall_corner",
                    corner,
                    rotation.then(Rotation::Clockwise90),
                );
            }
        }
        if !MansionGrid::is_house(grid, x - 1, y) {
            if !MansionGrid::is_house(grid, x, y - 1) {
                let corner = relative(
                    relative(position, rotation.rotate(Direction::West), 2),
                    rotation.rotate(Direction::North),
                    1,
                );
                self.add(
                    "small_wall_corner",
                    corner,
                    rotation.then(Rotation::CounterClockwise90),
                );
            }
            if !MansionGrid::is_house(grid, x, y + 1) {
                let corner = relative(
                    relative(position, rotation.rotate(Direction::West), 1),
                    rotation.rotate(Direction::South),
                    8,
                );
                self.add(
                    "small_wall_corner",
                    corner,
                    rotation.then(Rotation::Rotate180),
                );
            }
        }
    }

    /// The outer and inner roof corners of the uncovered cells.
    fn add_roof_corners(
        &mut self,
        origin: Vector3<i32>,
        rotation: Rotation,
        grid: &SimpleGrid,
        above: Option<&SimpleGrid>,
    ) {
        for y in 0..grid.height() {
            for x in 0..grid.width() {
                let position = self.roof_position(origin, rotation, x, y);
                let covered = above.is_some_and(|above| MansionGrid::is_house(above, x, y));
                if !MansionGrid::is_house(grid, x, y) || covered {
                    continue;
                }
                if !MansionGrid::is_house(grid, x + 1, y) {
                    let east = relative(position, rotation.rotate(Direction::East), 6);
                    if !MansionGrid::is_house(grid, x, y + 1) {
                        self.add(
                            "roof_corner",
                            relative(east, rotation.rotate(Direction::South), 6),
                            rotation,
                        );
                    } else if MansionGrid::is_house(grid, x + 1, y + 1) {
                        self.add(
                            "roof_inner_corner",
                            relative(east, rotation.rotate(Direction::South), 5),
                            rotation,
                        );
                    }
                    if !MansionGrid::is_house(grid, x, y - 1) {
                        self.add(
                            "roof_corner",
                            east,
                            rotation.then(Rotation::CounterClockwise90),
                        );
                    } else if MansionGrid::is_house(grid, x + 1, y - 1) {
                        let corner = relative(
                            relative(position, rotation.rotate(Direction::East), 9),
                            rotation.rotate(Direction::North),
                            2,
                        );
                        self.add(
                            "roof_inner_corner",
                            corner,
                            rotation.then(Rotation::Clockwise90),
                        );
                    }
                }
                if !MansionGrid::is_house(grid, x - 1, y) {
                    if !MansionGrid::is_house(grid, x, y + 1) {
                        self.add(
                            "roof_corner",
                            relative(position, rotation.rotate(Direction::South), 6),
                            rotation.then(Rotation::Clockwise90),
                        );
                    } else if MansionGrid::is_house(grid, x - 1, y + 1) {
                        let corner = relative(
                            relative(position, rotation.rotate(Direction::South), 8),
                            rotation.rotate(Direction::West),
                            3,
                        );
                        self.add(
                            "roof_inner_corner",
                            corner,
                            rotation.then(Rotation::CounterClockwise90),
                        );
                    }
                    if !MansionGrid::is_house(grid, x, y - 1) {
                        self.add("roof_corner", position, rotation.then(Rotation::Rotate180));
                    } else if MansionGrid::is_house(grid, x - 1, y - 1) {
                        self.add(
                            "roof_inner_corner",
                            relative(position, rotation.rotate(Direction::South), 1),
                            rotation.then(Rotation::Rotate180),
                        );
                    }
                }
            }
        }
    }
}
