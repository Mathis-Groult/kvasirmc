use pumpkin_data::{
    Block,
    block_properties::{HorizontalFacing, OakFenceLikeProperties, WhiteWoolStairsLikeProperties},
};
use pumpkin_util::{BlockDirection, math::block_box::BlockBox, random::RandomGenerator};

use crate::{
    ProtoChunk,
    generation::structure::{
        piece::StructurePieceType,
        structures::{
            StructurePiece, StructurePieceBase, StructurePiecesCollector, WorldPortalExt,
            nether_fortress::{NetherFortressPiece, tower},
        },
    },
};

/// Large interior room with nether wart crops (13 × 14 × 13).
pub struct CorridorNetherWartsRoomPiece {
    pub piece: NetherFortressPiece,
}

impl CorridorNetherWartsRoomPiece {
    pub fn create(
        collector: &mut StructurePiecesCollector,
        x: i32,
        y: i32,
        z: i32,
        orientation: BlockDirection,
        chain_length: u32,
    ) -> Option<Box<dyn StructurePieceBase>> {
        let bbox = BlockBox::rotated(x, y, z, -5, -3, 0, 13, 14, 13, &orientation);
        if !NetherFortressPiece::is_in_bounds(&bbox) || collector.get_intersecting(&bbox).is_some()
        {
            return None;
        }

        let mut piece = NetherFortressPiece::new(
            StructurePieceType::NetherFortressCorridorNetherWartsRoom,
            chain_length,
            bbox,
        );
        piece.piece.set_facing(Some(orientation));
        Some(Box::new(Self { piece }))
    }

    /// The stairs up to the top exit, their balcony fences and the opening above them.
    fn place_staircase(
        p: &StructurePiece,
        chunk: &mut ProtoChunk,
        bb: BlockBox,
        f_ns_west: &'static pumpkin_data::BlockState,
        f_ns_east: &'static pumpkin_data::BlockState,
    ) {
        let nb = Block::NETHER_BRICKS.default_state;
        let air = Block::AIR.default_state;

        // Staircase going up (north-facing stairs, going from z=4 up to z=10)
        let stair_n = nether_brick_stairs(HorizontalFacing::North);

        for j in 0..=6i32 {
            let k = j + 4;
            for l in 5..=7i32 {
                p.add_block(chunk, stair_n, l, 5 + j, k, &bb);
            }
            if (5..=8).contains(&k) {
                p.fill_with_outline(chunk, &bb, false, 5, 5, k, 7, j + 4, k, nb, nb);
            } else if (9..=10).contains(&k) {
                p.fill_with_outline(chunk, &bb, false, 5, 8, k, 7, j + 4, k, nb, nb);
            }
            if j >= 1 {
                p.fill_with_outline(chunk, &bb, false, 5, 6 + j, k, 7, 9 + j, k, air, air);
            }
        }
        for l in 5..=7i32 {
            p.add_block(chunk, stair_n, l, 12, 11, &bb);
        }

        // Stair balcony fences
        p.fill_with_outline(chunk, &bb, false, 5, 6, 7, 5, 7, 7, f_ns_east, f_ns_east);
        p.fill_with_outline(chunk, &bb, false, 7, 6, 7, 7, 7, 7, f_ns_west, f_ns_west);

        // Clear the arch above the top stair exit
        p.fill_with_outline(chunk, &bb, false, 5, 13, 12, 7, 13, 12, air, air);
    }

