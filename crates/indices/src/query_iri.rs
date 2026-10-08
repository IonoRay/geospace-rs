use ionoray_core::{Epoch, Sha256Digest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use turso::Database;

use crate::{
    AdjustedF107, Ig12, IndexDataset, IndexError, IndexSample, QualityFlag, Rz12, TimeInterval,
    ValueDerivation,
    time::{epoch_from_millis, epoch_millis},
};

const DAY_MS: i64 = 24 * 60 * 60 * 1_000;

/// IRI monthly indices interpolated to one UTC date.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IriMonthlyIndices {
    /// Interpolated 12-month running mean ionospheric index.
    pub ig12: IndexSample<Ig12>,
    /// Interpolated and officially scaled 12-month running mean sunspot number.
    pub rz12: IndexSample<Rz12>,
}

/// IRI adjusted solar-flux drivers for one UTC date.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IriF107Indices {
    /// Daily F10.7 adjusted to 1 AU.
    pub daily: IndexSample<AdjustedF107>,
    /// Centered 81-day adjusted F10.7 average.
    pub average_81_day: IndexSample<AdjustedF107>,
    /// Centered 365-day adjusted F10.7 average.
    pub average_365_day: IndexSample<AdjustedF107>,
}

/// Complete IRI-2020 point-driver index set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IriIndices {
    /// Interpolated monthly drivers.
    pub monthly: IriMonthlyIndices,
    /// Daily adjusted solar-flux drivers.
    pub f107: IriF107Indices,
}

pub(crate) async fn read_monthly(
    database: &Database,
    epoch: Epoch,
) -> Result<IriMonthlyIndices, IndexError> {
    let epoch_ms = epoch_millis(epoch)?;
    let connection = database.connect()?;
    let mut rows = connection
        .query(
            "SELECT d.date_utc_ms, d.ig12, d.rz12, d.quality, r.release_id, r.artifact_sha256 FROM iri_ig_rz_daily d JOIN active_release a ON a.release_id = d.release_id JOIN sample_provenance p ON p.release_id = d.release_id AND p.sample_key = 'iri_ig_rz_daily/' || d.date_utc_ms JOIN dataset_release r ON r.release_id = p.source_release_id WHERE d.date_utc_ms <= ? AND d.date_utc_ms > ? LIMIT 1",
            turso::params![epoch_ms, epoch_ms - DAY_MS],
        )
        .await?;
    let row = rows.next().await?.ok_or(IndexError::MissingValue {
        dataset: IndexDataset::IriIgRz,
        epoch_utc_ms: epoch_ms,
    })?;
    let start = row.get::<i64>(0)?;
    let release_id = row.get::<String>(4)?;
    let artifact = row.get::<String>(5)?.parse()?;
    let snapshot = snapshot(&[&release_id]);
    let meta = SampleMeta {
        start,
        quality: quality(&row.get::<String>(3)?),
        release_id,
        artifact,
        snapshot,
    };
    Ok(IriMonthlyIndices {
        ig12: meta.sample(Ig12::new(row.get(1)?)?),
        rz12: meta.sample(Rz12::new(row.get(2)?)?),
    })
}

pub(crate) async fn read_f107(
    database: &Database,
    epoch: Epoch,
) -> Result<IriF107Indices, IndexError> {
    let epoch_ms = epoch_millis(epoch)?;
    let connection = database.connect()?;
    let mut rows = connection
        .query(
            "SELECT d.date_utc_ms, d.f107_adjusted_sfu, d.f107a_81d_adjusted_sfu, d.f107a_365d_adjusted_sfu, r.release_id, r.artifact_sha256 FROM iri_f107_daily d JOIN active_release a ON a.release_id = d.release_id JOIN sample_provenance p ON p.release_id = d.release_id AND p.sample_key = 'iri_f107_daily/' || d.date_utc_ms JOIN dataset_release r ON r.release_id = p.source_release_id WHERE d.date_utc_ms <= ? AND d.date_utc_ms > ? LIMIT 1",
            turso::params![epoch_ms, epoch_ms - DAY_MS],
        )
        .await?;
    let row = rows.next().await?.ok_or(IndexError::MissingValue {
        dataset: IndexDataset::IriApF107,
        epoch_utc_ms: epoch_ms,
    })?;
    let start = row.get::<i64>(0)?;
    let release_id = row.get::<String>(4)?;
    let artifact = row.get::<String>(5)?.parse()?;
    let meta = SampleMeta {
        start,
        quality: QualityFlag::Unknown,
        snapshot: snapshot(&[&release_id]),
        release_id,
        artifact,
    };
    Ok(IriF107Indices {
        daily: meta.sample(AdjustedF107::new(row.get(1)?)?),
        average_81_day: meta.sample(AdjustedF107::new(row.get(2)?)?),
        average_365_day: meta.sample(AdjustedF107::new(row.get(3)?)?),
    })
}

