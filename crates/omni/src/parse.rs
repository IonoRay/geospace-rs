//! OMNI-specific extraction from cdf-rs records; no alternative CDF decoder.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::BufReader,
    path::Path,
};

use cdf::{
    cdf::Cdf,
    decode::Decoder,
    record::{
        collection::get_record_vec,
        vxr::{VariableIndexRecord, VariableIndexRecordChild},
        zvdr::ZVariableDescriptorRecord,
    },
    types::CdfType,
};

use crate::{
    OmniError, OmniProduct, SampleValue, VariableMetadata,
    range::{Month, epoch},
};

pub(crate) const VARIABLES: &[&str] = &[
    "F",
    "BX_GSE",
    "BY_GSE",
    "BZ_GSE",
    "BY_GSM",
    "BZ_GSM",
    "flow_speed",
    "Vx",
    "Vy",
    "Vz",
    "proton_density",
    "T",
    "Pressure",
    "IMF",
    "PLS",
    "IMF_PTS",
    "PLS_PTS",
    "percent_interp",
    "Timeshift",
    "RMS_Timeshift",
    "RMS_phase",
    "Time_btwn_obs",
    "RMS_SD_B",
    "RMS_SD_fld_vec",
];

pub(crate) fn decode(path: &Path, product: OmniProduct, month: Month) -> Result<Cdf, OmniError> {
    let mut cdf = Cdf::read_cdf_file(path)?;
    if cdf.is_compressed {
        return Err(invalid(
            "whole-file compression unsupported by cdf-rs 0.1.4",
        ));
    }
    // cdf-rs expands top-level VXR chains but exposes nested continuations as
    // offsets. Follow those with its public record-list API, for OMNI exports.
    let mut decoder = Decoder::new(BufReader::new(File::open(path)?))?;
    decoder.context.encoding = Some(cdf.cdr.encoding.clone());
    decoder.context.endianness = Some(cdf.cdr.encoding.get_endian()?);
    decoder.context.version = Some(cdf.cdr.cdf_version.clone());
    for var in &mut cdf.cdr.gdr.zvdr_vec {
        if var.name.to_string() != "Epoch" && !VARIABLES.contains(&var.name.to_string().as_str()) {
            continue;
        }
        scalar_values(var)?; // Reject unsupported shape/compression before decoding more values.
        decoder.context.var_data_type = Some(var.data_type.clone());
        decoder.context.var_data_len = Some(1.into());
        let mut links = Vec::new();
        for index in &var.vxr_vec {
            nested_links(index, &mut links);
        }
        let mut visited = BTreeSet::new();
        while let Some(link) = links.pop() {
            if !visited.insert(*link) {
                return Err(invalid("duplicate nested VXR continuation"));
            }
            let extra = get_record_vec::<_, VariableIndexRecord>(&mut decoder, &link)?;
            for index in &extra {
                nested_links(index, &mut links);
            }
            var.vxr_vec.extend(extra);
        }
    }
    let logical = cdf
        .cdr
        .gdr
        .adr_vec
        .iter()
        .find(|a| a.name.to_string() == "Logical_source")
        .and_then(|a| a.agredr_vec.first())
        .and_then(|a| a.value.first());
    if !matches!(logical, Some(CdfType::String(s)) if s.to_string().trim() == format!("omni_{}", product.dataset()))
    {
        return Err(invalid(
            "CDF Logical_source does not match selected product",
        ));
    }
    let times = times(&cdf)?;
    let mut previous = None;
    for &time in &times {
        let unix = unix_millis(time)?;
        let (year, m, ..) = epoch(unix).to_gregorian_utc();
        if year != i32::from(month.year)
            || m != month.month
            || unix.rem_euclid(60_000) != 0
            || previous.is_some_and(|p| p >= unix)
        {
            return Err(invalid(
                "Epoch must be strictly increasing UTC minutes within its source month",
            ));
        }
        previous = Some(unix);
    }
    if times.is_empty() {
        return Err(invalid("empty Epoch variable"));
    }
    for &name in VARIABLES {
        let variable = variable(&cdf, name)?;
        let values = scalar_values(variable)?;
        if values.len() != times.len() || values.iter().any(Option::is_none) {
            return Err(invalid(&format!(
                "incomplete dense records or count differs from Epoch for {name}"
            )));
        }
        if attribute(&cdf, variable, "FILLVAL")
            .and_then(|v| v.first())
            .and_then(numeric)
            .is_none()
        {
            return Err(invalid(&format!("missing numeric FILLVAL for {name}")));
        }
        let depends = attribute(&cdf, variable, "DEPEND_0");
        if !matches!(depends.and_then(|v| v.first()), Some(CdfType::String(s)) if s.to_string() == "Epoch")
        {
            return Err(invalid(&format!("unexpected time dependency for {name}")));
        }
        if VARIABLES[..13].contains(&name) && attribute(&cdf, variable, "UNITS").is_none() {
            return Err(invalid(&format!("missing UNITS for {name}")));
        }
    }
    Ok(cdf)
}

