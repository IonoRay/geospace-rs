mod driver_reads;

use std::path::Path;

use ionoray_core::Epoch;
use ionoray_store::{CheckMode, ReadGuard, Store, StoreRoot, StoreScope};
use serde::{Deserialize, Serialize};
use tracing::Instrument;

use crate::{
    Ae, Dst, F107, GeomagneticIndices, GeophysicalIndices, IndexDataset, IndexError, IndexSample,
    IriF107Indices, IriIndices, IriMonthlyIndices, RangeRequest, RangeSyncReport, SyncPolicy,
    SyncReport, VerifyReport, YearDownloadReport, YearQueryReport,
    database::open_existing_year,
    download::{download_year, query_year},
    maintenance::{sync_year, verify_year, year_is_ready},
    query::{
        read_ae, read_daily_ap, read_dst, read_f107, read_f107a, read_geomagnetic_indices,
        read_latest,
    },
    query_iri::{
        combine as combine_iri, read_f107 as read_iri_f107, read_monthly as read_iri_monthly,
    },
    range_sync::sync_range,
};

/// Entry point for independently maintaining and querying geophysical indices.
pub struct IndexStore {
    store: StoreRoot,
}

struct GuardedYearDatabase {
    database: turso::Database,
    _guard: ReadGuard,
}

/// Result of guaranteeing local yearly coverage.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EnsureReport {
    /// Requested year.
    pub year: u16,
    /// Whether complete local data already existed without network access.
    pub already_ready: bool,
    /// Synchronization performed for missing local data.
    pub sync: Option<SyncReport>,
}

impl IndexStore {
    /// Opens the scope-free root; datasets are initialized on first use.
    ///
    /// # Errors
    /// Returns [`IndexError`] when the home or object catalog cannot be opened.
    pub fn open(explicit_home: Option<&Path>) -> std::future::Ready<Result<Self, IndexError>> {
        std::future::ready(
            Store::open_root(explicit_home)
                .map(|store| Self { store })
                .map_err(IndexError::from),
        )
    }

    /// Synchronizes exactly the requested dataset fields and UTC interval.
    ///
    /// A successful [`RangeSyncReport`] may be partial; inspect its `gaps`
    /// before treating the requested model input as usable.
    ///
    /// # Errors
    /// Returns [`IndexError`] for invalid requests or local infrastructure
    /// failures. Individual unavailable source partitions are reported as
    /// partial coverage.
    pub async fn sync_range(&self, request: RangeRequest) -> Result<RangeSyncReport, IndexError> {
        Box::pin(sync_range(&self.store, request)).await
    }

    /// Queries every required upstream source without downloading complete bodies.
    ///
    /// # Errors
    /// Returns [`IndexError`] for HTTP or persistence failures.
    pub async fn query_year(&self, year: u16) -> Result<YearQueryReport, IndexError> {
        query_year(&self.store, year).await
    }

    /// Downloads and validates raw yearly source files without importing values.
    ///
    /// # Errors
    /// Returns [`IndexError`] for download, integrity, or format failures.
    pub async fn download_year(&self, year: u16) -> Result<YearDownloadReport, IndexError> {
        download_year(&self.store, year, CheckMode::Metadata).await
    }

    /// Redownloads every raw body and validates its content hash.
    ///
    /// # Errors
    /// Returns [`IndexError`] for download, integrity, or format failures.
    pub async fn force_download_year(&self, year: u16) -> Result<YearDownloadReport, IndexError> {
        download_year(&self.store, year, CheckMode::ForceContent).await
    }

    /// Queries upstream, downloads changed bodies, and imports ready releases.
    ///
    /// # Errors
    /// Returns [`IndexError`] when any synchronization phase fails.
    pub async fn sync_year(&self, year: u16, policy: SyncPolicy) -> Result<SyncReport, IndexError> {
        sync_year(&self.store, year, policy).await
    }

