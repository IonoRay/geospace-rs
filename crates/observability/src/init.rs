//! Uniform process-level tracing for `IonoRay` binaries and examples.

use std::{
    error::Error,
    fmt,
    fs::{self, OpenOptions},
    path::Path,
};

use tracing::{Span, info_span};
use tracing_appender::non_blocking::{NonBlocking, NonBlockingBuilder, WorkerGuard};
use tracing_subscriber::{EnvFilter, prelude::*};
use uuid::Uuid;

use crate::config::{ConsoleConfig, LogFormat, TracingConfig};
use crate::resources::Collector;
use crate::subscriber::{json_layer, pretty_layer};

use crate::resources::ProfileCloseError;
use crate::resources::ResourceLayer;

const DEFAULT_FILTER: &str = "info,hyper=warn,h2=warn,reqwest=warn,rustls=warn,turso=warn";

/// Error returned when the process-wide tracing subscriber cannot be installed.
#[derive(Debug)]
pub struct TracingInitError(String);

impl fmt::Display for TracingInitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for TracingInitError {}

/// Keeps the optional file writer alive and flushes it during process shutdown.
#[must_use = "dropping the tracing guard early can lose buffered file logs"]
pub struct TracingGuard {
    collector: Collector,
}

/// Builds the resource layer for applications which already own a tracing
/// subscriber. The returned guard has the same close contract as `init_tracing`.
/// No global subscriber is installed.
///
/// # Errors
///
/// Returns an error for invalid profiling environment configuration.
pub fn resource_layer_from_env() -> Result<(ResourceLayer, TracingGuard), TracingInitError> {
    let config = TracingConfig::from_env().map_err(TracingInitError)?;
    let collector = Collector::new(config.profile(), None, config.file().cloned());
    Ok((
        ResourceLayer::new(collector.clone()),
        TracingGuard { collector },
    ))
}

impl TracingGuard {
    /// Flushes tracing output after all active resource observation windows end.
    ///
    /// Returning an error leaves the collector running, so callers can retry
    /// after their concurrent point queries have completed.
    /// # Errors
    /// Returns an error without stopping collection when point queries remain active.
    pub fn close(&self) -> Result<(), ProfileCloseError> {
        self.collector.close()
    }
}

impl Drop for TracingGuard {
    fn drop(&mut self) {
        self.collector.release_owner();
    }
}

/// Installs the process-wide tracing subscriber.
///
/// The console sink is enabled by default: TTY stderr uses colored compact text
/// and redirected stderr uses NDJSON. Setting `IONORAY_LOG_FILE` adds an
/// independent NDJSON file sink. Timestamps use the computer's current local
/// UTC offset. `RUST_LOG` overrides the default `info` filter.
///
/// # Errors
///
/// Returns an error when configuration or file initialization fails, or when
/// another global subscriber is already installed.
pub fn init_tracing() -> Result<TracingGuard, TracingInitError> {
    let config = TracingConfig::from_env().map_err(TracingInitError)?;
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
    let (file_writer, file_guard) = config
        .file()
        .map(|path| open_file_writer(path))
        .transpose()?
        .map_or((None, None), |(writer, guard)| (Some(writer), Some(guard)));
    let collector = Collector::new(config.profile(), file_guard, config.file().cloned());
    if let Err(error) = install_sinks(
        config.console(),
        file_writer,
        filter,
        Some(collector.clone()),
    ) {
        let _ = collector.close();
        return Err(error);
    }
    tracing::info!(event = "resource.environment", scope = "process",
        pid = std::process::id(), os = std::env::consts::OS, architecture = std::env::consts::ARCH,
        observability_version = env!("CARGO_PKG_VERSION"), profile = ?config.profile(),
        collector_backend = "sysinfo-0.37.2+native", log_policy = "non_lossy",
        "tracing initialized; optional metrics are reported with explicit status");
    Ok(TracingGuard { collector })
}

