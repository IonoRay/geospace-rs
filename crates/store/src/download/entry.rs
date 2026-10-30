use super::{CheckMode, DownloadError, DownloadManager, DownloadOutcome, DownloadRequest};

impl DownloadManager<'_> {
    /// Checks a source with [`CheckMode::Metadata`] and commits changed bytes.
    /// # Errors
    /// Returns [`DownloadError`] for remote, local, or persistence failures.
    pub async fn download(
        &self,
        request: &DownloadRequest,
    ) -> Result<DownloadOutcome, DownloadError> {
        self.check(request, CheckMode::Metadata).await
    }

    /// Checks a source using the selected consistency depth.
    /// [`CheckMode::ForceContent`] hashes the complete local object and performs
    /// an unconditional GET so the remote body is compared by SHA-256.
    /// # Errors
    /// Returns [`DownloadError`] for HTTP, filesystem, integrity, or persistence
    /// failures. Warnings include source metadata even when debug spans are hidden.
    #[tracing::instrument(
        name = "download.check",
        level = "debug",
        skip_all,
        fields(source_id = request.source_id(), url = %request.url(), mode = ?mode)
    )]
    pub async fn check(
        &self,
        request: &DownloadRequest,
        mode: CheckMode,
    ) -> Result<DownloadOutcome, DownloadError> {
        let result = self.check_inner(request, mode).await;
        if let Err(error) = &result {
            let context = request.context();
            tracing::warn!(
                source_id = request.source_id(),
                url = %request.url(),
                logical_name = request.logical_name(),
                provider = context.map(|c| c.provider.as_str()),
                dataset = context.map(|c| c.dataset.as_str()),
                year = context.map(|c| c.year),
                month = context.and_then(|c| c.month),
                day = context.and_then(|c| c.day),
                edition = context.map(|c| c.edition.as_str()),
                mode = mode.as_str(),
                error = %error,
                "download check failed"
            );
        }
        result
    }
}
