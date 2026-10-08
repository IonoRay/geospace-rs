mod ap;
pub(crate) use ap::read_ap;

use ionoray_core::{Epoch, Sha256Digest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use turso::Database;

use crate::{
    Ae, Ap, Dst, F107, IndexDataset, IndexError, IndexSample, Kp, QualityFlag, TimeInterval,
    ValueDerivation,
    time::{epoch_from_millis, epoch_millis},
};

const HOUR_MS: i64 = 60 * 60 * 1_000;
const DAY_MS: i64 = 24 * HOUR_MS;

/// Model-ready geophysical indices resolved at one UTC epoch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeophysicalIndices {
    /// Three-hour planetary Kp.
    pub kp: IndexSample<Kp>,
    /// Three-hour planetary ap.
    pub ap: IndexSample<Ap>,
    /// Daily planetary Ap.
    pub ap_daily: IndexSample<Ap>,
    /// Observed daily 10.7 cm solar flux.
    pub f107: IndexSample<F107>,
    /// Centered 81-day observed F10.7 average.
    pub f107a: IndexSample<F107>,
    /// Hourly Dst.
    pub dst: IndexSample<Dst>,
    /// Hourly AE.
    pub ae: IndexSample<Ae>,
}

/// GFZ three-hour geomagnetic indices resolved at one UTC epoch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeomagneticIndices {
    /// Three-hour planetary Kp.
    pub kp: IndexSample<Kp>,
    /// Three-hour planetary ap.
    pub ap: IndexSample<Ap>,
}

pub(crate) async fn read_latest(
    gfz: &Database,
    dst: &Database,
    ae: &Database,
    epoch: Epoch,
) -> Result<GeophysicalIndices, IndexError> {
    let epoch_ms = epoch_millis(epoch)?;
    let geomagnetic = read_geomagnetic(gfz, epoch_ms).await?;
    let daily = read_daily(gfz, epoch_ms).await?;
    let dst = read_hourly(dst, "dst_hourly", "dst_nt", IndexDataset::Dst, epoch_ms).await?;
    let ae = read_hourly(ae, "ae_hourly", "ae_nt", IndexDataset::Ae, epoch_ms).await?;
    let snapshot = snapshot(&[
        &geomagnetic.release_id,
        &daily.meta.release_id,
        &dst.meta.release_id,
        &ae.meta.release_id,
    ]);
    Ok(GeophysicalIndices {
        kp: sample(
            Kp::new(f64::from(geomagnetic.kp_thirds) / 3.0)?,
            &geomagnetic,
            snapshot,
            ValueDerivation::Source,
        ),
        ap: sample(
            Ap::new(geomagnetic.ap)?,
            &geomagnetic,
            snapshot,
            ValueDerivation::Source,
        ),
        ap_daily: sample(
            Ap::new(daily.ap.ok_or(IndexError::MissingValue {
                dataset: IndexDataset::KpApF107,
                epoch_utc_ms: epoch_ms,
            })?)?,
            &daily.meta,
            snapshot,
            ValueDerivation::Source,
        ),
        f107: sample(
            F107::new(daily.f107.ok_or(IndexError::MissingValue {
                dataset: IndexDataset::KpApF107,
                epoch_utc_ms: epoch_ms,
            })?)?,
            &daily.meta,
            snapshot,
            daily.f107_derivation(),
        ),
        f107a: sample(
            F107::new(daily.f107a.ok_or(IndexError::MissingValue {
                dataset: IndexDataset::KpApF107,
                epoch_utc_ms: epoch_ms,
            })?)?,
            &daily.meta,
            snapshot,
            daily.f107a_derivation(),
        ),
        dst: sample(
            Dst::new(dst.value)?,
            &dst.meta,
            snapshot,
            ValueDerivation::Source,
        ),
        ae: sample(
            Ae::new(ae.value)?,
            &ae.meta,
            snapshot,
            ValueDerivation::Source,
        ),
    })
}

