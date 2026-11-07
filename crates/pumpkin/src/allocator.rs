//! `KvasirMC`: the process-wide memory allocator.
//!
//! The engine allocates small short-lived blocks all through the tick (entity queries, goal
//! selection, packets); glibc `malloc` and `free` were about 6 % of the sampled cycles with 3,000
//! mobs. With mimalloc linked in, cycles over a 40 s window fell to 0.93, 0.93, 0.79 and 0.78 of
//! the glibc build in four interleaved pairs (10 walking bots, same source).
//!
//! The engine must be started with `MIMALLOC_ARENA_EAGER_COMMIT=0` (the `KvasirMC` launcher sets
//! it). Without it, peak resident memory rose from 420 MiB to 610-700 MiB; with it, to 440-470 MiB.
//! mimalloc reads its options before `main`, so the process cannot set it for itself.

use mimalloc::MiMalloc;

#[global_allocator]
static ALLOCATOR: MiMalloc = MiMalloc;
