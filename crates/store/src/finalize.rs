use crate::{ScopedStore, StoreError};

impl ScopedStore {
    /// Finalizes raw retention after the caller atomically applies a candidate.
    ///
    /// A revision promotes the pending predecessor into the sole raw history;
    /// an append retains the existing history and clears the pending marker.
    ///
    /// # Errors
    /// Returns an error when the scoped catalog cannot be updated.
    pub async fn finalize_acceptance(
        &self,
        source_id: &str,
        revision: bool,
    ) -> Result<(), StoreError> {
        self.catalog.connect()?.execute(
            "UPDATE accepted_source SET history_sha256 = CASE WHEN ? THEN COALESCE(pending_previous_sha256, history_sha256) ELSE history_sha256 END, pending_previous_sha256 = NULL WHERE source_id = ?",
            turso::params![revision, source_id],
        ).await?;
        Ok(())
    }
}
