use serde::{Deserialize, Serialize};

/// Deterministic model version selection policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum VersionPolicy<V> {
    /// Resolve using the crate's local, versioned selection table.
    Auto,
    /// Require one exact scientific model version.
    Exact(V),
}
