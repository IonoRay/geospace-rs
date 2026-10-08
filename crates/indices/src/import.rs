use std::collections::{BTreeMap, HashSet};

use ionoray_core::{Epoch, Sha256Digest};
use sha2::{Digest, Sha256};
use turso::{Database, params};

use crate::history::{ChangeKind, SampleFingerprint, compare, fingerprints_for_edition};
use crate::{
    IndexError, ValidatedIndexFile,
    database::{YearDatabase, checkpoint, now_ms},
    parse::ParsedFile,
    time::epoch_millis,
};

mod applied;
pub(crate) use applied::is_applied;
use applied::{mark_applied, upsert_applied};
mod changes;
use changes::activate;
mod records;
use records::{copy_active_records, insert_records};

const PARSER_NAME: &str = "ionoray-indices";
/// Increment only when parsed sample semantics or declared-missing handling changes.
pub(crate) const PARSER_VERSION: &str = "indices-semantic-v3-explicit-missing";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ImportSummary {
    pub(crate) releases_created: usize,
    pub(crate) releases_reused: usize,
    pub(crate) records: usize,
    pub(crate) changes: usize,
    pub(crate) initial: usize,
    pub(crate) append: usize,
    pub(crate) backfill: usize,
    pub(crate) revision: usize,
    pub(crate) historical_revision: bool,
}

/// Imports a validation-time parse result without reopening or reparsing its artifact.
pub(crate) async fn import_parsed_file(
    year_database: &YearDatabase,
    file: &ValidatedIndexFile,
    parsed: ParsedFile,
) -> Result<ImportSummary, IndexError> {
    if is_applied(
        year_database,
        &file.source_id,
        file.download.artifact.digest,
    )
    .await?
    {
        return Ok(ImportSummary {
            releases_reused: 1,
            ..ImportSummary::default()
        });
    }
    let release_id = release_id(file);
    let candidate = fingerprints_for_edition(&parsed, file.edition);
    let partition = partition_key(file.month);
    let active = active_release(&year_database.database, &partition).await?;
    if release_exists(&year_database.database, &release_id).await? {
        let summary =
            reactivate_existing(year_database, &partition, &release_id, active.as_deref()).await?;
        mark_applied(year_database, file, &partition, Some(&release_id)).await?;
        checkpoint(&year_database.database, file.dataset, file.year).await?;
        return Ok(summary);
    }
    let previous = active_samples(&year_database.database, active.as_deref()).await?;
    let selection = select_samples(&previous, &candidate, i64::from(file.edition.priority()));
    reject_truncation(file, &selection)?;
    let previous_fingerprints = fingerprints(&previous);
    let change = compare(&previous_fingerprints, &selection.effective);
    if change.kind == ChangeKind::Unchanged {
        mark_applied(year_database, file, &partition, active.as_deref()).await?;
        checkpoint(&year_database.database, file.dataset, file.year).await?;
        return Ok(ImportSummary {
            releases_reused: 1,
            ..ImportSummary::default()
        });
    }
    let records = record_count(&parsed);
    let (coverage_start, coverage_end) = coverage(file.year, file.month)?;
    let mut connection = year_database.database.connect()?;
    let transaction = connection.transaction().await?;
    transaction
        .execute(
            "INSERT INTO dataset_release (release_id, artifact_sha256, source_id, edition, edition_priority, parser_name, parser_version, coverage_start_utc_ms, coverage_end_utc_ms, source_modified_at_utc_ms, fetched_at_utc_ms, imported_at_utc_ms, record_count, state) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'ready') ON CONFLICT(release_id) DO UPDATE SET artifact_sha256=excluded.artifact_sha256, source_id=excluded.source_id, edition=excluded.edition, edition_priority=excluded.edition_priority, parser_name=excluded.parser_name, parser_version=excluded.parser_version, coverage_start_utc_ms=excluded.coverage_start_utc_ms, coverage_end_utc_ms=excluded.coverage_end_utc_ms, source_modified_at_utc_ms=excluded.source_modified_at_utc_ms, fetched_at_utc_ms=excluded.fetched_at_utc_ms, imported_at_utc_ms=excluded.imported_at_utc_ms, record_count=excluded.record_count, state='ready'",
            params![
                release_id.as_str(),
                file.download.artifact.digest.to_string(),
                file.source_id.as_str(),
                file.edition.as_str(),
                i64::from(file.edition.priority()),
                PARSER_NAME,
                PARSER_VERSION,
                coverage_start,
                coverage_end,
                file.download.artifact.source_modified_at_utc_ms,
                file.download.finished_at_utc_ms,
                now_ms(),
                i64::try_from(records).map_err(|_| IndexError::InvalidNumber("record count"))?,
            ],
        )
        .await?;
    if let Some(active) = active.as_deref() {
        copy_active_records(&transaction, file.dataset, active, &release_id).await?;
        copy_active_metadata(&transaction, active, &release_id).await?;
    }
    insert_records(
        &transaction,
        &release_id,
        file.edition,
        parsed,
        &selection.selected,
    )
    .await?;
    insert_candidate_metadata(&transaction, &release_id, &candidate, &selection.selected).await?;
    activate(
        &transaction,
        &partition,
        active.as_deref(),
        &release_id,
        &change,
        &previous_fingerprints,
        &selection.effective,
    )
    .await?;
    upsert_applied(&transaction, file, &partition, Some(&release_id)).await?;
    transaction.commit().await?;
    checkpoint(&year_database.database, file.dataset, file.year).await?;
    let (initial, append, backfill, revision) = change_counts(change.kind);
    Ok(ImportSummary {
        releases_created: 1,
        releases_reused: 0,
        records,
        changes: 1,
        initial,
        append,
        backfill,
        revision,
        historical_revision: matches!(change.kind, ChangeKind::Backfill | ChangeKind::Revision),
    })
}

