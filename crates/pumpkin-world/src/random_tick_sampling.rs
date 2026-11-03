//! `KvasirMC`: picking the blocks that get a random tick this tick.
//!
//! Every tick, each active chunk draws `randomTickSpeed` random positions in each 16-block section
//! that holds a block (or fluid) that reacts to random ticks. Only the draws that land on such a
//! block are kept. This runs for every active chunk every tick, so it is the one place on the main
//! thread where the cost grows with the number of chunks, not with what happens in them.

use pumpkin_util::math::position::BlockPos;
use rand::RngExt;

use pumpkin_data::BlockStateId;
use pumpkin_data::block_properties::has_random_ticks;

use crate::{
    chunk::{ChunkData, palette::has_random_ticking_fluid},
    level::RandomTickSample,
};

/// A position inside a section: `x`, `y` and `z` each 0..16, packed into the low 12 bits of one
/// random number (4 bits each).
const fn unpack(random: u32) -> (usize, usize, usize) {
    (
        (random & 0xF) as usize,
        ((random >> 4) & 0xF) as usize,
        ((random >> 8) & 0xF) as usize,
    )
}

/// Appends to `out` the random-tick positions drawn in `chunk` this tick.
///
/// Nothing is drawn for sections whose bit in the chunk's mask is clear: they hold no block that
/// could react, so a draw there could never be kept.
pub fn sample_chunk(
    chunk: &ChunkData,
    samples_per_section: u32,
    rng: &mut impl RngExt,
    out: &mut Vec<RandomTickSample>,
) {
    let mask = chunk
        .section
        .randomly_ticking_mask
        .load(std::sync::atomic::Ordering::Relaxed);
    if mask == 0 || samples_per_section == 0 {
        return;
    }
    let sections = chunk
        .section
        .block_sections
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let chunk_x = chunk.x * 16;
    let chunk_z = chunk.z * 16;
    for index in 0..chunk.section.count {
        if mask & (1 << index) == 0 {
            continue;
        }
        let section_y = chunk.section.min_y + index as i32 * 16;
        for _ in 0..samples_per_section {
            let (x, y, z) = unpack(rng.random::<u32>());
            let state: BlockStateId = sections[index].get(x, y, z);
            let tick_block = has_random_ticks(state);
            let tick_fluid = has_random_ticking_fluid(state);
            if tick_block || tick_fluid {
                out.push(RandomTickSample {
                    position: BlockPos::new(
                        chunk_x + x as i32,
                        section_y + y as i32,
                        chunk_z + z as i32,
                    ),
                    tick_block,
                    tick_fluid,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use pumpkin_data::Block;
    use rand::{SeedableRng, rngs::StdRng};

    use super::*;

    /// A chunk at (3, -2) whose section 5 (y 16..32) is entirely `block`.
    fn chunk_filled_with(block: &Block) -> ChunkData {
        let chunk = ChunkData::empty(3, -2);
        for x in 0..16 {
            for y in 16..32 {
                for z in 0..16 {
                    chunk
                        .section
                        .set_block_absolute_y(x, y, z, block.default_state.id);
                }
            }
        }
        chunk
    }

    fn sample(chunk: &ChunkData, per_section: u32, seed: u64) -> Vec<RandomTickSample> {
        let mut out = Vec::new();
        sample_chunk(
            chunk,
            per_section,
            &mut StdRng::seed_from_u64(seed),
            &mut out,
        );
        out
    }

    #[test]
    fn a_full_section_of_ticking_blocks_yields_every_draw() {
        let chunk = chunk_filled_with(&Block::GRASS_BLOCK);
        assert_eq!(sample(&chunk, 3, 1).len(), 3);
        assert_eq!(sample(&chunk, 11, 2).len(), 11);
    }

    #[test]
    fn positions_lie_inside_the_section_in_world_coordinates() {
        let chunk = chunk_filled_with(&Block::GRASS_BLOCK);
        let samples = sample(&chunk, 500, 7);
        assert_eq!(samples.len(), 500);
        for s in &samples {
            assert!((48..64).contains(&s.position.0.x), "x {}", s.position.0.x);
            assert!((16..32).contains(&s.position.0.y), "y {}", s.position.0.y);
            assert!((-32..-16).contains(&s.position.0.z), "z {}", s.position.0.z);
            assert!(s.tick_block);
        }
    }

    #[test]
    fn every_position_of_the_section_can_be_drawn() {
        let chunk = chunk_filled_with(&Block::GRASS_BLOCK);
        let mut seen = std::collections::HashSet::new();
        for s in sample(&chunk, 60_000, 11) {
            seen.insert((s.position.0.x, s.position.0.y, s.position.0.z));
        }
        assert_eq!(
            seen.len(),
            16 * 16 * 16,
            "all 4096 positions turn up in 60000 draws"
        );
    }

    #[test]
    fn blocks_that_do_not_tick_are_never_reported() {
        let chunk = chunk_filled_with(&Block::STONE);
        assert!(sample(&chunk, 3, 1).is_empty());
        assert_eq!(
            chunk
                .section
                .randomly_ticking_mask
                .load(std::sync::atomic::Ordering::Relaxed),
            0
        );
    }

    #[test]
    fn an_empty_chunk_draws_nothing() {
        assert!(sample(&ChunkData::empty(0, 0), 3, 1).is_empty());
    }

    #[test]
    fn a_random_tick_speed_of_zero_draws_nothing() {
        let chunk = chunk_filled_with(&Block::GRASS_BLOCK);
        assert!(sample(&chunk, 0, 1).is_empty());
    }

    #[test]
    fn a_sparse_section_keeps_about_its_share_of_the_draws() {
        // one ticking block among 4096: with 819_200 draws about 200 land on it
        let chunk = ChunkData::empty(0, 0);
        chunk
            .section
            .set_block_absolute_y(4, 20, 9, Block::GRASS_BLOCK.default_state.id);
        let samples = sample(&chunk, 819_200, 5);
        assert!(
            (120..300).contains(&samples.len()),
            "{} kept",
            samples.len()
        );
        assert!(
            samples
                .iter()
                .all(|s| (s.position.0.x, s.position.0.y, s.position.0.z) == (4, 20, 9))
        );
    }

    #[test]
    fn only_sections_with_a_ticking_block_are_drawn_in() {
        let chunk = ChunkData::empty(0, 0);
        chunk
            .section
            .set_block_absolute_y(0, 40, 0, Block::GRASS_BLOCK.default_state.id);
        chunk
            .section
            .set_block_absolute_y(0, -50, 0, Block::GRASS_BLOCK.default_state.id);
        let mask = chunk
            .section
            .randomly_ticking_mask
            .load(std::sync::atomic::Ordering::Relaxed);
        assert_eq!(mask.count_ones(), 2, "sections of y 32..48 and -64..-48");
        let mut rng = StdRng::seed_from_u64(9);
        let mut out = Vec::new();
        sample_chunk(&chunk, 4, &mut rng, &mut out);
        assert!(out.iter().all(|s| (32..48).contains(&s.position.0.y) || (-64..-48).contains(&s.position.0.y)));
    }

    #[test]
    fn unpacking_uses_four_bits_per_axis_and_ignores_the_rest() {
        assert_eq!(unpack(0x0000_0000), (0, 0, 0));
        assert_eq!(unpack(0x0000_0FED), (0xD, 0xE, 0xF));
        assert_eq!(unpack(0xFFFF_F123), (3, 2, 1));
    }
}
