mod baseline;
mod error;
mod http;
mod ingest;
mod model;
mod query;
mod repository;
mod service;

#[cfg(test)]
mod contract_tests;
#[cfg(test)]
mod tests;

pub use error::DownloadError;
pub use model::{
    ArtifactRef, CheckMode, DownloadDisposition, DownloadObservation, DownloadOutcome,
    DownloadRequest, ObservationStatus, RemoteQueryOutcome, RemoteState, SourceContext,
};
pub use service::DownloadManager;
