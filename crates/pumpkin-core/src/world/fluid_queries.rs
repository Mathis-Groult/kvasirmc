//! Fluids: reading them and scheduling their ticks.

use super::{World, fluid_states};
use pumpkin_data::block_properties::blocks_movement;
use pumpkin_data::fluid::FluidState;
use pumpkin_data::{BlockDirection, HorizontalFacingExt};
use pumpkin_data::{BlockStateId, fluid::Fluid};
use pumpkin_util::math::{boundingbox::BoundingBox, position::BlockPos, vector3::Vector3};
use pumpkin_world::tick::TickPriority;

impl World {
    pub fn contains_any_liquid(&self, bounding_box: BoundingBox) -> bool {
        let min_x = bounding_box.min.x.floor() as i32;
        let max_x = bounding_box.max.x.ceil() as i32;
        let min_y = bounding_box.min.y.floor() as i32;
        let max_y = bounding_box.max.y.ceil() as i32;
        let min_z = bounding_box.min.z.floor() as i32;
        let max_z = bounding_box.max.z.ceil() as i32;

        for x in min_x..max_x {
            for y in min_y..max_y {
                for z in min_z..max_z {
                    let pos = BlockPos::new(x, y, z);
                    if self.get_fluid_and_fluid_state(&pos).0.id != Fluid::EMPTY.id {
                        return true;
                    }
                }
            }
        }

        false
    }

    // FlowingFluid.getFlow()
    pub fn get_fluid_velocity(
        &self,
        pos0: BlockPos,
        fluid0: &Fluid,
        state0: &FluidState,
    ) -> Vector3<f64> {
        let mut velo = Vector3::default();

        for dir in BlockDirection::horizontal() {
            let offset = dir.to_offset();
            let pos = pos0.offset(offset);

            let (neighbor_fluid, neighbor_state) = self.get_fluid_and_fluid_state(&pos);

            if neighbor_fluid.matches_type(fluid0) {
                let mut neighbor_height = neighbor_state.height;
                let mut amplitude = 0.0;

                if neighbor_height == 0.0 {
                    let state_id = self.get_block_state_id(&pos);
                    let block_id = state_id.to_block_id();
                    let block_state = state_id.to_state();

                    let blocks_movement = blocks_movement(block_state, block_id);

                    if !blocks_movement {
                        let down_pos = pos.down();
                        let (down_fluid, down_state) = self.get_fluid_and_fluid_state(&down_pos);

                        if down_fluid.matches_type(fluid0) {
                            neighbor_height = down_state.height;
                            if neighbor_height > 0.0 {
                                amplitude = f64::from(state0.height)
                                    - (f64::from(neighbor_height) - 0.888_888_9);
                            }
                        }
                    }
                } else if neighbor_height > 0.0 {
                    amplitude = f64::from(state0.height) - f64::from(neighbor_height);
                }

                if amplitude != 0.0 {
                    velo.x += f64::from(offset.x) * amplitude;
                    velo.z += f64::from(offset.z) * amplitude;
                }
            }
        }

        if state0.falling {
            for dir in BlockDirection::horizontal() {
                let pos = pos0.offset(dir.to_offset());

                if self.is_solid_face(fluid0.id, pos, dir.to_block_direction())
                    || self.is_solid_face(fluid0.id, pos.up(), dir.to_block_direction())
                {
                    if velo.length_squared() != 0.0 {
                        velo = velo.normalize();
                    }

                    velo.y -= 6.0;
                    break;
                }
            }
        }

        if velo.length_squared() == 0.0 {
            velo
        } else {
            velo.normalize()
        }
    }

    pub fn schedule_fluid_tick(
        &self,
        fluid: &Fluid,
        block_pos: BlockPos,
        delay: u8,
        priority: TickPriority,
    ) {
        self.level
            .schedule_fluid_tick(fluid, block_pos, delay, priority);
    }

    pub fn is_fluid_tick_scheduled(&self, block_pos: &BlockPos, fluid: &Fluid) -> bool {
        self.level.is_fluid_tick_scheduled(block_pos, fluid)
    }

