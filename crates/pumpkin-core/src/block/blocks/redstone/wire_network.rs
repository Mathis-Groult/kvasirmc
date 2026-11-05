//! Redstone dust, one pass per network (the idea of the Alternate Current mod, written afresh).
//!
//! When a dust block finds that its power is wrong, the whole network it belongs to is found,
//! every block's external power is read once, the settled powers are solved together by
//! [`kvasir_wirenet`], the blocks whose power changed are written, and each position that vanilla
//! would have told about the change is told once. Dust of the network is not told: it is already
//! settled. The old way (a power change tells 42 positions, each dust among them recomputes and
//! repeats) is kept in `redstone_wire.rs` for `[world.redstone] alternate_current = false`.
//!
//! The planning is pure: it reads the world through [`DustWorld`], so it is tested on a small grid
//! with no server. Only [`settle_from`] touches the real world.

use std::cell::Cell;
use std::sync::Arc;

use kvasir_wirenet::{LinkOutOfRange, Network, Scratch};
use pumpkin_data::block_properties::RedstoneWireLikeProperties;
use pumpkin_data::{Block, BlockDirection};
use pumpkin_util::math::position::BlockPos;
use pumpkin_world::world::BlockFlags;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::world::World;

use super::wire_power::get_block_signal;

/// What the dust rules need to know about a position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Occupant {
    /// A block of redstone dust showing this power.
    Dust { power: u8 },
    /// A solid block: dust can climb over it, and it keeps the dust above it from seeing down.
    Solid,
    /// Anything else (air, a slab, a torch...).
    Open,
}

/// The two questions planning asks of a world.
pub(super) trait DustWorld {
    fn occupant(&self, pos: &BlockPos) -> Occupant;
    /// The power given to the dust at `pos` by everything that is not dust.
    fn external_power(&self, pos: &BlockPos) -> u8;
}

impl DustWorld for World {
    fn occupant(&self, pos: &BlockPos) -> Occupant {
        let (block, state) = self.get_block_and_state(pos);
        if block == &Block::REDSTONE_WIRE {
            Occupant::Dust {
                power: RedstoneWireLikeProperties::from_state_id(state.id).power,
            }
        } else if state.is_solid_block() {
            Occupant::Solid
        } else {
            Occupant::Open
        }
    }

    fn external_power(&self, pos: &BlockPos) -> u8 {
        get_block_signal(self, pos)
    }
}

/// More dust than this in one network is refused (and the old update used instead): no world holds
/// such a network, so a bigger one means the discovery is not terminating.
const MAX_NETWORK: usize = 1 << 20;

/// Why a network could not be settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlanError {
    /// The description of the network is inconsistent.
    Link(LinkOutOfRange),
    /// The network has more than the limit of dust blocks.
    TooLarge { limit: usize },
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Link(error) => error.fmt(f),
            Self::TooLarge { limit } => write!(f, "the dust network has more than {limit} blocks"),
        }
    }
}

impl From<LinkOutOfRange> for PlanError {
    fn from(error: LinkOutOfRange) -> Self {
        Self::Link(error)
    }
}

/// A dust block is linked to at most one dust block per side on the same level or below, and one
/// above: two per side, four sides.
const MAX_LINKS: usize = 8;

/// Everything one planning pass fills in, kept between passes to avoid allocating.
#[derive(Default)]
struct Work {
    network: Network,
    scratch: Scratch,
    /// The dust of the network, numbered in the order it was found.
    positions: Vec<BlockPos>,
    /// The power each showed when found.
    current: Vec<u8>,
    index: FxHashMap<BlockPos, usize>,
    powers: Vec<u8>,
    /// The nodes whose power changes.
    changed: Vec<usize>,
    /// The positions to tell, once each, in the order to tell them.
    targets: Vec<BlockPos>,
    seen: FxHashSet<BlockPos>,
}

impl Work {
    fn clear(&mut self) {
        self.network.clear();
        self.positions.clear();
        self.current.clear();
        self.index.clear();
        self.changed.clear();
        self.targets.clear();
        self.seen.clear();
    }

