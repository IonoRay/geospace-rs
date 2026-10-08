//! Single ap read: a missing Kp must not reject a usable model driver.
use super::{
    Ap, Database, Epoch, HOUR_MS, IndexDataset, IndexError, IndexSample, SampleMeta,
    ValueDerivation, epoch_millis, quality, sample, snapshot,
};

pub(crate) async fn read_ap(
    database: &Database,
    epoch: Epoch,
) -> Result<IndexSample<Ap>, IndexError> {
    let epoch_ms = epoch_millis(epoch)?;
    let connection = database.connect()?;
    let mut rows = connection.query(
        "SELECT g.interval_start_utc_ms, g.ap, r.release_id, r.artifact_sha256, r.edition FROM geomagnetic_3h g JOIN active_release a ON a.release_id = g.release_id JOIN sample_provenance p ON p.release_id = g.release_id AND p.sample_key = 'geomagnetic_3h/' || g.interval_start_utc_ms JOIN dataset_release r ON r.release_id = p.source_release_id WHERE g.interval_start_utc_ms <= ? AND g.interval_start_utc_ms > ? LIMIT 1",
        turso::params![epoch_ms, epoch_ms - 3 * HOUR_MS],
    ).await?;
    let missing = || IndexError::MissingValue {
        dataset: IndexDataset::KpApF107,
        epoch_utc_ms: epoch_ms,
    };
    let row = rows.next().await?.ok_or_else(missing)?;
    let start = row.get::<i64>(0)?;
    let meta = SampleMeta {
        start,
        end: start + 3 * HOUR_MS,
        release_id: row.get(2)?,
        artifact: row.get::<String>(3)?.parse()?,
        quality: quality(&row.get::<String>(4)?),
    };
    Ok(sample(
        Ap::new(row.get::<Option<f64>>(1)?.ok_or_else(missing)?)?,
        &meta,
        snapshot(&[&meta.release_id]),
        ValueDerivation::Source,
    ))
}
