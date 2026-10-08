use serde::{Deserialize, Serialize};

/// Controls whether point preparation may maintain remote index data.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataPolicy {
    /// Reuse complete local data and synchronize only missing coverage.
    #[default]
    Ensure,
    /// Never access the network; recover local raw data and query available values.
    Offline,
    /// Check upstream metadata and import changed source data before querying.
    Refresh,
}