/// Creates the root span shared by every executable target.
///
/// Keeping the run ID and process ID on the root span makes every nested event
/// correlatable without repeating those fields at each library call site.
pub fn run_span(operation: &'static str) -> Span {
    let run_id = Uuid::now_v7();
    info_span!(
        "ionoray.run",
        operation,
        run_id = %run_id,
        pid = std::process::id()
    )
}

fn install(
    subscriber: impl tracing::Subscriber + Send + Sync + 'static,
) -> Result<(), TracingInitError> {
    tracing::subscriber::set_global_default(subscriber)
        .map_err(|error| TracingInitError(error.to_string()))
}

fn install_sinks(
    console: Option<ConsoleConfig>,
    file: Option<NonBlocking>,
    filter: EnvFilter,
    collector: Option<Collector>,
) -> Result<(), TracingInitError> {
    match (console, file) {
        (None, None) => install(
            tracing_subscriber::registry()
                .with(filter)
                .with(collector.map(ResourceLayer::new)),
        ),
        (
            Some(ConsoleConfig {
                format: LogFormat::Json,
                ..
            }),
            None,
        ) => install(
            tracing_subscriber::registry()
                .with(json_layer(std::io::stderr, filter))
                .with(collector.map(ResourceLayer::new)),
        ),
        (
            Some(ConsoleConfig {
                format: LogFormat::Pretty,
                ansi,
            }),
            None,
        ) => install(
            tracing_subscriber::registry()
                .with(pretty_layer(std::io::stderr, filter, ansi))
                .with(collector.map(ResourceLayer::new)),
        ),
        (None, Some(file)) => install(
            tracing_subscriber::registry()
                .with(json_layer(file, filter))
                .with(collector.map(ResourceLayer::new)),
        ),
        (
            Some(ConsoleConfig {
                format: LogFormat::Json,
                ..
            }),
            Some(file),
        ) => install(
            tracing_subscriber::registry()
                .with(json_layer(std::io::stderr, filter.clone()))
                .with(json_layer(file, filter))
                .with(collector.map(ResourceLayer::new)),
        ),
        (
            Some(ConsoleConfig {
                format: LogFormat::Pretty,
                ansi,
            }),
            Some(file),
        ) => install(
            tracing_subscriber::registry()
                .with(pretty_layer(std::io::stderr, filter.clone(), ansi))
                .with(json_layer(file, filter))
                .with(collector.map(ResourceLayer::new)),
        ),
    }
}

pub(crate) fn open_file_writer(
    path: &Path,
) -> Result<(NonBlocking, WorkerGuard), TracingInitError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| {
            TracingInitError(format!(
                "cannot create log directory `{}`: {error}",
                parent.display()
            ))
        })?;
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| {
            TracingInitError(format!(
                "cannot open log file `{}`: {error}",
                path.display()
            ))
        })?;
    Ok(NonBlockingBuilder::default()
        .lossy(false)
        .thread_name("ionoray-log-writer")
        .finish(file))
}

#[cfg(test)]
mod tests {
    use tracing_subscriber::prelude::*;

    use super::open_file_writer;
    use crate::subscriber::json_layer;

    #[test]
    fn file_writer_appends_and_flushes_ndjson() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("nested/run.ndjson");

        for sequence in 1..=2 {
            let (writer, guard) = open_file_writer(&path).expect("file writer");
            let subscriber = tracing_subscriber::registry().with(json_layer(
                writer,
                tracing_subscriber::EnvFilter::new("info"),
            ));
            tracing::subscriber::with_default(subscriber, || {
                tracing::info!(sequence, "append test event");
            });
            drop(guard);
        }

        let text = std::fs::read_to_string(path).expect("read appended log");
        let records = text
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("valid NDJSON"))
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["fields"]["sequence"], 1);
        assert_eq!(records[1]["fields"]["sequence"], 2);
    }
}
