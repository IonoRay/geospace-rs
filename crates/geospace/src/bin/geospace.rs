//! `geospace` command-line entry point.

use std::process::ExitCode;

use clap::Parser;
use ionoray_geospace::cli::{Cli, ExecuteStatus, execute};
use ionoray_observability::{init_tracing, run_span};
use tracing::Instrument;

#[tokio::main]
async fn main() -> ExitCode {
    let _tracing_guard = match init_tracing() {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!(
                "{}",
                serde_json::json!({
                    "level": "ERROR",
                    "target": "geospace",
                    "pid": std::process::id(),
                    "message": "cannot initialize tracing",
                    "error": error.to_string(),
                })
            );
            return ExitCode::FAILURE;
        }
    };
    match execute(Cli::parse())
        .instrument(run_span("geospace.cli"))
        .await
    {
        Ok(ExecuteStatus::Succeeded) => ExitCode::SUCCESS,
        Ok(ExecuteStatus::RecordFailures) => ExitCode::from(3),
        Err(error) => {
            tracing::error!(error = %error, "command failed");
            ExitCode::FAILURE
        }
    }
}
