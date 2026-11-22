//! The steps of `AnvilRegion::flush_with_sync`: plan the new locations on copies, write the
//! records, write the header, then commit the plan to the region and clean up.

use std::fs::File;
use std::path::PathBuf;

use bytes::BufMut;
use tracing::warn;

use super::{
    AnvilChunkData, AnvilRegion, Compression, EXTERNAL_CHUNK_THRESHOLD, EXTERNAL_STREAM_FLAG,
    SECTOR_BYTES, SectorBitmap, epoch_seconds, sector_count, sector_offset,
};
use crate::chunk::io::{write_all_at, write_file_atomically};

/// What a flush will do, worked out on copies so a failed write leaves the region as it was.
pub(super) struct FlushPlan {
    pub(super) used_sectors: SectorBitmap,
    pub(super) locations: Box<[u32]>,
    pub(super) timestamps: Box<[u32]>,
    /// Locations whose sectors are free once the header points elsewhere.
    pub(super) freed: Vec<u32>,
    /// `.mcc` files nothing points to any more.
    pub(super) stale_external: Vec<PathBuf>,
    /// Records to write, as (first sector, bytes).
    pub(super) writes: Vec<(u32, Vec<u8>)>,
}

impl AnvilRegion {
    /// A flush with nothing staged only syncs what an earlier flush left unsynced.
    pub(super) fn sync_unsynced(
        &mut self,
        sync: bool,
        sync_file: impl Fn(&File) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        if sync
            && self.unsynced
            && let Some(file) = &self.file
        {
            sync_file(file)?;
            self.unsynced = false;
        }
        Ok(())
    }

    /// Plans every staged chunk into free sectors, writing the `.mcc` files of oversized ones.
    pub(super) fn plan_flush(&self, sync: bool) -> std::io::Result<FlushPlan> {
        let mut plan = FlushPlan {
            used_sectors: self.used_sectors.clone(),
            locations: self.locations.clone(),
            timestamps: self.timestamps.clone(),
            freed: Vec::new(),
            stale_external: self.pending_external_deletes.clone(),
            writes: Vec::with_capacity(self.staged.len()),
        };
        let mut free_runs = self.used_sectors.free_runs();
        let now = epoch_seconds();

        let mut staged: Vec<_> = self.staged.iter().collect();
        staged.sort_unstable_by_key(|(index, _)| **index);
        for (&index, chunk) in staged {
            let (record, count) = self.record_for(
                index,
                chunk,
                plan.locations[index],
                &mut plan.stale_external,
                sync,
            )?;

            let offset = free_runs.allocate(count);
            plan.used_sectors.set(offset, count, true);
            if plan.locations[index] != 0 {
                plan.freed.push(plan.locations[index]);
            }
            plan.locations[index] = (offset << 8) | count;
            plan.timestamps[index] = now;
            plan.writes.push((offset, record));
        }
        Ok(plan)
    }

    /// The bytes to put in the region for one staged chunk and the sectors they take: the chunk
    /// itself, or for an oversized one a one-sector stub pointing to its `.mcc` file.
    fn record_for(
        &self,
        index: usize,
        chunk: &AnvilChunkData,
        old_location: u32,
        stale_external: &mut Vec<PathBuf>,
        sync: bool,
    ) -> std::io::Result<(Vec<u8>, u32)> {
        if chunk.sector_count() >= EXTERNAL_CHUNK_THRESHOLD {
            let path = self.external_path(index);
            write_file_atomically(&path, &chunk.compressed_data, sync)?;
            stale_external.retain(|pending| pending != &path);
            let mut stub = Vec::with_capacity(SECTOR_BYTES);
            stub.put_u32(1);
            stub.put_u8(
                chunk
                    .compression
                    .map_or(Compression::NO_COMPRESSION_ID, |c| c as u8)
                    | EXTERNAL_STREAM_FLAG,
            );
            stub.resize(SECTOR_BYTES, 0);
            return Ok((stub, 1));
        }
        if self.is_external_stub(old_location)? {
            stale_external.push(self.external_path(index));
        }
        Ok((chunk.to_record(), chunk.sector_count()))
    }

    /// Writes the records in file order, neighbouring ones in one write. Returns where the last
    /// record ends.
    pub(super) fn write_records(
        &mut self,
        mut writes: Vec<(u32, Vec<u8>)>,
    ) -> std::io::Result<u64> {
        writes.sort_unstable_by_key(|(offset, _)| *offset);
        let file = self.file()?;
        let mut file_end = 0;
        let mut run_start = 0;
        let mut run: Vec<u8> = Vec::new();
        for (offset, record) in writes {
            let position = u64::from(offset) * SECTOR_BYTES as u64;
            // Neighbouring records go out in one write.
            if !run.is_empty() && run_start + run.len() as u64 != position {
                write_all_at(file, &run, run_start)?;
                run.clear();
            }
            if run.is_empty() {
                run_start = position;
            }
            run.extend_from_slice(&record);
            file_end = position + record.len() as u64;
        }
        if !run.is_empty() {
            write_all_at(file, &run, run_start)?;
        }
        Ok(file_end)
    }

    /// Points the header at the new locations; a failed write gives the old ones back.
    pub(super) fn write_header(
        &mut self,
        locations: Box<[u32]>,
        timestamps: Box<[u32]>,
    ) -> std::io::Result<()> {
        let old_locations = std::mem::replace(&mut self.locations, locations);
        let old_timestamps = std::mem::replace(&mut self.timestamps, timestamps);
        let header = self.header_bytes();
        let file = self.file()?;
        if let Err(err) = write_all_at(file, &header, 0) {
            self.locations = old_locations;
            self.timestamps = old_timestamps;
            return Err(err);
        }
        Ok(())
    }

    /// Frees the old sectors, syncs, and removes what nothing points to any more.
    pub(super) fn commit_flush(
        &mut self,
        mut used_sectors: SectorBitmap,
        freed: Vec<u32>,
        stale_external: Vec<PathBuf>,
        file_end: u64,
        sync: bool,
        sync_file: impl Fn(&File) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        for location in freed {
            used_sectors.set(sector_offset(location), sector_count(location), false);
        }
        self.file_len = self.file_len.max(file_end).max(super::HEADER_BYTES as u64);
        self.used_sectors = used_sectors;
        self.pending_external_deletes = stale_external;
        self.unsynced = true;
        if sync && let Some(file) = &self.file {
            sync_file(file)?;
            self.unsynced = false;
        }
        for path in self.pending_external_deletes.drain(..) {
            if let Err(err) = std::fs::remove_file(&path)
                && err.kind() != std::io::ErrorKind::NotFound
            {
                warn!("Failed to remove {}: {err}", path.display());
            }
        }
        self.truncate_unused_tail();
        self.staged.clear();
        Ok(())
    }
}
