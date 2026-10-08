use crate::{IndexDataset, IndexEdition, IndexError, download::cache};

pub(crate) struct SourceFile {
    pub(crate) dataset: IndexDataset,
    pub(crate) year: u16,
    pub(crate) month: Option<u8>,
    pub(crate) logical_name: String,
    pub(crate) candidates: Vec<SourceCandidate>,
    pub(crate) cache: Option<cache::BundledSource>,
}

pub(crate) struct SourceCandidate {
    pub(crate) source_id: String,
    pub(crate) edition: IndexEdition,
    pub(crate) url: String,
}

pub(crate) fn files_for_year(year: u16) -> Result<Vec<SourceFile>, IndexError> {
    if year < 1958 {
        return Err(IndexError::UnsupportedYear {
            dataset: IndexDataset::Ae,
            year,
        });
    }
    let mut files = Vec::with_capacity(27);
    files.push(gfz(year));
    for month in 1..=12 {
        files.push(dst(year, month));
    }
    for month in 1..=12 {
        files.push(ae(year, month));
    }
    files.push(iri_ig_rz(year));
    files.push(iri_apf107(year));
    Ok(files)
}

pub(crate) fn files_for_dataset_year(
    dataset: IndexDataset,
    year: u16,
) -> Result<Vec<SourceFile>, IndexError> {
    match dataset {
        IndexDataset::KpApF107 if year >= 1932 => Ok(vec![gfz(year)]),
        IndexDataset::Dst => Ok((1..=12).map(|month| dst(year, month)).collect()),
        IndexDataset::Ae if year >= 1958 => Ok((1..=12).map(|month| ae(year, month)).collect()),
        IndexDataset::IriIgRz if year >= 1958 => Ok(vec![iri_ig_rz(year)]),
        IndexDataset::IriApF107 if year >= 1958 => Ok(vec![iri_apf107(year)]),
        IndexDataset::KpApF107
        | IndexDataset::Ae
        | IndexDataset::IriIgRz
        | IndexDataset::IriApF107 => Err(IndexError::UnsupportedYear { dataset, year }),
    }
}

/// Plans only monthly physical files intersecting a UTC half-open interval.
/// Rolling sources remain one physical source for the caller's first year.
pub(crate) fn files_for_dataset_interval(
    dataset: IndexDataset,
    start_utc_ms: i64,
    end_utc_ms: i64,
) -> Result<Vec<SourceFile>, IndexError> {
    let start = crate::time::epoch_from_millis(start_utc_ms).to_gregorian_utc();
    let end = crate::time::epoch_from_millis(end_utc_ms.saturating_sub(1)).to_gregorian_utc();
    let start_year = u16::try_from(start.0).map_err(|_| IndexError::InvalidNumber("range year"))?;
    let end_year = u16::try_from(end.0).map_err(|_| IndexError::InvalidNumber("range year"))?;
    match dataset {
        IndexDataset::Dst | IndexDataset::Ae => {
            let mut files = Vec::new();
            for year in start_year..=end_year {
                for month in 1..=12 {
                    let begins_after = year == start_year && month < start.1;
                    let ends_before = year == end_year && month > end.1;
                    if !begins_after && !ends_before {
                        files.push(if dataset == IndexDataset::Dst {
                            dst(year, month)
                        } else {
                            ae(year, month)
                        });
                    }
                }
            }
            Ok(files)
        }
        _ => files_for_dataset_year(dataset, start_year),
    }
}

fn gfz(year: u16) -> SourceFile {
    SourceFile {
        dataset: IndexDataset::KpApF107,
        year,
        month: None,
        logical_name: "Kp_ap_Ap_SN_F107_since_1932.txt".to_owned(),
        candidates: vec![SourceCandidate {
            // A rolling file is one physical source object.  The requested
            // year belongs to its parsed partition, not its identity.
            source_id: "gfz.kp-ap-f107.rolling".to_owned(),
            edition: IndexEdition::Rolling,
            url: "https://kp.gfz.de/fileadmin/files_for_gfz_cms/Kp_ap_Ap_SN_F107_since_1932.txt"
                .to_owned(),
        }],
        cache: cache::for_dataset(IndexDataset::KpApF107, year),
    }
}

