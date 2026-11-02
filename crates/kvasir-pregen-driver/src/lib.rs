//! Runs a pre-generation job against any chunk generator.
//!
//! The pure logic (order, resume point, progress, back-off) lives in `kvasir-pregen`. This crate is
//! the loop around it: it keeps a window of chunks in flight, grows or shrinks the window from the
//! server's tick times, saves the resume point once a second, and obeys pause, resume, stop and
//! cancel. The engine supplies three small things through traits: a [`Generator`], a [`TickMeter`]
//! and a [`JobStore`]. Tests supply fakes.
//!
//! The loop never sleeps between chunks: it waits for a chunk to finish (and hands out the next
//! one at once), for a command, or for the once-a-second checkpoint.

// Tests state expectations with `unwrap`; the engine's lints forbid it everywhere else.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod driver;
mod ports;
mod report;

pub use driver::{Control, Driver, Handle, Options, Outcome};
pub use ports::{Generator, JobStore, TickMeter};
pub use report::{Report, State};