    /// The two raised soul sand beds with nether wart, edged with stairs.
    fn place_wart_beds(p: &StructurePiece, chunk: &mut ProtoChunk, bb: BlockBox) {
        let nb = Block::NETHER_BRICKS.default_state;
        let stair_e = nether_brick_stairs(HorizontalFacing::East);
        let stair_w = nether_brick_stairs(HorizontalFacing::West);

        // Wart beds (raised platforms of soul sand with nether wart on top)
        p.fill_with_outline(chunk, &bb, false, 2, 5, 2, 3, 5, 3, nb, nb);
        p.fill_with_outline(chunk, &bb, false, 2, 5, 9, 3, 5, 10, nb, nb);
        p.fill_with_outline(chunk, &bb, false, 2, 5, 4, 2, 5, 8, nb, nb);
        p.fill_with_outline(chunk, &bb, false, 9, 5, 2, 10, 5, 3, nb, nb);
        p.fill_with_outline(chunk, &bb, false, 9, 5, 9, 10, 5, 10, nb, nb);
        p.fill_with_outline(chunk, &bb, false, 10, 5, 4, 10, 5, 8, nb, nb);

        // Stair edgings for the wart beds
        p.add_block(chunk, stair_w, 4, 5, 2, &bb);
        p.add_block(chunk, stair_w, 4, 5, 3, &bb);
        p.add_block(chunk, stair_w, 4, 5, 9, &bb);
        p.add_block(chunk, stair_w, 4, 5, 10, &bb);
        p.add_block(chunk, stair_e, 8, 5, 2, &bb);
        p.add_block(chunk, stair_e, 8, 5, 3, &bb);
        p.add_block(chunk, stair_e, 8, 5, 9, &bb);
        p.add_block(chunk, stair_e, 8, 5, 10, &bb);

        let soul_sand = Block::SOUL_SAND.default_state;
        let nether_wart = Block::NETHER_WART.default_state;

        p.fill_with_outline(chunk, &bb, false, 3, 4, 4, 4, 4, 8, soul_sand, soul_sand);
        p.fill_with_outline(chunk, &bb, false, 8, 4, 4, 9, 4, 8, soul_sand, soul_sand);
        p.fill_with_outline(
            chunk,
            &bb,
            false,
            3,
            5,
            4,
            4,
            5,
            8,
            nether_wart,
            nether_wart,
        );
        p.fill_with_outline(
            chunk,
            &bb,
            false,
            8,
            5,
            4,
            9,
            5,
            8,
            nether_wart,
            nether_wart,
        );
    }
}

impl StructurePieceBase for CorridorNetherWartsRoomPiece {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn get_structure_piece(&self) -> &StructurePiece {
        &self.piece.piece
    }

    fn get_structure_piece_mut(&mut self) -> &mut StructurePiece {
        &mut self.piece.piece
    }

    fn place(
        &mut self,
        chunk: &mut ProtoChunk,
        _block_registry: &dyn WorldPortalExt,
        _random: &mut RandomGenerator,
        _seed: i64,
        chunk_box: &BlockBox,
    ) {
        let bb = *chunk_box;
        let p = &self.piece.piece;

        let fence = |west: bool, east: bool, north: bool, south: bool| {
            let mut props = OakFenceLikeProperties::default(&Block::NETHER_BRICK_FENCE);
            props.west = west;
            props.east = east;
            props.north = north;
            props.south = south;
            pumpkin_data::BlockState::from_id(props.to_state_id(&Block::NETHER_BRICK_FENCE))
        };
        let f_ns_west = fence(true, false, true, true);
        let f_ns_east = fence(false, true, true, true);

        tower::place_shell(p, chunk, bb);
        tower::place_battlements(p, chunk, bb, fence);

        // Interior buttress fences
        let mut i = 3i32;
        while i <= 9 {
            p.fill_with_outline(chunk, &bb, false, 1, 7, i, 1, 8, i, f_ns_west, f_ns_west);
            p.fill_with_outline(chunk, &bb, false, 11, 7, i, 11, 8, i, f_ns_east, f_ns_east);
            i += 2;
        }

        Self::place_staircase(p, chunk, bb, f_ns_west, f_ns_east);
        Self::place_wart_beds(p, chunk, bb);
        tower::place_support_legs(p, chunk, bb);
    }
}

/// A nether brick stair facing `facing`, its other properties at their defaults.
fn nether_brick_stairs(facing: HorizontalFacing) -> &'static pumpkin_data::BlockState {
    let mut stair_props = WhiteWoolStairsLikeProperties::default(&Block::NETHER_BRICK_STAIRS);
    stair_props.facing = facing;
    pumpkin_data::BlockState::from_id(stair_props.to_state_id(&Block::NETHER_BRICK_STAIRS))
}

#[cfg(test)]
mod place_golden {
    use super::*;
    use crate::generation::proto_chunk_test::piece_golden;

    #[test]
    fn nether_warts_room_places_the_same_blocks() -> Result<(), Box<dyn std::error::Error>> {
        piece_golden::check("nether_fortress_nether_warts_room_place", |facing| {
            let bbox = BlockBox::rotated(8, 64, 8, -5, -3, 0, 13, 14, 13, &facing);
            let mut piece = NetherFortressPiece::new(
                StructurePieceType::NetherFortressCorridorNetherWartsRoom,
                0,
                bbox,
            );
            piece.piece.set_facing(Some(facing));
            Box::new(CorridorNetherWartsRoomPiece { piece })
        })
    }
}