    /// The node number of the dust at `pos`, giving it the next number when it is new.
    fn node_for(&mut self, pos: BlockPos, power: u8) -> usize {
        if let Some(&node) = self.index.get(&pos) {
            return node;
        }
        let node = self.positions.len();
        self.positions.push(pos);
        self.current.push(power);
        self.index.insert(pos, node);
        node
    }
}

/// Finds every dust block the one at `start` reads power from, directly or through other dust, and
/// describes them as a network.
///
/// The rule is the one the old code used for dust reading dust: a dust block reads dust on its own
/// level at its side; dust one up at its side when that side is solid and the block above it is
/// not; dust one down at its side when that side is not solid. With dust on solid blocks the rule
/// is mutual; dust on a hopper or a slab can read dust that does not read it back, and then the
/// reader is not found from the one it reads (it is told about the change like any other
/// neighbour, and settles its own network).
fn discover(
    world: &impl DustWorld,
    start: &BlockPos,
    start_power: u8,
    limit: usize,
    work: &mut Work,
) -> Result<(), PlanError> {
    work.node_for(*start, start_power);
    let mut next = 0;
    while next < work.positions.len() {
        if work.positions.len() > limit {
            return Err(PlanError::TooLarge { limit });
        }
        let pos = work.positions[next];
        next += 1;

        let up_is_solid = world.occupant(&pos.up()) == Occupant::Solid;
        let mut links = [0usize; MAX_LINKS];
        let mut count = 0;
        for direction in BlockDirection::horizontal() {
            let side = pos.offset(direction.to_offset());
            let occupant = world.occupant(&side);
            if let Occupant::Dust { power } = occupant {
                links[count] = work.node_for(side, power);
                count += 1;
            }
            let (other, other_side) = if occupant == Occupant::Solid {
                if up_is_solid {
                    continue;
                }
                (world.occupant(&side.up()), side.up())
            } else {
                (world.occupant(&side.down()), side.down())
            };
            if let Occupant::Dust { power } = other {
                links[count] = work.node_for(other_side, power);
                count += 1;
            }
        }
        work.network
            .push(world.external_power(&pos), &links[..count]);
    }
    Ok(())
}

/// Lists the positions to tell about the dust at `changed` changing, in vanilla's order and each
/// once: for every changed block, first the neighbours of its own position, then the neighbours of
/// each of its six neighbours. Positions in `members` (dust that was settled with it) are left out.
fn notification_targets(
    changed: impl IntoIterator<Item = BlockPos>,
    members: &FxHashMap<BlockPos, usize>,
    seen: &mut FxHashSet<BlockPos>,
    targets: &mut Vec<BlockPos>,
) {
    for wire in changed {
        let centres = std::iter::once(wire).chain(
            BlockDirection::all()
                .into_iter()
                .map(|direction| wire.offset(direction.to_offset())),
        );
        for centre in centres {
            for direction in BlockDirection::update_order() {
                let target = centre.offset(direction.to_offset());
                if !members.contains_key(&target) && seen.insert(target) {
                    targets.push(target);
                }
            }
        }
    }
}

/// Works out, without changing anything, what settling the network around `start` does: afterwards
/// `work.changed` holds the nodes whose power changes (with the new powers in `work.powers`) and
/// `work.targets` the positions to tell.
fn plan(world: &impl DustWorld, start: &BlockPos, work: &mut Work) -> Result<(), PlanError> {
    plan_within(world, start, MAX_NETWORK, work)
}

fn plan_within(
    world: &impl DustWorld,
    start: &BlockPos,
    limit: usize,
    work: &mut Work,
) -> Result<(), PlanError> {
    work.clear();
    let Occupant::Dust { power } = world.occupant(start) else {
        return Ok(());
    };
    discover(world, start, power, limit, work)?;
    work.network.settle(&mut work.scratch, &mut work.powers)?;
    for (node, (&settled, &shown)) in work.powers.iter().zip(&work.current).enumerate() {
        if settled != shown {
            work.changed.push(node);
        }
    }
    notification_targets(
        work.changed.iter().map(|&node| work.positions[node]),
        &work.index,
        &mut work.seen,
        &mut work.targets,
    );
    Ok(())
}

