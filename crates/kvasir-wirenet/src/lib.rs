//! Settles the power of a whole network of redstone dust in one pass.
//!
//! A dust block shows the larger of two things: the power given to it by everything that is not
//! dust (its *external* power: a lever, a block, a repeater, strong power through a solid block),
//! and one less than the strongest dust it is linked to. Because the second term always loses one,
//! a loop of dust cannot keep itself powered, and the powers that satisfy this rule are unique:
//! whatever order dust blocks are updated in, they end at the same values. So the whole network can
//! be solved at once, each dust block visited a single time, instead of letting every power change
//! of one block ripple through its neighbours again and again.
//!
//! This crate knows nothing about blocks or worlds: the caller describes the network (which dust
//! each block reads its power from, how much external power each block has) and reads the settled
//! powers back.

// Tests state expectations with `expect`; the engine's lints forbid it everywhere else.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::fmt;

use serde::{Deserialize, Serialize};

/// The strongest power a redstone signal can have.
pub const MAX_POWER: u8 = 15;

/// World-file settings for redstone dust.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    /// Settle a dust network in one pass (see the crate documentation) instead of letting each
    /// power change ripple on its own. Same settled powers; the order in which neighbouring blocks
    /// are notified, and the intermediate powers they see, differ from the vanilla cascade.
    pub alternate_current: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            alternate_current: true,
        }
    }
}

/// A link points at a node that does not exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkOutOfRange {
    /// The node that holds the link.
    pub node: usize,
    /// The link's target.
    pub link: usize,
}

impl fmt::Display for LinkOutOfRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "dust node {} is linked to node {}, which is not in the network",
            self.node, self.link
        )
    }
}

impl std::error::Error for LinkOutOfRange {}

/// A network of dust blocks: for each, its external power and the dust it reads power from.
///
/// Reading need not be mutual: dust on a hopper can read the dust below it while that dust, whose
/// rule is different, does not read back. The settled powers are still unique, because a block's
/// power is always one less than what it reads.
#[derive(Debug, Default, Clone)]
pub struct Network {
    externals: Vec<u8>,
    /// `spans[i]` is where node `i`'s links end in `links`; they start where node `i - 1`'s end.
    spans: Vec<usize>,
    links: Vec<usize>,
}

impl Network {
    /// An empty network.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            externals: Vec::new(),
            spans: Vec::new(),
            links: Vec::new(),
        }
    }

    /// Forgets all nodes, keeping the allocations for the next network.
    pub fn clear(&mut self) {
        self.externals.clear();
        self.spans.clear();
        self.links.clear();
    }

    /// The number of nodes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.externals.len()
    }

    /// Whether the network has no nodes.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.externals.is_empty()
    }

    /// Adds the next node: its external power (above [`MAX_POWER`] counts as [`MAX_POWER`]) and the
    /// nodes it reads power from. Nodes are numbered in the order they are added, from 0; a link
    /// may name a node that is added later.
    pub fn push(&mut self, external: u8, reads_from: &[usize]) {
        self.externals.push(external.min(MAX_POWER));
        self.links.extend_from_slice(reads_from);
        self.spans.push(self.links.len());
    }

    fn links_of(&self, node: usize) -> &[usize] {
        let end = self.spans[node];
        let start = if node == 0 { 0 } else { self.spans[node - 1] };
        &self.links[start..end]
    }

    /// Solves the network: after the call, `powers[i]` is the settled power of node `i`.
    ///
    /// # Errors
    /// [`LinkOutOfRange`] when a link names a node that was never added; `powers` is then unspecified.
    pub fn settle(
        &self,
        scratch: &mut Scratch,
        powers: &mut Vec<u8>,
    ) -> Result<(), LinkOutOfRange> {
        let count = self.len();
        for node in 0..count {
            if let Some(&link) = self.links_of(node).iter().find(|&&link| link >= count) {
                return Err(LinkOutOfRange { node, link });
            }
        }

        let Scratch { buckets, readers } = scratch;
        readers.index(self);

        powers.clear();
        powers.extend_from_slice(&self.externals);
        for (node, &power) in self.externals.iter().enumerate() {
            if power > 1 {
                // a node of power 1 gives its neighbours 0, which they already have
                buckets[usize::from(power)].push(node);
            }
        }

        // Strongest first: a node's power is final when its bucket is reached, because a node can
        // only be raised from a stronger one, which has been handled before.
        for level in (2..=MAX_POWER).rev() {
            let level_index = usize::from(level);
            while let Some(node) = buckets[level_index].pop() {
                if powers[node] != level {
                    // raised since it was queued here, and handled at its higher level
                    continue;
                }
                let give = level - 1;
                for &reader in readers.of(node) {
                    let slot = &mut powers[reader];
                    if *slot < give {
                        *slot = give;
                        buckets[usize::from(give)].push(reader);
                    }
                }
            }
        }
        Ok(())
    }
}

