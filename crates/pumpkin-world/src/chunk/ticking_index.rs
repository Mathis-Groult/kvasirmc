//! `KvasirMC`: which positions of one section react to random ticks.
//!
//! Every tick, every active chunk draws a few random positions in each section that holds such a
//! block, and only the draws that land on one are kept. Finding out whether a draw landed on one
//! means reading the section's block data at a random place: a cache miss that is almost always
//! wasted, because most draws land on stone or air. Keeping the positions of the blocks that do
//! react in a short list turns the draw around: pick a number from 0 to 4095, and if it is below
//! the list's length, the list's entry at that number *is* the hit. That is the same
//! distribution as drawing a position and looking at it (each of the 4096 positions equally
//! likely, a hit exactly when the position reacts, every reacting position equally likely), but a
//! miss costs one random number and no memory read.

use pumpkin_data::BlockStateId;
use pumpkin_data::block_properties::has_random_ticks;

use crate::chunk::palette::has_random_ticking_fluid;

/// Positions in a section: `y << 8 | z << 4 | x`, each coordinate 0..16.
pub const POSITIONS: u32 = 4096;
const POSITION_MASK: u16 = 0x0FFF;
const BLOCK: u16 = 1 << 12;
const FLUID: u16 = 1 << 13;

/// The position of `(x, y, z)` inside a section, each coordinate 0..16.
#[must_use]
pub const fn section_position(x: usize, y: usize, z: usize) -> u16 {
    (y << 8 | z << 4 | x) as u16
}

/// What reacts to random ticks at a position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ticking {
    /// `y << 8 | z << 4 | x`.
    pub position: u16,
    pub block: bool,
    pub fluid: bool,
}

/// The positions of a section that hold a block or a fluid that reacts to random ticks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TickingIndex {
    /// Position in the low 12 bits, kind in the next two. Unordered.
    entries: Vec<u16>,
}

/// The kind bits for a block state: none when it does not react at all.
fn kind_of(state: BlockStateId) -> u16 {
    let mut kind = 0;
    if has_random_ticks(state) {
        kind |= BLOCK;
    }
    if has_random_ticking_fluid(state) {
        kind |= FLUID;
    }
    kind
}

impl TickingIndex {
    /// The index of a whole section, given its states in position order (`y << 8 | z << 4 | x`).
    pub fn from_states(states: impl IntoIterator<Item = BlockStateId>) -> Self {
        let entries = states
            .into_iter()
            .take(POSITIONS as usize)
            .enumerate()
            .filter_map(|(position, state)| {
                let kind = kind_of(state);
                (kind != 0).then_some(position as u16 | kind)
            })
            .collect();
        Self { entries }
    }

    /// Notes that the state at `position` is now `state`.
    pub fn set(&mut self, position: u16, state: BlockStateId) {
        debug_assert!(u32::from(position) < POSITIONS);
        let kind = kind_of(state);
        let at = self
            .entries
            .iter()
            .position(|entry| entry & POSITION_MASK == position);
        match (at, kind) {
            (Some(i), 0) => {
                self.entries.swap_remove(i);
            }
            (Some(i), kind) => self.entries[i] = position | kind,
            (None, 0) => {}
            (None, kind) => self.entries.push(position | kind),
        }
    }

    /// How many positions react.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The `n`th reacting position, in no particular order.
    #[must_use]
    pub fn get(&self, n: usize) -> Option<Ticking> {
        self.entries.get(n).map(|&entry| Ticking {
            position: entry & POSITION_MASK,
            block: entry & BLOCK != 0,
            fluid: entry & FLUID != 0,
        })
    }

    /// The outcome of one draw: `draw` is a uniform number below [`POSITIONS`], standing for "a
    /// random position". It is a hit exactly when it is below the number of reacting positions,
    /// and then every reacting position is equally likely.
    #[must_use]
    pub fn hit(&self, draw: u32) -> Option<Ticking> {
        self.get(draw as usize)
    }
}

#[cfg(test)]
mod tests {
    use pumpkin_data::Block;

    use super::*;

    fn grass() -> BlockStateId {
        Block::GRASS_BLOCK.default_state.id
    }

    fn stone() -> BlockStateId {
        Block::STONE.default_state.id
    }

    fn position(x: u16, y: u16, z: u16) -> u16 {
        y << 8 | z << 4 | x
    }

    fn states_with(at: &[(u16, BlockStateId)]) -> Vec<BlockStateId> {
        let mut states = vec![stone(); POSITIONS as usize];
        for &(position, state) in at {
            states[position as usize] = state;
        }
        states
    }

    #[test]
    fn a_section_of_stone_has_nothing_that_reacts() {
        let index = TickingIndex::from_states(states_with(&[]));
        assert!(index.is_empty());
        assert_eq!(index.hit(0), None);
    }

    #[test]
    fn building_finds_exactly_the_positions_that_react() {
        let wanted = [position(3, 5, 7), position(0, 0, 0), position(15, 15, 15)];
        let states = states_with(&wanted.map(|p| (p, grass())));
        let index = TickingIndex::from_states(states);
        assert_eq!(index.len(), 3);
        let mut found: Vec<u16> = (0..3).map(|n| index.get(n).unwrap().position).collect();
        found.sort_unstable();
        let mut expected = wanted.to_vec();
        expected.sort_unstable();
        assert_eq!(found, expected);
        assert!(index.get(0).unwrap().block);
        assert!(!index.get(0).unwrap().fluid);
        assert_eq!(index.get(3), None);
    }

    #[test]
    fn setting_a_reacting_state_adds_and_a_plain_one_removes() {
        let mut index = TickingIndex::default();
        index.set(position(1, 2, 3), grass());
        assert_eq!(index.len(), 1);
        index.set(position(1, 2, 3), grass());
        assert_eq!(index.len(), 1, "the same position twice is still one entry");
        index.set(position(4, 4, 4), grass());
        assert_eq!(index.len(), 2);
        index.set(position(1, 2, 3), stone());
        assert_eq!(index.len(), 1);
        assert_eq!(index.get(0).unwrap().position, position(4, 4, 4));
        index.set(position(9, 9, 9), stone());
        assert_eq!(
            index.len(),
            1,
            "a plain state where nothing was changes nothing"
        );
    }

    #[test]
    fn a_fluid_that_reacts_is_marked_as_a_fluid() {
        let lava = Block::LAVA.default_state.id;
        let mut index = TickingIndex::default();
        index.set(position(2, 2, 2), lava);
        let entry = index.get(0).unwrap();
        assert!(entry.fluid, "lava reacts as a fluid");
        assert_eq!(entry.block, has_random_ticks(lava));
    }

    #[test]
    fn a_draw_is_a_hit_exactly_when_it_is_below_the_count() {
        let mut index = TickingIndex::default();
        for x in 0..5 {
            index.set(position(x, 0, 0), grass());
        }
        for draw in 0..POSITIONS {
            assert_eq!(index.hit(draw).is_some(), draw < 5, "draw {draw}");
        }
    }

    #[test]
    fn replacing_one_kind_with_another_at_a_position_keeps_one_entry() {
        let lava = Block::LAVA.default_state.id;
        let mut index = TickingIndex::default();
        index.set(position(6, 6, 6), grass());
        index.set(position(6, 6, 6), lava);
        assert_eq!(index.len(), 1);
        assert!(index.get(0).unwrap().fluid);
    }
}
