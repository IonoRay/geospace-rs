use std::{collections::BTreeMap, fs::File, io::Read};

use ionoray_store::CatalogArtifact;
use sha2::{Digest, Sha256};

use super::{
    ActivityCoverage, ActivityDataset, ActivityError, ActivityFile, ActivityRange, ActivityRecord,
    ActivityReport, parse, range::epoch, source::Slot, store::OnlineState,
};

pub(super) fn read_selected(
    dataset: ActivityDataset,
    range: ActivityRange,
    slots: &[Slot],
    accepted: &BTreeMap<String, CatalogArtifact>,
    mut states: BTreeMap<String, OnlineState>,
    unavailable_sources: Vec<String>,
    mut emit: impl FnMut(&ActivityRecord) -> Result<(), ActivityError>,
) -> Result<ActivityReport, ActivityError> {
    let mut per_day = BTreeMap::new();
    let mut files = BTreeMap::new();
    for (day, candidates) in slots {
        let selected = candidates
            .iter()
            .find_map(|s| accepted.get(&s.id))
            .ok_or_else(|| ActivityError::Missing {
                dataset: dataset.name(),
                partition: format!("{:04}-{:02}-{:02}", day.0, day.1, day.2),
            })?;
        per_day.insert(*day, selected.source_id.clone());
        files.insert(selected.source_id.clone(), selected);
    }
    let mut reports = Vec::new();
    let mut records = BTreeMap::new();
    for (source_id, source) in files {
        verify(source)?;
        let parsed = parse::parse(&source.artifact.path, dataset, range, &source.edition)?;
        for row in parsed.rows {
            let (year, month, day, ..) = epoch(row.time).to_gregorian_utc();
            let key = (
                u16::try_from(year).map_err(|e| ActivityError::Invalid(e.to_string()))?,
                month,
                day,
            );
            if per_day.get(&key) == Some(&source_id) {
                records.insert(
                    row.time,
                    (row, source_id.clone(), source.artifact.digest.to_string()),
                );
            }
        }
        let state = states.remove(&source_id).unwrap_or(OnlineState {
            previous_sha256: None,
            change: None,
            source_check: "offline_not_checked",
            browse_path: None,
        });
        reports.push(file_report(source_id, source, state, parsed.header));
    }
    let step = dataset.cadence_seconds() * 1000;
    let mut next = range.start_utc_ms.div_euclid(step) * step;
    if next < range.start_utc_ms {
        next += step;
    }
    let expected_records = usize::try_from(((range.end_utc_ms - next).max(0) + step - 1) / step)
        .map_err(|e| ActivityError::Invalid(e.to_string()))?;
    let count = records.len();
    let mut missing_time_ranges_utc_ms = Vec::new();
    let mut missing_values: BTreeMap<String, usize> =
        dataset.fields().iter().map(|&s| (s.into(), 0)).collect();
    for (time, (row, source_id, sha256)) in records {
        if time > next {
            missing_time_ranges_utc_ms.push([next, time]);
        }
        next = time + step;
        for (name, value) in &row.values {
            *missing_values.entry(name.clone()).or_default() +=
                usize::from(value.missing.is_some());
        }
        emit(&ActivityRecord {
            utc: epoch(time).to_rfc3339(),
            end_utc: epoch(time + step).to_rfc3339(),
            dataset,
            cadence_seconds: dataset.cadence_seconds(),
            quality: row.quality,
            source_flags: row.flags,
            units: if matches!(dataset, ActivityDataset::Hp30 | ActivityDataset::Hp60) {
                "1"
            } else {
                "mV/m"
            },
            values: row.values,
            source_id,
            sha256,
            source_line: row.line,
        })?;
    }
    if next < range.end_utc_ms {
        missing_time_ranges_utc_ms.push([next, range.end_utc_ms]);
    }
    Ok(ActivityReport {
        dataset,
        range,
        files: reports,
        unavailable_sources,
        coverage: ActivityCoverage {
            expected_records,
            records: count,
            missing_time_ranges_utc_ms,
            missing_values,
        },
    })
}

fn file_report(
    source_id: String,
    source: &CatalogArtifact,
    state: OnlineState,
    header: Vec<String>,
) -> ActivityFile {
    ActivityFile {
        source_id,
        edition: source.edition.clone(),
        source_url: source.canonical_url.clone(),
        artifact: source.artifact.clone(),
        browse_path: state.browse_path,
        previous_sha256: state.previous_sha256,
        change: state.change,
        source_check: state.source_check,
        header,
    }
}

pub(super) fn verify(source: &CatalogArtifact) -> Result<(), ActivityError> {
    let path = &source.artifact.path;
    if !std::fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(ActivityError::Invalid(format!(
            "CAS is not a regular file: {}",
            path.display()
        )));
    }
    let mut file = File::open(path)?;
    let mut buffer = vec![0; 65_536];
    let mut hash = Sha256::new();
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    if ionoray_core::Sha256Digest::from_bytes(hash.finalize().into()) != source.artifact.digest {
        return Err(ActivityError::Invalid(format!(
            "CAS hash mismatch: {}",
            path.display()
        )));
    }
    Ok(())
}
