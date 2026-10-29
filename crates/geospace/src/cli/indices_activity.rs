use std::{
    io::{self, Write},
    path::Path,
};

use clap::{Args, Subcommand, ValueEnum};
use ionoray_indices::activity::{ActivityDataset, ActivityRange, ActivityStore};

use super::output;
use crate::GeospaceError;

#[derive(Debug, Subcommand)]
pub(super) enum ActivityCommand {
    /// Check official sources, validate raw files, retain revisions and report gaps.
    Download {
        #[command(flatten)]
        args: RangeArgs,
        /// Force body transfer and compare SHA-256; false still checks upstream.
        #[arg(long, action = clap::ArgAction::Set, num_args = 0..=1,
            require_equals = true, default_missing_value = "true", default_value = "false")]
        force: bool,
    },
    /// Stream accepted local samples as JSONL; reports on stderr, no HTTP.
    Read(RangeArgs),
}

#[derive(Debug, Args)]
pub(super) struct RangeArgs {
    /// Native product; all requests Hp30/ap30, Hp60/ap60 and PCN independently.
    #[arg(long, value_enum, default_value = "all")]
    dataset: Dataset,
    /// Inclusive UTC epoch or YYYY-MM-DD (midnight UTC).
    #[arg(long)]
    start: String,
    /// Exclusive UTC epoch or YYYY-MM-DD.
    #[arg(long)]
    end: String,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Dataset {
    All,
    Hp30,
    Hp60,
    Pcn,
}

impl Dataset {
    fn selected(self) -> Vec<ActivityDataset> {
        match self {
            Self::All => vec![
                ActivityDataset::Hp30,
                ActivityDataset::Hp60,
                ActivityDataset::Pcn,
            ],
            Self::Hp30 => vec![ActivityDataset::Hp30],
            Self::Hp60 => vec![ActivityDataset::Hp60],
            Self::Pcn => vec![ActivityDataset::Pcn],
        }
    }
}

pub(super) async fn execute(
    home: Option<&Path>,
    command: ActivityCommand,
) -> Result<(), GeospaceError> {
    let args = match &command {
        ActivityCommand::Download { args, .. } | ActivityCommand::Read(args) => args,
    };
    let range = ActivityRange::parse(&args.start, &args.end)?;
    let datasets = args.dataset.selected();
    let store = ActivityStore::open(home)?;
    let mut failed = 0;
    match command {
        ActivityCommand::Download { force, .. } => {
            let mut reports = Vec::new();
            for dataset in datasets {
                match store.download(dataset, range, force).await {
                    Ok(report) => reports.push(serde_json::json!({"dataset":dataset,"status":"downloaded","report":report})),
                    Err(error) => {
                        failed += 1;
                        reports.push(serde_json::json!({"dataset":dataset,"status":"failed","error":error.to_string()}));
                    }
                }
            }
            output::json(&reports)?;
        }
        ActivityCommand::Read(_) => {
            let mut stdout = io::BufWriter::new(io::stdout().lock());
            for dataset in datasets {
                let result = store
                    .read(dataset, range, |record| {
                        serde_json::to_writer(&mut stdout, record).map_err(io::Error::other)?;
                        stdout.write_all(b"\n")?;
                        Ok(())
                    })
                    .await;
                let report = match result {
                    Ok(report) => {
                        serde_json::json!({"dataset":dataset,"status":"read","report":report})
                    }
                    Err(error) => {
                        failed += 1;
                        serde_json::json!({"dataset":dataset,"status":"failed","error":error.to_string()})
                    }
                };
                let mut stderr = io::stderr().lock();
                serde_json::to_writer(&mut stderr, &report)?;
                stderr.write_all(b"\n")?;
            }
            stdout.flush()?;
        }
    }
    if failed > 0 {
        return Err(GeospaceError::ActivityBatchFailures(failed));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::cli::Cli;
    use clap::Parser;
    #[test]
    fn native_products_parse_and_pcs_is_not_advertised() {
        for dataset in ["all", "hp30", "hp60", "pcn"] {
            assert!(
                Cli::try_parse_from([
                    "geospace",
                    "indices",
                    "activity",
                    "download",
                    "--dataset",
                    dataset,
                    "--start",
                    "2024-05-10",
                    "--end",
                    "2024-05-13",
                    "--force=false"
                ])
                .is_ok()
            );
        }
        assert!(
            Cli::try_parse_from([
                "geospace",
                "indices",
                "activity",
                "read",
                "--dataset",
                "pcs",
                "--start",
                "2024-05-10",
                "--end",
                "2024-05-13"
            ])
            .is_err()
        );
    }
}
