use std::collections::BTreeMap;

use turso::params;

use crate::{
    IndexError,
    database::now_ms,
    history::{Change, ChangeKind, SampleFingerprint, field_digest},
};

pub(super) async fn activate(
    transaction: &turso::transaction::Transaction<'_>,
    partition: &str,
    previous: Option<&str>,
    release: &str,
    change: &Change,
    previous_samples: &[SampleFingerprint],
    next_samples: &[SampleFingerprint],
) -> Result<(), IndexError> {
    transaction.execute("INSERT INTO active_release (partition_key, release_id) VALUES (?, ?) ON CONFLICT(partition_key) DO UPDATE SET release_id = excluded.release_id", params![partition, release]).await?;
    if matches!(change.kind, ChangeKind::Backfill | ChangeKind::Revision)
        && let Some(previous) = previous
    {
        transaction.execute("INSERT INTO retained_history_release (partition_key, release_id) VALUES (?, ?) ON CONFLICT(partition_key) DO UPDATE SET release_id = excluded.release_id", params![partition, previous]).await?;
    }
    transaction.execute("INSERT INTO release_change (previous_release_id, active_release_id, change_kind, added_count, modified_count, removed_count, changed_start_utc_ms, changed_end_utc_ms, committed_at_utc_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)", params![previous, release, change.kind.as_str(), i64::try_from(change.added).map_err(|_| IndexError::InvalidNumber("added count"))?, i64::try_from(change.modified).map_err(|_| IndexError::InvalidNumber("modified count"))?, i64::try_from(change.removed).map_err(|_| IndexError::InvalidNumber("removed count"))?, change.start_ms, change.end_ms, now_ms()]).await?;
    let mut rows = transaction.query("SELECT last_insert_rowid()", ()).await?;
    let change_id: i64 = rows
        .next()
        .await?
        .ok_or(IndexError::InvalidNumber("change id"))?
        .get(0)?;
    for next in next_samples {
        let old = previous_samples.iter().find(|old| old.key == next.key);
        if old.is_none_or(|old| old.digest != next.digest) {
            record_field_changes(transaction, change_id, old, next).await?;
        }
    }
    Ok(())
}

async fn record_field_changes(
    transaction: &turso::transaction::Transaction<'_>,
    change_id: i64,
    old: Option<&SampleFingerprint>,
    next: &SampleFingerprint,
) -> Result<(), IndexError> {
    let old_fields = old
        .map(|sample| {
            sample
                .fields
                .iter()
                .map(|field| (field.name.as_str(), field.value_json.as_str()))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let next_fields = next
        .fields
        .iter()
        .map(|field| (field.name.as_str(), field.value_json.as_str()))
        .collect::<BTreeMap<_, _>>();
    for field_name in old_fields.keys().chain(next_fields.keys()) {
        let old_value = old_fields.get(field_name).copied();
        let new_value = next_fields.get(field_name).copied();
        if old_value == new_value {
            continue;
        }
        let kind = match (old_value, new_value) {
            (None, Some(_)) => "added",
            (Some(_), None) => "removed",
            (Some(_), Some(_)) => "modified",
            (None, None) => continue,
        };
        transaction.execute("INSERT OR IGNORE INTO release_field_change (change_id, sample_key, field_name, old_value_json, new_value_json, old_semantic_sha256, new_semantic_sha256, change_kind) VALUES (?, ?, ?, ?, ?, ?, ?, ?)", params![change_id, next.key.clone(), *field_name, old_value, new_value, old_value.map(|value| field_digest(field_name, value).to_string()), new_value.map(|value| field_digest(field_name, value).to_string()), kind]).await?;
    }
    Ok(())
}
