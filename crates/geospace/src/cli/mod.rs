use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::{GeospaceError, capabilities};

mod data;
#[cfg(feature = "indices")]
mod indices;
#[cfg(feature = "indices")]
mod indices_range;
mod model;
#[cfg(any(feature = "igrf", feature = "iri", feature = "hwm", feature = "msis"))]
mod model_error;
mod model_evaluate;
mod model_types;
mod output;

/// Sequential production JSONL processor used by the CLI and its offline example.
pub use model::process;

/// `IonoRay` geospace data and model command line interface.
#[derive(Debug, Parser)]
#[command(name = "geospace", version, about)]
pub struct Cli {
    /// Select a data home instead of `IONORAY_HOME` or `~/.ionoray/geospace-rs`.
    #[arg(long, global = true)]
    home: Option<PathBuf>,
    /// Operation to perform.
    #[command(subcommand)]
    command: Command,
}

/// Overall result used by the binary to select its documented exit status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecuteStatus {
    /// The command completed and every submitted record succeeded.
    Succeeded,
    /// Input was fully consumed, but at least one individual record failed.
    RecordFailures,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Initialize or inspect local data infrastructure.
    Data {
        #[command(subcommand)]
        command: data::DataCommand,
    },
    /// Query, synchronize, verify, and read geophysical indices.
    #[cfg(feature = "indices")]
    Indices {
        #[command(subcommand)]
        command: indices::IndicesCommand,
    },
    /// Evaluate one model record or a sequential JSONL stream.
    Model {
        #[command(subcommand)]
        command: model::ModelCommand,
    },
    /// Print capabilities compiled into this binary.
    Capabilities,
}

/// Executes one parsed command and prints stable JSON to stdout.
///
/// # Errors
/// Returns [`GeospaceError`] when parsing domain input, maintenance, querying,
/// model execution, or JSON serialization fails.
pub async fn execute(cli: Cli) -> Result<ExecuteStatus, GeospaceError> {
    let status = match cli.command {
        Command::Data { command } => {
            data::execute(cli.home.as_deref(), command).await?;
            ExecuteStatus::Succeeded
        }
        #[cfg(feature = "indices")]
        Command::Indices { command } => {
            indices::execute(cli.home.as_deref(), command).await?;
            ExecuteStatus::Succeeded
        }
        Command::Model { command } => model::execute(cli.home.as_deref(), command).await?,
        Command::Capabilities => {
            output::json(&capabilities())?;
            ExecuteStatus::Succeeded
        }
    };
    Ok(status)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Command};

    #[test]
    fn model_batch_accepts_an_input_path() {
        let cli = Cli::try_parse_from(["geospace", "model", "batch", "--input", "-"]).unwrap();
        assert!(matches!(cli.command, Command::Model { .. }));
    }

    #[test]
    fn model_command_requires_a_subcommand() {
        let result = Cli::try_parse_from(["geospace", "model"]);
        assert!(result.is_err());
    }
}
