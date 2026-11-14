use pumpkin_data::{Block, block_properties::OakFenceLikeProperties};
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
            nether_fortress::{NetherFortressPiece, tower},
        },
    },
};

/// Large entrance tower connecting the bridge network to the interior (13 × 14 × 13).
pub struct CorridorExitPiece {
    pub piece: NetherFortressPiece,
}

impl CorridorExitPiece {
    pub fn create(
        collector: &mut StructurePiecesCollector,
        _random: &mut impl RandomImpl,
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
            StructurePieceType::NetherFortressCorridorExit,
            chain_length,
            bbox,
        );
        piece.piece.set_facing(Some(orientation));
        Some(Box::new(Self { piece }))
    }
}

impl StructurePieceBase for CorridorExitPiece {
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
        let nb = Block::NETHER_BRICKS.default_state;
        let air = Block::AIR.default_state;

        let fence = |west: bool, east: bool, north: bool, south: bool| {
            let mut props = OakFenceLikeProperties::default(&Block::NETHER_BRICK_FENCE);
            props.west = west;
            props.east = east;
            props.north = north;
            props.south = south;
            pumpkin_data::BlockState::from_id(props.to_state_id(&Block::NETHER_BRICK_FENCE))
        };

        tower::place_shell(p, chunk, bb);

        // Gate arch fence
        p.fill_with_outline(
            chunk,
            &bb,
            false,
            5,
            8,
            0,
            7,
            8,
            0,
            Block::NETHER_BRICK_FENCE.default_state,
            Block::NETHER_BRICK_FENCE.default_state,
        );
        tower::place_battlements(p, chunk, bb, fence);

        // Interior buttress fences
        let mut i = 3i32;
        while i <= 9 {
            p.fill_with_outline(
                chunk,
                &bb,
                false,
                1,
                7,
                i,
                1,
                8,
                i,
                fence(false, false, true, true), // + west connection
                fence(false, false, true, true),
            );
            p.fill_with_outline(
                chunk,
                &bb,
                false,
                11,
                7,
                i,
                11,
                8,
                i,
                fence(false, false, true, true), // + east connection
                fence(false, false, true, true),
            );
            i += 2;
        }
        tower::place_support_legs(p, chunk, bb);

        // Central lava well
        p.fill_with_outline(chunk, &bb, false, 5, 5, 5, 7, 5, 7, nb, nb);
        p.fill_with_outline(chunk, &bb, false, 6, 1, 6, 6, 4, 6, air, air);
        p.add_block(chunk, nb, 6, 0, 6, &bb);
        p.add_block(chunk, Block::LAVA.default_state, 6, 5, 6, &bb);

        let lava_pos = p.offset_pos(6, 5, 6);
        if bb.contains_pos(&lava_pos) {
            chunk.schedule_fluid_tick(
                lava_pos.x,
                lava_pos.y,
                lava_pos.z,
                &pumpkin_data::fluid::Fluid::LAVA,
            );
        }
    }
}

#[cfg(test)]
mod place_golden {
    use super::*;
    use crate::generation::proto_chunk_test::piece_golden;

    #[test]
    fn corridor_exit_places_the_same_blocks() -> Result<(), Box<dyn std::error::Error>> {
        piece_golden::check("nether_fortress_corridor_exit_place", |facing| {
            let bbox = BlockBox::rotated(8, 64, 8, -5, -3, 0, 13, 14, 13, &facing);
            let mut piece =
                NetherFortressPiece::new(StructurePieceType::NetherFortressCorridorExit, 0, bbox);
            piece.piece.set_facing(Some(facing));
            Box::new(CorridorExitPiece { piece })
        })
    }
}
