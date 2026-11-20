use pumpkin_data::BlockStateId;
use pumpkin_data::biome::Biome;
use pumpkin_data::block_properties::HorizontalAxis;
use pumpkin_data::dimension::Dimension;
use pumpkin_data::fluid::Fluid;
use pumpkin_data::{Block, BlockDirection};
use pumpkin_macros::pumpkin_block;
use pumpkin_util::math::position::BlockPos;
use pumpkin_world::tick::TickPriority;
use pumpkin_world::world::{BlockAccessor, BlockFlags};
use rand::RngExt;
use std::sync::Arc;

use crate::block::blocks::tnt::TNTBlock;
use crate::block::{
    BlockBehaviour, BrokenArgs, CanPlaceAtArgs, GetStateForNeighborUpdateArgs,
    OnEntityCollisionArgs, OnScheduledTickArgs, PlacedArgs,
};
use crate::world::World;
use crate::world::portal::nether::NetherPortal;

type FireProperties = pumpkin_data::block_properties::FireLikeProperties;

use super::FireBlockBase;

mod tick;

#[pumpkin_block("minecraft:fire")]
pub struct FireBlock;

impl FireBlock {
    #[must_use]
    pub fn get_fire_tick_delay() -> i32 {
        30 + rand::rng().random_range(0..10)
    }

    fn is_flammable(id: BlockStateId) -> bool {
        let block = id.to_block();

        if block.is_waterlogged(id) {
            return false;
        }

        block.flammable.as_ref().is_some_and(|f| f.burn_chance > 0)
    }

    fn are_blocks_around_flammable(block_accessor: &dyn BlockAccessor, pos: &BlockPos) -> bool {
        for direction in BlockDirection::all() {
            let neighbor_pos = pos.offset(direction.to_offset());
            let state_id = block_accessor.get_block_state_id(&neighbor_pos);
            if Self::is_flammable(state_id) {
                return true;
            }
        }
        false
    }

    pub fn get_state_for_position(
        &self,
        world: &World,
        block: &Block,
        pos: &BlockPos,
    ) -> BlockStateId {
        let down_pos = pos.down();
        let down_state = world.get_block_state(&down_pos);
        if Self::is_flammable(down_state.id) || down_state.is_side_solid(BlockDirection::Up) {
            return block.default_state.id;
        }
        let mut fire_props = FireProperties::from_state_id(block.default_state.id);
        for direction in BlockDirection::all() {
            let neighbor_pos = pos.offset(direction.to_offset());
            let neighbor_state_id = world.get_block_state_id(&neighbor_pos);
            if Self::is_flammable(neighbor_state_id) {
                match direction {
                    BlockDirection::North => fire_props.north = true,
                    BlockDirection::South => fire_props.south = true,
                    BlockDirection::East => fire_props.east = true,
                    BlockDirection::West => fire_props.west = true,
                    BlockDirection::Up => fire_props.up = true,
                    BlockDirection::Down => {}
                }
            }
        }
        fire_props.to_state_id(block)
    }

    // Used for spreading fire
    pub fn get_burn_chance(&self, world: &Arc<World>, pos: &BlockPos) -> i32 {
        let block_state = world.get_block_state(pos);
        if !block_state.is_air() {
            return 0;
        }
        let mut total_burn_chance = 0;

        for dir in BlockDirection::all() {
            let neighbor_block = world.get_block(&pos.offset(dir.to_offset()));
            if *world.get_fluid(&pos.offset(dir.to_offset())) != Fluid::EMPTY {
                continue; // Skip if there is a fluid
            }
            if let Some(flammable) = &neighbor_block.flammable {
                total_burn_chance = total_burn_chance.max(i32::from(flammable.spread_chance));
            }
        }

        total_burn_chance
    }

    fn is_near_rain(world: &World, pos: &BlockPos) -> bool {
        world.is_raining_at(pos)
            || world.is_raining_at(&pos.west())
            || world.is_raining_at(&pos.east())
            || world.is_raining_at(&pos.north())
            || world.is_raining_at(&pos.south())
    }

    // Get burn odds for a block, used in try_spreading_fire
    fn get_burn_odds(block: &Block) -> i32 {
        block.flammable.as_ref().map_or(0, |f| f.burn_chance.into())
    }

    fn is_increased_burnout_biome(world: &World, pos: &BlockPos) -> bool {
        // Fire burnout increases in the Nether
        if world.dimension == Dimension::THE_NETHER {
            return true;
        }

        // Fire burnout increases in specific biomes
        // TODO: Use proper tag or bool for this when available
        let biome_id = world.level.get_rough_biome(pos).id;
        matches!(
            biome_id,
            id if id == Biome::BAMBOO_JUNGLE.id
                || id == Biome::MUSHROOM_FIELDS.id
                || id == Biome::MANGROVE_SWAMP.id
                || id == Biome::SNOWY_SLOPES.id
                || id == Biome::FROZEN_PEAKS.id
                || id == Biome::JAGGED_PEAKS.id
                || id == Biome::SWAMP.id
                || id == Biome::JUNGLE.id
        )
    }

