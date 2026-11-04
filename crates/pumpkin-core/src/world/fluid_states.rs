//! Which block states hold a fluid.
//!
//! Entities ask "is there fluid here?" for every block their box touches, every tick, and
//! almost every answer is no. Working that out from the state costs a table walk and a
//! block-property test; a bit per state answers it with one memory read.

use std::sync::LazyLock;

use pumpkin_data::BlockStateId;

/// One bit per block state: set when the state holds a fluid (a fluid block, a waterlogged
/// block, or a plant that holds water).
struct Table {
    bits: Box<[u64]>,
}

impl Table {
    /// Asks `holds_fluid` once for every block state.
    fn build(holds_fluid: impl Fn(BlockStateId) -> bool) -> Self {
        let count = usize::from(BlockStateId::COUNT);
        let mut bits = vec![0u64; count.div_ceil(64)].into_boxed_slice();
        for raw in 0..BlockStateId::COUNT {
            if BlockStateId::new(raw).is_some_and(&holds_fluid) {
                bits[usize::from(raw) / 64] |= 1 << (raw % 64);
            }
        }
        Self { bits }
    }

    fn holds(&self, id: BlockStateId) -> bool {
        let raw = id.as_u16();
        self.bits
            .get(usize::from(raw) / 64)
            .is_some_and(|word| word & (1 << (raw % 64)) != 0)
    }
}

static TABLE: LazyLock<Table> = LazyLock::new(|| {
    Table::build(|id| super::World::fluid_of_state(id).id != pumpkin_data::fluid::Fluid::EMPTY.id)
});

/// Whether the block state holds a fluid. When false, the fluid of the state is `Fluid::EMPTY`.
#[must_use]
pub fn holds_fluid(id: BlockStateId) -> bool {
    TABLE.holds(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::World;
    use pumpkin_data::fluid::Fluid;

    #[test]
    fn the_table_agrees_with_the_slow_answer_for_every_block_state() {
        for raw in 0..BlockStateId::COUNT {
            let id = BlockStateId::new(raw).expect("below the count");
            assert_eq!(
                holds_fluid(id),
                World::fluid_of_state(id).id != Fluid::EMPTY.id,
                "block state {raw}"
            );
        }
    }

    #[test]
    fn air_holds_no_fluid_and_water_and_lava_do() {
        assert!(!holds_fluid(BlockStateId::AIR));
        assert!(holds_fluid(pumpkin_data::Block::WATER.default_state.id));
        assert!(holds_fluid(pumpkin_data::Block::LAVA.default_state.id));
        assert!(holds_fluid(pumpkin_data::Block::KELP.default_state.id));
    }

    #[test]
    fn a_table_built_for_all_or_for_no_states_answers_the_same_for_air() {
        let table = Table::build(|_| true);
        assert!(table.holds(BlockStateId::AIR));
        // a table built for no state at all
        let empty = Table::build(|_| false);
        assert!(!empty.holds(BlockStateId::AIR));
    }

    #[test]
    fn a_table_remembers_exactly_the_states_it_was_given() {
        let table = Table::build(|id| id.as_u16() % 3 == 1);
        for raw in 0..BlockStateId::COUNT {
            let id = BlockStateId::new(raw).unwrap();
            assert_eq!(table.holds(id), raw % 3 == 1, "state {raw}");
        }
    }
}
