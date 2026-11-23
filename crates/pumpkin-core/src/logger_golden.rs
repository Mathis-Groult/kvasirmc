//! A record of `init_logger` (`tests/golden/init_logger.txt`; rewrite it on purpose with
//! `KVASIR_BLESS=1`), made before it was split.
//!
//! `init_logger` installs a process-wide subscriber and can run only once, so each scene runs it
//! in its own child process: the test binary started again on `child` with the scene in an
//! environment variable and a fresh working directory (the file logger writes `logs/`). The child
//! logs one event per level, then prints what the console showed, the log files, and what
//! `LOGGER_IMPL` holds. Digits are masked (timestamps, thread ids).
//!
//! Not covered: the terminal branch (`use_tty` with a real tty on stdin), which needs a pty.

#![allow(clippy::print_stdout)]

use std::fmt::Write;
use std::process::{Command, Stdio};

use pumpkin_config::AdvancedConfiguration;

use crate::LOGGER_IMPL;
use crate::command::commands::script_golden::{self, TestResult};
use crate::init_logger;

const SCENE_VAR: &str = "KVASIR_LOGGER_SCENE";
const BEGIN: &str = "<<<scene";
const END: &str = "scene>>>";

/// Name, `key=value;..` over the `LoggingConfig` fields (`use_tty` is `commands.use_tty`), and
/// the `RUST_LOG` value when set. Unset keys keep the defaults, except `file`, which is empty.
type Scene = (&'static str, &'static str, Option<&'static str>);

const STYLE_SCENES: &[Scene] = &[
    ("defaults", "", None),
    ("no color", "color=false", None),
    ("target", "target=true", None),
    ("threads", "threads=true", None),
    ("thread ids", "thread_ids=true", None),
    (
        "all on, no color",
        "color=false;target=true;threads=true;thread_ids=true",
        None,
    ),
    (
        "all off",
        "color=false;target=false;threads=false;thread_ids=false;timestamp=false",
        None,
    ),
    ("no timestamp", "timestamp=false", None),
    (
        "no timestamp, format ignored",
        "timestamp=false;timestamp_format=[year]-[month]",
        None,
    ),
    (
        "custom timestamp format",
        "timestamp_format=[year]-[month]-[day]",
        None,
    ),
    ("bad timestamp format", "timestamp_format=[bogus]", None),
    (
        "no timestamp with target and threads",
        "timestamp=false;target=true;threads=true;thread_ids=true",
        None,
    ),
    ("tty requested, stdin is not a tty", "use_tty=true", None),
    ("disabled", "enabled=false", None),
    ("disabled with file", "enabled=false;file=latest.log", None),
];

const FILE_SCENES: &[Scene] = &[
    ("file", "file=latest.log", None),
    (
        "file, no timestamp",
        "file=latest.log;timestamp=false",
        None,
    ),
    ("file, other name", "file=server.log;color=false", None),
    ("file in a missing directory", "file=sub/x.log", None),
];

const LEVEL_SCENES: &[Scene] = &[
    ("level off", "level=off;file=latest.log", None),
    ("level error", "level=error;file=latest.log", None),
    ("level warn", "level=warn;file=latest.log", None),
    ("level info", "level=info;file=latest.log", None),
    ("level debug", "level=debug;file=latest.log", None),
    ("level trace", "level=trace;file=latest.log", None),
    ("level upper case", "level=WARN;file=latest.log", None),
    ("level number", "level=2;file=latest.log", None),
    ("level unknown", "level=bogus;file=latest.log", None),
    ("level empty", "level=;file=latest.log", None),
];

const ENV_SCENES: &[Scene] = &[
    (
        "RUST_LOG debug over level error",
        "level=error;file=latest.log",
        Some("debug"),
    ),
    (
        "RUST_LOG error over level trace",
        "level=trace;file=latest.log",
        Some("error"),
    ),
    ("RUST_LOG off", "level=info;file=latest.log", Some("off")),
    (
        "RUST_LOG not a level",
        "level=warn;file=latest.log",
        Some("garbage[="),
    ),
    (
        "RUST_LOG directive for this crate",
        "level=warn;file=latest.log",
        Some("pumpkin_core=trace"),
    ),
    (
        "RUST_LOG directive for another crate",
        "level=trace;file=latest.log",
        Some("other_crate=trace"),
    ),
    ("RUST_LOG empty", "level=warn;file=latest.log", Some("")),
];

/// A message holding escape codes: the console layer must pass it through unchanged.
const ESCAPE_SCENES: &[Scene] = &[
    ("escape codes in a message", "escape_event=true", None),
    (
        "escape codes, no color",
        "escape_event=true;color=false",
        None,
    ),
];

/// The part of `init_logger` that runs in the child: one scene, then a report on stdout.
#[test]
fn child() {
    let Ok(spec) = std::env::var(SCENE_VAR) else {
        return;
    };
    let mut advanced = AdvancedConfiguration::default();
    advanced.logging.file = String::new();
    let mut escape_event = false;
    for pair in spec.split(';').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').expect("key=value");
        let logging = &mut advanced.logging;
        match key {
            "enabled" => logging.enabled = value == "true",
            "level" => logging.level = value.to_string(),
            "threads" => logging.threads = value == "true",
            "thread_ids" => logging.thread_ids = value == "true",
            "target" => logging.target = value == "true",
            "color" => logging.color = value == "true",
            "timestamp" => logging.timestamp = value == "true",
            "timestamp_format" => logging.timestamp_format = value.to_string(),
            "file" => logging.file = value.to_string(),
            "use_tty" => advanced.commands.use_tty = value == "true",
            "escape_event" => escape_event = value == "true",
            other => panic!("unknown key {other}"),
        }
    }
    init_logger(&advanced);
    println!("\n{BEGIN}");
    tracing::error!("an error");
    tracing::warn!("a warning");
    tracing::info!("an info");
    tracing::debug!("a debug");
    tracing::trace!("a trace");
    tracing::info!(target: "other_crate", "another crate");
    if escape_event {
        tracing::info!("\x1b[31mred\x1b[0m");
    }
    println!("{}", state());
    print_log_files();
    println!("{END}");
}