/// Checks whether a parsed candidate can be accepted without mutating yearly state.
pub(crate) async fn validate_candidate(
    year_database: &YearDatabase,
    file: &ValidatedIndexFile,
    parsed: &ParsedFile,
) -> Result<(), IndexError> {
    let partition = partition_key(file.month);
    let active = active_release(&year_database.database, &partition).await?;
    let previous = active_samples(&year_database.database, active.as_deref()).await?;
    let candidate = fingerprints_for_edition(parsed, file.edition);
    let selection = select_samples(&previous, &candidate, i64::from(file.edition.priority()));
    reject_truncation(file, &selection)
}

async fn release_exists(database: &Database, release_id: &str) -> Result<bool, IndexError> {
    let connection = database.connect()?;
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM dataset_release WHERE release_id = ? AND state = 'ready'",
            [release_id],
        )
        .await?;
    let count = rows.next().await?.map_or(Ok(0), |row| row.get::<i64>(0))?;
    Ok(count == 1)
}

fn partition_key(month: Option<u8>) -> String {
    month.map_or_else(|| "rolling".to_owned(), |month| format!("month-{month:02}"))
}

async fn active_release(
    database: &Database,
    partition: &str,
) -> Result<Option<String>, IndexError> {
    let connection = database.connect()?;
    let mut rows = connection
        .query(
            "SELECT release_id FROM active_release WHERE partition_key = ?",
            [partition],
        )
        .await?;
    rows.next()
        .await?
        .map(|row| row.get(0))
        .transpose()
        .map_err(Into::into)
}

#[derive(Clone, Debug)]
struct ActiveSample {
    fingerprint: SampleFingerprint,
    priority: i64,
}

async fn active_samples(
    database: &Database,
    release_id: Option<&str>,
) -> Result<Vec<ActiveSample>, IndexError> {
    let Some(release_id) = release_id else {
        return Ok(Vec::new());
    };
    let connection = database.connect()?;
    let mut rows = connection.query("SELECT f.sample_key, f.epoch_utc_ms, f.semantic_sha256, f.value_json, source.edition_priority FROM release_sample_fingerprint f JOIN sample_provenance p ON p.release_id = f.release_id AND p.sample_key = f.sample_key JOIN dataset_release source ON source.release_id = p.source_release_id WHERE f.release_id = ? ORDER BY f.sample_key", [release_id]).await?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().await? {
        result.push(ActiveSample {
            fingerprint: SampleFingerprint::from_stored(
                row.get(0)?,
                row.get(1)?,
                row.get::<String>(2)?.parse()?,
                row.get(3)?,
            ),
            priority: row.get(4)?,
        });
    }
    Ok(result)
}

struct Selection {
    effective: Vec<SampleFingerprint>,
    selected: HashSet<String>,
    truncated: Vec<String>,
}

