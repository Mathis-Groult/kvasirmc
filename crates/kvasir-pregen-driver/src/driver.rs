//! The loop that runs a job.

use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use futures::{StreamExt, stream::FuturesUnordered};
use kvasir_pregen::{ChunkPos, InFlight, Job, Progress, Spiral, Throttle};
use tokio::{
    select,
    sync::{mpsc, watch},
    time::{Instant, MissedTickBehavior, interval},
};

use crate::{
    Generator, JobStore, TickMeter,
    report::{Report, State},
};

/// A command from the job's owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// Stop handing out chunks; those in flight finish. The job is kept, paused.
    Pause,
    /// Carry on after a pause.
    Resume,
    /// Finish what is in flight, keep the saved job, and return. For a server shutdown.
    Stop,
    /// Finish what is in flight, delete the saved job, and return.
    Cancel,
}

/// Tuning of a run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    /// Chunks in flight at the start, and the least the throttle will go to.
    pub min_window: u32,
    /// The most chunks ever in flight.
    pub max_window: u32,
    /// A server tick longer than this is a reason to slow down (milliseconds).
    pub tick_budget_ms: f64,
    /// How often the resume point is saved, the report published and the window adjusted.
    pub checkpoint_every: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            min_window: 2,
            max_window: 64,
            // A Minecraft tick is 50 ms; leave room for the players' own work.
            tick_budget_ms: 40.0,
            checkpoint_every: Duration::from_secs(1),
        }
    }
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Every chunk is generated; the saved job was removed.
    Finished,
    /// Cancelled; the saved job was removed.
    Cancelled,
    /// Stopped with the job saved, to continue at the next start.
    Stopped,
}

/// The owner's side: send commands, read the latest report.
#[derive(Debug, Clone)]
pub struct Handle {
    control: mpsc::UnboundedSender<Control>,
    report: watch::Receiver<Report>,
}

impl Handle {
    /// Sends a command. False when the job has already ended.
    #[must_use]
    pub fn send(&self, command: Control) -> bool {
        self.control.send(command).is_ok()
    }

    /// The latest report.
    #[must_use]
    pub fn report(&self) -> Report {
        self.report.borrow().clone()
    }
}

type Pending = Pin<Box<dyn Future<Output = ChunkPos> + Send>>;

/// One job and the means to run it.
pub struct Driver<G, M, S> {
    job: Job,
    generator: Arc<G>,
    meter: M,
    store: S,
    options: Options,
    control: mpsc::UnboundedReceiver<Control>,
    report: watch::Sender<Report>,
}

impl<G: Generator, M: TickMeter, S: JobStore> Driver<G, M, S> {
    /// Prepares a run of `job` (new, or loaded from the store). Nothing happens until
    /// [`run`](Self::run) is awaited.
    #[must_use]
    pub fn new(
        job: Job,
        generator: Arc<G>,
        meter: M,
        store: S,
        options: Options,
    ) -> (Self, Handle) {
        let (control_tx, control) = mpsc::unbounded_channel();
        let mut progress = Progress::new(job.total());
        progress.record(0, job.done);
        let first = Report {
            state: if job.paused {
                State::Paused
            } else {
                State::Running
            },
            progress,
            window: options.min_window.max(1),
        };
        let (report, report_rx) = watch::channel(first);
        let handle = Handle {
            control: control_tx,
            report: report_rx,
        };
        (
            Self {
                job,
                generator,
                meter,
                store,
                options,
                control,
                report,
            },
            handle,
        )
    }

    /// Runs the job to its end. Chunk futures are never dropped half way: on [`Control::Stop`] and
    /// [`Control::Cancel`] those in flight are awaited first, at most one window of them.
    pub async fn run(self) -> Outcome {
        let Self {
            job,
            generator,
            meter,
            store,
            options,
            mut control,
            report,
        } = self;
        let started = Instant::now();
        let mut core = Core::new(job, &options, report);
        let mut running: FuturesUnordered<Pending> = FuturesUnordered::new();
        let mut ticker = interval(options.checkpoint_every);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut control_open = true;

        loop {
            if core.may_hand_out() {
                core.hand_out(&running, &generator);
            }
            if running.is_empty() && (core.stop.is_some() || core.exhausted) {
                break;
            }
            select! {
                Some(pos) = running.next(), if !running.is_empty() => {
                    core.flight.finish(pos);
                }
                command = control.recv(), if control_open => {
                    if let Some(command) = command {
                        core.apply(command);
                        core.checkpoint(&store, started, &meter);
                    } else {
                        // The owner dropped its handle: the job carries on unattended.
                        control_open = false;
                    }
                }
                _ = ticker.tick() => core.checkpoint(&store, started, &meter),
            }
        }
        core.end(&store, started)
    }
}