pub(crate) async fn read_rz12(
    database: &Database,
    epoch: Epoch,
) -> Result<IndexSample<Rz12>, IndexError> {
    let (value, meta) = read_field(
        database,
        epoch,
        IndexDataset::IriIgRz,
        "iri_ig_rz_daily",
        "rz12",
        "d.quality",
    )
    .await?;
    Ok(meta.sample(Rz12::new(value)?))
}

pub(crate) async fn read_ig12(
    database: &Database,
    epoch: Epoch,
) -> Result<IndexSample<Ig12>, IndexError> {
    let (value, meta) = read_field(
        database,
        epoch,
        IndexDataset::IriIgRz,
        "iri_ig_rz_daily",
        "ig12",
        "d.quality",
    )
    .await?;
    Ok(meta.sample(Ig12::new(value)?))
}

pub(crate) async fn read_f107_daily(
    database: &Database,
    epoch: Epoch,
) -> Result<IndexSample<AdjustedF107>, IndexError> {
    let (value, meta) = read_field(
        database,
        epoch,
        IndexDataset::IriApF107,
        "iri_f107_daily",
        "f107_adjusted_sfu",
        "'unknown'",
    )
    .await?;
    Ok(meta.sample(AdjustedF107::new(value)?))
}

pub(crate) async fn read_f107_81_day(
    database: &Database,
    epoch: Epoch,
) -> Result<IndexSample<AdjustedF107>, IndexError> {
    let (value, meta) = read_field(
        database,
        epoch,
        IndexDataset::IriApF107,
        "iri_f107_daily",
        "f107a_81d_adjusted_sfu",
        "'unknown'",
    )
    .await?;
    Ok(meta.sample(AdjustedF107::new(value)?))
}

// Table, column and quality are internal constants, never caller-provided SQL.
async fn read_field(
    database: &Database,
    epoch: Epoch,
    dataset: IndexDataset,
    table: &str,
    column: &str,
    quality_column: &str,
) -> Result<(f64, SampleMeta), IndexError> {
    let epoch_ms = epoch_millis(epoch)?;
    let connection = database.connect()?;
    let sql = format!(
        "SELECT d.date_utc_ms, d.{column}, {quality_column}, r.release_id, r.artifact_sha256 FROM {table} d JOIN active_release a ON a.release_id = d.release_id JOIN sample_provenance p ON p.release_id = d.release_id AND p.sample_key = '{table}/' || d.date_utc_ms JOIN dataset_release r ON r.release_id = p.source_release_id WHERE d.date_utc_ms <= ? AND d.date_utc_ms > ? LIMIT 1"
    );
    let mut rows = connection
        .query(&sql, turso::params![epoch_ms, epoch_ms - DAY_MS])
        .await?;
    let missing = || IndexError::MissingValue {
        dataset,
        epoch_utc_ms: epoch_ms,
    };
    let row = rows.next().await?.ok_or_else(missing)?;
    let release_id = row.get::<String>(3)?;
    let meta = SampleMeta {
        start: row.get(0)?,
        quality: quality(&row.get::<String>(2)?),
        snapshot: snapshot(&[&release_id]),
        release_id,
        artifact: row.get::<String>(4)?.parse()?,
    };
    Ok((row.get::<Option<f64>>(1)?.ok_or_else(missing)?, meta))
}

pub(crate) fn combine(mut monthly: IriMonthlyIndices, mut f107: IriF107Indices) -> IriIndices {
    let snapshot = snapshot(&[&monthly.ig12.release_id, &f107.daily.release_id]);
    monthly.ig12.snapshot = snapshot;
    monthly.rz12.snapshot = snapshot;
    f107.daily.snapshot = snapshot;
    f107.average_81_day.snapshot = snapshot;
    f107.average_365_day.snapshot = snapshot;
    IriIndices { monthly, f107 }
}

#[derive(Clone)]
struct SampleMeta {
    start: i64,
    quality: QualityFlag,
    release_id: String,
    artifact: Sha256Digest,
    snapshot: Sha256Digest,
}

impl SampleMeta {
    fn sample<T>(&self, value: T) -> IndexSample<T> {
        IndexSample {
            value,
            interval: TimeInterval {
                start: epoch_from_millis(self.start),
                end: epoch_from_millis(self.start + DAY_MS),
            },
            quality: self.quality,
            derivation: ValueDerivation::Source,
            release_id: self.release_id.clone(),
            artifact: self.artifact,
            snapshot: self.snapshot,
        }
    }
}

fn snapshot(releases: &[&str]) -> Sha256Digest {
    let mut hasher = Sha256::new();
    for release in releases {
        hasher.update(release.as_bytes());
        hasher.update(b"\0");
    }
    Sha256Digest::from_bytes(hasher.finalize().into())
}

fn quality(value: &str) -> QualityFlag {
    match value {
        "final" => QualityFlag::Final,
        "predicted" => QualityFlag::Predicted,
        "provisional" => QualityFlag::Provisional,
        _ => QualityFlag::Unknown,
    }
}

#[cfg(test)]
mod tests;
