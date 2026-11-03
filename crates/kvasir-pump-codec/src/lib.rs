//! The chunk frame of a `.pump` region file.
//!
//! A frame is `u32` length of the raw chunk (big endian), `u64` XXH64 of the raw chunk (big endian),
//! then one standard zstd frame holding the raw chunk. The layout is the one the engine always
//! used, so worlds written before this crate still load, and any zstd decoder reads the payload.
//! What changed is the compressor: the pure-Rust encoder the engine used only implements its
//! fastest level and made frames about 3 times larger than libzstd's level 3 on real chunks.
//!
//! Contexts are expensive to build next to a 70 KB chunk, so each thread keeps one compressor and
//! one decompressor.

// Tests state expectations with `unwrap`; the engine's lints forbid it everywhere else.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::{cell::RefCell, io};

use xxhash_rust::xxh64::xxh64;
use zstd::bulk::{Compressor, Decompressor};

/// Bytes before the zstd frame: raw length (4) and checksum (8).
pub const HEADER_BYTES: usize = 12;

/// The most a frame may claim to hold. A damaged length must not make us allocate gigabytes.
pub const MAX_RAW_BYTES: usize = 64 * 1024 * 1024;

/// zstd's own default level: about 500 MB/s, within 15% of level 9 on real chunks (measured).
pub const LEVEL: i32 = 3;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CodecError {
    #[error("frame is {0} bytes, shorter than its {HEADER_BYTES}-byte header")]
    Truncated(usize),
    #[error("frame claims {0} raw bytes, above the limit of {MAX_RAW_BYTES}")]
    TooLarge(usize),
    #[error("chunk of {0} bytes does not fit the frame's 32-bit length")]
    ChunkTooLarge(usize),
    #[error("zstd refused the payload: {0}")]
    Zstd(io::Error),
    #[error("frame says {expected} raw bytes, the payload holds {found}")]
    LengthMismatch { expected: usize, found: usize },
    #[error("checksum of the raw chunk does not match the frame")]
    ChecksumMismatch,
}

thread_local! {
    static COMPRESSOR: RefCell<Option<Compressor<'static>>> = const { RefCell::new(None) };
    static DECOMPRESSOR: RefCell<Option<Decompressor<'static>>> = const { RefCell::new(None) };
}

/// Builds the frame for a raw chunk.
///
/// # Errors
/// The chunk is larger than 4 GiB, or zstd failed (out of memory).
pub fn encode(raw: &[u8]) -> Result<Vec<u8>, CodecError> {
    let length = u32::try_from(raw.len()).map_err(|_| CodecError::ChunkTooLarge(raw.len()))?;
    let compressed = COMPRESSOR.with(|slot| {
        let mut slot = slot.borrow_mut();
        let compressor = if let Some(compressor) = slot.as_mut() {
            compressor
        } else {
            slot.insert(Compressor::new(LEVEL).map_err(CodecError::Zstd)?)
        };
        compressor.compress(raw).map_err(CodecError::Zstd)
    })?;
    let mut frame = Vec::with_capacity(HEADER_BYTES + compressed.len());
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(&xxh64(raw, 0).to_be_bytes());
    frame.extend_from_slice(&compressed);
    Ok(frame)
}

