//! Template-based structure piece matching vanilla Minecraft `TemplateStructurePiece`.
//!
//! This module provides `TemplatePiece`, which implements `StructurePieceBase`
//! to place blocks from a loaded structure template into the world.

use std::sync::Arc;

use pumpkin_data::{BlockState, Mirror, Rotation};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_util::{
    math::{block_box::BlockBox, vector3::Vector3},
    random::{RandomGenerator, RandomImpl, hash_block_pos, legacy_rand::LegacyRand},
};

use crate::{
    ProtoChunk,
    generation::structure::{
        piece::StructurePieceType,
        structures::{StructurePiece, StructurePieceBase},
    },
    world::WorldPortalExt,
};

use super::{
    BlockStateResolver, PaletteEntry, StructurePlaceSettings, StructureTemplate, TemplateBlock,
};

/// A structure piece that places blocks from an NBT template matching vanilla `TemplateStructurePiece`.
///
/// This piece handles:
/// - Loading blocks from a `StructureTemplate`
/// - Applying rotation, mirror, and pivot transformations from `StructurePlaceSettings`
/// - Placing blocks at world coordinates
/// - Replacing jigsaw blocks with their `final_state`
/// - Skipping structure void blocks
pub struct TemplatePiece {
    /// The underlying structure piece metadata.
    pub piece: StructurePiece,

    /// The template identifier/name.
    pub template_name: String,

    /// The template to place.
    pub template: Arc<StructureTemplate>,

    /// Placement settings (rotation, mirror, pivot, processors, etc.).
    pub place_settings: StructurePlaceSettings,

    /// The origin position of the template in the world.
    pub template_position: Vector3<i32>,
}

impl TemplatePiece {
    /// Creates a new template piece from the given `template` at `origin` with legacy rotation & mirror.
    #[must_use]
    pub fn new(
        template: Arc<StructureTemplate>,
        rotation: Rotation,
        mirror: Mirror,
        origin: Vector3<i32>,
        piece_type: StructurePieceType,
    ) -> Self {
        let place_settings = StructurePlaceSettings::new()
            .set_rotation(rotation)
            .set_mirror(mirror);
        let bounding_box = template.get_bounding_box(&place_settings, origin);

        Self {
            piece: StructurePiece::new(piece_type, bounding_box, 0),
            template_name: String::new(),
            template,
            place_settings,
            template_position: origin,
        }
    }

    /// Creates a new template piece with full `StructurePlaceSettings` matching vanilla `TemplateStructurePiece`.
    #[must_use]
    pub fn from_settings(
        template: Arc<StructureTemplate>,
        template_name: String,
        place_settings: StructurePlaceSettings,
        origin: Vector3<i32>,
        piece_type: StructurePieceType,
    ) -> Self {
        let bounding_box = template.get_bounding_box(&place_settings, origin);

        Self {
            piece: StructurePiece::new(piece_type, bounding_box, 0),
            template_name,
            template,
            place_settings,
            template_position: origin,
        }
    }

    /// Creates a template piece with a specific chain length.
    #[must_use]
    pub fn with_chain_length(
        template: Arc<StructureTemplate>,
        rotation: Rotation,
        mirror: Mirror,
        origin: Vector3<i32>,
        piece_type: StructurePieceType,
        chain_length: u32,
    ) -> Self {
        let mut piece = Self::new(template, rotation, mirror, origin, piece_type);
        piece.piece.chain_length = chain_length;
        piece
    }

    /// Returns the template's size after applying rotation.
    #[must_use]
    pub fn rotated_size(&self) -> Vector3<i32> {
        self.place_settings
            .get_rotation()
            .transform_size(self.template.size)
    }

    /// Gets the rotation configured in place settings.
    #[must_use]
    pub const fn get_rotation(&self) -> Rotation {
        self.place_settings.get_rotation()
    }

    /// Gets a reference to the loaded template.
    #[must_use]
    pub const fn template(&self) -> &Arc<StructureTemplate> {
        &self.template
    }

    /// Gets the world template position.
    #[must_use]
    pub const fn template_position(&self) -> Vector3<i32> {
        self.template_position
    }

    /// Gets a reference to the structure placement settings.
    #[must_use]
    pub const fn place_settings(&self) -> &StructurePlaceSettings {
        &self.place_settings
    }

    /// Gets a mutable reference to the structure placement settings.
    pub const fn place_settings_mut(&mut self) -> &mut StructurePlaceSettings {
        &mut self.place_settings
    }

    /// Transforms a template-relative position to world coordinates.
    #[must_use]
    pub fn transform_pos(&self, local_pos: Vector3<i32>) -> Vector3<i32> {
        StructureTemplate::transform_block_pos(
            local_pos,
            self.place_settings.get_mirror(),
            self.place_settings.get_rotation(),
            self.place_settings.get_rotation_pivot(),
        ) + self.template_position
    }

    /// Moves the template position and bounding box by `(dx, dy, dz)`.
    pub fn move_piece(&mut self, dx: i32, dy: i32, dz: i32) {
        self.piece.translate(dx, dy, dz);
        self.template_position += Vector3::new(dx, dy, dz);
    }

    /// Checks if a block name is structure void (should not be placed).
    fn is_structure_void(name: &str) -> bool {
        name == "minecraft:structure_void" || name == "structure_void"
    }

