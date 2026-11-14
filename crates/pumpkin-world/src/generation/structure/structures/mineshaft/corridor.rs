//! Mineshaft corridors: supports, rails, cobwebs, cave spider spawners and chest minecarts.

use pumpkin_data::{
    Block, BlockState,
    block_properties::{
        HorizontalFacing, OakFenceLikeProperties, RailLikeProperties, RailShape,
        WallTorchLikeProperties,
    },
};
use pumpkin_nbt::compound::NbtCompound;
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

use super::{MineshaftType, generate_and_add_piece, is_falling_block, is_in_invalid_location};

pub struct MineShaftCorridor {
    pub piece: StructurePiece,
    pub shaft_type: MineshaftType,
    pub has_rails: bool,
    pub spider_corridor: bool,
    pub has_placed_spider: bool,
    pub num_sections: i32,
}

impl MineShaftCorridor {
    #[must_use]
    pub fn new(
        gen_depth: u32,
        random: &mut RandomGenerator,
        bounding_box: BlockBox,
        direction: BlockDirection,
        shaft_type: MineshaftType,
    ) -> Self {
        let has_rails = random.next_bounded_i32(3) == 0;
        let spider_corridor = !has_rails && random.next_bounded_i32(23) == 0;
        let axis = direction.get_axis();
        let num_sections = if axis == pumpkin_util::math::vector3::Axis::Z {
            (bounding_box.max.z - bounding_box.min.z + 1) / 5
        } else {
            (bounding_box.max.x - bounding_box.min.x + 1) / 5
        };

        let mut piece = StructurePiece::new(
            StructurePieceType::MineshaftCorridor,
            bounding_box,
            gen_depth,
        );
        piece.set_facing(Some(direction));

        Self {
            piece,
            shaft_type,
            has_rails,
            spider_corridor,
            has_placed_spider: false,
            num_sections,
        }
    }

    #[must_use]
    pub fn find_corridor_size(
        collector: &StructurePiecesCollector,
        random: &mut RandomGenerator,
        foot_x: i32,
        foot_y: i32,
        foot_z: i32,
        direction: BlockDirection,
    ) -> Option<BlockBox> {
        let max_len = random.next_bounded_i32(3) + 2;
        for corridor_length in (1..=max_len).rev() {
            let block_length = corridor_length * 5;
            let mut box_cand = match direction {
                BlockDirection::South => BlockBox::new(0, 0, 0, 2, 2, block_length - 1),
                BlockDirection::West => BlockBox::new(-(block_length - 1), 0, 0, 0, 2, 2),
                BlockDirection::East => BlockBox::new(0, 0, 0, block_length - 1, 2, 2),
                _ => BlockBox::new(0, 0, -(block_length - 1), 2, 2, 0),
            };
            box_cand.move_pos(foot_x, foot_y, foot_z);
            if collector.get_intersecting(&box_cand).is_none() {
                return Some(box_cand);
            }
        }
        None
    }

