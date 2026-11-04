//! A file with one line per tick: when it ended, its number, and what every phase took in it.
//!
//! The summaries (`/tickprofile`) say how ticks are on average and which were worst; this says
//! what happened tick by tick, so that spikes can be tied to a time, a period or a cause.

use std::{
    fs::File,
    io::{BufWriter, Write},
    path::Path,
    time::Instant,
};

use crate::Phase;

/// How many ticks pass between two flushes of the file: one second of play.
const FLUSH_EVERY: u64 = 20;

/// The first line: column names, `_us` for microseconds.
#[must_use]
pub fn header() -> String {
    let mut line = String::from("at_ms,tick");
    for phase in Phase::ALL {
        line.push(',');
        line.push_str(phase.name());
        line.push_str("_us");
    }
    line
}

/// One tick: `at_ms` since the trace began, the tick's number, the phases in nanoseconds in the
/// order of [`Phase::ALL`].
#[must_use]
pub fn line(at_ms: u64, tick: u64, phases_ns: &[u64; Phase::ALL.len()]) -> String {
    let mut line = format!("{at_ms},{tick}");
    for nanos in phases_ns {
        line.push(',');
        line.push_str(&(nanos / 1_000).to_string());
    }
    line
}

/// The open file of a running trace.
#[derive(Debug)]
pub struct Sink {
    out: BufWriter<File>,
    began: Instant,
    ticks: u64,
}

impl Sink {
    /// Creates (or empties) the file and writes the header.
    ///
    /// # Errors
    /// The file cannot be created or written.
    pub fn create(path: &Path) -> std::io::Result<Self> {
        let mut out = BufWriter::new(File::create(path)?);
        writeln!(out, "{}", header())?;
        out.flush()?;
        Ok(Self {
            out,
            began: Instant::now(),
            ticks: 0,
        })
    }

    /// Appends one tick. A write that fails is dropped: tracing must never stop a server.
    pub fn tick(&mut self, phases_ns: &[u64; Phase::ALL.len()]) {
        self.ticks += 1;
        let at_ms = u64::try_from(self.began.elapsed().as_millis()).unwrap_or(u64::MAX);
        let _ = writeln!(self.out, "{}", line(at_ms, self.ticks, phases_ns));
        if self.ticks.is_multiple_of(FLUSH_EVERY) {
            let _ = self.out.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_names_every_phase_in_order() {
        let header = header();
        let columns: Vec<&str> = header.split(',').collect();
        assert_eq!(columns.len(), 2 + Phase::ALL.len());
        assert_eq!(&columns[..3], ["at_ms", "tick", "server_us"]);
        assert_eq!(columns.last(), Some(&"tick_thread_runqueue_us"));
    }

    #[test]
    fn a_line_has_the_time_the_number_and_microseconds_per_phase() {
        let mut phases = [0u64; Phase::ALL.len()];
        phases[Phase::Server.index()] = 12_345_678;
        phases[Phase::EntityTicking.index()] = 999;
        let line = line(1500, 7, &phases);
        let fields: Vec<&str> = line.split(',').collect();
        assert_eq!(fields.len(), 2 + Phase::ALL.len());
        assert_eq!(&fields[..3], ["1500", "7", "12345"]);
        let at = 2 + Phase::EntityTicking.index();
        assert_eq!(fields[at], "0", "999 ns is under a microsecond");
        assert_eq!(header().split(',').nth(at), Some("entity_ticking_us"));
    }

    #[test]
    fn a_trace_file_gets_the_header_and_one_line_per_tick() {
        let path =
            std::env::temp_dir().join(format!("kvasir-trace-test-{}.csv", std::process::id()));
        let mut sink = Sink::create(&path).unwrap();
        let phases = [5_000u64; Phase::ALL.len()];
        for _ in 0..FLUSH_EVERY {
            sink.tick(&phases);
        }
        drop(sink);
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len() as u64, 1 + FLUSH_EVERY);
        assert_eq!(lines[0], header());
        assert!(lines[1].starts_with(|c: char| c.is_ascii_digit()));
        assert_eq!(lines[1].split(',').nth(1), Some("1"));
        assert_eq!(lines[20].split(',').nth(1), Some("20"));
    }
}
