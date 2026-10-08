use crate::{MaintenanceRun, MaintenanceSummary, ScopedStore, StoreError, maintenance};

impl ScopedStore {
    /// Begins an auditable maintenance operation in this dataset catalog.
    ///
    /// # Errors
    /// Returns an error when the audit record cannot be persisted.
    pub async fn begin_maintenance(
        &self,
        operation: &str,
        policy: &str,
        year: Option<u16>,
    ) -> Result<MaintenanceRun, StoreError> {
        maintenance::begin(&self.catalog, operation, policy, year).await
    }
    /// Marks a scoped maintenance operation successful.
    ///
    /// # Errors
    /// Returns an error when the audit record cannot be updated.
    pub async fn finish_maintenance(
        &self,
        run: &MaintenanceRun,
        summary: MaintenanceSummary,
    ) -> Result<(), StoreError> {
        maintenance::finish(&self.catalog, run, summary).await
    }
    /// Records a scoped maintenance failure.
    ///
    /// # Errors
    /// Returns an error when the audit failure cannot be persisted.
    pub async fn fail_maintenance(
        &self,
        run: &MaintenanceRun,
        kind: &str,
        message: &str,
    ) -> Result<(), StoreError> {
        maintenance::fail(&self.catalog, run, kind, message).await
    }
}