pub(crate) fn variable<'a>(
    cdf: &'a Cdf,
    name: &str,
) -> Result<&'a ZVariableDescriptorRecord, OmniError> {
    cdf.cdr
        .gdr
        .zvdr_vec
        .iter()
        .find(|v| v.name.to_string() == name)
        .ok_or_else(|| invalid(&format!("missing OMNI variable {name}")))
}

pub(crate) fn times(cdf: &Cdf) -> Result<Vec<f64>, OmniError> {
    let var = variable(cdf, "Epoch")?;
    if *var.data_type != 31 {
        return Err(invalid("expected CDF_EPOCH time type"));
    }
    scalar_values(var)?
        .into_iter()
        .map(|v| v.ok_or_else(|| invalid("missing Epoch record")))
        .collect()
}

#[allow(clippy::cast_possible_truncation)]
pub(crate) fn unix_millis(cdf_epoch: f64) -> Result<i64, OmniError> {
    // CDF_EPOCH uses Gregorian milliseconds since 0000-01-01, without leap seconds.
    let unix = cdf_epoch - 62_167_219_200_000.0;
    if !unix.is_finite() || unix.abs() > 253_402_300_800_000.0 || unix.fract() != 0.0 {
        return Err(invalid("invalid CDF_EPOCH"));
    }
    Ok(unix as i64)
}

pub(crate) fn scalar_values(
    var: &ZVariableDescriptorRecord,
) -> Result<Vec<Option<f64>>, OmniError> {
    if var.flags.is_compressed
        || *var.sparse_records != 0
        || *var.num_elements != 1
        || var.size_z_dims.iter().any(|n| **n != 1)
    {
        return Err(invalid(&format!(
            "unsupported compressed/sparse/non-scalar OMNI variable {}",
            var.name
        )));
    }
    let len = usize::try_from(*var.max_record + 1).map_err(|e| invalid(&e.to_string()))?;
    if len > 44_640 {
        return Err(invalid("one-minute monthly record count exceeds 31 days"));
    }
    let mut values = vec![None; len];
    for index in &var.vxr_vec {
        collect(index, &mut values)?;
    }
    Ok(values)
}

fn collect(index: &VariableIndexRecord, values: &mut [Option<f64>]) -> Result<(), OmniError> {
    for (i, child) in index.children.iter().enumerate() {
        match child {
            Some(VariableIndexRecordChild::VXR(next)) => collect(next, values)?,
            Some(VariableIndexRecordChild::VVR(block)) => {
                let first = index
                    .first_vec
                    .get(i)
                    .and_then(Option::as_ref)
                    .ok_or_else(|| invalid("missing VXR first record"))?;
                let start = usize::try_from(**first).map_err(|e| invalid(&e.to_string()))?;
                for (offset, record) in block.records.iter().enumerate() {
                    if let Some(slot) = values.get_mut(start + offset) {
                        if slot.is_some() {
                            return Err(invalid("overlapping CDF records"));
                        }
                        if record.data.len() != 1 {
                            return Err(invalid("expected scalar CDF record"));
                        }
                        *slot = Some(
                            numeric(&record.data[0])
                                .ok_or_else(|| invalid("unsupported numeric CDF type"))?,
                        );
                    }
                }
            }
            Some(VariableIndexRecordChild::CVVR(_)) => {
                return Err(invalid("compressed values unsupported by cdf-rs 0.1.4"));
            }
            None => {}
        }
    }
    Ok(())
}