    #[expect(clippy::too_many_lines)]
    pub(super) fn add_corridor_children(
        bb: &BlockBox,
        depth: u32,
        orientation: BlockDirection,
        start_piece_box: &BlockBox,
        shaft_type: MineshaftType,
        collector: &mut StructurePiecesCollector,
        random: &mut RandomGenerator,
    ) {
        let end_selection = random.next_bounded_i32(4);
        let rand_y = bb.min.y - 1 + random.next_bounded_i32(3);

        match orientation {
            BlockDirection::North => {
                if end_selection <= 1 {
                    generate_and_add_piece(
                        start_piece_box,
                        collector,
                        random,
                        bb.min.x,
                        rand_y,
                        bb.min.z - 1,
                        orientation,
                        depth,
                        shaft_type,
                    );
                } else if end_selection == 2 {
                    generate_and_add_piece(
                        start_piece_box,
                        collector,
                        random,
                        bb.min.x - 1,
                        rand_y,
                        bb.min.z,
                        BlockDirection::West,
                        depth,
                        shaft_type,
                    );
                } else {
                    generate_and_add_piece(
                        start_piece_box,
                        collector,
                        random,
                        bb.max.x + 1,
                        rand_y,
                        bb.min.z,
                        BlockDirection::East,
                        depth,
                        shaft_type,
                    );
                }
            }
            BlockDirection::South => {
                if end_selection <= 1 {
                    generate_and_add_piece(
                        start_piece_box,
                        collector,
                        random,
                        bb.min.x,
                        rand_y,
                        bb.max.z + 1,
                        orientation,
                        depth,
                        shaft_type,
                    );
                } else if end_selection == 2 {
                    generate_and_add_piece(
                        start_piece_box,
                        collector,
                        random,
                        bb.min.x - 1,
                        rand_y,
                        bb.max.z - 3,
                        BlockDirection::West,
                        depth,
                        shaft_type,
                    );
                } else {
                    generate_and_add_piece(
                        start_piece_box,
                        collector,
                        random,
                        bb.max.x + 1,
                        rand_y,
                        bb.max.z - 3,
                        BlockDirection::East,
                        depth,
                        shaft_type,
                    );
                }
            }
            BlockDirection::West => {
                if end_selection <= 1 {
                    generate_and_add_piece(
                        start_piece_box,
                        collector,
                        random,
                        bb.min.x - 1,
                        rand_y,
                        bb.min.z,
                        orientation,
                        depth,
                        shaft_type,
                    );
                } else if end_selection == 2 {
                    generate_and_add_piece(
                        start_piece_box,
                        collector,
                        random,
                        bb.min.x,
                        rand_y,
                        bb.min.z - 1,
                        BlockDirection::North,
                        depth,
                        shaft_type,
                    );
                } else {
                    generate_and_add_piece(
                        start_piece_box,
                        collector,
                        random,
                        bb.min.x,
                        rand_y,
                        bb.max.z + 1,
                        BlockDirection::South,
                        depth,
                        shaft_type,
                    );
                }
            }
            BlockDirection::East => {
                if end_selection <= 1 {
                    generate_and_add_piece(
                        start_piece_box,
                        collector,
                        random,
                        bb.max.x + 1,
                        rand_y,
                        bb.min.z,
                        orientation,
                        depth,
                        shaft_type,
                    );
                } else if end_selection == 2 {
                    generate_and_add_piece(
                        start_piece_box,
                        collector,
                        random,
                        bb.max.x - 3,
                        rand_y,
                        bb.min.z - 1,
                        BlockDirection::North,
                        depth,
                        shaft_type,
                    );
                } else {
                    generate_and_add_piece(
                        start_piece_box,
                        collector,
                        random,
                        bb.max.x - 3,
                        rand_y,
                        bb.max.z + 1,
                        BlockDirection::South,
                        depth,
                        shaft_type,
                    );
                }
            }
            _ => {}
        }

        if depth < 8 {
            if orientation != BlockDirection::North && orientation != BlockDirection::South {
                let mut x = bb.min.x + 3;
                while x + 3 <= bb.max.x {
                    let sel = random.next_bounded_i32(5);
                    if sel == 0 {
                        generate_and_add_piece(
                            start_piece_box,
                            collector,
                            random,
                            x,
                            bb.min.y,
                            bb.min.z - 1,
                            BlockDirection::North,
                            depth + 1,
                            shaft_type,
                        );
                    } else if sel == 1 {
                        generate_and_add_piece(
                            start_piece_box,
                            collector,
                            random,
                            x,
                            bb.min.y,
                            bb.max.z + 1,
                            BlockDirection::South,
                            depth + 1,
                            shaft_type,
                        );
                    }
                    x += 5;
                }
            } else {
                let mut z = bb.min.z + 3;
                while z + 3 <= bb.max.z {
                    let sel = random.next_bounded_i32(5);
                    if sel == 0 {
                        generate_and_add_piece(
                            start_piece_box,
                            collector,
                            random,
                            bb.min.x - 1,
                            bb.min.y,
                            z,
                            BlockDirection::West,
                            depth + 1,
                            shaft_type,
                        );
                    } else if sel == 1 {
                        generate_and_add_piece(
                            start_piece_box,
                            collector,
                            random,
                            bb.max.x + 1,
                            bb.min.y,
                            z,
                            BlockDirection::East,
                            depth + 1,
                            shaft_type,
                        );
                    }
                    z += 5;
                }
            }
        }
    }

