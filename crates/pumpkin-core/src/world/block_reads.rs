//! Reading blocks, heights, biomes and world bounds.

use super::World;
use pumpkin_data::BlockState;
use pumpkin_data::block_properties::is_air;
use pumpkin_data::chunk::Biome;
use pumpkin_data::dimension::Dimension;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_world::chunk::ChunkHeightmapType::{self};

impl World {
    #[must_use]
    pub fn is_flat(&self) -> bool {
        // Vanilla ServerLevel.isFlat uses the Overworld generator for every dimension.
        self.level_info
            .load()
            .world_gen_settings
            .dimensions
            .get(Dimension::OVERWORLD.minecraft_name)
            .is_some_and(|dimension| dimension.generator.generator_type == "minecraft:flat")
    }

    pub async fn get_block_state_id_async(&self, position: &BlockPos) -> BlockStateId {
        if !self.is_in_build_limit(*position) {
            return Block::AIR.default_state.id;
        }

        let (chunk_coordinate, relative) = position.chunk_and_chunk_relative_position();
        self.level
            .get_or_fetch_chunk(chunk_coordinate, |chunk| {
                chunk
                    .section
                    .get_block_absolute_y(relative.x as usize, relative.y, relative.z as usize)
                    .unwrap_or(Block::AIR.default_state.id)
            })
            .await
    }

    pub async fn get_block_state_async(&self, position: &BlockPos) -> &'static BlockState {
        let id = self.get_block_state_id_async(position).await;
        BlockState::from_id(id)
    }

    pub async fn get_heightmap_height_async(
        &self,
        height_map: ChunkHeightmapType,
        x: i32,
        z: i32,
    ) -> i32 {
        let chunk_pos = Vector2::new(x >> 4, z >> 4);
        self.level
            .get_or_fetch_chunk(chunk_pos, |chunk| {
                chunk
                    .heightmap
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(height_map, x, z, self.min_y)
            })
            .await
    }

    /// Gets the y position of the first non air block from the top down
    pub fn get_top_block(&self, position: Vector2<i32>) -> i32 {
        let chunk_pos = Vector2::new(position.x >> 4, position.y >> 4);
        let relative_x = (position.x & 15) as usize;
        let relative_z = (position.y & 15) as usize;

        self.level
            .read_chunk_sync(&chunk_pos, |chunk| {
                let height = chunk
                    .heightmap
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(
                        ChunkHeightmapType::WorldSurface,
                        position.x,
                        position.y,
                        self.dimension.min_y,
                    );

                if height >= self.dimension.min_y {
                    return height;
                }

                for y in (self.dimension.min_y..self.dimension.min_y + self.dimension.height).rev()
                {
                    if let Some(block_id) = chunk
                        .section
                        .get_block_absolute_y(relative_x, y, relative_z)
                        && !is_air(block_id)
                    {
                        return y;
                    }
                }
                self.dimension.min_y
            })
            .unwrap_or(self.dimension.min_y)
    }

    pub fn get_heightmap_height(&self, height_map: ChunkHeightmapType, x: i32, z: i32) -> i32 {
        let chunk_pos = Vector2::new(x >> 4, z >> 4);
        self.level
            .read_chunk_sync(&chunk_pos, |chunk| {
                chunk
                    .heightmap
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(height_map, x, z, self.min_y)
            })
            .unwrap_or(self.min_y)
    }

    pub(super) const fn is_within_build_height(&self, position: &BlockPos) -> bool {
        position.0.y >= self.dimension.min_y
            && position.0.y < self.dimension.min_y + self.dimension.height
    }

    pub fn get_biome(&self, position: &BlockPos) -> &'static Biome {
        let chunk_pos = position.chunk_position();
        if let Some(chunk) = self.level.loaded_chunks.get(&chunk_pos) {
            let id = chunk
                .section
                .get_rough_biome_absolute_y(
                    (position.0.x & 15) as usize,
                    position.0.y,
                    (position.0.z & 15) as usize,
                )
                .unwrap_or(0);
            Biome::from_id(id).unwrap_or(&Biome::PLAINS)
        } else {
            &Biome::PLAINS
        }
    }

    #[must_use]
    pub fn is_valid(dest: BlockPos) -> bool {
        Self::is_valid_horizontally(dest) && Self::is_valid_vertically(dest.0.y)
    }

    #[must_use]
    pub fn is_valid_horizontally(dest: BlockPos) -> bool {
        // Note: 30_000_000 is not valid, but -30_000_000 is.
        (-30_000_000..30_000_000).contains(&dest.0.x)
            && (-30_000_000..30_000_000).contains(&dest.0.z)
    }

    #[must_use]
    pub fn is_valid_vertically(y: i32) -> bool {
        // Note: 20_000_000 is not valid, but -20_000_000 is.
        (-20_000_000..20_000_000).contains(&y)
    }

    #[must_use]
    pub fn is_in_build_limit(&self, dest: BlockPos) -> bool {
        self.is_in_height_limit(dest.0.y) && Self::is_valid_horizontally(dest)
    }

    #[must_use]
    pub fn is_in_height_limit(&self, y: i32) -> bool {
        (self.get_bottom_y()..=self.get_top_y()).contains(&y)
    }

    pub const fn get_bottom_y(&self) -> i32 {
        self.dimension.min_y
    }

    pub const fn get_top_y(&self) -> i32 {
        self.dimension.min_y + self.dimension.height - 1
    }

    /// Gets a `Block` from the block registry. Returns `Block::AIR` if the block was not found.
    pub fn get_block(&self, position: &BlockPos) -> &'static Block {
        self.get_block_state_id_if_loaded(position)
            .map_or(&Block::AIR, Block::from_state_id)
    }

    #[must_use]
    pub fn get_block_state_id_if_loaded(&self, position: &BlockPos) -> Option<BlockStateId> {
        if !self.is_in_build_limit(*position) {
            return None;
        }

        let (chunk_coordinate, relative) = position.chunk_and_chunk_relative_position();
        self.level.read_chunk_sync(&chunk_coordinate, |chunk| {
            chunk
                .section
                .get_block_absolute_y(relative.x as usize, relative.y, relative.z as usize)
        })?
    }

    #[must_use]
    pub fn get_block_state_if_loaded(&self, position: &BlockPos) -> Option<&'static BlockState> {
        self.get_block_state_id_if_loaded(position)
            .map(BlockState::from_id)
    }

    #[must_use]
    pub fn is_loaded(&self, position: &BlockPos) -> bool {
        self.get_block_state_id_if_loaded(position).is_some()
    }

    pub fn get_block_state_id(&self, position: &BlockPos) -> BlockStateId {
        self.get_block_state_id_if_loaded(position)
            .unwrap_or(Block::AIR.default_state.id)
    }

    /// Gets the `BlockState` from the block registry. Returns Air if the block state was not found.
    pub fn get_block_state(&self, position: &BlockPos) -> &'static BlockState {
        let id = self.get_block_state_id(position);
        BlockState::from_id(id)
    }

    /// Gets the Block + Block state from the Block Registry, Returns Air if the Block state has not been found
    pub fn get_block_and_state(
        &self,
        position: &BlockPos,
    ) -> (&'static Block, &'static BlockState) {
        let id = self.get_block_state_id(position);
        BlockState::from_id_with_block(id)
    }

    /// Gets the Block + state id from the Block Registry, Returns Air if the Block state has not been found
    pub fn get_block_and_state_id(&self, position: &BlockPos) -> (&'static Block, BlockStateId) {
        let id = self.get_block_state_id(position);
        (Block::from_state_id(id), id)
    }
}
