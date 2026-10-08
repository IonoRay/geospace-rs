use serde::{Deserialize, Serialize};

use crate::Sha256Digest;

/// Provenance attached to one immutable data selection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DataProvenance {
    /// Stable dataset release identifier.
    pub release_id: String,
    /// Hash of the canonical release manifest.
    pub manifest: Sha256Digest,
    /// Hash of the complete resolved snapshot.
    pub snapshot: Sha256Digest,
    /// Whether an explicit fallback policy selected this release.
    pub fallback_used: bool,
}

/// Provenance attached to one model implementation and asset release.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ModelProvenance {
    /// Model family, such as `igrf` or `msis`.
    pub family: String,
    /// Scientific model version, distinct from the crate version.
    pub model_version: String,
    /// Rust crate version that supplied the implementation.
    pub implementation_version: String,
    /// Content identity of model coefficients or static assets.
    pub asset: Sha256Digest,
}
