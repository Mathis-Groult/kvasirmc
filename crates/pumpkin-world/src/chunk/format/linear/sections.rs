//! Reading a linear v2 file section by section: superblock, chunk bitmap, features, bucket size
//! table, footer, then the buckets themselves.

use std::io::{ErrorKind, Read};
use std::path::Path;

use bytes::{Buf, Bytes};
use ruzstd::decoding::StreamingDecoder;
use tracing::error;
use xxhash_rust::xxh64::xxh64;

use super::{
    BucketChunkEntry, BucketSizeEntry, CHUNK_COUNT, ChunkBitmap, CompressedBucket, LinearV2File,
    LinearV2Superblock, NbtFeatures, SIGNATURE,
};
use crate::chunk::ChunkReadingError;

fn unexpected_eof() -> ChunkReadingError {
    ChunkReadingError::IoError(std::io::Error::from(ErrorKind::UnexpectedEof))
}

/// Splits the next `len` bytes off `buf`.
fn take(buf: &mut Bytes, len: usize) -> Result<Bytes, ChunkReadingError> {
    if buf.remaining() < len {
        return Err(unexpected_eof());
    }
    Ok(buf.split_to(len))
}

/// The size table: one entry per bucket.
fn read_bucket_entries(
    buf: &mut Bytes,
    bucket_count: usize,
) -> Result<Vec<BucketSizeEntry>, ChunkReadingError> {
    (0..bucket_count)
        .map(|_| BucketSizeEntry::from_bytes(buf))
        .collect()
}

/// Checks that the footer signature follows the buckets, without consuming the bucket bytes.
fn check_footer(buf: &Bytes, entries: &[BucketSizeEntry]) -> Result<(), ChunkReadingError> {
    let total_compressed: usize = entries.iter().map(|e| e.size as usize).sum();
    if buf.remaining() < total_compressed + SIGNATURE.len() {
        return Err(unexpected_eof());
    }
    if buf[total_compressed..total_compressed + SIGNATURE.len()] != SIGNATURE {
        error!("Linear v2: invalid footer signature");
        return Err(ChunkReadingError::InvalidHeader);
    }
    Ok(())
}

fn decompress(compressed: &[u8]) -> Result<Vec<u8>, ChunkReadingError> {
    let mut decompressed = Vec::new();
    StreamingDecoder::new(compressed)
        .map_err(|_| ChunkReadingError::RegionIsInvalid)?
        .read_to_end(&mut decompressed)
        .map_err(ChunkReadingError::IoError)?;
    Ok(decompressed)
}

impl LinearV2File {
    /// A file with the superblock's coordinates and grid and no chunks yet.
    fn blank(path: &Path, superblock: &LinearV2Superblock) -> Self {
        Self {
            path: path.to_path_buf(),
            region_x: superblock.region_x,
            region_z: superblock.region_z,
            grid_size: superblock.grid_size,
            timestamps: vec![0u64; CHUNK_COUNT].into_boxed_slice(),
            chunks_data: vec![None; CHUNK_COUNT].into_boxed_slice(),
            buckets: vec![None; Self::bucket_count(superblock.grid_size)],
            staged: false,
            unsynced: false,
        }
    }

    pub(super) fn parse(raw_file: Bytes, path: &Path) -> Result<Self, ChunkReadingError> {
        let mut buf = raw_file;

        let superblock_bytes = take(&mut buf, LinearV2Superblock::SIZE)?;
        let superblock = LinearV2Superblock::from_bytes(&superblock_bytes)?;

        // The bitmap is not yet reliable per the spec, so it is skipped: actual presence is
        // determined by the per-chunk size field inside each bucket.
        take(&mut buf, ChunkBitmap::SIZE)?;
        let _features = NbtFeatures::from_bytes(&mut buf)?;

        let entries = read_bucket_entries(&mut buf, Self::bucket_count(superblock.grid_size))?;
        check_footer(&buf, &entries)?;

        let mut file = Self::blank(path, &superblock);
        for (bucket_idx, entry) in entries.iter().enumerate() {
            file.read_bucket(&mut buf, bucket_idx, entry)?;
        }
        Ok(file)
    }

    /// Reads one bucket's bytes off `buf`, keeps them, and fills in its chunks.
    fn read_bucket(
        &mut self,
        buf: &mut Bytes,
        bucket_idx: usize,
        entry: &BucketSizeEntry,
    ) -> Result<(), ChunkReadingError> {
        let compressed = buf.split_to(entry.size as usize);
        // A corrupt bucket keeps its bytes and stored hash, so a flush writes it back
        // unchanged until one of its chunks is saved.
        self.buckets[bucket_idx] = Some(CompressedBucket {
            data: Box::from(&compressed[..]),
            compression_level: entry.compression_level,
            xxhash: entry.xxhash,
        });

        let actual_hash = xxh64(&compressed, 0);
        if actual_hash != entry.xxhash {
            error!(
                "Linear v2: xxhash mismatch for bucket {bucket_idx} \
                 (expected {:#x}, got {:#x})",
                entry.xxhash, actual_hash
            );
            // Skip the corrupted bucket rather than aborting the whole file: the remaining
            // buckets may be intact.
            return Ok(());
        }

        let mut bucket_buf: Bytes = decompress(&compressed)?.into();
        self.read_chunks(bucket_idx, &mut bucket_buf)
    }

    /// Fills in the timestamps and data of the chunks of one decompressed bucket.
    fn read_chunks(
        &mut self,
        bucket_idx: usize,
        bucket_buf: &mut Bytes,
    ) -> Result<(), ChunkReadingError> {
        for local in 0..Self::chunks_per_bucket(self.grid_size) {
            let chunk_index = Self::global_chunk_index(bucket_idx, local, self.grid_size);
            let record = BucketChunkEntry::read_from(bucket_buf)?;
            self.timestamps[chunk_index] = record.timestamp;
            self.chunks_data[chunk_index] = record.data;
        }
        Ok(())
    }
}
