//! The parts of a fire's scheduled tick: going out, burning the blocks around, and spreading.

use std::sync::Arc;

use pumpkin_data::dimension::Dimension;
use pumpkin_data::tag::{self, Taggable};
use pumpkin_data::{Block, BlockDirection};
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::world::BlockFlags;
use rand::RngExt;

use super::{FireBlock, FireProperties};
use crate::world::World;

/// The chance, for each side of a fire, that the block there is burnt away.
const BURN_SIDES: [(BlockDirection, i32); 6] = [
    (BlockDirection::East, 300),
    (BlockDirection::West, 300),
    (BlockDirection::Down, 250),
    (BlockDirection::Up, 250),
    (BlockDirection::North, 300),
    (BlockDirection::South, 300),
];

/// What decides whether a fire spreads to one nearby cell.
#[derive(Clone, Copy)]
struct Ignition {
    height: i32,
    new_age: u8,
    difficulty: i32,
    spread_radius: i64,
}

impl FireBlock {
    /// Whether the block below keeps fire burning forever in this dimension.
    pub(super) fn is_infiniburn(dimension_id: u8, block_below: &Block) -> bool {
        match dimension_id {
            id if id == Dimension::OVERWORLD.id => {
                block_below.has_tag(&tag::Block::MINECRAFT_INFINIBURN_OVERWORLD)
            }
            id if id == Dimension::THE_NETHER.id => {
                block_below.has_tag(&tag::Block::MINECRAFT_INFINIBURN_NETHER)
            }
            id if id == Dimension::THE_END.id => {
                block_below.has_tag(&tag::Block::MINECRAFT_INFINIBURN_END)
            }
            _ => false,
        }
    }

    /// Puts the fire out when it has nothing to burn or is old on bare ground, and says so.
    pub(super) fn burns_out(world: &Arc<World>, pos: &BlockPos, new_age: u8) -> bool {
        // Check if fire should extinguish due to lack of fuel
        if !Self::are_blocks_around_flammable(world.as_ref(), pos) {
            let block_below_state = world.get_block_state(&pos.down());
            if !block_below_state.is_side_solid(BlockDirection::Up) || new_age > 3 {
                world.set_block_state(pos, Block::AIR.default_state.id, BlockFlags::NOTIFY_ALL);
                return true;
            }
        }

        // At max age, fire has a chance to extinguish if not on flammable block
        if new_age == 15
            && rand::rng().random_range(0..4) == 0
            && !Self::is_flammable(world.get_block_state_id(&pos.down()))
        {
            world.set_block_state(pos, Block::AIR.default_state.id, BlockFlags::NOTIFY_ALL);
            return true;
        }
        false
    }

    /// Burn adjacent blocks.
    pub(super) fn burn_adjacent(&self, world: &Arc<World>, pos: &BlockPos, new_age: u8) {
        let extra = if Self::is_increased_burnout_biome(world, pos) {
            -50 // Increases chance of block being destroyed
        } else {
            0
        };
        for (direction, chance) in BURN_SIDES {
            self.try_spreading_fire(
                world,
                &pos.offset(direction.to_offset()),
                chance + extra,
                new_age,
            );
        }
    }

    /// Try to spread fire to nearby air blocks.
    pub(super) fn spread_to_nearby(
        &self,
        world: &Arc<World>,
        block: &Block,
        pos: &BlockPos,
        new_age: u8,
    ) {
        // Respect the `fire_spread_radius_around_player` gamerule.
        // -1 = disabled (allow unlimited spread), 0 = disabled (no spread), >0 = radius in blocks
        let spread_radius = world
            .level_info
            .load()
            .game_rules
            .fire_spread_radius_around_player;

        let difficulty = world.level_info.load().difficulty as i32;
        for xx in -1..=1 {
            for zz in -1..=1 {
                for yy in -1..=4 {
                    if xx != 0 || yy != 0 || zz != 0 {
                        let offset_pos = pos.offset(Vector3::new(xx, yy, zz));
                        let ignition = Ignition {
                            height: yy,
                            new_age,
                            difficulty,
                            spread_radius,
                        };
                        self.try_ignite(world, block, pos, &offset_pos, ignition);
                    }
                }
            }
        }
    }

    /// Maybe sets fire to the air block `offset_pos` beside the fire at `pos`.
    fn try_ignite(
        &self,
        world: &Arc<World>,
        block: &Block,
        pos: &BlockPos,
        offset_pos: &BlockPos,
        ignition: Ignition,
    ) {
        let Ignition {
            height: yy,
            new_age,
            difficulty,
            spread_radius,
        } = ignition;
        let ignite_odds = self.get_burn_chance(world, offset_pos);
        if ignite_odds <= 0 {
            return;
        }
        // Skip if spreding is disabled or if there are no players nearby
        if spread_radius == 0 {
            return;
        }
        if spread_radius != -1 {
            let center = offset_pos.to_centered_f64();
            if world
                .get_closest_player(center, spread_radius as f64)
                .is_none()
            {
                return;
            }
        }

        // Calculate spread rate based on height
        let rate = if yy > 1 { 100 + (yy - 1) * 100 } else { 100 };

        // Calculate odds of spreading
        let mut odds = (ignite_odds + 40 + difficulty * 7) / (new_age as i32 + 30);

        // Reduce spread odds in certain biomes
        if Self::is_increased_burnout_biome(world, offset_pos) {
            odds /= 2; // Fire spreads 50% slower
        }

        if odds > 0
            && rand::rng().random_range(0..rate) <= odds
            && !Self::is_near_rain(world.as_ref(), offset_pos)
        {
            self.ignite(world, block, pos, offset_pos, new_age);
        }
    }

    /// Sets `offset_pos` on fire, unless a plugin cancels the spread.
    fn ignite(
        &self,
        world: &Arc<World>,
        block: &Block,
        pos: &BlockPos,
        offset_pos: &BlockPos,
        new_age: u8,
    ) {
        let spread_age = (new_age + rand::rng().random_range(0..5) / 4).min(15);
        let fire_state_id = self.get_state_for_position(world.as_ref(), block, offset_pos);
        let mut new_fire_props = FireProperties::from_state_id(fire_state_id);
        new_fire_props.age = spread_age;
        let new_state_id = new_fire_props.to_state_id(&Block::FIRE);

        if let Some(server) = world.server.upgrade() {
            let mut event = crate::plugin::api::events::block::block_spread::BlockSpreadEvent::new(
                *pos,
                *offset_pos,
                world.clone(),
                new_state_id,
            );
            server.plugin_manager.fire_blocking(&server, &mut event);
            if event.cancelled {
                return;
            }
        }

        world.set_block_state(offset_pos, new_state_id, BlockFlags::NOTIFY_NEIGHBORS);
    }
}
