//! Reading a packed palette (Java's `palette` + `data` pair) into a container: one function per
//! storage kind, sharing the word reader.

use std::hash::Hash;

use tracing::warn;

use super::{HeterogeneousPaletteData, PaletteStorage, PalettedContainer};

/// The palette index of every position in y, z, x order: `64 / bits_per_key` keys per word,
/// the unused high bits of a word ignored, a missing word read as zero.
fn lookup_indices(
    packed_data: &[i64],
    bits_per_key: u8,
    volume: usize,
) -> impl Iterator<Item = usize> + '_ {
    let index_mask = (1 << bits_per_key) - 1;
    let keys_per_i64 = usize::from(64 / bits_per_key);
    let mut packed_data_iter = packed_data.iter();
    let mut current_packed_word = *packed_data_iter.next().unwrap_or(&0);

    (0..volume).map(move |i| {
        let bit_index_in_word = i % keys_per_i64;
        if bit_index_in_word == 0 && i > 0 {
            current_packed_word = *packed_data_iter.next().unwrap_or(&0);
        }

        let lookup_index = ((current_packed_word as u64)
            >> (bit_index_in_word as u64 * u64::from(bits_per_key)))
            & index_mask;
        lookup_index as usize
    })
}

/// Palettes of at most 256 entries whose values are wider than a byte: one `u8` index per
/// position. An index past the palette counts as entry 0 in the counts and stays 0 in the cube.
pub(super) fn indexed<V: Hash + Eq + Copy + Default, const DIM: usize>(
    palette: &[V],
    packed_data: &[i64],
    bits_per_key: u8,
) -> PalettedContainer<V, DIM> {
    let mut indices = Box::new([[[0u8; DIM]; DIM]; DIM]);
    let mut counts = vec![0u16; palette.len()];
    let indices_flat = indices.as_flattened_mut().as_flattened_mut();

    let lookups = lookup_indices(
        packed_data,
        bits_per_key,
        PalettedContainer::<V, DIM>::VOLUME,
    );
    for (index_out, idx) in indices_flat.iter_mut().zip(lookups) {
        if idx < palette.len() {
            *index_out = idx as u8;
            counts[idx] += 1;
        } else {
            warn!("Lookup index out of bounds! Defaulting...");
            counts[0] += 1;
        }
    }

    PalettedContainer::Heterogeneous(Box::new(HeterogeneousPaletteData {
        storage: PaletteStorage::Indexed(indices),
        palette: palette.to_vec(),
        counts,
    }))
}

/// Every other palette: the values themselves, an index past the palette read as the default.
pub(super) fn dense<V: Hash + Eq + Copy + Default, const DIM: usize>(
    palette: &[V],
    packed_data: &[i64],
    bits_per_key: u8,
) -> PalettedContainer<V, DIM> {
    let decompressed_values: Vec<V> = lookup_indices(
        packed_data,
        bits_per_key,
        PalettedContainer::<V, DIM>::VOLUME,
    )
    .map(|lookup_index| {
        palette.get(lookup_index).copied().unwrap_or_else(|| {
            warn!("Lookup index out of bounds! Defaulting...");
            V::default()
        })
    })
    .collect();

    // Now, with all decompressed values, build the counts.
    let mut counts = vec![0; palette.len()];

    for &value in &decompressed_values {
        // This is the key optimization: find the index in the palette Vec
        // and increment the corresponding count.
        if let Some(index) = palette.iter().position(|v| v == &value) {
            counts[index] += 1;
        } else {
            // This case should ideally not happen if the palette is complete.
            warn!("Decompressed value not found in palette!");
        }
    }

    let mut cube = Box::new([[[V::default(); DIM]; DIM]; DIM]);
    cube.as_flattened_mut()
        .as_flattened_mut()
        .copy_from_slice(&decompressed_values);

    PalettedContainer::Heterogeneous(Box::new(HeterogeneousPaletteData {
        storage: PaletteStorage::Dense(cube),
        palette: palette.to_vec(),
        counts,
    }))
}
