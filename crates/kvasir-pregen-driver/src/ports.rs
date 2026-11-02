//! What the engine provides to the driver.

use std::future::Future;

use kvasir_pregen::{ChunkPos, Job};

/// Makes a chunk exist on disk. The future resolves when the chunk is generated and handed to the
/// saver; it is polled on the driver's task, so it must wait for the generator, not do the work.
pub trait Generator: Send + Sync + 'static {
    fn generate(&self, pos: ChunkPos) -> impl Future<Output = ()> + Send;
}

/// How long server ticks are taking right now.
pub trait TickMeter: Send + 'static {
    /// A high percentile of recent tick durations, in milliseconds.
    fn tick_ms(&self) -> f64;
}

/// Where the job is kept between runs. Writes are small and happen about once a second.
pub trait JobStore: Send + 'static {
    /// Saves the job so a restart continues from it.
    ///
    /// # Errors
    /// The job could not be written; the driver keeps going and says so once.
    fn save(&self, job: &Job) -> std::io::Result<()>;

    /// Forgets the job: it finished or was cancelled.
    ///
    /// # Errors
    /// The saved job could not be removed.
    fn clear(&self) -> std::io::Result<()>;
}