    const fn is_interior(
        &self,
        chunk: &ProtoChunk,
        x: i32,
        y: i32,
        z: i32,
        chunk_box: &BlockBox,
    ) -> bool {
        let pos = self.piece.offset_pos(x, y + 1, z);
        chunk_box.contains_pos(&pos) && pos.y < chunk.ocean_floor_wg_height_exclusive(pos.x, pos.z)
    }

    fn has_sturdy_neighbours(
        &self,
        chunk: &ProtoChunk,
        chunk_box: &BlockBox,
        x: i32,
        y: i32,
        z: i32,
        count: i32,
    ) -> bool {
        let world_pos = self.piece.offset_pos(x, y, z);
        let mut sturdy = 0;
        for dir in [
            pumpkin_data::BlockDirection::Down,
            pumpkin_data::BlockDirection::Up,
            pumpkin_data::BlockDirection::North,
            pumpkin_data::BlockDirection::South,
            pumpkin_data::BlockDirection::West,
            pumpkin_data::BlockDirection::East,
        ] {
            let neighbor_pos = world_pos + dir.to_offset();
            if chunk_box.contains_pos(&neighbor_pos) {
                let neighbor_state = chunk.get_block_state(&neighbor_pos);
                if neighbor_state.to_state().is_side_solid(dir.opposite()) {
                    sturdy += 1;
                    if sturdy >= count {
                        return true;
                    }
                }
            }
        }
        false
    }

    #[expect(clippy::too_many_arguments)]
    fn maybe_place_cobweb(
        &self,
        chunk: &mut ProtoChunk,
        chunk_box: &BlockBox,
        random: &mut RandomGenerator,
        prob: f32,
        x: i32,
        y: i32,
        z: i32,
    ) {
        if self.is_interior(chunk, x, y, z, chunk_box)
            && random.next_f32() < prob
            && self.has_sturdy_neighbours(chunk, chunk_box, x, y, z, 2)
        {
            self.piece
                .add_block(chunk, Block::COBWEB.default_state, x, y, z, chunk_box);
        }
    }

    fn is_supporting_box(
        &self,
        chunk: &ProtoChunk,
        chunk_box: &BlockBox,
        x0: i32,
        x1: i32,
        y1: i32,
        z0: i32,
    ) -> bool {
        for x in x0..=x1 {
            let world_pos = self.piece.offset_pos(x, y1 + 1, z0);
            if !chunk_box.contains_pos(&world_pos) {
                return false;
            }
            if chunk.get_block_state(&world_pos).to_state().is_air() {
                return false;
            }
        }
        true
    }

