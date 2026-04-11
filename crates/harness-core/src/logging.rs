use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use tracing_appender::rolling;
use tracing_subscriber::{EnvFilter, Layer, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::home::loopsmith_home;

const LOGS_DIR: &str = "logs";
const LOG_FILE_PREFIX: &str = "loopsmith";

/// Returns the path to the logs directory (`~/.loopsmith/logs/`).
pub fn logs_dir() -> Result<PathBuf> {
    Ok(loopsmith_home()?.join(LOGS_DIR))
}

/// Initializes the global tracing/logging system.
///
/// Logs are written to both:
/// - **stderr** (human-readable, respects `RUST_LOG` env var)
/// - **`~/.loopsmith/logs/loopsmith.YYYY-MM-DD.log`** (daily rotation, always at DEBUG level)
///
/// The `_guard` in the returned tuple **must** be held alive for the entire
/// program lifetime — dropping it flushes and closes the log file.
pub fn init_logging() -> Result<LogGuard> {
    let log_dir = logs_dir()?;
    fs::create_dir_all(&log_dir)
        .with_context(|| format!("failed to create log directory {}", log_dir.display()))?;

    let file_appender = rolling::daily(&log_dir, LOG_FILE_PREFIX);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(
            "info,loopsmith_core=info,loopsmith_worker_claude=info,loopsmith_worker_codex=info,loopsmith_worker_gemini=info,loopsmith_worker_simulated=info",
        )
    });

    let stderr_layer = fmt::layer().with_target(false).with_writer(std::io::stderr);

    let file_filter = EnvFilter::new("debug");
    let file_layer = fmt::layer()
        .with_ansi(false)
        .with_writer(non_blocking)
        .with_filter(file_filter);

    tracing_subscriber::registry()
        .with(stderr_layer.with_filter(env_filter))
        .with(file_layer)
        .init();

    Ok(LogGuard { _guard: guard })
}

/// Holds the non-blocking writer guard. Must be kept alive for the program's
/// entire lifetime to ensure all log entries are flushed to disk.
pub struct LogGuard {
    _guard: tracing_appender::non_blocking::WorkerGuard,
}
