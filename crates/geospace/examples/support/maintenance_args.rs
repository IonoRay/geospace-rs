//! Command-line contract for the explicit yearly maintenance example.

use std::{ops::RangeInclusive, path::PathBuf};

use clap::{Parser, ValueEnum};
use ionoray_indices::SyncPolicy;

const FIRST_SUPPORTED_YEAR: u16 = 1958;

/// Explicit inputs for annual index maintenance.
#[derive(Debug, Parser)]
#[command(
    name = "indices_maintenance",
    about = "Synchronize every supported index dataset for an explicit UTC-year range"
)]
pub struct MaintenanceArgs {
    /// Absolute store root; `~` is expanded by `StoreHome` before opening it.
    #[arg(long, value_name = "PATH", required = true)]
    pub home: PathBuf,
    /// Maintain exactly one UTC year.
    #[arg(long, required_unless_present = "start_year", conflicts_with_all = ["start_year", "end_year"])]
    pub year: Option<u16>,
    /// First UTC year in an inclusive maintenance range.
    #[arg(long, requires = "end_year", conflicts_with = "year")]
    pub start_year: Option<u16>,
    /// Last UTC year in an inclusive maintenance range.
    #[arg(long, requires = "start_year", conflicts_with = "year")]
    pub end_year: Option<u16>,
    /// Upstream access policy for every selected year.
    #[arg(long, value_enum, default_value_t = MaintenancePolicy::AlwaysCheck)]
    pub policy: MaintenancePolicy,
}

/// Command-line spellings for the existing index synchronization policies.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum MaintenancePolicy {
    /// Check metadata and download potentially changed content.
    #[default]
    AlwaysCheck,
    /// Redownload all source bodies and compare their digests.
    ForceDownload,
    /// Rebuild from locally available content without network access.
    Offline,
}

impl From<MaintenancePolicy> for SyncPolicy {
    fn from(value: MaintenancePolicy) -> Self {
        match value {
            MaintenancePolicy::AlwaysCheck => Self::AlwaysCheck,
            MaintenancePolicy::ForceDownload => Self::ForceDownload,
            MaintenancePolicy::Offline => Self::Offline,
        }
    }
}

impl MaintenancePolicy {
    /// Whether the selected runtime policy may query upstream sources.
    pub const fn permits_network(self) -> bool {
        !matches!(self, Self::Offline)
    }
}

/// Validates and expands the requested UTC-year selection against a supplied current year.
pub fn selected_years(
    args: &MaintenanceArgs,
    current_year: u16,
) -> Result<RangeInclusive<u16>, String> {
    let (start, end) = match (args.year, args.start_year, args.end_year) {
        (Some(year), None, None) => (year, year),
        (None, Some(start), Some(end)) => (start, end),
        _ => {
            return Err(
                "select one year with --year, or an inclusive range with --start-year and --end-year"
                    .to_owned(),
            );
        }
    };

    if start < FIRST_SUPPORTED_YEAR || end < FIRST_SUPPORTED_YEAR {
        return Err(format!(
            "years must be no earlier than {FIRST_SUPPORTED_YEAR} (got {start}..={end})"
        ));
    }
    if start > end {
        return Err(format!(
            "--start-year ({start}) must not exceed --end-year ({end})"
        ));
    }
    if end > current_year {
        return Err(format!(
            "years must not be after the current UTC year ({current_year}; got {start}..={end})"
        ));
    }
    Ok(start..=end)
}
