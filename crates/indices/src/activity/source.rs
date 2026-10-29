use ionoray_store::{DownloadRequest, SourceContext};
use url::Url;

use super::{ActivityDataset, ActivityError, ActivityRange};

#[derive(Clone)]
pub(super) struct Source {
    pub id: String,
    pub provider: &'static str,
    pub edition: &'static str,
    pub priority: i32,
    pub year: u16,
    pub month: Option<u8>,
    pub day: Option<u8>,
    pub url: Url,
    pub filename: String,
}

impl Source {
    pub fn request(&self, dataset: ActivityDataset) -> Result<DownloadRequest, ActivityError> {
        Ok(
            DownloadRequest::new(&self.id, self.url.clone(), &self.filename)?.with_context(
                SourceContext {
                    provider: self.provider.into(),
                    dataset: dataset.name().into(),
                    year: self.year,
                    month: self.month,
                    day: self.day,
                    edition: self.edition.into(),
                    priority: self.priority,
                },
            ),
        )
    }
}

pub(super) fn provider(dataset: ActivityDataset) -> &'static str {
    match dataset {
        ActivityDataset::Hp30 | ActivityDataset::Hp60 => "gfz",
        ActivityDataset::Pcn => "dtu-space",
    }
}

pub(super) type Slot = ((u16, u8, u8), Vec<Source>);

pub(super) fn slots(
    dataset: ActivityDataset,
    range: ActivityRange,
) -> Result<Vec<Slot>, ActivityError> {
    let mut result = Vec::new();
    for date @ (year, month, day) in range.days() {
        let sources = match dataset {
            ActivityDataset::Hp30 | ActivityDataset::Hp60 => {
                if year < 1985 {
                    return Err(ActivityError::Invalid("Hpo v3 begins in 1985".into()));
                }
                let prefix = if dataset == ActivityDataset::Hp30 {
                    "Hp30_ap30"
                } else {
                    "Hp60_ap60"
                };
                let filename = format!("{prefix}_complete_series.txt");
                vec![Source {
                    id: format!("gfz.{}.rolling", dataset.name()),
                    provider: "gfz",
                    edition: "rolling-v3",
                    priority: 0,
                    year,
                    month: None,
                    day: None,
                    url: Url::parse(&format!(
                        "https://kp.gfz.de/fileadmin/files_for_gfz_cms/{filename}"
                    ))?,
                    filename,
                }]
            }
            ActivityDataset::Pcn => {
                if year < 1975 {
                    return Err(ActivityError::Invalid("PCN begins in 1975".into()));
                }
                let annual = format!("pcn{year}d.dat");
                let mut candidates = vec![Source {
                    id: format!("dtu.pcn.definitive.{year}"),
                    provider: "dtu-space",
                    edition: "definitive",
                    priority: 300,
                    year,
                    month: None,
                    day: None,
                    url: Url::parse(&format!(
                        "https://ftp.space.dtu.dk/WDC/indices/pcn/PCN_definitive/{annual}"
                    ))?,
                    filename: annual,
                }];
                for (directory, suffix, edition, priority) in [
                    ("PCN_provisional", "p", "provisional", 200),
                    ("PCN_quicklook", "q", "quicklook", 100),
                ] {
                    let filename = format!("pcn{year}{month:02}{day:02}{suffix}.csv");
                    candidates.push(Source {
                        id: format!("dtu.pcn.{edition}.{year}{month:02}{day:02}"),
                        provider: "dtu-space",
                        edition,
                        priority,
                        year,
                        month: Some(month),
                        day: Some(day),
                        url: Url::parse(&format!(
                            "https://ftp.space.dtu.dk/WDC/indices/pcn/{directory}/{filename}"
                        ))?,
                        filename,
                    });
                }
                candidates
            }
        };
        result.push((date, sources));
    }
    Ok(result)
}

pub(super) fn absent(error: &ionoray_store::DownloadError) -> bool {
    matches!(
        error,
        ionoray_store::DownloadError::UnexpectedStatus(404 | 410)
    )
}

pub(super) fn validate_partition(
    source: &Source,
    dataset: ActivityDataset,
    parsed: &super::parse::Parsed,
) -> Result<(), ActivityError> {
    if dataset != ActivityDataset::Pcn {
        return Ok(());
    }
    for time in [parsed.source_first, parsed.source_last] {
        let (year, month, day, ..) = super::range::epoch(time).to_gregorian_utc();
        if year != i32::from(source.year)
            || source.month.is_some_and(|m| m != month)
            || source.day.is_some_and(|d| d != day)
        {
            return Err(ActivityError::Invalid(
                "PCN calendar coverage disagrees with its annual/daily source identity".into(),
            ));
        }
    }
    Ok(())
}