fn dst(year: u16, month: u8) -> SourceFile {
    let year_month = format!("{year}{month:02}");
    let filename = format!("dst{:02}{month:02}.for.request", year % 100);
    SourceFile {
        dataset: IndexDataset::Dst,
        year,
        month: Some(month),
        logical_name: filename.clone(),
        candidates: wdc_candidates("dst", &year_month, &filename, true),
        cache: None,
    }
}

fn ae(year: u16, month: u8) -> SourceFile {
    let year_month = format!("{year}{month:02}");
    let filename = format!("ae{:02}{month:02}.for.request", year % 100);
    SourceFile {
        dataset: IndexDataset::Ae,
        year,
        month: Some(month),
        logical_name: filename.clone(),
        candidates: wdc_candidates("ae", &year_month, &filename, false),
        cache: None,
    }
}

fn iri_ig_rz(year: u16) -> SourceFile {
    iri_rolling(
        IndexDataset::IriIgRz,
        year,
        "ig_rz.dat",
        "https://irimodel.org/indices/ig_rz.dat",
    )
}

fn iri_apf107(year: u16) -> SourceFile {
    iri_rolling(
        IndexDataset::IriApF107,
        year,
        "apf107.dat",
        "https://irimodel.org/indices/apf107.dat",
    )
}

fn iri_rolling(dataset: IndexDataset, year: u16, logical_name: &str, url: &str) -> SourceFile {
    SourceFile {
        dataset,
        year,
        month: None,
        logical_name: logical_name.to_owned(),
        candidates: vec![SourceCandidate {
            // Do not turn a cumulative IRI file into a new remote identity
            // for every requested year.
            source_id: format!("iri.{}.rolling", dataset.as_str()),
            edition: IndexEdition::Rolling,
            url: url.to_owned(),
        }],
        cache: cache::for_dataset(dataset, year),
    }
}

fn wdc_candidates(
    prefix: &str,
    year_month: &str,
    filename: &str,
    has_final: bool,
) -> Vec<SourceCandidate> {
    let editions = if has_final {
        &[
            (IndexEdition::Final, "final"),
            (IndexEdition::Provisional, "provisional"),
            (IndexEdition::Realtime, "realtime"),
        ][..]
    } else {
        &[
            (IndexEdition::Provisional, "provisional"),
            (IndexEdition::Realtime, "realtime"),
        ][..]
    };
    editions
        .iter()
        .map(|(edition, maturity)| SourceCandidate {
            source_id: format!("wdc-kyoto.{prefix}.{maturity}.{year_month}"),
            edition: *edition,
            url: format!(
                "https://wdc.kugi.kyoto-u.ac.jp/{prefix}_{maturity}/{year_month}/{filename}"
            ),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gfz_model_drivers_are_independent_of_ae_year_coverage() {
        let files = files_for_dataset_year(IndexDataset::KpApF107, 1950).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].dataset, IndexDataset::KpApF107);
        assert!(matches!(
            files_for_year(1950),
            Err(IndexError::UnsupportedYear {
                dataset: IndexDataset::Ae,
                year: 1950
            })
        ));
    }

    #[test]
    fn monthly_interval_plan_uses_only_intersecting_months() {
        let start = crate::time::epoch_millis(
            ionoray_core::Epoch::maybe_from_gregorian_utc(2020, 1, 31, 23, 0, 0, 0).unwrap(),
        )
        .unwrap();
        let end = crate::time::epoch_millis(
            ionoray_core::Epoch::maybe_from_gregorian_utc(2020, 3, 1, 1, 0, 0, 0).unwrap(),
        )
        .unwrap();
        let files = files_for_dataset_interval(IndexDataset::Dst, start, end).unwrap();
        assert_eq!(
            files.iter().map(|file| file.month).collect::<Vec<_>>(),
            vec![Some(1), Some(2), Some(3)]
        );
    }
}