pub(crate) fn metadata(cdf: &Cdf) -> Result<Vec<VariableMetadata>, OmniError> {
    VARIABLES
        .iter()
        .map(|&name| {
            let variable = variable(cdf, name)?;
            let attributes = cdf
                .cdr
                .gdr
                .adr_vec
                .iter()
                .filter_map(|a| {
                    a.azedr_vec
                        .iter()
                        .find(|entry| *entry.num == *variable.num)
                        .map(|entry| {
                            (
                                a.name.to_string(),
                                entry
                                    .value
                                    .iter()
                                    .map(|v| match v {
                                        CdfType::String(s) => s.to_string(),
                                        _ => numeric(v)
                                            .map_or_else(|| format!("{v:?}"), |n| n.to_string()),
                                    })
                                    .collect(),
                            )
                        })
                })
                .collect::<BTreeMap<_, _>>();
            Ok(VariableMetadata {
                name: name.into(),
                attributes,
            })
        })
        .collect()
}

pub(crate) fn attribute<'a>(
    cdf: &'a Cdf,
    var: &ZVariableDescriptorRecord,
    name: &str,
) -> Option<&'a Vec<CdfType>> {
    cdf.cdr
        .gdr
        .adr_vec
        .iter()
        .find(|a| a.name.to_string() == name)?
        .azedr_vec
        .iter()
        .find(|a| *a.num == *var.num)
        .map(|a| &a.value)
}

pub(crate) fn sample(cdf: &Cdf, var: &ZVariableDescriptorRecord, raw: Option<f64>) -> SampleValue {
    let bound = |name| {
        attribute(cdf, var, name)
            .and_then(|v| v.first())
            .and_then(numeric)
    };
    let missing = match raw {
        None => Some("absent_record"),
        Some(v) if !v.is_finite() => Some("non_finite"),
        Some(v) if bound("FILLVAL") == Some(v) => Some("fill_value"),
        _ => None,
    };
    let outside_valid_range = missing.is_none()
        && raw.is_some_and(|v| {
            bound("VALIDMIN").is_some_and(|min| v < min)
                || bound("VALIDMAX").is_some_and(|max| v > max)
        });
    SampleValue {
        raw: raw.filter(|v| v.is_finite()),
        value: raw.filter(|_| missing.is_none()),
        missing,
        outside_valid_range,
    }
}

pub(crate) fn numeric(value: &CdfType) -> Option<f64> {
    match value {
        CdfType::Real4(v) => Some(f64::from(**v)),
        CdfType::Real8(v) => Some(**v),
        CdfType::Int4(v) => Some(f64::from(**v)),
        CdfType::Int2(v) => Some(f64::from(**v)),
        CdfType::Int1(v) => Some(f64::from(**v)),
        CdfType::Uint1(v) => Some(f64::from(**v)),
        CdfType::Uint2(v) => Some(f64::from(**v)),
        CdfType::Uint4(v) => Some(f64::from(**v)),
        CdfType::Epoch(v) => Some(**v),
        _ => None,
    }
}

fn invalid(message: &str) -> OmniError {
    OmniError::Invalid(message.into())
}

fn nested_links(index: &VariableIndexRecord, links: &mut Vec<cdf::types::CdfInt8>) {
    for child in index.children.iter().flatten() {
        if let VariableIndexRecordChild::VXR(child) = child {
            if let Some(next) = &child.vxr_next {
                links.push(next.clone());
            }
            nested_links(child, links);
        }
    }
}

#[cfg(test)]
#[path = "parse_tests.rs"]
mod tests;