pub(crate) async fn read_geomagnetic_indices(
    gfz: &Database,
    epoch: Epoch,
) -> Result<GeomagneticIndices, IndexError> {
    let epoch_ms = epoch_millis(epoch)?;
    let geomagnetic = read_geomagnetic(gfz, epoch_ms).await?;
    let snapshot = snapshot(&[&geomagnetic.release_id]);
    Ok(GeomagneticIndices {
        kp: sample(
            Kp::new(f64::from(geomagnetic.kp_thirds) / 3.0)?,
            &geomagnetic,
            snapshot,
            ValueDerivation::Source,
        ),
        ap: sample(
            Ap::new(geomagnetic.ap)?,
            &geomagnetic,
            snapshot,
            ValueDerivation::Source,
        ),
    })
}

pub(crate) async fn read_daily_ap(
    gfz: &Database,
    epoch: Epoch,
) -> Result<IndexSample<Ap>, IndexError> {
    let epoch_ms = epoch_millis(epoch)?;
    let daily = read_daily(gfz, epoch_ms).await?;
    Ok(sample(
        Ap::new(daily.ap.ok_or(IndexError::MissingValue {
            dataset: IndexDataset::KpApF107,
            epoch_utc_ms: epoch_ms,
        })?)?,
        &daily.meta,
        snapshot(&[&daily.meta.release_id]),
        ValueDerivation::Source,
    ))
}

pub(crate) async fn read_f107(
    gfz: &Database,
    epoch: Epoch,
) -> Result<IndexSample<F107>, IndexError> {
    let epoch_ms = epoch_millis(epoch)?;
    let daily = read_daily(gfz, epoch_ms).await?;
    Ok(sample(
        F107::new(daily.f107.ok_or(IndexError::MissingValue {
            dataset: IndexDataset::KpApF107,
            epoch_utc_ms: epoch_ms,
        })?)?,
        &daily.meta,
        snapshot(&[&daily.meta.release_id]),
        daily.f107_derivation(),
    ))
}

pub(crate) async fn read_f107a(
    gfz: &Database,
    epoch: Epoch,
) -> Result<IndexSample<F107>, IndexError> {
    let epoch_ms = epoch_millis(epoch)?;
    let daily = read_daily(gfz, epoch_ms).await?;
    Ok(sample(
        F107::new(daily.f107a.ok_or(IndexError::MissingValue {
            dataset: IndexDataset::KpApF107,
            epoch_utc_ms: epoch_ms,
        })?)?,
        &daily.meta,
        snapshot(&[&daily.meta.release_id]),
        daily.f107a_derivation(),
    ))
}

pub(crate) async fn read_dst(
    database: &Database,
    epoch: Epoch,
) -> Result<IndexSample<Dst>, IndexError> {
    let hourly = read_hourly(
        database,
        "dst_hourly",
        "dst_nt",
        IndexDataset::Dst,
        epoch_millis(epoch)?,
    )
    .await?;
    Ok(sample(
        Dst::new(hourly.value)?,
        &hourly.meta,
        snapshot(&[&hourly.meta.release_id]),
        ValueDerivation::Source,
    ))
}

pub(crate) async fn read_ae(
    database: &Database,
    epoch: Epoch,
) -> Result<IndexSample<Ae>, IndexError> {
    let hourly = read_hourly(
        database,
        "ae_hourly",
        "ae_nt",
        IndexDataset::Ae,
        epoch_millis(epoch)?,
    )
    .await?;
    Ok(sample(
        Ae::new(hourly.value)?,
        &hourly.meta,
        snapshot(&[&hourly.meta.release_id]),
        ValueDerivation::Source,
    ))
}

