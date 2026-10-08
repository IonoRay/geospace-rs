//! Semantic comparison and retention decisions for one yearly active snapshot.

use std::collections::BTreeMap;

use ionoray_core::Sha256Digest;
use sha2::{Digest, Sha256};

use crate::{IndexEdition, parse::ParsedFile};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SampleFingerprint {
    pub(crate) key: String,
    pub(crate) epoch_ms: i64,
    pub(crate) digest: Sha256Digest,
    pub(crate) value_json: String,
    pub(crate) fields: Vec<SampleField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SampleField {
    pub(crate) name: String,
    pub(crate) value_json: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChangeKind {
    Initial,
    Unchanged,
    Append,
    Backfill,
    Revision,
    RejectedTruncation,
}

impl ChangeKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::Unchanged => "unchanged",
            Self::Append => "append",
            Self::Backfill => "backfill",
            Self::Revision => "revision",
            Self::RejectedTruncation => "rejected_truncation",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Change {
    pub(crate) kind: ChangeKind,
    pub(crate) added: usize,
    pub(crate) modified: usize,
    pub(crate) removed: usize,
    pub(crate) start_ms: Option<i64>,
    pub(crate) end_ms: Option<i64>,
}

pub(crate) fn fingerprints(parsed: &ParsedFile) -> Vec<SampleFingerprint> {
    let mut values = Vec::new();
    match parsed {
        ParsedFile::Gfz { geomagnetic, daily } => {
            for value in geomagnetic {
                let mut fingerprint = sample("geomagnetic_3h", value.epoch_ms, |hash| {
                    optional_integer(hash, value.kp_thirds);
                    optional_number(hash, value.ap);
                });
                json_value(
                    &mut fingerprint,
                    format!(
                        "{{\"kp_thirds\":{},\"ap\":{}}}",
                        option_integer_json(value.kp_thirds),
                        option_json(value.ap)
                    ),
                );
                values.push(fingerprint);
            }
            for value in daily {
                let mut fingerprint = sample("space_weather_daily", value.epoch_ms, |hash| {
                    optional_number(hash, value.ap_daily);
                    optional_number(hash, value.sunspot_number);
                    optional_number(hash, value.f107_observed);
                    integer(hash, i32::from(value.f107_observed_interpolation_gap_days));
                    optional_number(hash, value.f107_adjusted);
                    optional_number(hash, value.f107a);
                    integer(hash, i32::from(value.f107a_interpolated_input_count));
                    text(hash, &value.quality);
                });
                json_value(
                    &mut fingerprint,
                    format!(
                        "{{\"ap_daily\":{},\"sunspot_number\":{},\"f107_observed\":{},\"f107_observed_interpolation_gap_days\":{},\"f107_adjusted\":{},\"f107a_81d\":{},\"f107a_81d_interpolated_input_count\":{},\"quality\":{}}}",
                        option_json(value.ap_daily),
                        option_json(value.sunspot_number),
                        option_json(value.f107_observed),
                        value.f107_observed_interpolation_gap_days,
                        option_json(value.f107_adjusted),
                        option_json(value.f107a),
                        value.f107a_interpolated_input_count,
                        json_string(&value.quality)
                    ),
                );
                values.push(fingerprint);
            }
        }
        ParsedFile::Dst(records) => collect_hourly(&mut values, "dst_hourly", records),
        ParsedFile::Ae(records) => collect_hourly(&mut values, "ae_hourly", records),
        ParsedFile::IriIgRz(records) => {
            for value in records {
                let mut fingerprint = sample("iri_ig_rz_daily", value.epoch_ms, |hash| {
                    number(hash, value.ig12);
                    number(hash, value.rz12);
                    text(hash, value.quality);
                });
                json_value(
                    &mut fingerprint,
                    format!(
                        "{{\"ig12\":{},\"rz12\":{},\"quality\":{}}}",
                        value.ig12,
                        value.rz12,
                        json_string(value.quality)
                    ),
                );
                values.push(fingerprint);
            }
        }
        ParsedFile::IriApF107(records) => {
            for value in records {
                let mut fingerprint = sample("iri_f107_daily", value.epoch_ms, |hash| {
                    number(hash, value.f107_adjusted);
                    number(hash, value.f107a_81_adjusted);
                    number(hash, value.f107a_365_adjusted);
                });
                json_value(
                    &mut fingerprint,
                    format!(
                        "{{\"f107_adjusted\":{},\"f107a_81d_adjusted\":{},\"f107a_365d_adjusted\":{}}}",
                        value.f107_adjusted, value.f107a_81_adjusted, value.f107a_365_adjusted
                    ),
                );
                values.push(fingerprint);
            }
        }
    }
    values.sort_by(|left, right| left.key.cmp(&right.key));
    values
}

/// Includes the source maturity because a final source replacing provisional
/// values is a semantic provenance change even when its numeric payload matches.
pub(crate) fn fingerprints_for_edition(
    parsed: &ParsedFile,
    edition: IndexEdition,
) -> Vec<SampleFingerprint> {
    fingerprints(parsed)
        .into_iter()
        .map(|mut sample| {
            if sample.key.starts_with("dst_hourly/") || sample.key.starts_with("ae_hourly/") {
                sample.fields.push(SampleField {
                    name: "quality".to_owned(),
                    value_json: format!("\"{}\"", quality(edition)),
                });
                sample.value_json = object_json(&sample.fields);
            }
            let mut hash = Sha256::new();
            hash.update(sample.digest.to_string().as_bytes());
            text(&mut hash, edition.as_str());
            sample.digest = Sha256Digest::from_bytes(hash.finalize().into());
            sample
        })
        .collect()
}

fn collect_hourly(
    output: &mut Vec<SampleFingerprint>,
    table: &str,
    values: &[crate::parse::HourlyRecord],
) {
    for value in values {
        let mut fingerprint = sample(table, value.epoch_ms, |hash| {
            optional_number(hash, value.value);
        });
        json_value(
            &mut fingerprint,
            value.value.map_or_else(
                || "{\"value\":null}".to_owned(),
                |value| format!("{{\"value\":{value}}}"),
            ),
        );
        output.push(fingerprint);
    }
}

fn sample(name: &str, epoch_ms: i64, fill: impl FnOnce(&mut Sha256)) -> SampleFingerprint {
    let mut hash = Sha256::new();
    text(&mut hash, name);
    hash.update(epoch_ms.to_le_bytes());
    fill(&mut hash);
    SampleFingerprint {
        key: format!("{name}/{epoch_ms}"),
        epoch_ms,
        digest: Sha256Digest::from_bytes(hash.finalize().into()),
        value_json: "{}".to_owned(),
        fields: Vec::new(),
    }
}

fn json_value(sample: &mut SampleFingerprint, value_json: String) {
    sample.value_json = value_json;
    sample.fields = parse_object_fields(&sample.value_json);
}

impl SampleFingerprint {
    pub(crate) fn from_stored(
        key: String,
        epoch_ms: i64,
        digest: Sha256Digest,
        value_json: String,
    ) -> Self {
        let fields = parse_object_fields(&value_json);
        Self {
            key,
            epoch_ms,
            digest,
            value_json,
            fields,
        }
    }

    pub(crate) fn is_missing(&self) -> bool {
        self.fields.iter().any(|field| field.value_json == "null")
    }
}

pub(crate) fn field_digest(name: &str, value_json: &str) -> Sha256Digest {
    let mut hash = Sha256::new();
    text(&mut hash, name);
    text(&mut hash, value_json);
    Sha256Digest::from_bytes(hash.finalize().into())
}

const fn quality(edition: IndexEdition) -> &'static str {
    match edition {
        IndexEdition::Final => "final",
        IndexEdition::Provisional | IndexEdition::Realtime => "provisional",
        IndexEdition::Rolling => "unknown",
    }
}

fn object_json(fields: &[SampleField]) -> String {
    let body = fields
        .iter()
        .map(|field| format!("\"{}\":{}", field.name, field.value_json))
        .collect::<Vec<_>>()
        .join(",");
    format!("{{{body}}}")
}

fn parse_object_fields(value: &str) -> Vec<SampleField> {
    let Some(body) = value
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
    else {
        return Vec::new();
    };
    let mut fields = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (index, byte) in body.bytes().enumerate() {
        if escaped {
            escaped = false;
        } else if byte == b'\\' && quoted {
            escaped = true;
        } else if byte == b'"' {
            quoted = !quoted;
        } else if byte == b',' && !quoted {
            parse_field(&body[start..index], &mut fields);
            start = index + 1;
        }
    }
    parse_field(&body[start..], &mut fields);
    fields
}

fn parse_field(value: &str, fields: &mut Vec<SampleField>) {
    let Some((name, value_json)) = value.split_once(':') else {
        return;
    };
    let Some(name) = name
        .strip_prefix('"')
        .and_then(|name| name.strip_suffix('"'))
    else {
        return;
    };
    fields.push(SampleField {
        name: name.to_owned(),
        value_json: value_json.to_owned(),
    });
}

fn text(hash: &mut Sha256, value: &str) {
    hash.update((value.len() as u64).to_le_bytes());
    hash.update(value.as_bytes());
}
fn integer(hash: &mut Sha256, value: i32) {
    hash.update(value.to_le_bytes());
}
fn optional_integer(hash: &mut Sha256, value: Option<i32>) {
    hash.update([u8::from(value.is_some())]);
    if let Some(value) = value {
        integer(hash, value);
    }
}
fn number(hash: &mut Sha256, value: f64) {
    hash.update(value.to_bits().to_le_bytes());
}
fn optional_number(hash: &mut Sha256, value: Option<f64>) {
    hash.update([u8::from(value.is_some())]);
    if let Some(value) = value {
        number(hash, value);
    }
}
fn option_json(value: Option<f64>) -> String {
    value.map_or_else(|| "null".to_owned(), |value| value.to_string())
}
fn option_integer_json(value: Option<i32>) -> String {
    value.map_or_else(|| "null".to_owned(), |value| value.to_string())
}

fn json_string(value: &str) -> String {
    let mut result = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            value if value.is_control() => {
                use std::fmt::Write as _;
                write!(result, "\\u{:04x}", u32::from(value))
                    .expect("writing to String cannot fail");
            }
            value => result.push(value),
        }
    }
    result.push('"');
    result
}

