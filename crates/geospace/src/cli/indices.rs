use std::path::Path;

use clap::{Subcommand, ValueEnum};
use ionoray_core::Epoch;
use ionoray_indices::{IndexStore, SyncPolicy};

use crate::GeospaceError;

use super::output;

#[derive(Debug, Subcommand)]
pub(super) enum IndicesCommand {
    /// Maintain requested fields over a UTC half-open interval.
    SyncRange(super::indices_range::RangeArgs),
    /// Query upstream metadata without downloading complete bodies.
    Query {
        #[arg(long)]
        year: u16,
    },
    /// Download and validate raw source files without importing values.
    Download {
        #[arg(long)]
        year: u16,
        /// Unconditionally redownload every body and compare SHA-256.
        #[arg(long)]
        force: bool,
    },
    /// Query upstream, download changes, validate, and import yearly databases.
    Sync {
        #[arg(long)]
        year: u16,
        #[arg(long, value_enum, default_value_t = CliSyncPolicy::AlwaysCheck)]
        policy: CliSyncPolicy,
    },
    /// Guarantee local coverage, synchronizing only when it is missing.
    Ensure {
        #[arg(long)]
        year: u16,
    },
    /// Verify local CAS bytes and yearly release coverage.
    Verify {
        #[arg(long)]
        year: u16,
    },
    /// Rebuild yearly databases only from existing CAS objects.
    Reindex {
        #[arg(long)]
        year: u16,
    },
    /// Force redownload and rebuild missing or corrupt local data.
    Repair {
        #[arg(long)]
        year: u16,
    },
    /// Read the latest deterministic core-index release at one UTC epoch.
    Read {
        /// ISO-8601 UTC epoch, for example `2020-07-01T12:00:00 UTC`.
        #[arg(long)]
        at: String,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(super) enum CliSyncPolicy {
    AlwaysCheck,
    ForceDownload,
    Offline,
}

pub(super) async fn execute(
    home: Option<&Path>,
    command: IndicesCommand,
) -> Result<(), GeospaceError> {
    let store = IndexStore::open(home).await?;
    match command {
        IndicesCommand::SyncRange(args) => {
            super::indices_range::execute(&store, args).await?;
        }
        IndicesCommand::Query { year } => output::json(&store.query_year(year).await?)?,
        IndicesCommand::Download { year, force } => {
            let report = if force {
                store.force_download_year(year).await?
            } else {
                store.download_year(year).await?
            };
            output::json(&report)?;
        }
        IndicesCommand::Sync { year, policy } => {
            output::json(&store.sync_year(year, policy.into()).await?)?;
        }
        IndicesCommand::Ensure { year } => output::json(&store.ensure_year(year).await?)?,
        IndicesCommand::Verify { year } => output::json(&store.verify_year(year).await?)?,
        IndicesCommand::Reindex { year } => output::json(&store.reindex_year(year).await?)?,
        IndicesCommand::Repair { year } => output::json(&store.repair_year(year).await?)?,
        IndicesCommand::Read { at } => {
            let epoch = at
                .parse::<Epoch>()
                .map_err(|error| GeospaceError::InvalidEpoch(error.to_string()))?;
            output::json(&store.at(epoch).await?)?;
        }
    }
    Ok(())
}

impl From<CliSyncPolicy> for SyncPolicy {
    fn from(value: CliSyncPolicy) -> Self {
        match value {
            CliSyncPolicy::AlwaysCheck => Self::AlwaysCheck,
            CliSyncPolicy::ForceDownload => Self::ForceDownload,
            CliSyncPolicy::Offline => Self::Offline,
        }
    }
}
