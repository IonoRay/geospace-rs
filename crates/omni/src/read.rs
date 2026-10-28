use std::{collections::BTreeMap, fs::File, io::Read};

use ionoray_store::ArtifactRef;
use sha2::{Digest, Sha256};

use crate::{
    CoverageReport, OmniError, OmniProduct, OmniRange, OmniRecord, OmniStore, ReadReport,
    SourceCheck, VariableCoverage, parse, range::epoch, selection::selection_report,
    store::missing_month,
};

impl OmniStore {
    /// Reads only accepted local CDFs; never performs HTTP or substitutes samples.
    /// Streams records to `emit` and returns provenance and a coverage report.
    /// `None` selects local HRO2 first, then local HRO per month.
    /// # Errors
    /// Missing months, corrupt objects, CDF failures, or sink errors fail the read.
    /// A sink may already have received earlier months; only a successful return
    /// certifies completion of the requested read.
    pub async fn read(
        &self,
        range: OmniRange,
        product: Option<OmniProduct>,
        mut emit: impl FnMut(&OmniRecord) -> Result<(), OmniError>,
    ) -> Result<ReadReport, OmniError> {
        let _guard = self.scoped.try_acquire_read_guard()?;
        let mut files = Vec::new();
        let mut selections = Vec::new();
        let mut variables: BTreeMap<String, VariableCoverage> = parse::VARIABLES
            .iter()
            .map(|&name| (name.into(), VariableCoverage::default()))
            .collect();
        let mut expected_next = range.start.div_euclid(60_000) * 60_000;
        if expected_next < range.start {
            expected_next += 60_000;
        }
        let expected_records =
            usize::try_from(((range.end - expected_next).max(0) + 59_999) / 60_000)
                .map_err(|e| OmniError::Invalid(e.to_string()))?;
        let mut records = 0;
        let mut missing_times = Vec::new();
        // Resolve every month first so missing local files cannot produce partial stdout.
        let mut selected = Vec::new();
        for month in range.months() {
            let source = self
                .select_month(month, product)
                .await?
                .ok_or_else(|| missing_month(month, product))?;
            selected.push((month, source));
        }
        for (month, selected) in selected {
            let identity = &selected.identity;
            verify(&selected.source.artifact)?;
            let cdf = parse::decode(&selected.source.artifact.path, identity.product, month)?;
            let times = parse::times(&cdf)?;
            let columns = parse::VARIABLES
                .iter()
                .map(|&name| {
                    let var = parse::variable(&cdf, name)?;
                    Ok((name, var, parse::scalar_values(var)?))
                })
                .collect::<Result<Vec<_>, OmniError>>()?;
            for (record_index, &time) in times.iter().enumerate() {
                let unix = parse::unix_millis(time)?;
                if unix < range.start || unix >= range.end {
                    continue;
                }
                if unix > expected_next {
                    missing_times.push([expected_next, unix]);
                }
                expected_next = unix + 60_000;
                let mut values = BTreeMap::new();
                for (name, var, data) in &columns {
                    let sample = parse::sample(&cdf, var, data[record_index]);
                    let coverage = variables.entry((*name).into()).or_default();
                    coverage.missing += usize::from(sample.missing.is_some());
                    coverage.outside_valid_range += usize::from(sample.outside_valid_range);
                    values.insert((*name).into(), sample);
                }
                emit(&OmniRecord {
                    utc: epoch(unix).to_rfc3339(),
                    cdf_epoch_ms: time,
                    product: identity.product,
                    cadence: identity.cadence,
                    partition: identity.partition.clone(),
                    file_version: identity.file_version.clone(),
                    filename: identity.filename.clone(),
                    sha256: identity.sha256.to_string(),
                    record_index,
                    values,
                })?;
                records += 1;
            }
            selections.push(selection_report(
                identity,
                None,
                product,
                SourceCheck::OfflineNotChecked,
            ));
            files.push(selected.file_report(&cdf, None)?);
        }
        if expected_next < range.end {
            missing_times.push([expected_next, range.end]);
        }
        Ok(ReadReport {
            range,
            files,
            selections,
            coverage: CoverageReport {
                expected_records,
                records,
                missing_time_ranges_utc_ms: missing_times,
                variables,
            },
            propagation: "OMNI upstream time-shifted to Earth's bow-shock nose; no additional shift applied",
        })
    }
}

pub(crate) fn verify(artifact: &ArtifactRef) -> Result<(), OmniError> {
    if !std::fs::symlink_metadata(&artifact.path)?
        .file_type()
        .is_file()
    {
        return Err(OmniError::Invalid(format!(
            "CAS is not a regular file: {}",
            artifact.path.display()
        )));
    }
    let mut file = File::open(&artifact.path)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0_u8; 65_536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    let actual = ionoray_core::Sha256Digest::from_bytes(hash.finalize().into());
    if actual != artifact.digest {
        return Err(OmniError::Invalid(format!(
            "CAS SHA-256 mismatch: {}",
            artifact.path.display()
        )));
    }
    Ok(())
}