fn state() -> String {
    match LOGGER_IMPL.get() {
        Some(Some((wrapper, level, config))) => format!(
            "state: level={level} color={} threads={} thread_ids={} target={} timestamp={} readline={}",
            config.color,
            config.threads,
            config.thread_ids,
            config.target,
            config.timestamp,
            wrapper.take_readline().is_some()
        ),
        Some(None) => "state: logger off".to_string(),
        None => "state: unset".to_string(),
    }
}

fn print_log_files() {
    let Ok(dir) = std::fs::read_dir("logs") else {
        println!("logs: none");
        return;
    };
    let mut names: Vec<_> = dir.map(|entry| entry.expect("entry").file_name()).collect();
    names.sort();
    for name in names {
        let path = std::path::Path::new("logs").join(&name);
        println!("logs/{}:", name.to_string_lossy());
        for line in std::fs::read_to_string(path).expect("log").lines() {
            println!("  {line}");
        }
    }
}

/// Starts the test binary on `child` for one scene, in its own working directory.
fn spawn(index: usize, scene: &Scene) -> (std::path::PathBuf, std::process::Child) {
    let dir = std::env::temp_dir().join(format!(
        "kvasir-logger-golden-{}-{index}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scene dir");
    let mut command = Command::new(std::env::current_exe().expect("test binary"));
    command
        .args(["--exact", "logger_golden::child", "--nocapture"])
        .args(["--test-threads=1"])
        .env(SCENE_VAR, scene.1)
        .env_remove("RUST_LOG")
        .env_remove("KVASIR_BLESS")
        .current_dir(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(rust_log) = scene.2 {
        command.env("RUST_LOG", rust_log);
    }
    (dir, command.spawn().expect("spawn"))
}

/// The child's report: its stdout between the markers, digits masked, escapes made visible.
fn report(stdout: &[u8]) -> String {
    let text = String::from_utf8_lossy(stdout);
    let mut inside = false;
    let mut out = String::new();
    for line in text.lines() {
        if line == END {
            inside = false;
        }
        if inside {
            let masked: String = line
                .chars()
                .map(|c| if c.is_ascii_digit() { '#' } else { c })
                .collect();
            writeln!(out, "  {}", masked.replace('\x1b', "<esc>")).expect("write");
        }
        if line == BEGIN {
            inside = true;
        }
    }
    if out.is_empty() {
        out.push_str("  (no report)\n");
    }
    out
}

fn run_scenes(scenes: &[Scene], first_index: usize, now: &mut String) {
    // Six children at a time: each is a whole test binary.
    for (chunk_no, chunk) in scenes.chunks(6).enumerate() {
        let children: Vec<_> = chunk
            .iter()
            .enumerate()
            .map(|(i, scene)| (scene, spawn(first_index + chunk_no * 6 + i, scene)))
            .collect();
        for (scene, (dir, child)) in children {
            let output = child.wait_with_output().expect("wait");
            let rust_log = scene
                .2
                .map_or(String::new(), |v| format!(" RUST_LOG={v:?}"));
            writeln!(
                now,
                "== {} [{}]{rust_log} exit={}",
                scene.0,
                scene.1,
                output.status.success()
            )
            .expect("write");
            now.push_str(&report(&output.stdout));
            std::fs::remove_dir_all(dir).expect("remove scene dir");
        }
    }
}

#[test]
fn init_logger_golden() -> TestResult {
    if std::env::var(SCENE_VAR).is_ok() {
        return Ok(());
    }
    let mut now = String::new();
    let mut index = 0;
    for scenes in [
        STYLE_SCENES,
        FILE_SCENES,
        LEVEL_SCENES,
        ENV_SCENES,
        ESCAPE_SCENES,
    ] {
        run_scenes(scenes, index, &mut now);
        index += scenes.len();
    }
    script_golden::bless_or_compare("init_logger.txt", &now)?;
    Ok(())
}
