//! The driver against a fake generator, tick meter and store, on tokio's paused clock: a chunk
//! "takes" a set number of seconds and the test takes none.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::missing_panics_doc)]

use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    },
    time::Duration,
};

use kvasir_pregen::{ChunkPos, Job, Shape};
use kvasir_pregen_driver::{
    Control, Driver, Generator, Handle, JobStore, Options, Outcome, State, TickMeter,
};
use tokio::{task::JoinHandle, time::sleep};

const ORIGIN: ChunkPos = ChunkPos { x: 0, z: 0 };

/// Generates by waiting; records what was asked and how many ran at once.
#[derive(Default)]
struct FakeWorld {
    delay: Duration,
    generated: Mutex<Vec<ChunkPos>>,
    running: AtomicUsize,
    peak: AtomicUsize,
}

impl FakeWorld {
    fn with_delay(delay: Duration) -> Arc<Self> {
        Arc::new(Self {
            delay,
            ..Self::default()
        })
    }

    fn done(&self) -> Vec<ChunkPos> {
        self.generated.lock().unwrap().clone()
    }

    fn peak(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }
}

impl Generator for FakeWorld {
    async fn generate(&self, pos: ChunkPos) {
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        sleep(self.delay).await;
        self.running.fetch_sub(1, Ordering::SeqCst);
        self.generated.lock().unwrap().push(pos);
    }
}

/// A tick time the test sets.
#[derive(Clone)]
struct Meter(Arc<AtomicU32>);

impl Meter {
    fn at(ms: u32) -> Self {
        Self(Arc::new(AtomicU32::new(ms)))
    }
}

impl TickMeter for Meter {
    fn tick_ms(&self) -> f64 {
        f64::from(self.0.load(Ordering::SeqCst))
    }
}

#[derive(Clone, Default)]
struct Mem {
    saved: Arc<Mutex<Option<Job>>>,
    clears: Arc<AtomicUsize>,
    failing: Arc<AtomicBool>,
}

impl Mem {
    fn saved(&self) -> Option<Job> {
        self.saved.lock().unwrap().clone()
    }
}

impl JobStore for Mem {
    fn save(&self, job: &Job) -> std::io::Result<()> {
        if self.failing.load(Ordering::SeqCst) {
            return Err(std::io::Error::other("disk full"));
        }
        *self.saved.lock().unwrap() = Some(job.clone());
        Ok(())
    }

