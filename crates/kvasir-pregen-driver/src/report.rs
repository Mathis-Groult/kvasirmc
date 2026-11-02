//! What a running job tells its owner.

use std::fmt;

use kvasir_pregen::Progress;

/// Where the job is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Running,
    Paused,
    Finished,
    Cancelled,
    /// Stopped for a server shutdown; the saved job continues next start.
    Stopped,
}

impl State {
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Finished => "finished",
            Self::Cancelled => "cancelled",
            Self::Stopped => "stopped",
        }
    }
}

/// A snapshot of the job, published about once a second.
#[derive(Debug, Clone)]
pub struct Report {
    pub state: State,
    pub progress: Progress,
    /// Chunks the throttle currently allows in flight.
    pub window: u32,
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {}, window {}",
            self.state.word(),
            self.progress,
            self.window
        )
    }
}