    /// Places all blocks from the template into the chunk matching vanilla `postProcess`.
    fn place_blocks(&mut self, chunk: &mut ProtoChunk, chunk_box: &BlockBox) {
        self.place_settings = self
            .place_settings
            .clone()
            .set_bounding_box(Some(*chunk_box));
        self.piece.bounding_box = self
            .template
            .get_bounding_box(&self.place_settings, self.template_position);
        let box_limit = self.piece.bounding_box;

        for block in &self.template.blocks {
            self.place_block(chunk, chunk_box, &box_limit, block);
        }
    }

    /// The palette entry and block entity data a template block places: `None` for structure
    /// blocks (data markers), structure voids and a jigsaw whose final state is a void; a
    /// jigsaw is replaced with its `final_state` and loses its own block entity data.
    fn placed_entry(&self, block: &TemplateBlock) -> Option<(PaletteEntry, Option<NbtCompound>)> {
        let palette_entry = &self.template.palette[block.state as usize];
        if palette_entry.name == "minecraft:structure_block" {
            return None;
        }
        let (placed_entry, block_entity_nbt) = if palette_entry.name == "minecraft:jigsaw" {
            let final_state = block
                .nbt
                .as_ref()
                .and_then(|nbt| nbt.get_string("final_state"))
                .unwrap_or("minecraft:air");
            (PaletteEntry::from_string(final_state), None)
        } else {
            (palette_entry.clone(), block.nbt.clone())
        };
        if Self::is_structure_void(&placed_entry.name) {
            return None;
        }
        Some((placed_entry, block_entity_nbt))
    }

    /// Writes one template block: transform, bounds, waterlogging, block state, block entity.
    fn place_block(
        &self,
        chunk: &mut ProtoChunk,
        chunk_box: &BlockBox,
        box_limit: &BlockBox,
        block: &TemplateBlock,
    ) {
        let Some((mut placed_entry, block_entity_nbt)) = self.placed_entry(block) else {
            return;
        };
        let rotation = self.place_settings.get_rotation();
        let mirror = self.place_settings.get_mirror();
        let Some(mut state) = BlockStateResolver::resolve(&placed_entry, rotation, mirror) else {
            return;
        };

        let world_pos = self.transform_pos(block.pos);
        // Both the piece bounding box and the current chunk box limit the write.
        if !box_limit.contains_pos(&world_pos) || !chunk_box.contains_pos(&world_pos) {
            return;
        }

        if self.place_settings.should_apply_waterlogging()
            && chunk.get_block_state(&world_pos).to_block_id() == pumpkin_data::Block::WATER.id
            && let Some(waterlogged) = placed_entry.properties.iter_mut().find_map(|p| match p {
                pumpkin_data::Property::Waterlogged(w) => Some(w),
                _ => None,
            })
        {
            *waterlogged = true;
            if let Some(waterlogged_state) =
                BlockStateResolver::resolve(&placed_entry, rotation, mirror)
            {
                state = waterlogged_state;
            }
        }

        chunk.set_block_state(world_pos.x, world_pos.y, world_pos.z, state);
        if let Some(nbt) = Self::block_entity(state, world_pos, block_entity_nbt.as_ref()) {
            chunk.add_block_entity(nbt);
        }
    }

    /// The block entity a placed block gets: the template data without its position and id, a
    /// loot table without a seed seeded from the position; `None` when the block has neither
    /// template data nor a block entity type.
    fn block_entity(
        state: &BlockState,
        world_pos: Vector3<i32>,
        template_nbt: Option<&NbtCompound>,
    ) -> Option<NbtCompound> {
        let final_block = pumpkin_data::Block::from_id(state.id.to_block_id());
        let block_entity_id = super::get_block_entity_id(final_block.name);
        if template_nbt.is_none() && block_entity_id.is_none() {
            return None;
        }
        let fallback_id = block_entity_id.unwrap_or(final_block.name);
        let mut placed_nbt = NbtCompound::new();
        placed_nbt.put_string("id", fallback_id.to_string());
        placed_nbt.put_int("x", world_pos.x);
        placed_nbt.put_int("y", world_pos.y);
        placed_nbt.put_int("z", world_pos.z);

        if let Some(template_nbt) = template_nbt {
            for (key, value) in &template_nbt.child_tags {
                if !matches!(key.as_ref(), "x" | "y" | "z" | "id") {
                    placed_nbt.child_tags.insert(key.clone(), value.clone());
                }
            }
        }

        if placed_nbt.get_string("LootTable").is_some()
            && placed_nbt.get_long("LootTableSeed").is_none()
        {
            let mut random =
                LegacyRand::from_seed(hash_block_pos(world_pos.x, world_pos.y, world_pos.z) as u64);
            placed_nbt.put_long("LootTableSeed", random.next_i64());
        }
        Some(placed_nbt)
    }
}

impl StructurePieceBase for TemplatePiece {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn place(
        &mut self,
        chunk: &mut ProtoChunk,
        _block_registry: &dyn WorldPortalExt,
        _random: &mut RandomGenerator,
        _seed: i64,
        chunk_box: &BlockBox,
    ) {
        self.place_blocks(chunk, chunk_box);
    }

    fn translate(&mut self, x: i32, y: i32, z: i32) {
        self.move_piece(x, y, z);
    }

    fn get_structure_piece(&self) -> &StructurePiece {
        &self.piece
    }

    fn get_structure_piece_mut(&mut self) -> &mut StructurePiece {
        &mut self.piece
    }
}

#[cfg(test)]
#[path = "template_piece_golden.rs"]
mod golden;