    /// Guarantees local coverage, synchronizing only when local data is incomplete.
    ///
    /// # Errors
    /// Returns [`IndexError`] when local validation or required synchronization fails.
    pub async fn ensure_year(&self, year: u16) -> Result<EnsureReport, IndexError> {
        if year_is_ready(&self.store, year).await? {
            return Ok(EnsureReport {
                year,
                already_ready: true,
                sync: None,
            });
        }
        Ok(EnsureReport {
            year,
            already_ready: false,
            sync: Some(sync_year(&self.store, year, SyncPolicy::AlwaysCheck).await?),
        })
    }

    /// Verifies CAS bytes, origins, releases, and yearly coverage without networking.
    ///
    /// # Errors
    /// Returns [`IndexError`] when local content is missing or inconsistent.
    pub async fn verify_year(&self, year: u16) -> Result<VerifyReport, IndexError> {
        verify_year(&self.store, year).await
    }

    /// Reimports every matching local CAS artifact without accessing the network.
    ///
    /// # Errors
    /// Returns [`IndexError`] when CAS content cannot be validated or imported.
    pub async fn reindex_year(&self, year: u16) -> Result<SyncReport, IndexError> {
        sync_year(&self.store, year, SyncPolicy::Offline).await
    }

    /// Repairs missing or corrupt local data by forcing complete remote downloads.
    ///
    /// # Errors
    /// Returns [`IndexError`] when remote recovery or import fails.
    pub async fn repair_year(&self, year: u16) -> Result<SyncReport, IndexError> {
        sync_year(&self.store, year, SyncPolicy::ForceDownload).await
    }