fn select_samples(
    previous: &[ActiveSample],
    candidate: &[SampleFingerprint],
    candidate_priority: i64,
) -> Selection {
    let candidates: BTreeMap<_, _> = candidate
        .iter()
        .map(|value| (value.key.as_str(), value))
        .collect();
    let mut merged: BTreeMap<&str, SampleFingerprint> = BTreeMap::new();
    let mut selected = HashSet::new();
    let mut truncated = Vec::new();
    for old in previous {
        match candidates.get(old.fingerprint.key.as_str()) {
            Some(next)
                if candidate_priority >= old.priority
                    || (old.fingerprint.is_missing() && !next.is_missing()) =>
            {
                merged.insert(next.key.as_str(), (*next).clone());
                selected.insert(next.key.clone());
            }
            Some(_) => {
                merged.insert(old.fingerprint.key.as_str(), old.fingerprint.clone());
            }
            None => {
                if candidate_priority == old.priority {
                    truncated.push(old.fingerprint.key.clone());
                }
                merged.insert(old.fingerprint.key.as_str(), old.fingerprint.clone());
            }
        }
    }
    for next in candidate {
        if !merged.contains_key(next.key.as_str()) {
            merged.insert(next.key.as_str(), next.clone());
            selected.insert(next.key.clone());
        }
    }
    Selection {
        effective: merged.into_values().collect(),
        selected,
        truncated,
    }
}

fn fingerprints(samples: &[ActiveSample]) -> Vec<SampleFingerprint> {
    samples
        .iter()
        .map(|sample| sample.fingerprint.clone())
        .collect()
}

fn reject_truncation(file: &ValidatedIndexFile, selection: &Selection) -> Result<(), IndexError> {
    if selection.truncated.is_empty() {
        return Ok(());
    }
    Err(IndexError::Validation {
        dataset: file.dataset,
        path: file.download.artifact.path.clone(),
        reason: format!(
            "candidate omits {} previously selected samples at the same maturity; refusing possible truncation",
            selection.truncated.len()
        ),
    })
}

async fn copy_active_metadata(
    transaction: &turso::transaction::Transaction<'_>,
    active: &str,
    release: &str,
) -> Result<(), IndexError> {
    transaction.execute("INSERT INTO release_sample_fingerprint (release_id, sample_key, epoch_utc_ms, semantic_sha256, value_json) SELECT ?, sample_key, epoch_utc_ms, semantic_sha256, value_json FROM release_sample_fingerprint WHERE release_id = ?", params![release, active]).await?;
    transaction.execute("INSERT INTO sample_provenance (release_id, sample_key, source_release_id) SELECT ?, sample_key, source_release_id FROM sample_provenance WHERE release_id = ?", params![release, active]).await?;
    Ok(())
}

async fn insert_candidate_metadata(
    transaction: &turso::transaction::Transaction<'_>,
    release: &str,
    samples: &[SampleFingerprint],
    selected: &HashSet<String>,
) -> Result<(), IndexError> {
    for sample in samples {
        if !selected.contains(&sample.key) {
            continue;
        }
        transaction.execute("INSERT OR REPLACE INTO release_sample_fingerprint (release_id, sample_key, epoch_utc_ms, semantic_sha256, value_json) VALUES (?, ?, ?, ?, ?)", params![release, sample.key.clone(), sample.epoch_ms, sample.digest.to_string(), sample.value_json.clone()]).await?;
        transaction.execute("INSERT OR REPLACE INTO sample_provenance (release_id, sample_key, source_release_id) VALUES (?, ?, ?)", params![release, sample.key.clone(), release]).await?;
    }
    Ok(())
}

async fn reactivate_existing(
    year_database: &YearDatabase,
    partition: &str,
    release: &str,
    previous: Option<&str>,
) -> Result<ImportSummary, IndexError> {
    if previous == Some(release) {
        return Ok(ImportSummary {
            releases_reused: 1,
            ..ImportSummary::default()
        });
    }
    let previous_samples = active_samples(&year_database.database, previous).await?;
    let candidate_samples = active_samples(&year_database.database, Some(release)).await?;
    let selection = select_existing_samples(&previous_samples, &candidate_samples);
    if !selection.truncated.is_empty() || selection.effective != fingerprints(&candidate_samples) {
        return Ok(ImportSummary {
            releases_reused: 1,
            ..ImportSummary::default()
        });
    }
    let previous_fingerprints = fingerprints(&previous_samples);
    let change = compare(&previous_fingerprints, &selection.effective);
    let mut connection = year_database.database.connect()?;
    let transaction = connection.transaction().await?;
    activate(
        &transaction,
        partition,
        previous,
        release,
        &change,
        &previous_fingerprints,
        &selection.effective,
    )
    .await?;
    transaction.commit().await?;
    let (initial, append, backfill, revision) = change_counts(change.kind);
    Ok(ImportSummary {
        releases_reused: 1,
        changes: 1,
        initial,
        append,
        backfill,
        revision,
        historical_revision: matches!(change.kind, ChangeKind::Backfill | ChangeKind::Revision),
        ..ImportSummary::default()
    })
}

