//! The parts `init_logger` puts together: the level, the console filter, the file logger, the
//! console writer with its optional line editor, and the subscriber install.

use std::io::{IsTerminal, stdin};
use std::str::FromStr;
use std::sync::Mutex;

use rustyline::{Config, Editor, history::FileHistory};
use tracing::error;
use tracing_subscriber::fmt::{self, time::OffsetTime};
use tracing_subscriber::layer::{Layered, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, Registry, filter::LevelFilter};

use pumpkin_config::logging::LoggingConfig as LoggingSettings;

use crate::LoggingConfig;
use crate::logging::{ConsoleWriter, GzipRollingLogger, PumpkinCommandCompleter};

/// `RUST_LOG` when set, else the configured level; a level that does not parse is `INFO`.
pub fn resolve_level(env: Option<&str>, configured: &str) -> LevelFilter {
    LevelFilter::from_str(env.unwrap_or(configured)).unwrap_or(LevelFilter::INFO)
}

/// The console filter: `RUST_LOG` when it parses as a filter, else the level itself.
pub fn env_filter(level: LevelFilter) -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level.to_string()))
}

/// The log file layer; none when no file is configured or it cannot be opened.
pub fn open_file_logger(level: LevelFilter, file: &str) -> Option<GzipRollingLogger> {
    if file.is_empty() {
        return None;
    }
    match GzipRollingLogger::new(level, file.to_string()) {
        Ok(logger) => Some(logger),
        Err(err) => {
            error!("Failed to initialize file logger: {err}");
            None
        }
    }
}

type LineEditor = Editor<PumpkinCommandCompleter, FileHistory>;

/// The console writer, and the line editor when `use_tty` is set and stdin is a terminal.
#[expect(clippy::print_stderr)]
pub fn console(use_tty: bool) -> (ConsoleWriter, Option<LineEditor>) {
    if !(use_tty && stdin().is_terminal()) {
        return (ConsoleWriter::new(None), None);
    }
    let rl_config = Config::builder()
        .auto_add_history(true)
        .completion_type(rustyline::CompletionType::List)
        .edit_mode(rustyline::EditMode::Emacs)
        .build();
    match Editor::with_config(rl_config) {
        Ok(mut rl) => {
            rl.set_helper(Some(PumpkinCommandCompleter::new()));
            let printer = rl.create_external_printer().ok().map(|p| {
                let boxed: Box<dyn rustyline::ExternalPrinter + Send> = Box::new(p);
                boxed
            });
            (ConsoleWriter::new(printer), Some(rl))
        }
        Err(e) => {
            eprintln!("Failed to initialize console input ({e}); falling back to simple logger");
            (ConsoleWriter::new(None), None)
        }
    }
}

/// The timestamp timer: the configured format, or `[hour]:[minute]:[second]` when it is invalid.
fn timestamp_timer(
    format: &str,
) -> OffsetTime<Vec<time::format_description::BorrowedFormatItem<'static>>> {
    let local_offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
    let format_str: &'static str = Box::leak(format.to_string().into_boxed_str());
    let timer_format = time::format_description::parse(format_str)
        .unwrap_or_else(|_| time::macros::format_description!("[hour]:[minute]:[second]").to_vec());
    OffsetTime::new(local_offset, timer_format)
}

fn install_layers<L>(env_filter: EnvFilter, fmt_layer: L, file_logger: Option<GzipRollingLogger>)
where
    L: Layer<Layered<EnvFilter, Registry>> + Send + Sync + 'static,
{
    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .with(file_logger)
        .init();
}

/// Installs the global subscriber: console layer (with or without timestamps) and file layer.
pub fn install(
    logging: &LoggingSettings,
    console: ConsoleWriter,
    env_filter: EnvFilter,
    file_logger: Option<GzipRollingLogger>,
) {
    let fmt_layer = fmt::layer()
        .with_writer(Mutex::new(console))
        .with_ansi(logging.color)
        .with_ansi_sanitization(false)
        .with_target(logging.target)
        .with_thread_names(logging.threads)
        .with_thread_ids(logging.thread_ids);
    if logging.timestamp {
        let timer = timestamp_timer(&logging.timestamp_format);
        install_layers(env_filter, fmt_layer.with_timer(timer), file_logger);
    } else {
        install_layers(env_filter, fmt_layer.without_time(), file_logger);
    }
}

pub const fn logging_config(logging: &LoggingSettings) -> LoggingConfig {
    LoggingConfig {
        color: logging.color,
        threads: logging.threads,
        thread_ids: logging.thread_ids,
        target: logging.target,
        timestamp: logging.timestamp,
    }
}
