//! Field reads used by automatic model preparation.
use super::{Epoch, IndexDataset, IndexError, IndexSample, IndexStore};
use crate::{AdjustedF107, Ap, Ig12, Rz12};

impl IndexStore {
    /// Reads three-hour ap without requiring Kp.
    ///
    /// # Errors
    /// Returns [`IndexError`] when the requested sample or its provenance is unavailable.
    pub async fn ap_at(&self, epoch: Epoch) -> Result<IndexSample<Ap>, IndexError> {
        crate::read_trace::read("ap", async {
            let database = self.open_at(IndexDataset::KpApF107, epoch).await?;
            crate::query::read_ap(&database.database, epoch).await
        })
        .await
    }

    /// Reads IRI Rz12 without requiring IG12.
    ///
    /// # Errors
    /// Returns [`IndexError`] when the requested sample or its provenance is unavailable.
    pub async fn iri_rz12_at(&self, epoch: Epoch) -> Result<IndexSample<Rz12>, IndexError> {
        crate::read_trace::read("iri_rz12", async {
            let database = self.open_at(IndexDataset::IriIgRz, epoch).await?;
            crate::query_iri::read_rz12(&database.database, epoch).await
        })
        .await
    }

    /// Reads IRI IG12 without requiring Rz12.
    ///
    /// # Errors
    /// Returns [`IndexError`] when the requested sample or its provenance is unavailable.
    pub async fn iri_ig12_at(&self, epoch: Epoch) -> Result<IndexSample<Ig12>, IndexError> {
        crate::read_trace::read("iri_ig12", async {
            let database = self.open_at(IndexDataset::IriIgRz, epoch).await?;
            crate::query_iri::read_ig12(&database.database, epoch).await
        })
        .await
    }

    /// Reads IRI daily adjusted F10.7 only.
    ///
    /// # Errors
    /// Returns [`IndexError`] when the requested sample or its provenance is unavailable.
    pub async fn iri_f107_daily_at(
        &self,
        epoch: Epoch,
    ) -> Result<IndexSample<AdjustedF107>, IndexError> {
        crate::read_trace::read("iri_f107_daily", async {
            let database = self.open_at(IndexDataset::IriApF107, epoch).await?;
            crate::query_iri::read_f107_daily(&database.database, epoch).await
        })
        .await
    }

    /// Reads IRI centered 81-day adjusted F10.7 only.
    ///
    /// # Errors
    /// Returns [`IndexError`] when the requested sample or its provenance is unavailable.
    pub async fn iri_f107_81_day_at(
        &self,
        epoch: Epoch,
    ) -> Result<IndexSample<AdjustedF107>, IndexError> {
        crate::read_trace::read("iri_f107_81_day", async {
            let database = self.open_at(IndexDataset::IriApF107, epoch).await?;
            crate::query_iri::read_f107_81_day(&database.database, epoch).await
        })
        .await
    }
}
