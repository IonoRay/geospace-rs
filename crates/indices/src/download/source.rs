use crate::{IndexDataset, IndexEdition, IndexError, download::cache};

pub(crate) struct SourceFile {
    pub(crate) dataset: IndexDataset,
    pub(crate) year: u16,
    pub(crate) month: Option<u8>,
    /// Day within the month for a daily physical source.
    pub(crate) day: Option<u8>,
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
        files.extend(ae_files(
            year,
            month,
            1,
            crate::parse::days_in_month(year, month),
        ));
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
        IndexDataset::Ae if year >= 1958 => Ok((1..=12)
            .flat_map(|month| ae_files(year, month, 1, crate::parse::days_in_month(year, month)))
            .collect()),
        IndexDataset::IriIgRz if year >= 1958 => Ok(vec![iri_ig_rz(year)]),
        IndexDataset::IriApF107 if year >= 1958 => Ok(vec![iri_apf107(year)]),
        IndexDataset::KpApF107
        | IndexDataset::Ae
        | IndexDataset::IriIgRz
        | IndexDataset::IriApF107 => Err(IndexError::UnsupportedYear { dataset, year }),
    }
}

/// Plans only physical monthly/daily files intersecting a UTC half-open interval.
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
                        if dataset == IndexDataset::Dst {
                            files.push(dst(year, month));
                        } else {
                            let first = if year == start_year && month == start.1 {
                                start.2
                            } else {
                                1
                            };
                            let last = if year == end_year && month == end.1 {
                                end.2
                            } else {
                                crate::parse::days_in_month(year, month)
                            };
                            files.extend(ae_files(year, month, first, last));
                        }
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
        day: None,
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
        day: None,
        logical_name: filename.clone(),
        candidates: wdc_candidates("dst", &year_month, &filename, true),
        cache: None,
    }
}

fn ae_files(year: u16, month: u8, first: u8, last: u8) -> Vec<SourceFile> {
    if year >= 2021 {
        return (first..=last)
            .map(|day| ae_daily(year, month, day))
            .collect();
    }
    // Kyoto's historical provisional series is published as monthly E02 files.
    let year_month = format!("{year}{month:02}");
    let filename = format!("ae{:02}{month:02}.for.request", year % 100);
    vec![SourceFile {
        dataset: IndexDataset::Ae,
        year,
        month: Some(month),
        day: None,
        logical_name: filename.clone(),
        candidates: vec![SourceCandidate {
            source_id: format!("wdc-kyoto.ae.provisional.{year_month}"),
            edition: IndexEdition::Provisional,
            url: format!("https://wdc.kugi.kyoto-u.ac.jp/ae_provisional/{year_month}/{filename}"),
        }],
        cache: None,
    }]
}

fn ae_daily(year: u16, month: u8, day: u8) -> SourceFile {
    let stamp = format!("{year}{month:02}{day:02}");
    let filename = format!("ae{:02}{month:02}{day:02}", year % 100);
    SourceFile {
        dataset: IndexDataset::Ae,
        year,
        month: Some(month),
        day: Some(day),
        logical_name: filename.clone(),
        candidates: vec![
            SourceCandidate {
                source_id: format!("wdc-kyoto.ae.provisional.{stamp}"),
                edition: IndexEdition::Provisional,
                url: format!(
                    "https://wdc.kugi.kyoto-u.ac.jp/ae_provisional/{year}{month:02}/{filename}.for.request"
                ),
            },
            SourceCandidate {
                source_id: format!("wdc-kyoto.ae.realtime.{stamp}"),
                edition: IndexEdition::Realtime,
                url: format!(
                    "https://wdc.kugi.kyoto-u.ac.jp/ae_realtime/data_dir/{year}/{month:02}/{day:02}/{filename}"
                ),
            },
        ],
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
        day: None,
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
    #[test]
    fn ae_daily_plan_handles_leap_days_and_half_open_intervals() {
        let files = files_for_dataset_year(IndexDataset::Ae, 2024).unwrap();
        assert_eq!(files.len(), 366);
        let leap = files
            .iter()
            .find(|file| file.month == Some(2) && file.day == Some(29))
            .unwrap();
        assert_eq!(
            leap.candidates[1].url,
            "https://wdc.kugi.kyoto-u.ac.jp/ae_realtime/data_dir/2024/02/29/ae240229"
        );
        let start = crate::parse::epoch_ms(2024, 2, 29, 23).unwrap();
        let end = crate::parse::epoch_ms(2024, 3, 1, 0).unwrap();
        let bounded = files_for_dataset_interval(IndexDataset::Ae, start, end).unwrap();
        assert_eq!(bounded.len(), 1);
        assert_eq!(bounded[0].day, Some(29));
        assert_eq!(
            files_for_dataset_year(IndexDataset::Ae, 2023)
                .unwrap()
                .len(),
            365
        );
        let historic = files_for_dataset_year(IndexDataset::Ae, 2020).unwrap();
        assert_eq!(historic.len(), 12);
        assert_eq!(historic[0].day, None);
        assert_eq!(historic[0].candidates.len(), 1);
    }
}
