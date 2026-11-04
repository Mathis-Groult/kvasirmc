//! `KvasirMC`: picking the blocks that get a random tick this tick.
//!
//! Every tick, each active chunk draws `randomTickSpeed` random positions in each 16-block section
//! that holds a block (or fluid) that reacts to random ticks. Only the draws that land on such a
//! block are kept. This runs for every active chunk every tick, so it is the one place on the main
//! thread where the cost grows with the number of chunks, not with what happens in them.

use pumpkin_util::math::position::BlockPos;
use rand::RngExt;

use crate::{chunk::ChunkData, chunk::ticking_index::POSITIONS, level::RandomTickSample};

/// Appends to `out` the random-tick positions drawn in `chunk` this tick.
///
/// Each section whose bit in the chunk's mask is set gets `samples_per_section` draws. A draw is
/// "a uniformly random position of the section, kept if it holds a block or fluid that reacts".
/// The section keeps a list of the positions that do (see
/// [`TickingIndex`](crate::chunk::ticking_index::TickingIndex)), so the draw is made as a number
/// below 4096 and is a hit exactly when it is below the list's length: the same distribution, and
/// a miss, which is nearly every draw, reads no block data.
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
    let caches = chunk
        .section
        .random_tick_sections
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(caches) = caches.as_ref() else {
        return;
    };
    let chunk_x = chunk.x * 16;
    let chunk_z = chunk.z * 16;
    for (index, cache) in caches.iter().enumerate().take(chunk.section.count) {
        if mask & (1 << index) == 0 {
            continue;
        }
        let section_y = chunk.section.min_y + index as i32 * 16;
        for _ in 0..samples_per_section {
            let draw = rng.random::<u32>() & (POSITIONS - 1);
            let Some(hit) = cache.ticking.hit(draw) else {
                continue;
            };
            let position = i32::from(hit.position);
            out.push(RandomTickSample {
                position: BlockPos::new(
                    chunk_x + (position & 15),
                    section_y + (position >> 8),
                    chunk_z + ((position >> 4) & 15),
                ),
                tick_block: hit.block,
                tick_fluid: hit.fluid,
            });
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
    fn the_hits_of_a_section_are_spread_evenly_over_its_ticking_positions() {
        // three ticking blocks: each should get about a third of the hits
        let chunk = ChunkData::empty(0, 0);
        let spots = [(1, 20, 2), (7, 25, 9), (14, 30, 4)];
        for (x, y, z) in spots {
            chunk
                .section
                .set_block_absolute_y(x, y, z, Block::GRASS_BLOCK.default_state.id);
        }
        let samples = sample(&chunk, 1_228_800, 3); // 3 ticking of 4096: about 900 hits
        let mut per_spot = [0usize; 3];
        for s in &samples {
            let at = (s.position.0.x, s.position.0.y, s.position.0.z);
            let n = spots
                .iter()
                .position(|&(x, y, z): &(usize, i32, usize)| (x as i32, y, z as i32) == at)
                .expect("a hit is always on a ticking block");
            per_spot[n] += 1;
        }
        assert!(
            (700..1100).contains(&samples.len()),
            "{} hits, expected about 900",
            samples.len()
        );
        for count in per_spot {
            let share = count as f64 / samples.len() as f64;
            assert!((0.25..0.42).contains(&share), "shares {per_spot:?}");
        }
    }

    #[test]
    fn the_hit_rate_is_the_share_of_ticking_positions() {
        // 256 of 4096 positions (a layer of grass) is 6.25 % of the draws
        let chunk = ChunkData::empty(0, 0);
        for x in 0..16 {
            for z in 0..16 {
                chunk
                    .section
                    .set_block_absolute_y(x, 40, z, Block::GRASS_BLOCK.default_state.id);
            }
        }
        let samples = sample(&chunk, 160_000, 21);
        let rate = samples.len() as f64 / 160_000.0;
        assert!((0.058..0.067).contains(&rate), "rate {rate}");
        assert!(samples.iter().all(|s| s.position.0.y == 40));
    }

    #[test]
    fn a_fluid_that_reacts_is_reported_as_one() {
        let chunk = ChunkData::empty(0, 0);
        chunk
            .section
            .set_block_absolute_y(2, 10, 3, Block::LAVA.default_state.id);
        let samples = sample(&chunk, 819_200, 17);
        assert!(!samples.is_empty());
        assert!(samples.iter().all(|s| s.tick_fluid));
    }
}