async fn read_geomagnetic(database: &Database, epoch_ms: i64) -> Result<Geomagnetic, IndexError> {
    let connection = database.connect()?;
    let mut rows = connection
        .query(
            "SELECT g.interval_start_utc_ms, g.kp_thirds, g.ap, r.release_id, r.artifact_sha256, r.edition FROM geomagnetic_3h g JOIN active_release a ON a.release_id = g.release_id JOIN sample_provenance p ON p.release_id = g.release_id AND p.sample_key = 'geomagnetic_3h/' || g.interval_start_utc_ms JOIN dataset_release r ON r.release_id = p.source_release_id WHERE g.interval_start_utc_ms <= ? AND g.interval_start_utc_ms > ? LIMIT 1",
            turso::params![epoch_ms, epoch_ms - 3 * HOUR_MS],
        )
        .await?;
    let row = rows.next().await?.ok_or(IndexError::MissingValue {
        dataset: IndexDataset::KpApF107,
        epoch_utc_ms: epoch_ms,
    })?;
    let start: i64 = row.get(0)?;
    let kp_thirds = row.get::<Option<i64>>(1)?.ok_or(IndexError::MissingValue {
        dataset: IndexDataset::KpApF107,
        epoch_utc_ms: epoch_ms,
    })?;
    Ok(Geomagnetic {
        kp_thirds: i32::try_from(kp_thirds).map_err(|_| IndexError::InvalidNumber("Kp thirds"))?,
        ap: row.get::<Option<f64>>(2)?.ok_or(IndexError::MissingValue {
            dataset: IndexDataset::KpApF107,
            epoch_utc_ms: epoch_ms,
        })?,
        release_id: row.get(3)?,
        artifact: row.get::<String>(4)?.parse()?,
        quality: quality(&row.get::<String>(5)?),
        start,
        end: start + 3 * HOUR_MS,
    })
}

async fn read_daily(database: &Database, epoch_ms: i64) -> Result<Daily, IndexError> {
    let connection = database.connect()?;
    let mut rows = connection
        .query(
            "SELECT d.date_utc_ms, d.ap_daily, d.f107_observed_sfu, d.f107_observed_interpolation_gap_days, d.f107a_81d_sfu, d.f107a_81d_interpolated_input_count, d.quality, r.release_id, r.artifact_sha256 FROM space_weather_daily d JOIN active_release a ON a.release_id = d.release_id JOIN sample_provenance p ON p.release_id = d.release_id AND p.sample_key = 'space_weather_daily/' || d.date_utc_ms JOIN dataset_release r ON r.release_id = p.source_release_id WHERE d.date_utc_ms <= ? AND d.date_utc_ms > ? LIMIT 1",
            turso::params![epoch_ms, epoch_ms - DAY_MS],
        )
        .await?;
    let row = rows.next().await?.ok_or(IndexError::MissingValue {
        dataset: IndexDataset::KpApF107,
        epoch_utc_ms: epoch_ms,
    })?;
    let start: i64 = row.get(0)?;
    Ok(Daily {
        ap: row.get(1)?,
        f107: row.get(2)?,
        f107_interpolation_gap_days: u16::try_from(row.get::<i64>(3)?)
            .map_err(|_| IndexError::InvalidNumber("F10.7 interpolation gap days"))?,
        f107a: row.get(4)?,
        f107a_interpolated_input_count: u16::try_from(row.get::<i64>(5)?)
            .map_err(|_| IndexError::InvalidNumber("F10.7a interpolated input count"))?,
        meta: SampleMeta {
            start,
            end: start + DAY_MS,
            quality: quality(&row.get::<String>(6)?),
            release_id: row.get(7)?,
            artifact: row.get::<String>(8)?.parse()?,
        },
    })
}