/// Working memory for [`Network::settle`], kept between calls so that a settle does not allocate.
/// A settle drains every bucket it fills, so the memory is empty between calls.
#[derive(Debug, Default)]
pub struct Scratch {
    buckets: [Vec<usize>; MAX_POWER as usize + 1],
    readers: Readers,
}

impl Scratch {
    /// Empty working memory.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Who reads each node: the direction power flows, where a [`Network`] lists the opposite.
#[derive(Debug, Default)]
struct Readers {
    /// `start[i]..start[i + 1]` is where node `i`'s readers are in `list`.
    start: Vec<usize>,
    /// Where the next reader of each node goes while `list` is being filled.
    fill: Vec<usize>,
    list: Vec<usize>,
}

impl Readers {
    /// Links are known to be in range.
    fn index(&mut self, network: &Network) {
        let count = network.len();
        self.start.clear();
        self.start.resize(count + 1, 0);
        for &link in &network.links {
            self.start[link + 1] += 1;
        }
        for node in 0..count {
            self.start[node + 1] += self.start[node];
        }
        self.fill.clear();
        self.fill.extend_from_slice(&self.start[..count]);
        self.list.clear();
        self.list.resize(network.links.len(), 0);
        for reader in 0..count {
            for &link in network.links_of(reader) {
                let slot = &mut self.fill[link];
                self.list[*slot] = reader;
                *slot += 1;
            }
        }
    }