pub(crate) fn compare(previous: &[SampleFingerprint], next: &[SampleFingerprint]) -> Change {
    let previous = map(previous);
    let next = map(next);
    if previous.is_empty() {
        return summary(
            ChangeKind::Initial,
            next.values().copied(),
            next.len(),
            0,
            0,
        );
    }
    let removed = previous
        .keys()
        .filter(|key| !next.contains_key(*key))
        .count();
    let added: Vec<_> = next
        .iter()
        .filter(|(key, _)| !previous.contains_key(*key))
        .map(|(_, value)| *value)
        .collect();
    let modified: Vec<_> = next
        .iter()
        .filter_map(|(key, value)| {
            previous
                .get(key)
                .filter(|old| old.digest != value.digest)
                .map(|_| *value)
        })
        .collect();
    if removed != 0 {
        return summary(
            ChangeKind::RejectedTruncation,
            next.values().copied(),
            added.len(),
            modified.len(),
            removed,
        );
    }
    if added.is_empty() && modified.is_empty() {
        return summary(ChangeKind::Unchanged, next.values().copied(), 0, 0, 0);
    }
    let latest = previous
        .values()
        .map(|sample| sample.epoch_ms)
        .max()
        .unwrap_or(i64::MIN);
    let kind = if modified.is_empty() && added.iter().all(|sample| sample.epoch_ms > latest) {
        ChangeKind::Append
    } else if modified.is_empty() {
        ChangeKind::Backfill
    } else {
        ChangeKind::Revision
    };
    let added_count = added.len();
    let modified_count = modified.len();
    let changed = added.into_iter().chain(modified).collect::<Vec<_>>();
    summary(
        kind,
        changed.iter().copied(),
        added_count,
        modified_count,
        0,
    )
}

