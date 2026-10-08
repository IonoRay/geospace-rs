use super::model::Failure;
use crate::GeospaceError as Error;
#[cfg(feature = "hwm")]
use crate::hwm::HwmError;
#[cfg(feature = "igrf")]
use crate::igrf::IgrfError;
#[cfg(feature = "iri")]
use crate::iri::IriError;
#[cfg(feature = "msis")]
use crate::msis::MsisError;
#[cfg(feature = "indices")]
use ionoray_indices::IndexError;

pub(super) fn failure(error: &Error) -> Failure {
    let code = match error {
        Error::Position(_) | Error::InvalidEpoch(_) | Error::InvalidYear(_) => "invalid_request",
        #[cfg(feature = "igrf")]
        Error::Igrf(IgrfError::EpochOutOfRange(_) | IgrfError::AltitudeOutOfRange(_)) => {
            "invalid_request"
        }
        #[cfg(feature = "iri")]
        Error::Iri(
            IriError::InvalidDriver { .. }
            | IriError::AltitudeOutOfRange(_)
            | IriError::EpochOutOfRange(_)
            | IriError::InputOutOfRange { .. },
        ) => "invalid_request",
        #[cfg(feature = "hwm")]
        Error::Hwm(
            HwmError::InvalidAp(_)
            | HwmError::AltitudeBelowSurface(_)
            | HwmError::InputOutOfRange { .. },
        ) => "invalid_request",
        #[cfg(feature = "msis")]
        Error::Msis(MsisError::InvalidDriver { .. } | MsisError::AltitudeBelowSurface(_)) => {
            return ("invalid_request", error.to_string());
        }
        #[cfg(feature = "indices")]
        Error::PartialIndices(report)
            if report.source_check_status == ionoray_indices::SourceCheckStatus::Failed =>
        {
            "data_refresh_failed"
        }
        #[cfg(feature = "indices")]
        Error::PartialIndices(_)
        | Error::Indices(
            IndexError::MissingYearData { .. }
            | IndexError::MissingValue { .. }
            | IndexError::UnsupportedYear { .. }
            | IndexError::SourceUnavailable { .. },
        ) => "data_unavailable",
        Error::Store(_) => "data_access",
        #[cfg(feature = "indices")]
        Error::Indices(_) => "data_access",
        #[cfg(feature = "iri")]
        Error::Iri(IriError::BackendUnavailable) => "model_unavailable",
        #[cfg(feature = "hwm")]
        Error::Hwm(HwmError::BackendUnavailable) => "model_unavailable",
        #[cfg(feature = "msis")]
        Error::Msis(MsisError::BackendUnavailable) => "model_unavailable",
        #[cfg(feature = "igrf")]
        Error::Igrf(_) => "model_failed",
        #[cfg(feature = "iri")]
        Error::Iri(_) => "model_failed",
        #[cfg(feature = "hwm")]
        Error::Hwm(_) => "model_failed",
        #[cfg(feature = "msis")]
        Error::Msis(_) => "model_failed",
        _ => "internal_error",
    };
    (code, error.to_string())
}

#[cfg(all(test, feature = "indices"))]
mod tests {
    use super::*;
    use ionoray_core::{Duration, Epoch};
    use ionoray_indices::{
        IndexDataset, IndexField, IndexStore, RangeRequest, RangeSyncStatus, SourceCheckStatus,
        SyncMode,
    };

    #[tokio::test]
    async fn partial_reports_keep_gap_and_refresh_failure_codes_distinct() {
        let home = tempfile::tempdir().unwrap();
        let store = IndexStore::open(Some(home.path())).await.unwrap();
        let start = "2020-07-01T12:00:00 UTC".parse::<Epoch>().unwrap();
        let gap = store
            .sync_range(RangeRequest {
                dataset: IndexDataset::Dst,
                fields: vec![IndexField::Dst],
                start,
                end: start + Duration::from_milliseconds(1.0),
                mode: SyncMode::Offline,
                force: false,
            })
            .await
            .unwrap();
        assert_eq!(
            failure(&Error::PartialIndices(Box::new(gap))).0,
            "data_unavailable"
        );

        let mut refreshed = store
            .sync_range(RangeRequest {
                dataset: IndexDataset::KpApF107,
                fields: vec![IndexField::Ap3h],
                start,
                end: start + Duration::from_milliseconds(1.0),
                mode: SyncMode::Offline,
                force: false,
            })
            .await
            .unwrap();
        assert_eq!(refreshed.coverage_status, RangeSyncStatus::Complete);
        // Synthetic failed refresh layered on real complete local coverage.
        refreshed.source_check_status = SourceCheckStatus::Failed;
        refreshed.status = RangeSyncStatus::Partial;
        assert_eq!(
            failure(&Error::PartialIndices(Box::new(refreshed))).0,
            "data_refresh_failed"
        );
    }
}
