//! Turso-native local storage, schema rebuild, and download infrastructure.

mod accepted;
mod cas;
mod database;
mod download;
mod error;
mod finalize;
mod home;
mod layout;
mod maintenance;
mod objects;
mod prune;
mod scope;
mod scope_maintenance;
mod store;
mod time;
mod view;

pub use database::{DatabaseKind, DatabaseStatus};
pub use download::{
    ArtifactRef, CheckMode, DownloadDisposition, DownloadError, DownloadManager,
    DownloadObservation, DownloadOutcome, DownloadRequest, ObservationStatus, RemoteQueryOutcome,
    RemoteState, SourceContext,
};
pub use error::StoreError;
pub use home::StoreHome;
pub use layout::StoreLayout;
pub use maintenance::{MaintenanceRun, MaintenanceSummary};
pub use objects::{ArtifactOrigin, CatalogArtifact};
pub use prune::PruneReport;
pub use scope::{AcceptedArtifact, ReadGuard, ScopedStore, StoreScope, SyncGuard};
pub use store::{Store, StoreInitReport, StoreRoot, StoreStatus};
