use super::ChunkSections;
use crate::chunk::palette::BlockPalette;
use pumpkin_data::{Block, block_properties::has_random_ticks};

/// A block set at (x, y, z) moves the heights of column (x, z) and of no other column, for
/// both writers that keep the heightmaps (`set_block_absolute_y`, `set_relative_block`).
#[test]
fn a_placed_block_raises_its_own_column_only() {
    use super::{ChunkData, ChunkHeightmapType};
    let types = [
        ChunkHeightmapType::WorldSurface,
        ChunkHeightmapType::MotionBlocking,
        ChunkHeightmapType::MotionBlockingNoLeaves,
    ];
    let heights = |chunk: &ChunkData, x: i32, z: i32| {
        let heightmap = chunk.heightmap.lock().unwrap();
        types.map(|kind| heightmap.get(kind, x, z, chunk.section.min_y))
    };
    let min_y = ChunkData::empty(0, 0).section.min_y;

    let chunk = ChunkData::empty(0, 0);
    for y in 64..=70 {
        chunk.set_block_absolute_y(3, y, 9, Block::STONE.default_state.id);
    }
    assert_eq!(heights(&chunk, 3, 9), [70; 3]);
    for (x, z) in [(3, 0), (3, 6), (9, 3), (0, 0)] {
        assert_eq!(heights(&chunk, x, z), [min_y - 1; 3], "column {x} {z}");
    }
    chunk.set_block_absolute_y(3, 70, 9, Block::AIR.default_state.id);
    assert_eq!(heights(&chunk, 3, 9), [69; 3]);

    let mut chunk = ChunkData::empty(0, 0);
    chunk.set_relative_block(3, (70 - min_y) as usize, 9, Block::STONE.default_state.id);
    assert_eq!(heights(&chunk, 3, 9), [70; 3]);
    assert_eq!(heights(&chunk, 3, 6), [min_y - 1; 3]);
}

#[test]
fn random_tick_cache_initializes_from_palette_contents() {
    let mut sections = vec![BlockPalette::default(), BlockPalette::default()];
    sections[1].set(0, 0, 0, Block::LAVA.default_state.id);

    let (cache, _mask) = ChunkSections::build_random_tick_sections_cache(&sections);
    let cache = cache.unwrap();
    assert!(!cache[0].is_randomly_ticking());
    assert!(cache[1].random_ticking_fluid_count > 0);
    assert!(cache[1].is_randomly_ticking());
}

#[test]
fn random_tick_cache_updates_on_block_mutation() {
    let min_y = -64;
    let sections = ChunkSections::new(1, min_y);

    assert!(
        sections
            .random_tick_sections
            .read()
            .unwrap()
            .as_ref()
            .is_none_or(|c| !c[0].is_randomly_ticking()),
        "fresh sections should not be randomly ticking"
    );

    let random_block_state = Block::WHEAT.default_state.id;
    assert!(
        has_random_ticks(random_block_state),
        "test requires a known randomly ticking block state"
    );

    sections.set_block_absolute_y(0, min_y, 0, random_block_state);
    {
        let cache = sections.random_tick_sections.read().unwrap();
        let cache = cache.as_ref().unwrap();
        assert_eq!(cache[0].random_ticking_block_count, 1);
        assert_eq!(cache[0].random_ticking_fluid_count, 0);
        assert!(cache[0].is_randomly_ticking());
    };

    sections.set_block_absolute_y(0, min_y, 0, Block::STONE.default_state.id);
    {
        let cache = sections.random_tick_sections.read().unwrap();
        let cache = cache.as_ref().unwrap();
        assert_eq!(cache[0].random_ticking_block_count, 0);
        assert_eq!(cache[0].random_ticking_fluid_count, 0);
        assert!(!cache[0].is_randomly_ticking());
    };

    sections.set_block_absolute_y(0, min_y, 0, Block::LAVA.default_state.id);
    {
        let cache = sections.random_tick_sections.read().unwrap();
        let cache = cache.as_ref().unwrap();
        assert!(cache[0].random_ticking_fluid_count > 0);
        assert!(cache[0].is_randomly_ticking());
    }
}

use crate::chunk::{
    ChunkData, RandomTickSectionCache, palette::has_random_ticking_fluid,
    ticking_index::section_position,
};

/// Every position of section 0 whose state reacts to random ticks, by looking at all 4096.
fn brute_force_ticking(sections: &ChunkSections) -> Vec<u16> {
    let blocks = sections.block_sections.read().unwrap();
    let mut found = Vec::new();
    for y in 0..16 {
        for z in 0..16 {
            for x in 0..16 {
                let state = blocks[0].get(x, y, z);
                if has_random_ticks(state) || has_random_ticking_fluid(state) {
                    found.push(section_position(x, y, z));
                }
            }
        }
    }
    found.sort_unstable();
    found
}