fn map(samples: &[SampleFingerprint]) -> BTreeMap<&str, &SampleFingerprint> {
    samples
        .iter()
        .map(|sample| (sample.key.as_str(), sample))
        .collect()
}
fn summary<'a>(
    kind: ChangeKind,
    changed: impl IntoIterator<Item = &'a SampleFingerprint>,
    added: usize,
    modified: usize,
    removed: usize,
) -> Change {
    let epochs: Vec<_> = changed.into_iter().map(|sample| sample.epoch_ms).collect();
    Change {
        kind,
        added,
        modified,
        removed,
        start_ms: epochs.iter().min().copied(),
        end_ms: epochs.iter().max().and_then(|value| value.checked_add(1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{HourlyRecord, ParsedFile};

    fn state(values: &[(i64, f64)]) -> Vec<SampleFingerprint> {
        fingerprints(&ParsedFile::Dst(
            values
                .iter()
                .map(|(epoch_ms, value)| HourlyRecord {
                    epoch_ms: *epoch_ms,
                    value: Some(*value),
                })
                .collect(),
        ))
    }

    #[test]
    fn classifies_a_b_c_d_e_and_retains_only_revision_predecessors() {
        let state_a = state(&[(0, 1.0)]);
        let state_b = state(&[(0, 1.0), (1, 2.0)]);
        let state_c = state(&[(0, 3.0), (1, 2.0)]);
        let state_d = state(&[(0, 3.0), (1, 2.0), (2, 4.0)]);
        let state_e = state(&[(0, 5.0), (1, 2.0), (2, 4.0)]);
        assert_eq!(compare(&[], &state_a).kind, ChangeKind::Initial);
        assert_eq!(compare(&state_a, &state_b).kind, ChangeKind::Append);
        assert_eq!(compare(&state_b, &state_c).kind, ChangeKind::Revision);
        assert_eq!(compare(&state_c, &state_d).kind, ChangeKind::Append);
        assert_eq!(compare(&state_d, &state_e).kind, ChangeKind::Revision);
    }

    #[test]
    fn classifies_backfill_and_rejects_truncation() {
        let initial = state(&[(1, 1.0)]);
        let backfill = state(&[(0, 0.5), (1, 1.0)]);
        let shortened = state(&[(0, 0.5)]);
        assert_eq!(compare(&initial, &backfill).kind, ChangeKind::Backfill);
        assert_eq!(
            compare(&backfill, &shortened).kind,
            ChangeKind::RejectedTruncation
        );
    }

    #[test]
    fn a_b_a_is_a_revision_activation_not_an_append() {
        let state_a = state(&[(0, 1.0)]);
        let state_b = state(&[(0, 2.0)]);
        assert_eq!(compare(&state_a, &state_b).kind, ChangeKind::Revision);
        assert_eq!(compare(&state_b, &state_a).kind, ChangeKind::Revision);
    }
}
