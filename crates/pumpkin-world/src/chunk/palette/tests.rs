use super::{BlockPalette, NetworkPalette, PalettedContainer};
use pumpkin_data::{Block, BlockStateId};

#[test]
fn matching_visits_distinct_values_after_mutations() {
    let mut palette = BlockPalette::default();
    let plate = Block::STONE_PRESSURE_PLATE.default_state.id;
    assert!(!palette.contains_matching(|state| state == plate));
    palette.set(1, 2, 3, plate);
    assert!(palette.contains_matching(|state| state == plate));
    let mut visited = Vec::new();
    assert!(!palette.contains_matching(|state| {
        visited.push(state);
        false
    }));
    assert_eq!(visited.len(), 2);
    palette.set(1, 2, 3, Block::AIR.default_state.id);
    assert!(!palette.contains_matching(|state| state == plate));
}

#[test]
fn matching_ignores_unused_entries_in_packed_palettes() {
    let indexed = PalettedContainer::<u16, 2>::from_palette_and_packed_data(&[10, 20, 30], &[2], 2);
    assert!(indexed.contains_matching(|value| value == 10));
    assert!(indexed.contains_matching(|value| value == 30));
    assert!(!indexed.contains_matching(|value| value == 20));

    let dense = PalettedContainer::<u8, 2>::from_palette_and_packed_data(&[10, 20, 30], &[2], 2);
    assert!(dense.contains_matching(|value| value == 10));
    assert!(dense.contains_matching(|value| value == 30));
    assert!(!dense.contains_matching(|value| value == 20));
}

#[test]
fn matching_includes_fallback_for_invalid_packed_indices() {
    let palette =
        PalettedContainer::<u16, 2>::from_palette_and_packed_data(&[10, 20, 30], &[0xffff], 2);
    assert!(palette.iter().all(|value| value == 10));
    assert!(palette.contains_matching(|value| value == 10));
    assert!(!palette.contains_matching(|value| value == 20 || value == 30));
}

#[test]
fn matching_uses_stored_indices_when_binary_counts_are_incorrect()
-> Result<(), Box<dyn std::error::Error>> {
    use bytes::BufMut;

    let plate = Block::STONE_PRESSURE_PLATE.default_state.id;
    let air = Block::AIR.default_state.id;
    let mut encoded = vec![1, 0, 2];
    encoded.put_u16(plate.as_u16());
    encoded.put_u16(0);
    encoded.put_u16(air.as_u16());
    encoded.put_u16(BlockPalette::VOLUME as u16);
    encoded.resize(encoded.len() + BlockPalette::VOLUME, 0);
    let palette = BlockPalette::read_pump_binary(&mut encoded.as_slice())?;
    assert!(palette.iter().all(|value| value == plate));
    assert!(palette.contains_matching(|value| value == plate));
    assert!(!palette.contains_matching(|value| value == air));
    Ok(())
}

fn network_palette_values(palette: NetworkPalette<u16>) -> Option<Box<[u16]>> {
    match palette {
        NetworkPalette::Single(value) => Some(Box::new([value])),
        NetworkPalette::Indirect(values) => Some(values),
        NetworkPalette::Direct => None,
    }
}

fn assert_bulk_matches_mutations(value_at: impl Fn(usize, usize, usize) -> BlockStateId) {
    let mut mutated = BlockPalette::default();
    for y in 0..BlockPalette::SIZE {
        for z in 0..BlockPalette::SIZE {
            for x in 0..BlockPalette::SIZE {
                mutated.set(x, y, z, value_at(x, y, z));
            }
        }
    }
    let bulk = BlockPalette::from_fn(value_at);

    assert_eq!(
        mutated.iter().collect::<Vec<_>>(),
        bulk.iter().collect::<Vec<_>>()
    );
    assert_eq!(
        mutated.random_ticking_counts(),
        bulk.random_ticking_counts()
    );
    assert_eq!(mutated.non_air_block_count(), bulk.non_air_block_count());
    assert_eq!(mutated.liquid_block_count(), bulk.liquid_block_count());

    let mutated_network = mutated.convert_network();
    let bulk_network = bulk.convert_network();
    assert_eq!(mutated_network.bits_per_entry, bulk_network.bits_per_entry);
    assert_eq!(mutated_network.packed_data, bulk_network.packed_data);
    assert_eq!(
        network_palette_values(mutated_network.palette),
        network_palette_values(bulk_network.palette)
    );
}

#[test]
fn bulk_palette_matches_individual_mutations() {
    let states = [
        Block::AIR.default_state.id,
        Block::STONE.default_state.id,
        Block::WATER.default_state.id,
        Block::LAVA.default_state.id,
    ];
    assert_bulk_matches_mutations(|x, y, z| states[(x + y + z) % states.len()]);
}

#[test]
fn bulk_palette_handles_homogeneous_sections() {
    assert_bulk_matches_mutations(|_, _, _| Block::STONE.default_state.id);
}

#[test]
fn bulk_palette_handles_direct_network_palettes() {
    assert_bulk_matches_mutations(|x, y, z| {
        BlockStateId::new_or_air(((y * 256 + z * 16 + x) % 300) as u16)
    });
}

#[test]
fn bedrock_palette_uses_supported_bit_widths() {
    let palette = BlockPalette::from_fn(|x, y, z| {
        BlockStateId::new(((y * 256 + z * 16 + x) % 65) as u16).unwrap()
    });
    let network = palette.convert_be_network();

    assert_eq!(network.bits_per_entry, 8);
    assert_eq!(network.packed_data.len(), 1024);
    assert!(matches!(network.palette, NetworkPalette::Indirect(values) if values.len() == 65));
}

#[test]
fn bedrock_water_palette_uses_bedrock_block_order() {
    let mut palette = BlockPalette::default();
    palette.set(1, 2, 3, Block::SEAGRASS.default_state.id);

    let network = palette.convert_be_water_network().unwrap();
    let (x, y, z) = (1, 2, 3);
    let bedrock_index = x * 256 + z * 16 + y;

    assert_eq!(network.bits_per_entry, 1);
    assert_eq!(
        network.packed_data[bedrock_index / 32],
        1 << (bedrock_index % 32)
    );
    assert_eq!(
        network
            .packed_data
            .iter()
            .filter(|&&word| word != 0)
            .count(),
        1
    );
}