fn indexed_ticking(cache: &RandomTickSectionCache) -> Vec<u16> {
    let mut found: Vec<u16> = (0..cache.ticking.len())
        .map(|n| cache.ticking.get(n).unwrap().position)
        .collect();
    found.sort_unstable();
    found
}

#[test]
fn the_ticking_index_lists_the_same_positions_as_a_scan_of_the_section() {
    let min_y = -64;
    let sections = ChunkSections::new(1, min_y);
    let wheat = Block::WHEAT.default_state.id;
    let lava = Block::LAVA.default_state.id;
    let stone = Block::STONE.default_state.id;
    // scattered writes, an overwrite and a removal
    for (x, y, z, state) in [
        (3, 5, 7, wheat),
        (0, 0, 0, wheat),
        (15, 15, 15, lava),
        (8, 1, 9, wheat),
        (3, 5, 7, stone),
        (4, 5, 7, wheat),
    ] {
        sections.set_block_absolute_y(x, min_y + y, z, state);
    }
    let expected = brute_force_ticking(&sections);
    assert_eq!(expected.len(), 4);
    {
        let cache = sections.random_tick_sections.read().unwrap();
        assert_eq!(indexed_ticking(&cache.as_ref().unwrap()[0]), expected);
    };
    // the index built from the blocks alone, as on loading a chunk, is the same
    let blocks = sections.block_sections.read().unwrap();
    let (rebuilt, mask) = ChunkSections::build_random_tick_sections_cache(&blocks);
    assert_eq!(mask, 1);
    assert_eq!(indexed_ticking(&rebuilt.unwrap()[0]), expected);
}

#[test]
fn the_index_follows_batched_writes_too() {
    let min_y = -64;
    let chunk = ChunkData::empty(0, 0);
    let wheat = Block::WHEAT.default_state.id;
    chunk.set_blocks_batch([
        (1, min_y, 1, wheat),
        (2, min_y + 3, 2, wheat),
        (1, min_y, 1, Block::STONE.default_state.id),
    ]);
    let expected = brute_force_ticking(&chunk.section);
    assert_eq!(expected, vec![section_position(2, 3, 2)]);
    let cache = chunk.section.random_tick_sections.read().unwrap();
    assert_eq!(indexed_ticking(&cache.as_ref().unwrap()[0]), expected);
}

#[test]
fn heightmap_is_opaque() {
    use crate::chunk::ChunkHeightmapType;

    let air = Block::AIR.default_state;
    let stone = Block::STONE.default_state;
    let leaves = Block::OAK_LEAVES.default_state;
    let water = Block::WATER.default_state;

    // WORLD_SURFACE: Everything except air
    assert!(!ChunkHeightmapType::WorldSurface.is_opaque(air));
    assert!(ChunkHeightmapType::WorldSurface.is_opaque(stone));
    assert!(ChunkHeightmapType::WorldSurface.is_opaque(leaves));
    assert!(ChunkHeightmapType::WorldSurface.is_opaque(water));

    // MOTION_BLOCKING: Blocks movement OR is liquid
    assert!(!ChunkHeightmapType::MotionBlocking.is_opaque(air));
    assert!(ChunkHeightmapType::MotionBlocking.is_opaque(stone));
    assert!(ChunkHeightmapType::MotionBlocking.is_opaque(leaves)); // Leaves block movement
    assert!(ChunkHeightmapType::MotionBlocking.is_opaque(water)); // Water is liquid

    // MOTION_BLOCKING_NO_LEAVES: Blocks movement OR is liquid, but NOT leaves
    assert!(!ChunkHeightmapType::MotionBlockingNoLeaves.is_opaque(air));
    assert!(ChunkHeightmapType::MotionBlockingNoLeaves.is_opaque(stone));
    assert!(!ChunkHeightmapType::MotionBlockingNoLeaves.is_opaque(leaves)); // Excludes leaves
    assert!(ChunkHeightmapType::MotionBlockingNoLeaves.is_opaque(water)); // Water is liquid
}

#[test]
fn chunk_custom_data() {
    use pumpkin_nbt::tag::NbtTag;

    let chunk = super::ChunkData::empty(0, 0);
    assert!(!chunk.has_custom_data("my_plugin", "test_key"));
    assert_eq!(chunk.get_custom_data("my_plugin", "test_key"), None);

    chunk.set_custom_data(
        "my_plugin",
        "test_key",
        NbtTag::String("hello_pumpkin".into()),
    );
    assert!(chunk.has_custom_data("my_plugin", "test_key"));
    assert_eq!(
        chunk.get_custom_data("my_plugin", "test_key"),
        Some(NbtTag::String("hello_pumpkin".into()))
    );

    chunk.set_custom_data("my_plugin", "number_key", NbtTag::Int(42));
    assert_eq!(
        chunk.get_custom_data("my_plugin", "number_key"),
        Some(NbtTag::Int(42))
    );

    chunk.remove_custom_data("my_plugin", "test_key");
    assert!(!chunk.has_custom_data("my_plugin", "test_key"));
    assert!(chunk.has_custom_data("my_plugin", "number_key"));
}
