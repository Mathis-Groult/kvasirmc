//! `KvasirMC`: world pre-generation inside the engine.
//!
//! The pure logic is `kvasir-pregen` (shapes, spiral order, resume point, progress, back-off) and
//! the loop is `kvasir-pregen-driver`. This file only adapts them to the engine: it generates
//! chunks through the world's `Level`, reads tick times from the `Server`, keeps the job file next
//! to the dimension, and holds one running job per dimension.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError, Weak, atomic::Ordering},
    time::Duration,
};

use kvasir_pregen::{ChunkPos, Job, ShapeError, tick_percentile_ms};
use kvasir_pregen_driver::{
    Control, Driver, FileStore, Generator, Handle, Options, Outcome, Report, TickMeter,
};
use pumpkin_util::math::vector2::Vector2;
use pumpkin_world::level::Level;
use tokio::time::{MissedTickBehavior, interval};
use tracing::{info, warn};

use crate::{server::Server, world::World};

/// How often a running job writes its progress to the console log.
const LOG_EVERY: Duration = Duration::from_secs(10);

/// The percentile of recent tick times the throttle watches.
const TICK_PERCENTILE: usize = 95;

/// Where a dimension's job is kept: inside the dimension's own folder, so that copying or deleting
/// the world takes the job with it.
fn job_file(level: &Level) -> FileStore {
    FileStore::new(
        level
            .level_folder
            .dim_folder
            .join("kvasir")
            .join("pregen.json"),
    )
}

/// Generates a chunk by asking the level for it, which takes a full-chunk ticket, waits for the
/// chunk system to generate or load it, and lets go of the ticket.
struct LevelGenerator(Arc<Level>);

impl Generator for LevelGenerator {
    async fn generate(&self, pos: ChunkPos) {
        self.0
            .get_or_fetch_chunk(Vector2::new(pos.x, pos.z), |_| ())
            .await;
    }
}

/// Reads the server's recent tick times. Holds the server weakly: the job must not keep it alive.
struct ServerTicks(Weak<Server>);

impl TickMeter for ServerTicks {
    fn tick_ms(&self) -> f64 {
        let Some(server) = self.0.upgrade() else {
            return 0.0;
        };
        let filled = usize::try_from(server.tick_count.load(Ordering::Relaxed))
            .unwrap_or(0)
            .min(100);
        tick_percentile_ms(&server.get_tick_times_nanos_copy(), filled, TICK_PERCENTILE)
    }
}

/// Why a job could not be started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartError {
    /// The dimension already has a job: cancel it first.
    AlreadyRunning,
    Shape(ShapeError),
}

/// The running pre-generation jobs of a server, by dimension.
#[derive(Default)]
pub struct PregenService {
    jobs: Arc<Mutex<HashMap<String, Handle>>>,
}

impl PregenService {
    fn jobs(&self) -> std::sync::MutexGuard<'_, HashMap<String, Handle>> {
        self.jobs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts `job` in `world`. The job file is written within a second.
    ///
    /// # Errors
    /// The dimension already has a job.
    pub fn start(
        &self,
        server: &Arc<Server>,
        world: &Arc<World>,
        job: Job,
    ) -> Result<(), StartError> {
        let dimension = world.dimension.minecraft_name.to_string();
        let mut jobs = self.jobs();
        if jobs.contains_key(&dimension) {
            return Err(StartError::AlreadyRunning);
        }
        let (driver, handle) = Driver::new(
            job,
            Arc::new(LevelGenerator(Arc::clone(&world.level))),
            ServerTicks(Arc::downgrade(server)),
            job_file(&world.level),
            Options::default(),
        );
        jobs.insert(dimension.clone(), handle.clone());
        drop(jobs);

        let registry = Arc::clone(&self.jobs);
        server.spawn_task(async move {
            run_logged(driver, handle, &dimension).await;
            registry
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&dimension);
        });
        Ok(())
    }

    /// Sends a command to the dimension's job. False when it has none.
    #[must_use]
    pub fn control(&self, dimension: &str, command: Control) -> bool {
        self.jobs()
            .get(dimension)
            .is_some_and(|handle| handle.send(command))
    }

    /// The latest report of the dimension's job.
    #[must_use]
    pub fn report(&self, dimension: &str) -> Option<Report> {
        self.jobs().get(dimension).map(Handle::report)
    }

    /// Takes up the jobs a previous run left in the worlds' folders. A paused job stays paused.
    pub fn resume_saved(&self, server: &Arc<Server>) {
        for world in server.worlds.load().iter() {
            let dimension = world.dimension.minecraft_name;
            let store = job_file(&world.level);
            match store.load() {
                Ok(None) => {}
                Ok(Some(job)) if job.world != dimension => warn!(
                    "Ignoring {}: it belongs to {} but sits in {dimension}",
                    store.path().display(),
                    job.world
                ),
                Ok(Some(job)) => {
                    info!(
                        "Resuming pre-generation of {dimension}: {} chunks of {} done{}",
                        job.done,
                        job.total(),
                        if job.paused { " (paused)" } else { "" }
                    );
                    if let Err(error) = self.start(server, world, job) {
                        warn!("Cannot resume pre-generation of {dimension}: {error:?}");
                    }
                }
                Err(error) => warn!("Not resuming pre-generation: {error}"),
            }
        }
    }

    /// Asks every job to finish what it has in flight, save, and end. For a server shutdown; the
    /// jobs are awaited with the server's other tasks.
    pub fn stop_all(&self) {
        for handle in self.jobs().values() {
            let _ = handle.send(Control::Stop);
        }
    }
}

/// Runs the driver, writing its progress to the log while it goes.
async fn run_logged<G, M, S>(driver: Driver<G, M, S>, handle: Handle, dimension: &str)
where
    G: Generator,
    M: TickMeter,
    S: kvasir_pregen_driver::JobStore,
{
    let run = driver.run();
    tokio::pin!(run);
    let mut ticker = interval(LOG_EVERY);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker.tick().await; // the first tick is immediate
    let outcome = loop {
        tokio::select! {
            outcome = &mut run => break outcome,
            _ = ticker.tick() => info!("Pre-generation of {dimension}: {}", handle.report()),
        }
    };
    let report = handle.report();
    match outcome {
        Outcome::Finished => info!(
            "Pre-generation of {dimension} finished: {}",
            report.progress
        ),
        Outcome::Cancelled => info!(
            "Pre-generation of {dimension} cancelled: {}",
            report.progress
        ),
        Outcome::Stopped => info!("Pre-generation of {dimension} saved: {}", report.progress),
    }
}