    fn clear(&self) -> std::io::Result<()> {
        *self.saved.lock().unwrap() = None;
        self.clears.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

const fn options(min: u32, max: u32) -> Options {
    Options {
        min_window: min,
        max_window: max,
        tick_budget_ms: 40.0,
        checkpoint_every: Duration::from_secs(1),
    }
}

fn job(radius: u32) -> Job {
    Job::new("minecraft:overworld", ORIGIN, Shape::Square { radius }).unwrap()
}

fn start(
    job: Job,
    world: &Arc<FakeWorld>,
    meter: &Meter,
    store: &Mem,
    options: Options,
) -> (Handle, JoinHandle<Outcome>) {
    let (driver, handle) = Driver::new(
        job,
        Arc::clone(world),
        meter.clone(),
        store.clone(),
        options,
    );
    (handle, tokio::spawn(driver.run()))
}

fn distinct(chunks: &[ChunkPos]) -> HashSet<ChunkPos> {
    chunks.iter().copied().collect()
}

fn every_chunk(job: &Job) -> HashSet<ChunkPos> {
    job.spiral().collect()
}

#[tokio::test(start_paused = true)]
async fn generates_every_chunk_once_and_finishes() {
    let world = FakeWorld::with_delay(Duration::from_millis(300));
    let (store, meter) = (Mem::default(), Meter::at(5));
    let job = job(6);
    let all = every_chunk(&job);
    let (handle, task) = start(job, &world, &meter, &store, options(2, 8));
    assert_eq!(task.await.unwrap(), Outcome::Finished);

    let done = world.done();
    assert_eq!(done.len(), all.len(), "no chunk twice");
    assert_eq!(distinct(&done), all);
    let report = handle.report();
    assert_eq!(report.state, State::Finished);
    assert_eq!(report.progress.done(), 169);
    assert_eq!(report.progress.percent(), 100.0);
    assert!(store.saved().is_none(), "a finished job leaves no file");
    assert_eq!(store.clears.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn an_idle_server_gets_a_growing_window_up_to_the_maximum_and_no_more() {
    let world = FakeWorld::with_delay(Duration::from_millis(500));
    let (store, meter) = (Mem::default(), Meter::at(5));
    let (_handle, task) = start(job(12), &world, &meter, &store, options(2, 8));
    assert_eq!(task.await.unwrap(), Outcome::Finished);
    assert_eq!(world.peak(), 8);
}

#[tokio::test(start_paused = true)]
async fn a_busy_server_keeps_the_window_at_its_minimum() {
    let world = FakeWorld::with_delay(Duration::from_millis(500));
    let (store, meter) = (Mem::default(), Meter::at(90));
    let (handle, task) = start(job(5), &world, &meter, &store, options(2, 8));
    assert_eq!(task.await.unwrap(), Outcome::Finished);
    assert_eq!(world.peak(), 2);
    assert_eq!(handle.report().window, 2);
}

#[tokio::test(start_paused = true)]
async fn the_window_follows_the_server_up_and_back_down() {
    let world = FakeWorld::with_delay(Duration::from_millis(500));
    let (store, meter) = (Mem::default(), Meter::at(5));
    let (handle, task) = start(job(30), &world, &meter, &store, options(1, 16));
    sleep(Duration::from_secs(25)).await;
    let grown = handle.report().window;
    assert!(grown >= 10, "window {grown} after 25 idle seconds");
    meter.0.store(120, Ordering::SeqCst);
    sleep(Duration::from_secs(5)).await;
    let shrunk = handle.report().window;
    assert!(shrunk <= 2, "window {shrunk} after 5 busy seconds");
    assert!(handle.send(Control::Cancel));
    assert_eq!(task.await.unwrap(), Outcome::Cancelled);
}

#[tokio::test(start_paused = true)]
async fn pause_hands_out_nothing_more_and_resume_carries_on() {
    let world = FakeWorld::with_delay(Duration::from_millis(400));
    let (store, meter) = (Mem::default(), Meter::at(5));
    let job = job(8);
    let all = every_chunk(&job);
    let (handle, task) = start(job, &world, &meter, &store, options(2, 4));
    sleep(Duration::from_secs(3)).await;
    assert!(handle.send(Control::Pause));
    sleep(Duration::from_secs(2)).await; // in-flight chunks settle
    let frozen = world.done().len();
    assert!(
        frozen > 0 && frozen < all.len(),
        "paused part way: {frozen}"
    );
    assert_eq!(handle.report().state, State::Paused);
    assert!(store.saved().unwrap().paused, "the file says paused");

    sleep(Duration::from_secs(60)).await;
    assert_eq!(world.done().len(), frozen, "nothing generated while paused");

    assert!(handle.send(Control::Resume));
    assert_eq!(task.await.unwrap(), Outcome::Finished);
    assert_eq!(distinct(&world.done()), all);
    assert_eq!(world.done().len(), all.len());
}

#[tokio::test(start_paused = true)]
async fn a_job_loaded_paused_waits_for_resume() {
    let world = FakeWorld::with_delay(Duration::from_millis(100));
    let (store, meter) = (Mem::default(), Meter::at(5));
    let mut paused = job(3);
    paused.paused = true;
    let (handle, task) = start(paused, &world, &meter, &store, options(2, 4));
    sleep(Duration::from_secs(30)).await;
    assert!(world.done().is_empty());
    assert_eq!(handle.report().state, State::Paused);
    assert!(handle.send(Control::Resume));
    assert_eq!(task.await.unwrap(), Outcome::Finished);
    assert_eq!(world.done().len(), 49);
}

#[tokio::test(start_paused = true)]
async fn cancel_finishes_what_is_in_flight_and_removes_the_file() {
    let world = FakeWorld::with_delay(Duration::from_millis(400));
    let (store, meter) = (Mem::default(), Meter::at(5));
    let (handle, task) = start(job(20), &world, &meter, &store, options(2, 4));
    sleep(Duration::from_secs(3)).await;
    assert!(handle.send(Control::Cancel));
    assert_eq!(task.await.unwrap(), Outcome::Cancelled);
    assert_eq!(
        world.running.load(Ordering::SeqCst),
        0,
        "nothing left half done"
    );
    let done = world.done().len();
    assert!(
        done > 0 && done < 1681,
        "cancelled part way, not run to the end: {done}"
    );
    assert!(store.saved().is_none());
    assert_eq!(handle.report().state, State::Cancelled);
    assert!(!handle.send(Control::Pause), "the job has ended");
}

#[tokio::test(start_paused = true)]
async fn a_stop_after_a_cancel_does_not_undo_the_cancel() {
    let world = FakeWorld::with_delay(Duration::from_millis(400));
    let (store, meter) = (Mem::default(), Meter::at(5));
    let (handle, task) = start(job(20), &world, &meter, &store, options(2, 4));
    sleep(Duration::from_secs(2)).await;
    assert!(handle.send(Control::Cancel));
    assert!(handle.send(Control::Stop));
    assert_eq!(task.await.unwrap(), Outcome::Cancelled);
    assert!(store.saved().is_none(), "the cancelled job's file is gone");
}

#[tokio::test(start_paused = true)]
async fn stop_keeps_the_file_and_the_next_start_loses_nothing() {
    let world = FakeWorld::with_delay(Duration::from_millis(400));
    let (store, meter) = (Mem::default(), Meter::at(5));
    let job0 = job(10);
    let all = every_chunk(&job0);
    let (handle, task) = start(job0, &world, &meter, &store, options(2, 6));
    sleep(Duration::from_secs(5)).await;
    assert!(handle.send(Control::Stop));
    assert_eq!(task.await.unwrap(), Outcome::Stopped);
    let saved = store.saved().expect("a stopped job stays on disk");
    assert_eq!(handle.report().state, State::Stopped);
    // after a stop everything handed out has finished, so the file is exact
    assert_eq!(saved.done as usize, world.done().len());
    assert!(
        world.done().len() < all.len(),
        "stopped part way, not run to the end"
    );
    assert!(!saved.is_finished());

    let second = FakeWorld::with_delay(Duration::from_millis(400));
    let (_h, task) = start(saved, &second, &meter, &store, options(2, 6));
    assert_eq!(task.await.unwrap(), Outcome::Finished);
    let mut both = world.done();
    both.extend(second.done());
    assert_eq!(distinct(&both), all);
    assert_eq!(both.len(), all.len(), "a clean stop repeats nothing");
}

#[tokio::test(start_paused = true)]
async fn a_crash_loses_nothing_and_repeats_at_most_a_window() {
    let world = FakeWorld::with_delay(Duration::from_millis(700));
    let (store, meter) = (Mem::default(), Meter::at(5));
    let job0 = job(10);
    let all = every_chunk(&job0);
    let (_handle, task) = start(job0, &world, &meter, &store, options(2, 6));
    sleep(Duration::from_millis(5300)).await; // between two checkpoints
    task.abort(); // the process dies: no stop, no final save
    let _ = task.await;
    let saved = store.saved().expect("checkpoints were written");

    let second = FakeWorld::with_delay(Duration::from_millis(700));
    let (_h, task) = start(saved, &second, &meter, &store, options(2, 6));
    assert_eq!(task.await.unwrap(), Outcome::Finished);

    let mut both = world.done();
    both.extend(second.done());
    assert_eq!(distinct(&both), all, "no chunk missing");
    let repeated = both.len() - all.len();
    assert!(
        repeated <= 40,
        "{repeated} chunks done twice: the last second's work, not the world"
    );
}

#[tokio::test(start_paused = true)]
async fn a_failing_store_does_not_stop_the_job() {
    let world = FakeWorld::with_delay(Duration::from_millis(300));
    let (store, meter) = (Mem::default(), Meter::at(5));
    store.failing.store(true, Ordering::SeqCst);
    let (_handle, task) = start(job(5), &world, &meter, &store, options(2, 4));
    assert_eq!(task.await.unwrap(), Outcome::Finished);
    assert_eq!(world.done().len(), 121);
}

#[tokio::test(start_paused = true)]
async fn dropping_the_handle_leaves_the_job_running() {
    let world = FakeWorld::with_delay(Duration::from_millis(300));
    let (store, meter) = (Mem::default(), Meter::at(5));
    let (handle, task) = start(job(5), &world, &meter, &store, options(2, 4));
    drop(handle);
    assert_eq!(task.await.unwrap(), Outcome::Finished);
    assert_eq!(world.done().len(), 121);
}

#[tokio::test(start_paused = true)]
async fn the_report_has_a_speed_and_an_estimate_while_running() {
    let world = FakeWorld::with_delay(Duration::from_millis(500));
    let (store, meter) = (Mem::default(), Meter::at(5));
    let (handle, task) = start(job(40), &world, &meter, &store, options(4, 4));
    sleep(Duration::from_secs(10)).await;
    let report = handle.report();
    // 4 in flight, 0.5 s each: 8 chunks a second
    let rate = report.progress.rate().expect("a rate after ten seconds");
    assert!((rate - 8.0).abs() < 1.0, "rate {rate}");
    assert!(report.progress.eta_secs().is_some());
    assert_eq!(report.state, State::Running);
    assert!(handle.send(Control::Cancel));
    let _ = task.await;
}

#[tokio::test(start_paused = true)]
async fn an_already_finished_job_ends_at_once() {
    let world = FakeWorld::with_delay(Duration::from_millis(300));
    let (store, meter) = (Mem::default(), Meter::at(5));
    let mut finished = job(2);
    finished.done = finished.total();
    finished.cursor = kvasir_pregen::Cursor { ring: 3, index: 0 };
    let (_handle, task) = start(finished, &world, &meter, &store, options(2, 4));
    assert_eq!(task.await.unwrap(), Outcome::Finished);
    assert!(world.done().is_empty());
}