thread_local! {
    /// The planning memory of this thread, taken while a settle runs: writing a block can run a
    /// nested settle on the same thread, which then makes its own.
    static SPARE: Cell<Option<Box<Work>>> = const { Cell::new(None) };
}

/// Settles the whole dust network that contains `start` and tells what needs telling.
///
/// # Errors
/// [`PlanError`] when the discovered network is inconsistent or absurdly large, which is a bug here;
/// nothing was changed and the caller should fall back to the old update.
pub(super) fn settle_from(world: &Arc<World>, start: &BlockPos) -> Result<(), PlanError> {
    let mut work = SPARE.with(Cell::take).unwrap_or_default();
    let planned = plan(world.as_ref(), start, &mut work);
    if planned.is_ok() {
        for &node in &work.changed {
            write_power(world, &work.positions[node], work.powers[node]);
        }
        for target in &work.targets {
            world.update_neighbor(target, &Block::REDSTONE_WIRE);
        }
    }
    SPARE.with(|spare| spare.set(Some(work)));
    planned
}

/// Sets the power of the dust at `pos`, reading the block again first: an earlier write of this
/// settle may have changed its shape.
fn write_power(world: &Arc<World>, pos: &BlockPos, power: u8) {
    let (block, state) = world.get_block_and_state(pos);
    if block != &Block::REDSTONE_WIRE {
        return;
    }
    let mut wire = RedstoneWireLikeProperties::from_state_id(state.id);
    if wire.power == power {
        return;
    }
    wire.power = power;
    world.set_block_state(
        pos,
        wire.to_state_id(&Block::REDSTONE_WIRE),
        BlockFlags::NOTIFY_LISTENERS,
    );
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pumpkin_util::math::vector3::Vector3;

    use super::*;

    fn at(x: i32, y: i32, z: i32) -> BlockPos {
        BlockPos(Vector3::new(x, y, z))
    }

    /// A small world: unlisted positions are open, dust has a power, and `source` positions give
    /// the dust next to them power 15 (a redstone block).
    #[derive(Default)]
    struct Grid {
        cells: HashMap<BlockPos, Occupant>,
        sources: Vec<BlockPos>,
    }

    impl Grid {
        fn dust(mut self, pos: BlockPos, power: u8) -> Self {
            self.cells.insert(pos, Occupant::Dust { power });
            self
        }

        fn solid(mut self, pos: BlockPos) -> Self {
            self.cells.insert(pos, Occupant::Solid);
            self
        }

        fn source(mut self, pos: BlockPos) -> Self {
            self.cells.insert(pos, Occupant::Open);
            self.sources.push(pos);
            self
        }

        /// Dust in a row along x from `from` to `to`, all showing `power`.
        fn row(mut self, y: i32, z: i32, from: i32, to: i32, power: u8) -> Self {
            for x in from..=to {
                self = self.dust(at(x, y, z), power);
            }
            self
        }
    }

    impl DustWorld for Grid {
        fn occupant(&self, pos: &BlockPos) -> Occupant {
            self.cells.get(pos).copied().unwrap_or(Occupant::Open)
        }

        fn external_power(&self, pos: &BlockPos) -> u8 {
            let touches = BlockDirection::all()
                .into_iter()
                .any(|direction| self.sources.contains(&pos.offset(direction.to_offset())));
            if touches { 15 } else { 0 }
        }
    }

    /// The settled power of the dust at each listed position when the network around `start` is
    /// planned.
    fn settled_at(world: &Grid, start: BlockPos, positions: &[BlockPos]) -> Vec<u8> {
        let mut work = Work::default();
        plan(world, &start, &mut work).expect("consistent");
        positions
            .iter()
            .map(|pos| work.powers[work.index[pos]])
            .collect()
    }

    #[test]
    fn a_line_of_dust_fed_at_one_end_settles_in_one_plan() {
        let world = Grid::default().source(at(-1, 0, 0)).row(0, 0, 0, 17, 0);
        let line: Vec<BlockPos> = (0..=17).map(|x| at(x, 0, 0)).collect();
        assert_eq!(
            settled_at(&world, at(5, 0, 0), &line),
            vec![15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 0]
        );
    }

    #[test]
    fn the_whole_network_is_found_from_any_of_its_blocks() {
        let world = Grid::default().row(0, 0, 0, 9, 0);
        let mut work = Work::default();
        plan(&world, &at(7, 0, 0), &mut work).expect("consistent");
        assert_eq!(work.positions.len(), 10);
        assert_eq!(work.network.len(), 10);
    }

    #[test]
    fn dust_a_block_apart_is_another_network() {
        let world = Grid::default().row(0, 0, 0, 4, 0).row(0, 2, 0, 4, 0);
        let mut work = Work::default();
        plan(&world, &at(0, 0, 0), &mut work).expect("consistent");
        assert_eq!(work.positions.len(), 5);
    }

    #[test]
    fn dust_climbs_over_a_solid_block_when_nothing_covers_it() {
        // dust at x=0 (y=0), a solid block at x=1 (y=0), dust on top of it at x=1 (y=1)
        let world = Grid::default()
            .source(at(-1, 0, 0))
            .dust(at(0, 0, 0), 0)
            .solid(at(1, 0, 0))
            .dust(at(1, 1, 0), 0);
        assert_eq!(
            settled_at(&world, at(0, 0, 0), &[at(0, 0, 0), at(1, 1, 0)]),
            vec![15, 14]
        );
    }

    #[test]
    fn a_solid_block_above_the_dust_stops_it_climbing() {
        let world = Grid::default()
            .source(at(-1, 0, 0))
            .dust(at(0, 0, 0), 0)
            .solid(at(0, 1, 0))
            .solid(at(1, 0, 0))
            .dust(at(1, 1, 0), 0);
        let mut work = Work::default();
        plan(&world, &at(0, 0, 0), &mut work).expect("consistent");
        assert_eq!(work.positions, vec![at(0, 0, 0)]);
    }

    #[test]
    fn dust_steps_down_past_an_open_block_and_not_past_a_solid_one() {
        // dust high at x=0 (y=1); open at x=1 (y=1); dust low at x=1 (y=0)
        let down = Grid::default()
            .source(at(-1, 1, 0))
            .solid(at(0, 0, 0))
            .dust(at(0, 1, 0), 0)
            .dust(at(1, 0, 0), 0);
        assert_eq!(
            settled_at(&down, at(0, 1, 0), &[at(0, 1, 0), at(1, 0, 0)]),
            vec![15, 14]
        );
        let blocked = down.solid(at(1, 1, 0));
        let mut work = Work::default();
        plan(&blocked, &at(0, 1, 0), &mut work).expect("consistent");
        assert_eq!(work.positions, vec![at(0, 1, 0)]);
    }

    #[test]
    fn dust_on_solid_blocks_reads_both_ways_so_the_network_is_the_same_from_either_end() {
        let world = Grid::default()
            .dust(at(0, 0, 0), 0)
            .solid(at(1, 0, 0))
            .dust(at(1, 1, 0), 0)
            .solid(at(2, 0, 0))
            .dust(at(2, 1, 0), 0)
            .dust(at(3, 0, 0), 0);
        for start in [at(0, 0, 0), at(1, 1, 0), at(3, 0, 0)] {
            let mut work = Work::default();
            plan(&world, &start, &mut work).expect("consistent");
            let mut found = work.positions.clone();
            found.sort_by_key(|pos| (pos.0.x, pos.0.y, pos.0.z));
            assert_eq!(
                found,
                vec![at(0, 0, 0), at(1, 1, 0), at(2, 1, 0), at(3, 0, 0)],
                "from {start:?}"
            );
        }
    }

    #[test]
    fn dust_on_a_hopper_reads_the_dust_below_but_is_not_read_back() {
        // dust A low at x=0; the block at x=1, y=0 is open (a hopper is not a solid block), dust B on
        // it at (1, 1, 0). B reads down to A (its side over A is open); A cannot read up.
        let world = Grid::default()
            .source(at(-1, 0, 0))
            .dust(at(0, 0, 0), 0)
            .dust(at(1, 1, 0), 0);
        let mut from_a = Work::default();
        plan(&world, &at(0, 0, 0), &mut from_a).expect("consistent");
        assert_eq!(from_a.positions, vec![at(0, 0, 0)], "A reads nothing");
        assert_eq!(
            settled_at(&world, at(1, 1, 0), &[at(1, 1, 0), at(0, 0, 0)]),
            vec![14, 15]
        );
    }

    #[test]
    fn a_ring_of_dust_with_no_source_settles_to_nothing_even_if_it_shows_power() {
        let mut world = Grid::default();
        for (x, z) in [
            (0, 0),
            (1, 0),
            (2, 0),
            (2, 1),
            (2, 2),
            (1, 2),
            (0, 2),
            (0, 1),
        ] {
            world = world.dust(at(x, 0, z), 7);
        }
        let mut work = Work::default();
        plan(&world, &at(0, 0, 0), &mut work).expect("consistent");
        assert_eq!(work.positions.len(), 8);
        assert_eq!(work.powers, vec![0; 8]);
        assert_eq!(work.changed.len(), 8);
    }

    #[test]
    fn only_the_blocks_whose_power_changes_are_listed() {
        // already correct everywhere: 15 14 13
        let world = Grid::default()
            .source(at(-1, 0, 0))
            .dust(at(0, 0, 0), 15)
            .dust(at(1, 0, 0), 14)
            .dust(at(2, 0, 0), 13);
        let mut work = Work::default();
        plan(&world, &at(0, 0, 0), &mut work).expect("consistent");
        assert!(work.changed.is_empty());
        assert!(work.targets.is_empty());
    }

    #[test]
    fn one_work_memory_serves_one_network_after_another() {
        let first = Grid::default().source(at(-1, 0, 0)).row(0, 0, 0, 5, 0);
        let second = Grid::default()
            .source(at(100, 0, 50))
            .row(0, 51, 100, 102, 0);
        let mut reused = Work::default();
        plan(&first, &at(0, 0, 0), &mut reused).expect("consistent");
        plan(&second, &at(100, 0, 51), &mut reused).expect("consistent");
        let mut fresh = Work::default();
        plan(&second, &at(100, 0, 51), &mut fresh).expect("consistent");
        assert_eq!(reused.positions, fresh.positions);
        assert_eq!(reused.powers, fresh.powers);
        assert_eq!(reused.changed, fresh.changed);
        assert_eq!(reused.targets, fresh.targets);
        assert_eq!(reused.index.len(), 3);
    }

    #[test]
    fn a_network_beyond_the_limit_is_refused_and_one_at_the_limit_is_not() {
        let world = Grid::default().row(0, 0, 0, 9, 0);
        let mut work = Work::default();
        assert_eq!(
            plan_within(&world, &at(0, 0, 0), 5, &mut work),
            Err(PlanError::TooLarge { limit: 5 })
        );
        assert!(plan_within(&world, &at(0, 0, 0), 10, &mut work).is_ok());
        assert_eq!(work.positions.len(), 10);
        assert!(
            PlanError::TooLarge { limit: 5 }
                .to_string()
                .contains("more than 5")
        );
    }

    #[test]
    fn one_work_memory_serves_a_network_again_with_nothing_left_over() {
        // the first plan leaves powers shown, targets told and positions seen behind it
        let world = Grid::default()
            .source(at(-1, 0, 0))
            .dust(at(0, 0, 0), 9)
            .dust(at(1, 0, 0), 9)
            .dust(at(2, 0, 0), 9);
        let mut reused = Work::default();
        plan(&world, &at(0, 0, 0), &mut reused).expect("consistent");
        plan(&world, &at(0, 0, 0), &mut reused).expect("consistent");
        let mut fresh = Work::default();
        plan(&world, &at(0, 0, 0), &mut fresh).expect("consistent");
        assert_eq!(reused.changed, fresh.changed);
        assert_eq!(reused.targets, fresh.targets);
        assert_eq!(reused.current, fresh.current);
        assert_eq!(reused.current, vec![9, 9, 9]);
        assert!(!reused.targets.is_empty());
    }

    #[test]
    fn a_start_that_is_not_dust_plans_nothing() {
        let world = Grid::default().solid(at(0, 0, 0));
        let mut work = Work::default();
        plan(&world, &at(0, 0, 0), &mut work).expect("consistent");
        assert!(work.positions.is_empty());
        assert!(work.changed.is_empty());
    }

    #[test]
    fn one_changed_block_tells_its_24_surrounding_positions_once_each() {
        let members: FxHashMap<BlockPos, usize> = std::iter::once((at(0, 0, 0), 0)).collect();
        let mut seen = FxHashSet::default();
        let mut targets = Vec::new();
        notification_targets([at(0, 0, 0)], &members, &mut seen, &mut targets);
        assert_eq!(targets.len(), 24);
        assert!(
            !targets.contains(&at(0, 0, 0)),
            "the changed block itself is a member"
        );
        // every position within two blocks (taxicab) except the block itself
        for dx in -2..=2i32 {
            for dy in -2..=2i32 {
                for dz in -2..=2i32 {
                    let distance = dx.abs() + dy.abs() + dz.abs();
                    let expected = distance > 0 && distance <= 2;
                    assert_eq!(
                        targets.contains(&at(dx, dy, dz)),
                        expected,
                        "{dx} {dy} {dz}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_first_position_told_is_the_one_vanilla_tells_first() {
        let members: FxHashMap<BlockPos, usize> = std::iter::once((at(0, 0, 0), 0)).collect();
        let mut targets = Vec::new();
        notification_targets(
            [at(0, 0, 0)],
            &members,
            &mut FxHashSet::default(),
            &mut targets,
        );
        // update order is west, east, down, up, north, south, around the block itself first
        assert_eq!(
            targets[..6],
            [
                at(-1, 0, 0),
                at(1, 0, 0),
                at(0, -1, 0),
                at(0, 1, 0),
                at(0, 0, -1),
                at(0, 0, 1)
            ]
        );
    }

    #[test]
    fn two_changed_neighbours_tell_their_shared_surroundings_once_and_skip_each_other() {
        let members: FxHashMap<BlockPos, usize> =
            [(at(0, 0, 0), 0), (at(1, 0, 0), 1)].into_iter().collect();
        let mut targets = Vec::new();
        notification_targets(
            [at(0, 0, 0), at(1, 0, 0)],
            &members,
            &mut FxHashSet::default(),
            &mut targets,
        );
        let mut unique = targets.clone();
        unique.sort_by_key(|pos| (pos.0.x, pos.0.y, pos.0.z));
        unique.dedup();
        assert_eq!(unique.len(), targets.len(), "no position twice");
        let taxicab = |a: BlockPos, b: BlockPos| {
            (a.0.x - b.0.x).abs() + (a.0.y - b.0.y).abs() + (a.0.z - b.0.z).abs()
        };
        for x in -4..=5 {
            for y in -4..=4 {
                for z in -4..=4 {
                    let pos = at(x, y, z);
                    let near = |wire: BlockPos| (1..=2).contains(&taxicab(pos, wire));
                    let is_member = pos == at(0, 0, 0) || pos == at(1, 0, 0);
                    let expected = !is_member && (near(at(0, 0, 0)) || near(at(1, 0, 0)));
                    assert_eq!(targets.contains(&pos), expected, "{x} {y} {z}");
                }
            }
        }
    }

    #[test]
    fn a_position_next_to_dust_that_is_not_in_the_network_is_still_told() {
        // dust two blocks apart: the second is not a member and sits in the first's surroundings
        let members: FxHashMap<BlockPos, usize> = std::iter::once((at(0, 0, 0), 0)).collect();
        let mut targets = Vec::new();
        notification_targets(
            [at(0, 0, 0)],
            &members,
            &mut FxHashSet::default(),
            &mut targets,
        );
        assert!(targets.contains(&at(0, 0, 2)));
    }
}