const fn change_counts(kind: ChangeKind) -> (usize, usize, usize, usize) {
    match kind {
        ChangeKind::Initial => (1, 0, 0, 0),
        ChangeKind::Append => (0, 1, 0, 0),
        ChangeKind::Backfill => (0, 0, 1, 0),
        ChangeKind::Revision => (0, 0, 0, 1),
        ChangeKind::Unchanged | ChangeKind::RejectedTruncation => (0, 0, 0, 0),
    }
}

fn select_existing_samples(previous: &[ActiveSample], candidate: &[ActiveSample]) -> Selection {
    let candidates = candidate
        .iter()
        .map(|sample| (sample.fingerprint.key.as_str(), sample))
        .collect::<BTreeMap<_, _>>();
    let mut effective = BTreeMap::new();
    let mut truncated = Vec::new();
    for old in previous {
        match candidates.get(old.fingerprint.key.as_str()) {
            Some(next)
                if next.priority >= old.priority
                    || (old.fingerprint.is_missing() && !next.fingerprint.is_missing()) =>
            {
                effective.insert(next.fingerprint.key.as_str(), next.fingerprint.clone());
            }
            Some(_) => {
                effective.insert(old.fingerprint.key.as_str(), old.fingerprint.clone());
            }
            None => {
                truncated.push(old.fingerprint.key.clone());
                effective.insert(old.fingerprint.key.as_str(), old.fingerprint.clone());
            }
        }
    }
    for next in candidate {
        effective
            .entry(next.fingerprint.key.as_str())
            .or_insert_with(|| next.fingerprint.clone());
    }
    Selection {
        effective: effective.into_values().collect(),
        selected: HashSet::new(),
        truncated,
    }
}

fn release_id(file: &ValidatedIndexFile) -> String {
    let mut hasher = Sha256::new();
    for value in [
        file.dataset.provider().to_owned(),
        file.dataset.as_str().to_owned(),
        file.year.to_string(),
        file.month
            .map_or_else(String::new, |value| value.to_string()),
        file.edition.as_str().to_owned(),
        file.download.artifact.digest.to_string(),
        PARSER_NAME.to_owned(),
        PARSER_VERSION.to_owned(),
    ] {
        hasher.update(value.as_bytes());
        hasher.update(b"\0");
    }
    Sha256Digest::from_bytes(hasher.finalize().into()).to_string()
}

fn record_count(parsed: &ParsedFile) -> usize {
    match parsed {
        ParsedFile::Gfz { geomagnetic, daily } => geomagnetic.len() + daily.len(),
        ParsedFile::Dst(records) | ParsedFile::Ae(records) => records.len(),
        ParsedFile::IriIgRz(records) => records.len(),
        ParsedFile::IriApF107(records) => records.len(),
    }
}

fn coverage(year: u16, month: Option<u8>) -> Result<(i64, i64), IndexError> {
    let start_month = month.unwrap_or(1);
    let (end_year, end_month) = match month {
        None | Some(12) => (
            year.checked_add(1)
                .ok_or(IndexError::InvalidNumber("coverage end year"))?,
            1,
        ),
        Some(value) => (year, value + 1),
    };
    let start = Epoch::maybe_from_gregorian_utc(i32::from(year), start_month, 1, 0, 0, 0, 0)
        .map_err(|_| IndexError::InvalidNumber("coverage start"))?;
    let end = Epoch::maybe_from_gregorian_utc(i32::from(end_year), end_month, 1, 0, 0, 0, 0)
        .map_err(|_| IndexError::InvalidNumber("coverage end"))?;
    Ok((epoch_millis(start)?, epoch_millis(end)?))
}

#[cfg(test)]
mod tests;
