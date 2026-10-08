use std::collections::HashSet;

use turso::params;

use crate::{IndexDataset, IndexEdition, IndexError, parse::ParsedFile};

pub(super) async fn insert_records(
    transaction: &turso::transaction::Transaction<'_>,
    release_id: &str,
    edition: IndexEdition,
    parsed: ParsedFile,
    selected: &HashSet<String>,
) -> Result<(), IndexError> {
    let quality = quality(edition);
    match parsed {
        ParsedFile::Gfz { geomagnetic, daily } => {
            for record in geomagnetic {
                if !selected.contains(&format!("geomagnetic_3h/{}", record.epoch_ms)) {
                    continue;
                }
                transaction.execute("INSERT OR REPLACE INTO geomagnetic_3h (release_id, interval_start_utc_ms, kp_thirds, ap) VALUES (?, ?, ?, ?)", params![release_id, record.epoch_ms, record.kp_thirds.map(i64::from), record.ap]).await?;
            }
            for record in daily {
                if !selected.contains(&format!("space_weather_daily/{}", record.epoch_ms)) {
                    continue;
                }
                transaction.execute("INSERT OR REPLACE INTO space_weather_daily (release_id, date_utc_ms, ap_daily, sunspot_number, f107_observed_sfu, f107_observed_interpolation_gap_days, f107_adjusted_sfu, f107a_81d_sfu, f107a_81d_interpolated_input_count, quality) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)", params![release_id, record.epoch_ms, record.ap_daily, record.sunspot_number, record.f107_observed, i64::from(record.f107_observed_interpolation_gap_days), record.f107_adjusted, record.f107a, i64::from(record.f107a_interpolated_input_count), record.quality]).await?;
            }
        }
        ParsedFile::Dst(records) => {
            for record in records {
                if selected.contains(&format!("dst_hourly/{}", record.epoch_ms)) {
                    transaction.execute("INSERT OR REPLACE INTO dst_hourly (release_id, epoch_utc_ms, dst_nt, quality) VALUES (?, ?, ?, ?)", params![release_id, record.epoch_ms, record.value, quality]).await?;
                }
            }
        }
        ParsedFile::Ae(records) => {
            for record in records {
                if selected.contains(&format!("ae_hourly/{}", record.epoch_ms)) {
                    transaction.execute("INSERT OR REPLACE INTO ae_hourly (release_id, epoch_utc_ms, ae_nt, quality) VALUES (?, ?, ?, ?)", params![release_id, record.epoch_ms, record.value, quality]).await?;
                }
            }
        }
        ParsedFile::IriIgRz(records) => {
            for record in records {
                if selected.contains(&format!("iri_ig_rz_daily/{}", record.epoch_ms)) {
                    transaction.execute("INSERT OR REPLACE INTO iri_ig_rz_daily (release_id, date_utc_ms, ig12, rz12, quality) VALUES (?, ?, ?, ?, ?)", params![release_id, record.epoch_ms, record.ig12, record.rz12, record.quality]).await?;
                }
            }
        }
        ParsedFile::IriApF107(records) => {
            for record in records {
                if selected.contains(&format!("iri_f107_daily/{}", record.epoch_ms)) {
                    transaction.execute("INSERT OR REPLACE INTO iri_f107_daily (release_id, date_utc_ms, f107_adjusted_sfu, f107a_81d_adjusted_sfu, f107a_365d_adjusted_sfu) VALUES (?, ?, ?, ?, ?)", params![release_id, record.epoch_ms, record.f107_adjusted, record.f107a_81_adjusted, record.f107a_365_adjusted]).await?;
                }
            }
        }
    }
    Ok(())
}

pub(super) async fn copy_active_records(
    transaction: &turso::transaction::Transaction<'_>,
    dataset: IndexDataset,
    active: &str,
    release: &str,
) -> Result<(), IndexError> {
    let statements: &[&str] = match dataset {
        IndexDataset::KpApF107 => &[
            "INSERT INTO geomagnetic_3h (release_id, interval_start_utc_ms, kp_thirds, ap) SELECT ?, interval_start_utc_ms, kp_thirds, ap FROM geomagnetic_3h WHERE release_id = ?",
            "INSERT INTO space_weather_daily (release_id, date_utc_ms, ap_daily, sunspot_number, f107_observed_sfu, f107_observed_interpolation_gap_days, f107_adjusted_sfu, f107a_81d_sfu, f107a_81d_interpolated_input_count, quality) SELECT ?, date_utc_ms, ap_daily, sunspot_number, f107_observed_sfu, f107_observed_interpolation_gap_days, f107_adjusted_sfu, f107a_81d_sfu, f107a_81d_interpolated_input_count, quality FROM space_weather_daily WHERE release_id = ?",
        ],
        IndexDataset::Dst => &[
            "INSERT INTO dst_hourly (release_id, epoch_utc_ms, dst_nt, quality) SELECT ?, epoch_utc_ms, dst_nt, quality FROM dst_hourly WHERE release_id = ?",
        ],
        IndexDataset::Ae => &[
            "INSERT INTO ae_hourly (release_id, epoch_utc_ms, ae_nt, quality) SELECT ?, epoch_utc_ms, ae_nt, quality FROM ae_hourly WHERE release_id = ?",
        ],
        IndexDataset::IriIgRz => &[
            "INSERT INTO iri_ig_rz_daily (release_id, date_utc_ms, ig12, rz12, quality) SELECT ?, date_utc_ms, ig12, rz12, quality FROM iri_ig_rz_daily WHERE release_id = ?",
        ],
        IndexDataset::IriApF107 => &[
            "INSERT INTO iri_f107_daily (release_id, date_utc_ms, f107_adjusted_sfu, f107a_81d_adjusted_sfu, f107a_365d_adjusted_sfu) SELECT ?, date_utc_ms, f107_adjusted_sfu, f107a_81d_adjusted_sfu, f107a_365d_adjusted_sfu FROM iri_f107_daily WHERE release_id = ?",
        ],
    };
    for statement in statements {
        transaction
            .execute(statement, params![release, active])
            .await?;
    }
    Ok(())
}

const fn quality(edition: IndexEdition) -> &'static str {
    match edition {
        IndexEdition::Final => "final",
        IndexEdition::Provisional | IndexEdition::Realtime => "provisional",
        IndexEdition::Rolling => "unknown",
    }
}
