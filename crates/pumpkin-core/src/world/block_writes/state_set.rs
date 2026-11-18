//! The steps of `World::on_block_state_set` after the chunk write: the replaced block entity,
//! client sync with neighbour and indirect shape updates, and villager/portal POIs.

use std::sync::Arc;

use pumpkin_data::{Block, BlockStateId};
use pumpkin_util::math::position::BlockPos;
use pumpkin_world::world::BlockFlags;

use crate::world::{World, villager_poi};

impl World {
    /// Removes the block entity of a replaced block whose new block keeps no entity of the
    /// same type, after its replaced callback unless the flags skip it.
    pub(super) fn remove_replaced_block_entity(
        self: &Arc<Self>,
        position: &BlockPos,
        old_block: &Block,
        new_block: &Block,
        is_new_block: bool,
        flags: BlockFlags,
    ) {
        if is_new_block
            && old_block.default_state.block_entity_type != u16::MAX
            && old_block.default_state.block_entity_type
                != new_block.default_state.block_entity_type
            && let Some(entity) = self.get_block_entity(position)
        {
            if !flags.contains(BlockFlags::SKIP_BLOCK_ENTITY_REPLACED_CALLBACK) {
                entity.on_block_replaced(self, position);
            }
            self.remove_block_entity(position);
        }
    }

    /// Level.java `setBlock` once the state is in place: client sync, neighbour updates,
    /// comparator output and the indirect shape updates of the old and the new state.
    pub(super) fn notify_after_block_state_set(
        self: &Arc<Self>,
        position: &BlockPos,
        old_block: &Block,
        new_block: &Block,
        replaced_block_state_id: BlockStateId,
        block_state_id: BlockStateId,
        flags: BlockFlags,
    ) {
        if flags.contains(BlockFlags::NOTIFY_LISTENERS) {
            self.send_block_updated(position, replaced_block_state_id, block_state_id);
        }

        if flags.contains(BlockFlags::NOTIFY_NEIGHBORS) {
            self.update_neighbors_at(position, old_block, None);
            if block_state_id.has_analog_output_signal() {
                self.update_neighbour_for_output_signal(position, new_block);
            }
        }

        if !flags.intersects(BlockFlags::MOVED | BlockFlags::UPDATE_KNOWN_SHAPE) {
            let mut neighbour_update_flags = flags;
            neighbour_update_flags.remove(BlockFlags::NOTIFY_NEIGHBORS);
            neighbour_update_flags.remove(BlockFlags::SKIP_DROPS);
            self.block_registry.prepare(
                self,
                position,
                old_block,
                replaced_block_state_id,
                neighbour_update_flags,
            );
            self.block_registry
                .update_neighbors(self, position, neighbour_update_flags);
            self.block_registry.prepare(
                self,
                position,
                new_block,
                block_state_id,
                neighbour_update_flags,
            );
        }
    }

    /// Moves the villager job site and the POI entry from the old block to the new one.
    pub(super) fn update_poi_after_block_state_set(
        &self,
        position: &BlockPos,
        old_block: &Block,
        new_block: &Block,
        is_new_block: bool,
    ) {
        self.villager_poi
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .update_block(*position, new_block);

        if is_new_block {
            let mut poi = self
                .portal_poi
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if villager_poi::profession_for_block(old_block).is_some() {
                poi.remove(position);
            }
            if let Some(poi_type) = villager_poi::poi_type_for_block(new_block) {
                poi.add_with_free_tickets(*position, poi_type, 1);
            }
        }
    }
}