/// Reads a frame back into the raw chunk, checking its length and checksum.
///
/// # Errors
/// The frame is cut short, claims too much, does not decompress, or does not match its own
/// length and checksum.
pub fn decode(frame: &[u8]) -> Result<Vec<u8>, CodecError> {
    let (header, payload) = frame
        .split_first_chunk::<HEADER_BYTES>()
        .ok_or(CodecError::Truncated(frame.len()))?;
    let (length, checksum) = header.split_at(4);
    let length = u32::from_be_bytes(length.try_into().map_err(|_| CodecError::Truncated(0))?);
    let checksum = u64::from_be_bytes(checksum.try_into().map_err(|_| CodecError::Truncated(0))?);
    let length = usize::try_from(length).map_err(|_| CodecError::TooLarge(usize::MAX))?;
    if length > MAX_RAW_BYTES {
        return Err(CodecError::TooLarge(length));
    }
    let raw = DECOMPRESSOR.with(|slot| {
        let mut slot = slot.borrow_mut();
        let decompressor = if let Some(decompressor) = slot.as_mut() {
            decompressor
        } else {
            slot.insert(Decompressor::new().map_err(CodecError::Zstd)?)
        };
        // The capacity is the claimed length: a payload that holds more is refused, not grown.
        decompressor
            .decompress(payload, length)
            .map_err(CodecError::Zstd)
    })?;
    if raw.len() != length {
        return Err(CodecError::LengthMismatch {
            expected: length,
            found: raw.len(),
        });
    }
    if xxh64(&raw, 0) != checksum {
        return Err(CodecError::ChecksumMismatch);
    }
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use ruzstd::{
        decoding::StreamingDecoder,
        encoding::{CompressionLevel, compress_to_vec},
    };

    use super::*;

    /// One real chunk from a pre-generated world (73 209 bytes), kept compressed with the zstd
    /// command line at level 19 so that the fixture does not depend on this crate.
    fn real_chunk() -> Vec<u8> {
        let packed = include_bytes!("../tests/fixtures/real-chunk.nbt.zst");
        let mut raw = Vec::new();
        StreamingDecoder::new(&packed[..])
            .unwrap()
            .read_to_end(&mut raw)
            .unwrap();
        raw
    }

    /// The engine's frame as it was written before this crate: ruzstd's fastest level.
    fn old_frame(raw: &[u8]) -> Vec<u8> {
        let mut frame = Vec::new();
        frame.extend_from_slice(&u32::try_from(raw.len()).unwrap().to_be_bytes());
        frame.extend_from_slice(&xxh64(raw, 0).to_be_bytes());
        frame.extend_from_slice(&compress_to_vec(raw, CompressionLevel::Fastest));
        frame
    }

    #[test]
    fn the_fixture_is_the_real_chunk_it_claims_to_be() {
        let raw = real_chunk();
        assert_eq!(raw.len(), 73_209);
        assert_eq!(&raw[..4], b"PCHK", "the engine's own chunk magic");
    }

    #[test]
    fn xxh64_matches_the_published_vectors() {
        assert_eq!(xxh64(b"", 0), 0xEF46_DB37_51D8_E999);
        assert_eq!(xxh64(b"abc", 0), 0x44BC_2CF5_AD77_0999);
    }

    #[test]
    fn a_frame_is_length_checksum_then_a_standard_zstd_frame() {
        let raw = real_chunk();
        let frame = encode(&raw).unwrap();
        assert_eq!(&frame[..4], &73_209u32.to_be_bytes());
        assert_eq!(&frame[4..12], &xxh64(&raw, 0).to_be_bytes());
        assert_eq!(
            &frame[12..16],
            &[0x28, 0xB5, 0x2F, 0xFD],
            "zstd magic number"
        );
        // an independent decoder reads the payload: engines and tools that predate this crate still can
        let mut decoded = Vec::new();
        StreamingDecoder::new(&frame[12..])
            .unwrap()
            .read_to_end(&mut decoded)
            .unwrap();
        assert_eq!(decoded, raw);
    }

    #[test]
    fn round_trips_empty_tiny_repetitive_and_incompressible_chunks() {
        let mut noise = vec![0u8; 100_000];
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        for byte in &mut noise {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = state.to_be_bytes()[3];
        }
        for raw in [Vec::new(), vec![7], vec![0; 1_000_000], noise, real_chunk()] {
            let frame = encode(&raw).unwrap();
            assert_eq!(decode(&frame).unwrap(), raw, "{} bytes", raw.len());
        }
    }

    #[test]
    fn worlds_written_before_this_crate_still_load() {
        let raw = real_chunk();
        assert_eq!(decode(&old_frame(&raw)).unwrap(), raw);
    }

    #[test]
    fn a_real_chunk_is_at_least_twice_as_small_as_the_old_writer_made_it() {
        let raw = real_chunk();
        let now = encode(&raw).unwrap().len();
        let before = old_frame(&raw).len();
        // measured at the time of writing: 5 374 against 14 663 bytes (zstd command line, level 3: 5 362)
        assert!(now * 2 < before, "{now} against {before}");
        assert!(now < 6_000, "{now}");
    }

    /// Damage must never produce wrong data. A few bit flips land in bytes zstd ignores and decode
    /// to the very same chunk; every other flip must be refused.
    #[test]
    fn a_flipped_bit_never_gives_back_a_different_chunk() {
        let raw = real_chunk();
        let frame = encode(&raw).unwrap();
        let mut refused = 0;
        let mut harmless = 0;
        for position in 0..frame.len() {
            let mut damaged = frame.clone();
            damaged[position] ^= 0x10;
            match decode(&damaged) {
                Err(_) => refused += 1,
                Ok(decoded) => {
                    assert_eq!(decoded, raw, "byte {position} changed the chunk silently");
                    harmless += 1;
                }
            }
        }
        assert!(
            refused > frame.len() * 99 / 100,
            "{refused} refused, {harmless} harmless"
        );
    }

    #[test]
    fn a_wrong_checksum_is_named() {
        let mut frame = encode(&real_chunk()).unwrap();
        frame[11] ^= 1;
        assert!(matches!(decode(&frame), Err(CodecError::ChecksumMismatch)));
    }

    #[test]
    fn a_length_that_disagrees_with_the_payload_is_refused_in_both_directions() {
        let raw = real_chunk();
        let mut frame = encode(&raw).unwrap();
        // claims more than the payload holds
        frame[..4].copy_from_slice(&(raw.len() as u32 + 10).to_be_bytes());
        assert!(matches!(
            decode(&frame),
            Err(CodecError::LengthMismatch { .. } | CodecError::Zstd(_))
        ));
        // claims less: zstd must not be allowed to grow past what the header said
        frame[..4].copy_from_slice(&(raw.len() as u32 - 10).to_be_bytes());
        assert!(decode(&frame).is_err());
    }

    #[test]
    fn an_absurd_length_is_refused_before_anything_is_allocated() {
        let mut frame = encode(&[1, 2, 3]).unwrap();
        frame[..4].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(matches!(decode(&frame), Err(CodecError::TooLarge(_))));
        frame[..4].copy_from_slice(&(MAX_RAW_BYTES as u32 + 1).to_be_bytes());
        assert!(matches!(decode(&frame), Err(CodecError::TooLarge(_))));
    }

    #[test]
    fn the_limit_itself_is_allowed_by_the_length_check() {
        let mut frame = encode(&[1, 2, 3]).unwrap();
        frame[..4].copy_from_slice(&(MAX_RAW_BYTES as u32).to_be_bytes());
        // past the length check, the payload then disagrees with the claimed length
        assert!(!matches!(decode(&frame), Err(CodecError::TooLarge(_))));
    }

    #[test]
    fn short_input_is_truncated_not_a_panic() {
        for len in 0..HEADER_BYTES {
            assert!(matches!(
                decode(&vec![0; len]),
                Err(CodecError::Truncated(n)) if n == len
            ));
        }
        let frame = encode(&real_chunk()).unwrap();
        assert!(decode(&frame[..frame.len() - 5]).is_err());
        assert!(decode(&frame[..HEADER_BYTES]).is_err());
    }

    #[test]
    fn threads_do_not_share_state_and_each_reuses_its_own_context() {
        let raw = real_chunk();
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let raw = raw.clone();
                std::thread::spawn(move || {
                    for _ in 0..20 {
                        let frame = encode(&raw).unwrap();
                        assert_eq!(decode(&frame).unwrap(), raw);
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
    }
}
