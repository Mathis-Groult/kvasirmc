//! World pre-generation, as pure logic: what to generate, in which order, how to resume exactly,
//! how far it got, and how many chunks to have in flight. The engine drives it; nothing here
//! touches a server, a file or a clock.

// Tests state expectations with `unwrap`; the engine's lints forbid it everywhere else.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod job;
mod progress;
mod shape;
mod spiral;
mod throttle;
mod window;

pub use job::{Job, JobError, SCHEMA};
pub use progress::Progress;
pub use shape::{ChunkPos, MAX_RADIUS, Shape, ShapeError};
pub use spiral::{Cursor, Spiral, ring_cell, ring_len};
pub use throttle::{Throttle, tick_percentile_ms};
pub use window::InFlight;