    pub(crate) fn get_fluid_from_state_id(id: BlockStateId) -> &'static pumpkin_data::fluid::Fluid {
        if !fluid_states::holds_fluid(id) {
            return &Fluid::EMPTY;
        }
        Self::fluid_of_state(id)
    }

    /// The fluid a block state holds, worked out from the state (see `fluid_states` for the
    /// table that answers the common "none" at once).
    pub(super) fn fluid_of_state(id: BlockStateId) -> &'static pumpkin_data::fluid::Fluid {
        if let Some(fluid) = Fluid::from_state_id(id) {
            return fluid.to_flowing();
        }
        // These blocks contain source water without a `waterlogged` property.
        if matches!(
            id.to_block_id(),
            pumpkin_data::BlockId::KELP
                | pumpkin_data::BlockId::KELP_PLANT
                | pumpkin_data::BlockId::SEAGRASS
                | pumpkin_data::BlockId::TALL_SEAGRASS
                | pumpkin_data::BlockId::BUBBLE_COLUMN
        ) || id.is_waterlogged()
        {
            &Fluid::FLOWING_WATER
        } else {
            &Fluid::EMPTY
        }
    }

    pub(super) fn fluid_state_from_block_state(id: BlockStateId) -> (&'static Fluid, FluidState) {
        let fluid = Self::get_fluid_from_state_id(id);
        if fluid.id == Fluid::EMPTY.id {
            // Not a fluid block (almost every block): nothing of the below applies.
            let empty = &Fluid::EMPTY;
            return (
                fluid,
                empty.states[empty.default_state_index as usize].clone(),
            );
        }
        let source = if fluid.matches_type(&Fluid::WATER) {
            &Fluid::WATER
        } else if fluid.matches_type(&Fluid::LAVA) {
            &Fluid::LAVA
        } else {
            &Fluid::EMPTY
        };
        let mut state = source.states[source.default_state_index as usize].clone();

        if matches!(
            id.to_block_id(),
            pumpkin_data::BlockId::WATER | pumpkin_data::BlockId::LAVA
        ) {
            // LiquidBlock#getFluidState: source, amounts 7..1, then falling amount 8.
            // The fluid family's first state is not the state of the actual block.
            let level =
                pumpkin_data::block_properties::WaterLikeProperties::from_state_id(id).level;
            let amount = if level == 0 || level >= 8 {
                8
            } else {
                8 - level
            };
            state.height = f32::from(amount) / 9.0;
            state.level = i16::from(amount);
            state.is_source = level == 0;
            state.is_still = state.is_source;
            state.falling = level >= 8;
            state.block_state_id = pumpkin_data::block_properties::WaterLikeProperties {
                level: level.min(8),
            }
            .to_state_id(id.to_block());
        }

        // Keep the normalized family used by fluid callbacks, independently of source state.
        (fluid, state)
    }

    pub fn get_fluid(&self, position: &BlockPos) -> &'static pumpkin_data::fluid::Fluid {
        let id = self.get_block_state_id(position);
        Self::get_fluid_from_state_id(id)
    }

    pub fn get_block_and_fluid(
        &self,
        position: &BlockPos,
    ) -> (
        &'static pumpkin_data::Block,
        &'static pumpkin_data::fluid::Fluid,
    ) {
        let id = self.get_block_state_id(position);
        (id.to_block(), Self::get_fluid_from_state_id(id))
    }

    pub fn get_fluid_and_fluid_state(&self, position: &BlockPos) -> (&'static Fluid, FluidState) {
        let id = self.get_block_state_id(position);
        Self::fluid_state_from_block_state(id)
    }

    /// `FluidState#getHeight` includes the full block when the same fluid is above.
    /// Keep `state.height` as the own height used by flow-velocity calculations.
    pub fn get_fluid_height(&self, position: &BlockPos, fluid: &Fluid, state: &FluidState) -> f32 {
        if state.is_empty {
            0.0
        } else if fluid.matches_type(self.get_fluid(&position.up())) {
            1.0
        } else {
            state.height
        }
    }
}