    /// Reads the latest deterministic release of every supported index at an epoch.
    ///
    /// # Errors
    /// Returns [`IndexError`] when yearly data or a required value is unavailable.
    #[tracing::instrument(
        name = "indices.read",
        level = "debug",
        skip(self),
        fields(epoch = %epoch),
        err(level = "warn")
    )]
    pub async fn at(&self, epoch: Epoch) -> Result<GeophysicalIndices, IndexError> {
        let (year, ..) = epoch.to_gregorian_utc();
        let year = u16::try_from(year).map_err(|_| IndexError::InvalidNumber("query year"))?;
        let gfz = self.open_existing(IndexDataset::KpApF107, year).await?;
        let dst = self.open_existing(IndexDataset::Dst, year).await?;
        let ae = self.open_existing(IndexDataset::Ae, year).await?;
        read_latest(&gfz.database, &dst.database, &ae.database, epoch).await
    }

    /// Reads GFZ three-hour Kp and ap without requiring daily F10.7 values.
    ///
    /// # Errors
    /// Returns [`IndexError`] when yearly GFZ data or the interval is unavailable.
    pub async fn geomagnetic_at(&self, epoch: Epoch) -> Result<GeomagneticIndices, IndexError> {
        crate::read_trace::read("geomagnetic", async {
            let gfz = self.open_gfz_at(epoch).await?;
            read_geomagnetic_indices(&gfz.database, epoch).await
        })
        .await
    }

    /// Reads hourly Dst at an epoch.
    ///
    /// # Errors
    /// Returns [`IndexError`] when yearly Dst data or the hour is unavailable.
    pub async fn dst_at(&self, epoch: Epoch) -> Result<IndexSample<Dst>, IndexError> {
        crate::read_trace::read("dst", async {
            let database = self.open_at(IndexDataset::Dst, epoch).await?;
            read_dst(&database.database, epoch).await
        })
        .await
    }

    /// Reads hourly AE at an epoch.
    ///
    /// # Errors
    /// Returns [`IndexError`] when yearly AE data or the hour is unavailable.
    pub async fn ae_at(&self, epoch: Epoch) -> Result<IndexSample<Ae>, IndexError> {
        crate::read_trace::read("ae", async {
            let database = self.open_at(IndexDataset::Ae, epoch).await?;
            read_ae(&database.database, epoch).await
        })
        .await
    }

    /// Reads daily Ap without requiring solar-flux values.
    ///
    /// # Errors
    /// Returns [`IndexError`] when yearly GFZ data or the day is unavailable.
    pub async fn daily_ap_at(&self, epoch: Epoch) -> Result<IndexSample<crate::Ap>, IndexError> {
        crate::read_trace::read("daily_ap", async {
            let gfz = self.open_gfz_at(epoch).await?;
            read_daily_ap(&gfz.database, epoch).await
        })
        .await
    }

    /// Reads observed daily F10.7 at an epoch.
    ///
    /// # Errors
    /// Returns [`IndexError`] when yearly GFZ data or the value is unavailable.
    pub async fn f107_at(&self, epoch: Epoch) -> Result<IndexSample<F107>, IndexError> {
        crate::read_trace::read("f107", async {
            let gfz = self.open_gfz_at(epoch).await?;
            read_f107(&gfz.database, epoch).await
        })
        .await
    }

    /// Reads the centered 81-day observed F10.7 average at an epoch.
    ///
    /// # Errors
    /// Returns [`IndexError`] when yearly GFZ data or the value is unavailable.
    pub async fn f107a_at(&self, epoch: Epoch) -> Result<IndexSample<F107>, IndexError> {
        crate::read_trace::read("f107a", async {
            let gfz = self.open_gfz_at(epoch).await?;
            read_f107a(&gfz.database, epoch).await
        })
        .await
    }

    /// Reads interpolated IRI IG12 and Rz12 drivers at an epoch.
    ///
    /// # Errors
    /// Returns [`IndexError`] when the official monthly driver data is unavailable.
    pub async fn iri_monthly_at(&self, epoch: Epoch) -> Result<IriMonthlyIndices, IndexError> {
        crate::read_trace::read("iri_monthly", async {
            let database = self.open_at(IndexDataset::IriIgRz, epoch).await?;
            read_iri_monthly(&database.database, epoch).await
        })
        .await
    }

    /// Reads daily and centered 81-day/365-day adjusted IRI F10.7 drivers.
    ///
    /// # Errors
    /// Returns [`IndexError`] when the official daily driver data is unavailable.
    pub async fn iri_f107_at(&self, epoch: Epoch) -> Result<IriF107Indices, IndexError> {
        crate::read_trace::read("iri_f107", async {
            let database = self.open_at(IndexDataset::IriApF107, epoch).await?;
            read_iri_f107(&database.database, epoch).await
        })
        .await
    }

    /// Reads the complete official IRI point-driver set at an epoch.
    ///
    /// # Errors
    /// Returns [`IndexError`] when either monthly or daily driver data is unavailable.
    pub async fn iri_at(&self, epoch: Epoch) -> Result<IriIndices, IndexError> {
        let monthly = self.iri_monthly_at(epoch).await?;
        let f107 = self.iri_f107_at(epoch).await?;
        Ok(combine_iri(monthly, f107))
    }

    async fn open_gfz_at(&self, epoch: Epoch) -> Result<GuardedYearDatabase, IndexError> {
        let (year, ..) = epoch.to_gregorian_utc();
        let year = u16::try_from(year).map_err(|_| IndexError::InvalidNumber("query year"))?;
        self.open_existing(IndexDataset::KpApF107, year).await
    }

    async fn open_at(
        &self,
        dataset: IndexDataset,
        epoch: Epoch,
    ) -> Result<GuardedYearDatabase, IndexError> {
        let (year, ..) = epoch.to_gregorian_utc();
        let year = u16::try_from(year).map_err(|_| IndexError::InvalidNumber("query year"))?;
        self.open_existing(dataset, year).await
    }

    async fn open_existing(
        &self,
        dataset: IndexDataset,
        year: u16,
    ) -> Result<GuardedYearDatabase, IndexError> {
        let span = tracing::debug_span!(
            "index.open",
            dataset = ?dataset,
            year,
            elapsed_us = tracing::field::Empty,
        );
        let started = std::time::Instant::now();
        let scoped = self
            .store
            .scoped(StoreScope::new(dataset.scope_name())?)
            .await?;
        let guard = scoped.try_acquire_read_guard()?;
        let result = open_existing_year(
            self.store
                .layout()
                .index_database(dataset.provider(), dataset.as_str(), year),
            dataset,
            year,
        )
        .instrument(span.clone())
        .await;
        span.record(
            "elapsed_us",
            u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
        );
        result.map(|database| GuardedYearDatabase {
            database: database.database,
            _guard: guard,
        })
    }
}

#[cfg(test)]
mod tests;