    #[expect(clippy::too_many_arguments)]
    fn place_support(
        &self,
        chunk: &mut ProtoChunk,
        chunk_box: &BlockBox,
        x0: i32,
        y0: i32,
        z: i32,
        y1: i32,
        x1: i32,
        random: &mut RandomGenerator,
    ) {
        if !self.is_supporting_box(chunk, chunk_box, x0, x1, y1, z) {
            return;
        }

        let planks = self.shaft_type.planks();
        let fence_block = self.shaft_type.fence_block();

        let mut props_w = OakFenceLikeProperties::default(fence_block);
        props_w.west = true;
        let fence_w = BlockState::from_id(props_w.to_state_id(fence_block));

        let mut props_e = OakFenceLikeProperties::default(fence_block);
        props_e.east = true;
        let fence_e = BlockState::from_id(props_e.to_state_id(fence_block));

        self.piece
            .fill(chunk, chunk_box, x0, y0, z, x0, y1 - 1, z, fence_w);
        self.piece
            .fill(chunk, chunk_box, x1, y0, z, x1, y1 - 1, z, fence_e);

        if random.next_bounded_i32(4) == 0 {
            self.piece
                .fill(chunk, chunk_box, x0, y1, z, x0, y1, z, planks);
            self.piece
                .fill(chunk, chunk_box, x1, y1, z, x1, y1, z, planks);
        } else {
            self.piece
                .fill(chunk, chunk_box, x0, y1, z, x1, y1, z, planks);

            let mut props_s = WallTorchLikeProperties::default(&Block::WALL_TORCH);
            props_s.facing = HorizontalFacing::South;
            let torch_s = BlockState::from_id(props_s.to_state_id(&Block::WALL_TORCH));

            let mut props_n = WallTorchLikeProperties::default(&Block::WALL_TORCH);
            props_n.facing = HorizontalFacing::North;
            let torch_n = BlockState::from_id(props_n.to_state_id(&Block::WALL_TORCH));

            if random.next_f32() < 0.05 {
                self.piece
                    .add_block(chunk, torch_s, x0 + 1, y1, z - 1, chunk_box);
            }
            if random.next_f32() < 0.05 {
                self.piece
                    .add_block(chunk, torch_n, x0 + 1, y1, z + 1, chunk_box);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn create_chest(
        &self,
        chunk: &mut ProtoChunk,
        chunk_box: &BlockBox,
        random: &mut RandomGenerator,
        x: i32,
        y: i32,
        z: i32,
        loot_table: &str,
    ) -> bool {
        let pos = self.piece.offset_pos(x, y, z);
        if chunk_box.contains_pos(&pos) {
            let state = chunk.get_block_state(&pos);
            let below_pos = Vector3::new(pos.x, pos.y - 1, pos.z);
            let below_state = chunk.get_block_state(&below_pos);
            if state.to_state().is_air() && !below_state.to_state().is_air() {
                let shape = if random.next_bool() {
                    RailShape::NorthSouth
                } else {
                    RailShape::EastWest
                };
                let mut rail_props = RailLikeProperties::default(&Block::RAIL);
                rail_props.shape = shape;
                let rail_state = BlockState::from_id(rail_props.to_state_id(&Block::RAIL));
                self.piece.add_block(chunk, rail_state, x, y, z, chunk_box);
                let seed = random.next_i64();

                let mut nbt = NbtCompound::new();
                nbt.put_string("id", "minecraft:chest_minecart".to_string());
                nbt.put_list(
                    "Pos",
                    vec![
                        (f64::from(pos.x) + 0.5).into(),
                        (f64::from(pos.y) + 0.5).into(),
                        (f64::from(pos.z) + 0.5).into(),
                    ],
                );
                nbt.put_list("Motion", vec![0.0f64.into(), 0.0f64.into(), 0.0f64.into()]);
                nbt.put_list("Rotation", vec![0.0f32.into(), 0.0f32.into()]);
                nbt.put_string("LootTable", loot_table.to_string());
                if seed != 0 {
                    nbt.put_long("LootTableSeed", seed);
                }
                chunk.add_structure_entity(nbt);
                return true;
            }
        }
        false
    }

    fn set_planks_block(
        &self,
        chunk: &mut ProtoChunk,
        chunk_box: &BlockBox,
        planks: &'static BlockState,
        x: i32,
        y: i32,
        z: i32,
    ) {
        if !self.is_interior(chunk, x, y, z, chunk_box) {
            return;
        }
        let pos = self.piece.offset_pos(x, y, z);
        let existing_state = chunk.get_block_state(&pos);
        if !existing_state
            .to_state()
            .is_side_solid(BlockDirection::Up.into())
        {
            chunk.set_block_state(pos.x, pos.y, pos.z, planks);
        }
    }

    fn place_double_lower_or_upper_support(
        &self,
        chunk: &mut ProtoChunk,
        chunk_box: &BlockBox,
        x: i32,
        y: i32,
        z: i32,
    ) {
        let wood = self.shaft_type.wood();
        let planks_id = self.shaft_type.planks().id.to_block_id();

        let pos1 = self.piece.offset_pos(x, y, z);
        if chunk_box.contains_pos(&pos1) && chunk.get_block_state(&pos1).to_block_id() == planks_id
        {
            self.fill_pillar_down_or_chain_up(chunk, wood, x, y, z, chunk_box);
        }

        let pos2 = self.piece.offset_pos(x + 2, y, z);
        if chunk_box.contains_pos(&pos2) && chunk.get_block_state(&pos2).to_block_id() == planks_id
        {
            self.fill_pillar_down_or_chain_up(chunk, wood, x + 2, y, z, chunk_box);
        }
    }

    fn fill_pillar_down_or_chain_up(
        &self,
        chunk: &mut ProtoChunk,
        pillar_state: &'static BlockState,
        x: i32,
        y: i32,
        z: i32,
        chunk_box: &BlockBox,
    ) {
        let world_pos = self.piece.offset_pos(x, y, z);
        if !chunk_box.contains_pos(&world_pos) {
            return;
        }

        let world_y = world_pos.y;
        let mut distance_from_world_y = 1;
        let mut check_below = true;
        let mut check_above = true;

        let min_y = chunk.bottom_y() as i32;
        let max_y = chunk.bottom_y() as i32 + chunk.height() as i32 - 1;

        while check_below || check_above {
            if check_below {
                let below_y = world_y - distance_from_world_y;
                let below_pos = Vector3::new(world_pos.x, below_y, world_pos.z);
                let below_state = chunk.get_block_state(&below_pos);
                let empty_below = StructurePiece::is_replaceable_by_structures(
                    below_state.to_state(),
                    below_state.to_block(),
                ) && below_state.to_block_id() != Block::LAVA.id;

                if !empty_below
                    && below_state
                        .to_state()
                        .is_side_solid(BlockDirection::Up.into())
                {
                    for py in (below_y + 1)..world_y {
                        chunk.set_block_state(world_pos.x, py, world_pos.z, pillar_state);
                    }
                    return;
                }
                check_below = distance_from_world_y <= 20 && empty_below && below_y > min_y + 1;
            }

            if check_above {
                let above_y = world_y + distance_from_world_y;
                let above_pos = Vector3::new(world_pos.x, above_y, world_pos.z);
                let above_state = chunk.get_block_state(&above_pos);
                let empty_above = StructurePiece::is_replaceable_by_structures(
                    above_state.to_state(),
                    above_state.to_block(),
                );

                if !empty_above
                    && above_state
                        .to_state()
                        .is_center_solid(BlockDirection::Down.into())
                    && !is_falling_block(above_state.to_block())
                {
                    chunk.set_block_state(
                        world_pos.x,
                        world_y + 1,
                        world_pos.z,
                        self.shaft_type.fence(),
                    );
                    for py in (world_y + 2)..above_y {
                        chunk.set_block_state(
                            world_pos.x,
                            py,
                            world_pos.z,
                            Block::IRON_CHAIN.default_state,
                        );
                    }
                    return;
                }
                check_above = distance_from_world_y <= 50 && empty_above && above_y < max_y;
            }

            distance_from_world_y += 1;
        }
    }
}

impl StructurePieceBase for MineShaftCorridor {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn get_structure_piece(&self) -> &StructurePiece {
        &self.piece
    }

    fn get_structure_piece_mut(&mut self) -> &mut StructurePiece {
        &mut self.piece
    }

    #[allow(clippy::too_many_lines)]
    fn place(
        &mut self,
        chunk: &mut ProtoChunk,
        _block_registry: &dyn WorldPortalExt,
        random: &mut RandomGenerator,
        _seed: i64,
        chunk_box: &BlockBox,
    ) {
        if is_in_invalid_location(&self.piece.bounding_box, chunk, chunk_box) {
            return;
        }

        let air = Block::CAVE_AIR.default_state;
        let length = self.num_sections * 5 - 1;
        let planks = self.shaft_type.planks();

        self.piece
            .fill(chunk, chunk_box, 0, 0, 0, 2, 1, length, air);

        for y in 2..=2 {
            for x in 0..=2 {
                for z in 0..=length {
                    if random.next_f32() <= 0.8 {
                        self.piece.add_block(chunk, air, x, y, z, chunk_box);
                    }
                }
            }
        }

        if self.spider_corridor {
            for y in 0..=1 {
                for x in 0..=2 {
                    for z in 0..=length {
                        if random.next_f32() <= 0.6 && self.is_interior(chunk, x, y, z, chunk_box) {
                            self.piece.add_block(
                                chunk,
                                Block::COBWEB.default_state,
                                x,
                                y,
                                z,
                                chunk_box,
                            );
                        }
                    }
                }
            }
        }

        for section in 0..self.num_sections {
            let z = 2 + section * 5;
            self.place_support(chunk, chunk_box, 0, 0, z, 2, 2, random);
            self.maybe_place_cobweb(chunk, chunk_box, random, 0.1, 0, 2, z - 1);
            self.maybe_place_cobweb(chunk, chunk_box, random, 0.1, 2, 2, z - 1);
            self.maybe_place_cobweb(chunk, chunk_box, random, 0.1, 0, 2, z + 1);
            self.maybe_place_cobweb(chunk, chunk_box, random, 0.1, 2, 2, z + 1);
            self.maybe_place_cobweb(chunk, chunk_box, random, 0.05, 0, 2, z - 2);
            self.maybe_place_cobweb(chunk, chunk_box, random, 0.05, 2, 2, z - 2);
            self.maybe_place_cobweb(chunk, chunk_box, random, 0.05, 0, 2, z + 2);
            self.maybe_place_cobweb(chunk, chunk_box, random, 0.05, 2, 2, z + 2);

            if random.next_bounded_i32(100) == 0 {
                self.create_chest(
                    chunk,
                    chunk_box,
                    random,
                    2,
                    0,
                    z - 1,
                    "minecraft:chests/abandoned_mineshaft",
                );
            }
            if random.next_bounded_i32(100) == 0 {
                self.create_chest(
                    chunk,
                    chunk_box,
                    random,
                    0,
                    0,
                    z + 1,
                    "minecraft:chests/abandoned_mineshaft",
                );
            }

            if self.spider_corridor && !self.has_placed_spider {
                let spawner_z = z - 1 + random.next_bounded_i32(3);
                let spawner_pos = self.piece.offset_pos(1, 0, spawner_z);
                if chunk_box.contains_pos(&spawner_pos)
                    && self.is_interior(chunk, 1, 0, spawner_z, chunk_box)
                {
                    self.has_placed_spider = true;
                    chunk.set_block_state(
                        spawner_pos.x,
                        spawner_pos.y,
                        spawner_pos.z,
                        Block::SPAWNER.default_state,
                    );

                    let mut nbt = NbtCompound::new();
                    nbt.put_string("id", "minecraft:mob_spawner".to_string());
                    nbt.put_int("x", spawner_pos.x);
                    nbt.put_int("y", spawner_pos.y);
                    nbt.put_int("z", spawner_pos.z);
                    let mut spawn_data = NbtCompound::new();
                    let mut entity = NbtCompound::new();
                    entity.put_string("id", "minecraft:cave_spider".to_string());
                    spawn_data.put_compound("entity", entity);
                    nbt.put_compound("SpawnData", spawn_data);
                    chunk.add_block_entity(nbt);
                }
            }
        }

        for x in 0..=2 {
            for z2 in 0..=length {
                self.set_planks_block(chunk, chunk_box, planks, x, -1, z2);
            }
        }

        self.place_double_lower_or_upper_support(chunk, chunk_box, 0, -1, 2);
        if self.num_sections > 1 {
            let last_support = length - 2;
            self.place_double_lower_or_upper_support(chunk, chunk_box, 0, -1, last_support);
        }

        if self.has_rails {
            let mut rail_props = RailLikeProperties::default(&Block::RAIL);
            rail_props.shape = RailShape::NorthSouth;
            let rail_state = BlockState::from_id(rail_props.to_state_id(&Block::RAIL));
            for z3 in 0..=length {
                let floor_pos = self.piece.offset_pos(1, -1, z3);
                let floor = if chunk_box.contains_pos(&floor_pos) {
                    chunk.get_block_state(&floor_pos)
                } else {
                    Block::AIR.default_state.id
                };
                if !floor.to_state().is_air() && floor.to_state().is_full_cube() {
                    let prob = if self.is_interior(chunk, 1, 0, z3, chunk_box) {
                        0.7
                    } else {
                        0.9
                    };
                    if random.next_f32() < prob {
                        self.piece.add_block(chunk, rail_state, 1, 0, z3, chunk_box);
                    }
                }
            }
        }
    }
}
