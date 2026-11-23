//! Rebuilding a proto chunk from saved chunk data (`ProtoChunk::from_chunk_data`).

use pumpkin_data::BlockState;

use super::ProtoChunk;
use crate::chunk::palette::{BiomePalette, BlockPalette};
use crate::chunk::{ChunkData, ChunkHeightmapType, ChunkSections};
use crate::chunk_system::StagedChunkEnum;
use crate::generation::generator::WorldGenerator;

impl ProtoChunk {
    /// Takes over the light, blending data and block entities the chunk data carries.
    pub(super) fn restore_saved_parts(&mut self, chunk_data: &ChunkData) {
        self.light = chunk_data
            .light_engine
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        self.blending_data.clone_from(&chunk_data.blending_data);

        // Chunks loaded from disk carry their block entities in the chunk data.
        // Keep them so the world can create the block entities when the chunk
        // becomes active, and so saving the chunk again does not drop them.
        self.pending_block_entities = chunk_data
            .pending_block_entities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .cloned()
            .collect();
    }

    /// Copies the block and biome sections that lie inside the proto chunk's height.
    pub(super) fn restore_sections(&mut self, section_data: &ChunkSections) {
        let block_sections = section_data
            .block_sections
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let biome_sections = section_data
            .biome_sections
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        for (section_idx, block_palette) in block_sections.iter().enumerate() {
            let section_base_y = section_idx as i32 * 16;

            if section_base_y >= self.height() as i32 {
                continue;
            }

            self.restore_blocks(block_palette, section_base_y + section_data.min_y);
            if let Some(biome_palette) = biome_sections.get(section_idx) {
                self.restore_biomes(biome_palette, section_idx);
            }
        }
    }

    fn restore_blocks(&mut self, block_palette: &BlockPalette, section_floor_y: i32) {
        for x in 0..16 {
            for y in 0..16 {
                for z in 0..16 {
                    let block_state = BlockState::from_id(block_palette.get(x, y, z));
                    self.set_block_state(
                        x as i32,
                        section_floor_y + y as i32,
                        z as i32,
                        block_state,
                    );
                }
            }
        }
    }

    fn restore_biomes(&mut self, biome_palette: &BiomePalette, section_idx: usize) {
        for x in 0..4 {
            for y in 0..4 {
                for z in 0..4 {
                    let biome_y_idx = (section_idx * 4) + y;
                    let index =
                        self.local_biome_pos_to_biome_index(x as i32, biome_y_idx as i32, z as i32);
                    self.flat_biome_map[index] = biome_palette.get(x, y, z);
                }
            }
        }
    }

    /// Reads the saved heightmaps into the flat maps (the ocean floor map keeps what the block
    /// writes made of it).
    pub(super) fn restore_height_maps(&mut self, chunk_data: &ChunkData) {
        let heightmap_data = chunk_data
            .heightmap
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let min_y = chunk_data.section.min_y;

        for z in 0..16 {
            for x in 0..16 {
                let index = Self::local_position_to_height_map_index(x, z);
                let saved = |kind| heightmap_data.get(kind, x, z, min_y) as i16;

                self.flat_motion_blocking_height_map[index] =
                    saved(ChunkHeightmapType::MotionBlocking);
                self.flat_motion_blocking_no_leaves_height_map[index] =
                    saved(ChunkHeightmapType::MotionBlockingNoLeaves);
                let surface_height = saved(ChunkHeightmapType::WorldSurface);
                self.flat_surface_height_map[index] = surface_height;
                self.flat_surface_wg_height_map[index] = surface_height;
                self.flat_ocean_floor_wg_height_map[index] =
                    self.flat_ocean_floor_height_map[index];
            }
        }
    }

    /// Sets the saved stage; a noise generator first rebuilds the structure starts (and the
    /// references) that are not saved.
    pub(super) fn resume_stage(
        &mut self,
        saved_stage: StagedChunkEnum,
        generator: &WorldGenerator,
    ) {
        self.stage = saved_stage;
        if let WorldGenerator::Noise(generator) = generator
            && (StagedChunkEnum::StructureStart..StagedChunkEnum::Features).contains(&saved_stage)
        {
            // Structure starts and references are currently transient proto-chunk data.
            // Rebuild them when resuming a partially generated chunk so structures that
            // cross chunk boundaries are not truncated at the unload boundary.
            self.stage = StagedChunkEnum::Biomes;
            self.set_structure_starts(generator);
            if saved_stage >= StagedChunkEnum::StructureReferences {
                self.set_structure_references(generator);
            }
            self.stage = saved_stage;
        }
    }
}