async fn read_hourly(
    database: &Database,
    table: &str,
    column: &str,
    dataset: IndexDataset,
    epoch_ms: i64,
) -> Result<Hourly, IndexError> {
    let sql = format!(
        "SELECT h.epoch_utc_ms, h.{column}, h.quality, r.release_id, r.artifact_sha256 FROM {table} h JOIN active_release a ON a.release_id = h.release_id JOIN sample_provenance p ON p.release_id = h.release_id AND p.sample_key = '{table}/' || h.epoch_utc_ms JOIN dataset_release r ON r.release_id = p.source_release_id WHERE h.epoch_utc_ms <= ? AND h.epoch_utc_ms > ? LIMIT 1"
    );
    let connection = database.connect()?;
    let mut rows = connection
        .query(&sql, turso::params![epoch_ms, epoch_ms - HOUR_MS])
        .await?;
    let row = rows.next().await?.ok_or(IndexError::MissingValue {
        dataset,
        epoch_utc_ms: epoch_ms,
    })?;
    let start: i64 = row.get(0)?;
    Ok(Hourly {
        value: row.get::<Option<f64>>(1)?.ok_or(IndexError::MissingValue {
            dataset,
            epoch_utc_ms: epoch_ms,
        })?,
        meta: SampleMeta {
            start,
            end: start + HOUR_MS,
            quality: quality(&row.get::<String>(2)?),
            release_id: row.get(3)?,
            artifact: row.get::<String>(4)?.parse()?,
        },
    })
}

fn sample<T>(
    value: T,
    meta: &impl Metadata,
    snapshot: Sha256Digest,
    derivation: ValueDerivation,
) -> IndexSample<T> {
    IndexSample {
        value,
        interval: TimeInterval {
            start: epoch_from_millis(meta.start()),
            end: epoch_from_millis(meta.end()),
        },
        quality: meta.quality(),
        derivation,
        release_id: meta.release_id().to_owned(),
        artifact: meta.artifact(),
        snapshot,
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
    match value.as_bytes().first() {
        Some(b'f' | b'F' | b'D') => QualityFlag::Final,
        Some(b'p' | b'P' | b'r' | b'R') => QualityFlag::Provisional,
        _ => QualityFlag::Unknown,
    }
}

trait Metadata {
    fn start(&self) -> i64;
    fn end(&self) -> i64;
    fn quality(&self) -> QualityFlag;
    fn release_id(&self) -> &str;
    fn artifact(&self) -> Sha256Digest;
}

struct SampleMeta {
    start: i64,
    end: i64,
    quality: QualityFlag,
    release_id: String,
    artifact: Sha256Digest,
}

impl Metadata for SampleMeta {
    fn start(&self) -> i64 {
        self.start
    }
    fn end(&self) -> i64 {
        self.end
    }
    fn quality(&self) -> QualityFlag {
        self.quality
    }
    fn release_id(&self) -> &str {
        &self.release_id
    }
    fn artifact(&self) -> Sha256Digest {
        self.artifact
    }
}

struct Geomagnetic {
    kp_thirds: i32,
    ap: f64,
    release_id: String,
    artifact: Sha256Digest,
    quality: QualityFlag,
    start: i64,
    end: i64,
}

impl Metadata for Geomagnetic {
    fn start(&self) -> i64 {
        self.start
    }
    fn end(&self) -> i64 {
        self.end
    }
    fn quality(&self) -> QualityFlag {
        self.quality
    }
    fn release_id(&self) -> &str {
        &self.release_id
    }
    fn artifact(&self) -> Sha256Digest {
        self.artifact
    }
}

struct Daily {
    ap: Option<f64>,
    f107: Option<f64>,
    f107_interpolation_gap_days: u16,
    f107a: Option<f64>,
    f107a_interpolated_input_count: u16,
    meta: SampleMeta,
}

impl Daily {
    const fn f107_derivation(&self) -> ValueDerivation {
        if self.f107_interpolation_gap_days == 0 {
            ValueDerivation::Source
        } else {
            ValueDerivation::LinearInterpolation {
                gap_days: self.f107_interpolation_gap_days,
            }
        }
    }

    const fn f107a_derivation(&self) -> ValueDerivation {
        ValueDerivation::CenteredMean {
            window_days: 81,
            interpolated_input_count: self.f107a_interpolated_input_count,
        }
    }
}
struct Hourly {
    value: f64,
    meta: SampleMeta,
}

#[cfg(test)]
mod tests;