    fn of(&self, node: usize) -> &[usize] {
        &self.list[self.start[node]..self.start[node + 1]]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A line of `length` nodes, each linked to the next and previous one.
    fn line(length: usize, externals: &[(usize, u8)]) -> Network {
        let mut network = Network::new();
        for node in 0..length {
            let mut links = Vec::new();
            if node > 0 {
                links.push(node - 1);
            }
            if node + 1 < length {
                links.push(node + 1);
            }
            let external = externals
                .iter()
                .find(|(at, _)| *at == node)
                .map_or(0, |(_, power)| *power);
            network.push(external, &links);
        }
        network
    }

    fn settled(network: &Network) -> Vec<u8> {
        let mut powers = Vec::new();
        network
            .settle(&mut Scratch::new(), &mut powers)
            .expect("links are in range");
        powers
    }

    #[test]
    fn a_source_loses_one_power_per_block() {
        let network = line(18, &[(0, 15)]);
        assert_eq!(
            settled(&network),
            vec![15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 0]
        );
    }

    #[test]
    fn a_source_in_the_middle_spreads_both_ways() {
        let network = line(7, &[(3, 4)]);
        assert_eq!(settled(&network), vec![1, 2, 3, 4, 3, 2, 1]);
    }

    #[test]
    fn the_stronger_signal_wins_at_every_node_and_a_weaker_source_counts_near_itself() {
        // from the 15: 15 14 13 12 11 10 9 8 7; from the 9 at the far end: 1 2 3 4 5 6 7 8 9
        let network = line(9, &[(0, 15), (8, 9)]);
        assert_eq!(settled(&network), vec![15, 14, 13, 12, 11, 10, 9, 8, 9]);
    }

    #[test]
    fn external_power_above_the_maximum_counts_as_the_maximum() {
        let network = line(3, &[(0, 200)]);
        assert_eq!(settled(&network), vec![15, 14, 13]);
    }

    #[test]
    fn a_node_keeps_its_own_external_power_when_neighbours_are_weaker() {
        let network = line(3, &[(0, 3), (1, 9)]);
        assert_eq!(settled(&network), vec![8, 9, 8]);
    }

    #[test]
    fn no_power_anywhere_settles_to_zero() {
        assert_eq!(settled(&line(5, &[])), vec![0; 5]);
    }

    #[test]
    fn a_loop_cannot_keep_itself_powered() {
        // a ring of 6: the source is removed, nothing is left even though every block has a
        // powered neighbour in the old state
        let mut network = Network::new();
        for node in 0..6usize {
            network.push(0, &[(node + 5) % 6, (node + 1) % 6]);
        }
        assert_eq!(settled(&network), vec![0; 6]);
    }

    #[test]
    fn a_ring_with_a_source_takes_the_shorter_way_round() {
        let mut network = Network::new();
        for node in 0..6usize {
            let external = if node == 0 { 10 } else { 0 };
            network.push(external, &[(node + 5) % 6, (node + 1) % 6]);
        }
        assert_eq!(settled(&network), vec![10, 9, 8, 7, 8, 9]);
    }

    #[test]
    fn a_node_read_by_four_gives_power_to_all_of_them() {
        let mut network = Network::new();
        network.push(8, &[]);
        for _ in 0..4 {
            network.push(0, &[0]);
        }
        assert_eq!(settled(&network), vec![8, 7, 7, 7, 7]);
    }

    #[test]
    fn reading_one_way_passes_power_one_way() {
        // node 1 reads node 0; node 0 does not read node 1
        let mut network = Network::new();
        network.push(9, &[]);
        network.push(0, &[0]);
        assert_eq!(settled(&network), vec![9, 8]);
        let mut backwards = Network::new();
        backwards.push(0, &[]);
        backwards.push(9, &[0]);
        assert_eq!(settled(&backwards), vec![0, 9]);
    }

    #[test]
    fn readers_are_found_for_nodes_in_any_order() {
        // 3 reads 0, 2 reads 0 and 3, 1 reads 2; a chain 0 -> {3, 2} -> 2 -> 1 with 0 as the source
        let mut network = Network::new();
        network.push(12, &[]);
        network.push(0, &[2]);
        network.push(0, &[0, 3]);
        network.push(0, &[0]);
        assert_eq!(settled(&network), vec![12, 10, 11, 11]);
    }

    #[test]
    fn separate_networks_in_one_description_do_not_mix() {
        let mut network = Network::new();
        network.push(5, &[1]);
        network.push(0, &[0]);
        network.push(0, &[3]);
        network.push(0, &[2]);
        assert_eq!(settled(&network), vec![5, 4, 0, 0]);
    }

    #[test]
    fn an_empty_network_settles_to_nothing() {
        assert!(settled(&Network::new()).is_empty());
    }

    #[test]
    fn a_link_to_a_missing_node_is_an_error_and_names_it() {
        let mut network = Network::new();
        network.push(1, &[1]);
        network.push(0, &[0, 7]);
        let mut powers = Vec::new();
        let error = network
            .settle(&mut Scratch::new(), &mut powers)
            .expect_err("node 7 does not exist");
        assert_eq!(error, LinkOutOfRange { node: 1, link: 7 });
        assert!(error.to_string().contains("node 1"));
    }

    #[test]
    fn a_network_can_be_cleared_and_reused_with_the_same_scratch() {
        let mut scratch = Scratch::new();
        let mut powers = Vec::new();
        let mut network = line(4, &[(0, 15)]);
        network.settle(&mut scratch, &mut powers).expect("in range");
        assert_eq!(powers, vec![15, 14, 13, 12]);
        network.clear();
        assert!(network.is_empty());
        network.push(3, &[1]);
        network.push(0, &[0]);
        network.settle(&mut scratch, &mut powers).expect("in range");
        assert_eq!(powers, vec![3, 2]);
    }

    /// Reference by definition: start from the external powers and apply the local rule in a
    /// scrambled order until nothing changes.
    fn by_relaxation(network: &Network, seed: &mut u64) -> Vec<u8> {
        let mut powers: Vec<u8> = network.externals.clone();
        let count = network.len();
        loop {
            let mut changed = false;
            let mut order: Vec<usize> = (0..count).collect();
            for at in (1..count).rev() {
                let other = (next(seed) % (at as u64 + 1)) as usize;
                order.swap(at, other);
            }
            for node in order {
                let best = network
                    .links_of(node)
                    .iter()
                    .map(|&link| powers[link].saturating_sub(1))
                    .max()
                    .unwrap_or(0)
                    .max(network.externals[node]);
                if best != powers[node] {
                    powers[node] = best;
                    changed = true;
                }
            }
            if !changed {
                return powers;
            }
        }
    }

    fn next(seed: &mut u64) -> u64 {
        // xorshift64*: deterministic, no dependency
        *seed ^= *seed >> 12;
        *seed ^= *seed << 25;
        *seed ^= *seed >> 27;
        seed.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn random_network(seed: &mut u64) -> Network {
        let count = (next(seed) % 40 + 1) as usize;
        let mut adjacent = vec![Vec::new(); count];
        let edges = next(seed) % (count as u64 * 2) + 1;
        for _ in 0..edges {
            let a = (next(seed) % count as u64) as usize;
            let b = (next(seed) % count as u64) as usize;
            if a != b && !adjacent[a].contains(&b) {
                adjacent[a].push(b);
                // most reading is mutual; some is one way
                if !next(seed).is_multiple_of(3) && !adjacent[b].contains(&a) {
                    adjacent[b].push(a);
                }
            }
        }
        let mut network = Network::new();
        for links in &adjacent {
            let external = match next(seed) % 4 {
                0 => (next(seed) % 17) as u8,
                _ => 0,
            };
            network.push(external, links);
        }
        network
    }

    #[test]
    fn it_matches_a_scrambled_relaxation_on_random_graphs_with_loops() {
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut scratch = Scratch::new();
        let mut powers = Vec::new();
        for round in 0..2_000 {
            let network = random_network(&mut seed);
            network.settle(&mut scratch, &mut powers).expect("in range");
            assert_eq!(
                powers,
                by_relaxation(&network, &mut seed),
                "round {round}, {} nodes",
                network.len()
            );
        }
    }

    #[test]
    fn settled_powers_obey_the_rule_everywhere() {
        let mut seed = 42u64;
        let mut scratch = Scratch::new();
        let mut powers = Vec::new();
        for _ in 0..500 {
            let network = random_network(&mut seed);
            network.settle(&mut scratch, &mut powers).expect("in range");
            for node in 0..network.len() {
                let from_links = network
                    .links_of(node)
                    .iter()
                    .map(|&link| powers[link].saturating_sub(1))
                    .max()
                    .unwrap_or(0);
                assert_eq!(powers[node], from_links.max(network.externals[node]));
                assert!(powers[node] <= MAX_POWER);
            }
        }
    }

    #[test]
    fn the_default_config_turns_it_on_and_a_file_can_turn_it_off() {
        assert!(Config::default().alternate_current);
        let off: Config = serde_json::from_str(r#"{"alternate_current": false}"#).expect("parses");
        assert!(!off.alternate_current);
        let missing: Config = serde_json::from_str("{}").expect("parses");
        assert!(missing.alternate_current);
    }
}
