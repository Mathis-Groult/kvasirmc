//! How far a job has got and how long it will take.

use std::{collections::VecDeque, fmt};

/// Samples older than this are forgotten: the rate follows the machine's current speed, not the
/// first minute's.
const WINDOW_MS: u64 = 30_000;

/// Progress of a job: counts, a speed measured over the last half minute, an estimate.
#[derive(Debug, Clone)]
pub struct Progress {
    total: u64,
    samples: VecDeque<(u64, u64)>,
}

impl Progress {
    #[must_use]
    pub const fn new(total: u64) -> Self {
        Self {
            total,
            samples: VecDeque::new(),
        }
    }

    /// Notes that `done` chunks were finished at `now_ms` (any monotonic clock, in milliseconds).
    pub fn record(&mut self, now_ms: u64, done: u64) {
        self.samples.push_back((now_ms, done));
        while self
            .samples
            .front()
            .is_some_and(|&(at, _)| now_ms.saturating_sub(at) > WINDOW_MS)
        {
            self.samples.pop_front();
        }
    }

    /// Chunks finished at the last sample.
    #[must_use]
    pub fn done(&self) -> u64 {
        self.samples.back().map_or(0, |&(_, done)| done)
    }

    #[must_use]
    pub const fn total(&self) -> u64 {
        self.total
    }

    /// Percent finished, 0 to 100 (an empty job counts as finished).
    #[must_use]
    pub fn percent(&self) -> f64 {
        if self.total == 0 {
            return 100.0;
        }
        100.0 * self.done().min(self.total) as f64 / self.total as f64
    }

    /// Chunks per second over the sample window, once there are two samples apart in time.
    #[must_use]
    pub fn rate(&self) -> Option<f64> {
        let (first_at, first_done) = *self.samples.front()?;
        let (last_at, last_done) = *self.samples.back()?;
        let elapsed_ms = last_at.checked_sub(first_at).filter(|&ms| ms > 0)?;
        Some(last_done.saturating_sub(first_done) as f64 * 1000.0 / elapsed_ms as f64)
    }

    /// Seconds left at the current rate, or `None` before a rate exists or when nothing moves.
    #[must_use]
    pub fn eta_secs(&self) -> Option<u64> {
        let rate = self.rate().filter(|&rate| rate > 0.0)?;
        let left = self.total.saturating_sub(self.done());
        Some((left as f64 / rate).ceil() as u64)
    }
}

impl fmt::Display for Progress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{} chunks ({:.1}%)",
            self.done().min(self.total),
            self.total,
            self.percent()
        )?;
        if let Some(rate) = self.rate() {
            write!(f, ", {rate:.1} chunks/s")?;
        }
        if let Some(eta) = self.eta_secs() {
            write!(f, ", about {} left", duration_text(eta))?;
        }
        Ok(())
    }
}

/// `3h 05m`, `7m 09s`, `42s`.
fn duration_text(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_two_samples_in_time_before_it_talks_about_speed() {
        let mut progress = Progress::new(1000);
        assert_eq!(progress.rate(), None);
        progress.record(0, 0);
        assert_eq!(progress.rate(), None);
        progress.record(0, 5);
        assert_eq!(progress.rate(), None, "same instant");
        progress.record(2000, 40);
        assert_eq!(progress.rate(), Some(20.0));
    }

    #[test]
    fn the_estimate_is_what_is_left_over_the_rate() {
        let mut progress = Progress::new(1000);
        progress.record(0, 0);
        progress.record(10_000, 100); // 10 chunks/s, 900 left
        assert_eq!(progress.rate(), Some(10.0));
        assert_eq!(progress.eta_secs(), Some(90));
    }

    #[test]
    fn a_stalled_job_has_no_estimate() {
        let mut progress = Progress::new(1000);
        progress.record(0, 50);
        progress.record(10_000, 50);
        assert_eq!(progress.rate(), Some(0.0));
        assert_eq!(progress.eta_secs(), None);
    }

    #[test]
    fn old_samples_are_forgotten_so_the_rate_follows_the_present() {
        let mut progress = Progress::new(10_000);
        progress.record(0, 0);
        progress.record(10_000, 5_000); // fast start: 500/s
        progress.record(60_000, 5_050); // then slow: 1/s over the last 50 s? only samples within 30 s count
        progress.record(70_000, 5_060);
        // window keeps the samples from 40 s on: (60 s, 5050) and (70 s, 5060) -> 1 chunk/s
        assert_eq!(progress.rate(), Some(1.0));
    }

    #[test]
    fn percent_is_bounded_and_an_empty_job_is_finished() {
        let mut progress = Progress::new(200);
        progress.record(0, 50);
        assert_eq!(progress.percent(), 25.0);
        progress.record(1, 250);
        assert_eq!(progress.percent(), 100.0);
        assert_eq!(Progress::new(0).percent(), 100.0);
    }

    #[test]
    fn reads_as_a_sentence() {
        let mut progress = Progress::new(1000);
        progress.record(0, 0);
        progress.record(10_000, 100);
        assert_eq!(
            progress.to_string(),
            "100/1000 chunks (10.0%), 10.0 chunks/s, about 1m 30s left"
        );
        assert_eq!(Progress::new(5).to_string(), "0/5 chunks (0.0%)");
    }

    #[test]
    fn durations_are_short() {
        assert_eq!(duration_text(42), "42s");
        assert_eq!(duration_text(429), "7m 09s");
        assert_eq!(duration_text(11_100), "3h 05m");
    }
}