/// The job's state, apart from the things `select!` borrows.
struct Core {
    job: Job,
    spiral: Spiral,
    flight: InFlight,
    progress: Progress,
    throttle: Throttle,
    tick_budget_ms: f64,
    paused: bool,
    stop: Option<Control>,
    exhausted: bool,
    save_failing: bool,
    report: watch::Sender<Report>,
}

impl Core {
    fn new(job: Job, options: &Options, report: watch::Sender<Report>) -> Self {
        let mut progress = Progress::new(job.total());
        progress.record(0, job.done);
        Self {
            spiral: job.spiral(),
            flight: InFlight::new(job.cursor, job.done),
            progress,
            throttle: Throttle::new(options.min_window, options.max_window),
            tick_budget_ms: options.tick_budget_ms,
            paused: job.paused,
            stop: None,
            exhausted: false,
            save_failing: false,
            report,
            job,
        }
    }

    const fn may_hand_out(&self) -> bool {
        !self.paused && self.stop.is_none() && !self.exhausted
    }

    /// Fills the window with the next chunks of the spiral.
    fn hand_out<G: Generator>(&mut self, running: &FuturesUnordered<Pending>, generator: &Arc<G>) {
        while self.flight.unfinished() < self.throttle.window() as usize {
            let before = self.spiral.cursor();
            let Some(pos) = self.spiral.next() else {
                self.exhausted = true;
                return;
            };
            self.flight.hand_out(pos, before, self.spiral.cursor());
            let generator = Arc::clone(generator);
            running.push(Box::pin(async move {
                generator.generate(pos).await;
                pos
            }));
        }
    }

    fn apply(&mut self, command: Control) {
        match command {
            Control::Pause => self.paused = true,
            Control::Resume => self.paused = false,
            Control::Stop => {
                // A cancel already asked for stays a cancel.
                self.stop.get_or_insert(Control::Stop);
            }
            Control::Cancel => self.stop = Some(Control::Cancel),
        }
    }

    /// Saves the resume point, adjusts the window to the server's tick times, publishes a report.
    fn checkpoint<S: JobStore, M: TickMeter>(&mut self, store: &S, started: Instant, meter: &M) {
        let now_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.progress.record(now_ms, self.flight.finished_total());
        self.throttle.observe(meter.tick_ms(), self.tick_budget_ms);
        self.save(store);
        self.publish(if self.paused {
            State::Paused
        } else {
            State::Running
        });
    }

    fn save<S: JobStore>(&mut self, store: &S) {
        self.job.cursor = self.flight.resume_cursor();
        self.job.done = self.flight.finished_total();
        self.job.paused = self.paused;
        match store.save(&self.job) {
            Ok(()) => {
                if self.save_failing {
                    tracing::info!("pre-generation: the job file is being saved again");
                    self.save_failing = false;
                }
            }
            Err(error) => {
                if !self.save_failing {
                    tracing::warn!(%error, "pre-generation: cannot save the job file; a restart would start over");
                    self.save_failing = true;
                }
            }
        }
    }

    fn publish(&self, state: State) {
        self.report.send_replace(Report {
            state,
            progress: self.progress.clone(),
            window: self.throttle.window(),
        });
    }

    /// The loop is over: everything in flight is finished.
    fn end<S: JobStore>(&mut self, store: &S, started: Instant) -> Outcome {
        let now_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.progress.record(now_ms, self.flight.finished_total());
        let (outcome, state) = match self.stop {
            Some(Control::Cancel) => (Outcome::Cancelled, State::Cancelled),
            Some(_) => (Outcome::Stopped, State::Stopped),
            None => (Outcome::Finished, State::Finished),
        };
        if outcome == Outcome::Stopped {
            self.save(store);
        } else if let Err(error) = store.clear() {
            tracing::warn!(%error, "pre-generation: cannot remove the job file");
        }
        self.publish(state);
        outcome
    }
}