    fn try_spreading_fire(&self, world: &Arc<World>, pos: &BlockPos, chance: i32, age: u8) {
        let state_id = world.get_block_state_id(pos);
        let block = state_id.to_block();
        if block.is_waterlogged(state_id) {
            return;
        }
        let odds = Self::get_burn_odds(block);
        if rand::rng().random_range(0..chance) < odds {
            if let Some(server) = world.server.upgrade() {
                let mut event = crate::plugin::api::events::block::block_burn::BlockBurnEvent {
                    world: world.clone(),
                    igniting_block: &Block::FIRE,
                    block,
                    block_pos: *pos,
                    cancelled: false,
                };
                server.plugin_manager.fire_blocking(&server, &mut event);
                if event.cancelled {
                    return;
                }
            }
            let old_block = block;
            if rand::rng().random_range(0..(age + 10) as i32) < 5
                && !Self::is_near_rain(world.as_ref(), pos)
            {
                let new_age = (age + (rand::rng().random_range(0..5) / 4)).min(15) as u8;
                let state_id = self.get_state_for_position(world.as_ref(), &Block::FIRE, pos);
                let mut fire_props = FireProperties::from_state_id(state_id);
                fire_props.age = new_age;
                let new_state_id = fire_props.to_state_id(&Block::FIRE);
                world.set_block_state(pos, new_state_id, BlockFlags::NOTIFY_ALL);
            } else {
                world.set_block_state(pos, Block::AIR.default_state.id, BlockFlags::NOTIFY_ALL);
            }

            // The TNT was already replaced above, so `prime` would find no TNT here.
            if old_block == &Block::TNT {
                TNTBlock::spawn_primed(world, pos);
            }
        }
    }
}

impl BlockBehaviour for FireBlock {
    fn placed(&self, args: PlacedArgs<'_>) {
        if args.old_state_id == args.state_id {
            // Already a fire
            return;
        }

        let dimension = &args.world.dimension;
        // First lets check if we are in OverWorld or Nether, its not possible to place an Nether portal in other dimensions in Vanilla
        if (dimension == &Dimension::OVERWORLD || dimension == &Dimension::THE_NETHER)
            && let Some(portal) =
                NetherPortal::get_new_portal(args.world, args.position, HorizontalAxis::X)
        {
            portal.create(args.world);
            return;
        }

        args.world.schedule_block_tick(
            args.block,
            *args.position,
            Self::get_fire_tick_delay() as u8,
            TickPriority::Normal,
        );
    }

    fn on_entity_collision(&self, args: OnEntityCollisionArgs<'_>) {
        FireBlockBase::apply_fire_collision(&args, false);
    }

    fn get_state_for_neighbor_update(
        &self,
        args: GetStateForNeighborUpdateArgs<'_>,
    ) -> BlockStateId {
        if self.can_place_at(CanPlaceAtArgs {
            server: None,
            world: Some(args.world),
            block_accessor: args.world,
            block: &Block::FIRE,
            state: Block::FIRE.default_state,
            position: args.position,
            direction: None,
            player: None,
            use_item_on: None,
        }) {
            let age = FireProperties::from_state_id(args.state_id).age;
            let state_id = self.get_state_for_position(args.world, args.block, args.position);
            let mut props = FireProperties::from_state_id(state_id);
            props.age = age;
            props.to_state_id(args.block)
        } else {
            Block::AIR.default_state.id
        }
    }

    fn can_place_at(&self, args: CanPlaceAtArgs<'_>) -> bool {
        let state = args.block_accessor.get_block_state(&args.position.down());
        if state.is_side_solid(BlockDirection::Up) {
            return true;
        }
        Self::are_blocks_around_flammable(args.block_accessor, args.position)
    }

    fn on_scheduled_tick(&self, args: OnScheduledTickArgs<'_>) {
        let (world, block, pos) = (args.world, args.block, args.position);

        // Schedule next tick first
        world.schedule_block_tick(
            block,
            *pos,
            Self::get_fire_tick_delay() as u8,
            TickPriority::Normal,
        );

        // Check if fire can survive
        if !self.can_place_at(CanPlaceAtArgs {
            server: None,
            world: Some(world),
            block_accessor: world.as_ref(),
            block,
            state: block.default_state,
            position: pos,
            direction: None,
            player: None,
            use_item_on: None,
        }) {
            world.set_block_state(
                pos,
                Block::AIR.default_state.id,
                BlockFlags::NOTIFY_NEIGHBORS,
            );
            return;
        }

        let block_state = world.get_block_state(pos);
        let block_below = world.get_block(&pos.down());

        let infiniburn = Self::is_infiniburn(world.dimension.id, block_below);

        let mut fire_props = FireProperties::from_state_id(block_state.id);
        let age = fire_props.age;

        // Check if rain should extinguish the fire
        if !infiniburn && Self::is_near_rain(world.as_ref(), pos) {
            let rain_chance = 0.2 + (age as f32) * 0.03;
            if rand::random::<f32>() < rain_chance {
                world.set_block_state(
                    pos,
                    Block::AIR.default_state.id,
                    BlockFlags::NOTIFY_NEIGHBORS,
                );
                return;
            }
        }

        // Increment age
        let random = (rand::rng().random_range(0..3) / 2) as u8;
        let new_age = (age + random).min(15);
        if new_age != age {
            fire_props.age = new_age;
            let new_state_id = fire_props.to_state_id(&Block::FIRE);
            world.set_block_state(pos, new_state_id, BlockFlags::NOTIFY_ALL);
        }

        if !infiniburn && Self::burns_out(world, pos, new_age) {
            return;
        }

        self.burn_adjacent(world, pos, new_age);
        self.spread_to_nearby(world, block, pos, new_age);
    }

    fn broken(&self, args: BrokenArgs<'_>) {
        {
            FireBlockBase::broken(args.world, *args.position);
        }
    }
}
