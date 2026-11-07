//! Telling neighbouring blocks that a block changed.

use super::{World, neighbor_updater};
use crate::block::OnNeighborUpdateArgs;
use crate::block::entities::BlockEntity;
use pumpkin_data::BlockDirection;
use pumpkin_data::block_properties::is_air;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_util::math::position::BlockPos;
use pumpkin_world::world::BlockFlags;
use std::sync::Arc;

impl World {
    /// Updates neighboring blocks of a block with a specified source block
    pub fn update_neighbors_at(
        self: &Arc<Self>,
        block_pos: &BlockPos,
        source_block: &Block,
        except: Option<BlockDirection>,
    ) {
        neighbor_updater::update_neighbors_at(self, block_pos, source_block, except);
    }

    /// Updates neighboring blocks of a block
    pub fn update_neighbors(
        self: &Arc<Self>,
        block_pos: &BlockPos,
        except: Option<BlockDirection>,
    ) {
        let source_block = self.get_block(block_pos);
        self.update_neighbors_at(block_pos, source_block, except);
    }

    /// Whether a dust network is settled in one pass (`[world.redstone] alternate_current`).
    pub(crate) fn settles_dust_networks(&self) -> bool {
        self.server.upgrade().map_or_else(
            || kvasir_wirenet::Config::default().alternate_current,
            |server| server.advanced_config.world.redstone.alternate_current,
        )
    }

    pub fn update_neighbor(self: &Arc<Self>, neighbor_block_pos: &BlockPos, source_block: &Block) {
        neighbor_updater::update_neighbor(self, neighbor_block_pos, source_block);
    }

    pub(super) fn execute_neighbor_update(
        self: &Arc<Self>,
        position: &BlockPos,
        source_position: &BlockPos,
        source_block: &Block,
        include_fluid: bool,
    ) {
        let (block, fluid) = if include_fluid {
            let (block, fluid) = self.get_block_and_fluid(position);
            (block, Some(fluid))
        } else {
            (self.get_block(position), None)
        };

        let mut event = crate::plugin::api::events::block::block_physics::BlockPhysicsEvent::new(
            *position,
            *source_position,
        );
        if let Some(server) = self.server.upgrade() {
            server.plugin_manager.fire_blocking(&server, &mut event);
        }
        if event.cancelled {
            return;
        }

        if let Some(pumpkin_block) = self.block_registry.get_pumpkin_block(block.id) {
            pumpkin_block.on_neighbor_update(OnNeighborUpdateArgs {
                world: self,
                block,
                position,
                source_block,
                notify: false,
            });
        }

        if let Some(fluid) = fluid
            && let Some(pumpkin_fluid) = self.block_registry.get_pumpkin_fluid(fluid.id)
        {
            pumpkin_fluid.on_neighbor_update(self, fluid, position, false);
        }
    }

    pub fn update_neighbour_for_output_signal(
        self: &Arc<Self>,
        pos: &BlockPos,
        changed_block: &Block,
    ) {
        for direction in BlockDirection::horizontal() {
            let mut relative_pos = pos.offset(direction.to_offset());
            if self.is_loaded(&relative_pos) {
                let state = self.get_block_state(&relative_pos);
                if state.id.to_block() == &Block::COMPARATOR {
                    self.update_neighbor(&relative_pos, changed_block);
                } else if state.is_solid_block() {
                    relative_pos = relative_pos.offset(direction.to_offset());
                    if self.is_loaded(&relative_pos) {
                        let second_state = self.get_block_state(&relative_pos);
                        if second_state.id.to_block() == &Block::COMPARATOR {
                            self.update_neighbor(&relative_pos, changed_block);
                        }
                    }
                }
            }
        }
    }

    /// Sends output-signal updates for block entities changed this tick, in list order.
    pub(super) fn flush_comparator_updates(
        self: &Arc<Self>,
        block_entities: &[Arc<dyn BlockEntity>],
    ) {
        for be in block_entities {
            // Vanilla `BlockEntity.setChanged` -> `Level.updateNeighbourForOutputSignal`.
            if !be.is_comparator_dirty() {
                continue;
            }
            be.clear_comparator_dirty();
            let pos = be.get_position();
            // The list is a snapshot, so the chunk may be gone by now.
            if let Some(state_id) = self.get_block_state_id_if_loaded(&pos) {
                self.update_neighbour_for_output_signal(&pos, state_id.to_block());
            }
        }
    }

    pub fn update_from_neighbor_shapes(
        self: &Arc<Self>,
        state_id: BlockStateId,
        pos: &BlockPos,
    ) -> BlockStateId {
        let mut current_state_id = state_id;
        let block = Block::from_state_id(state_id);
        for direction in BlockDirection::all() {
            let neighbor_pos = pos.offset(direction.to_offset());
            let neighbor_state_id = self.get_block_state_id(&neighbor_pos);
            current_state_id = self.block_registry.get_state_for_neighbor_update(
                self,
                block,
                current_state_id,
                pos,
                direction,
                &neighbor_pos,
                neighbor_state_id,
            );
        }
        current_state_id
    }

    pub fn replace_with_state_for_neighbor_update(
        self: &Arc<Self>,
        block_pos: &BlockPos,
        direction: BlockDirection,
        flags: BlockFlags,
    ) {
        neighbor_updater::update_shape(self, block_pos, direction, flags);
    }

    pub(super) fn execute_shape_update(
        self: &Arc<Self>,
        block_pos: &BlockPos,
        direction: BlockDirection,
        neighbor_pos: &BlockPos,
        neighbor_state_id: BlockStateId,
        flags: BlockFlags,
    ) {
        let (block, block_state_id) = self.get_block_and_state_id(block_pos);

        if flags.contains(BlockFlags::SKIP_REDSTONE_WIRE_STATE_REPLACEMENT)
            && *block == Block::REDSTONE_WIRE
        {
            return;
        }

        let new_state_id = self.block_registry.get_state_for_neighbor_update(
            self,
            block,
            block_state_id,
            block_pos,
            direction,
            neighbor_pos,
            neighbor_state_id,
        );

        if new_state_id != block_state_id {
            if is_air(new_state_id) {
                self.break_block(block_pos, None, flags | BlockFlags::NOTIFY_ALL);
            } else {
                self.set_block_state(block_pos, new_state_id, flags);
            }
        }
    }
}
