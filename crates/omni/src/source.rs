use ionoray_store::{DownloadRequest, SourceContext};
use url::Url;

use crate::{OmniError, OmniProduct, range::Month};

pub(crate) const BASE: &str = "https://cdaweb.gsfc.nasa.gov/pub/data/omni/omni_cdaweb/";

pub(crate) fn directory(base: &str, product: OmniProduct, month: Month) -> String {
    format!("{base}{}/{}/", product.dataset(), month.year)
}

pub(crate) fn request(
    base: &str,
    product: OmniProduct,
    month: Month,
    filename: &str,
) -> Result<DownloadRequest, OmniError> {
    let url = Url::parse(&format!("{}{filename}", directory(base, product, month)))
        .map_err(|e| OmniError::Invalid(e.to_string()))?;
    Ok(DownloadRequest::new(
        format!(
            "nasa.omni.{}.{:04}{:02}",
            product.dataset(),
            month.year,
            month.month
        ),
        url,
        filename,
    )?
    .with_context(SourceContext {
        provider: "nasa-spdf".into(),
        dataset: product.dataset().into(),
        year: month.year,
        month: Some(month.month),
        day: None,
        edition: filename.into(),
        priority: 0,
    }))
}

/// Select only exact monthly CDF basenames; never follow arbitrary HTML URLs.
pub(crate) fn latest_filename(
    html: &str,
    product: OmniProduct,
    month: Month,
) -> Result<Option<String>, OmniError> {
    if !html.contains("Index of") {
        return Err(OmniError::Invalid(
            "unexpected NASA directory response (expected Index of)".into(),
        ));
    }
    let prefix = format!(
        "omni_{}_{:04}{:02}01_v",
        product.dataset(),
        month.year,
        month.month
    );
    let mut best: Option<(u32, String)> = None;
    for token in html.split(|c: char| c.is_whitespace() || "\"'<>/=?#".contains(c)) {
        if let Some(version) = token
            .strip_prefix(&prefix)
            .and_then(|s| s.strip_suffix(".cdf"))
            && !version.is_empty()
            && version.bytes().all(|b| b.is_ascii_digit())
        {
            let version = version
                .parse::<u32>()
                .map_err(|e| OmniError::Invalid(e.to_string()))?;
            if best.as_ref().is_none_or(|(current, _)| version > *current) {
                best = Some((version, token.to_owned()));
            }
        }
    }
    Ok(best.map(|(_, filename)| filename))
}

pub(crate) fn missing(error: &ionoray_store::DownloadError) -> bool {
    matches!(
        error,
        ionoray_store::DownloadError::UnexpectedStatus(404 | 410)
    )
}

pub(crate) fn file_version(
    filename: &str,
    product: OmniProduct,
    month: Month,
) -> Result<String, OmniError> {
    let prefix = format!(
        "omni_{}_{:04}{:02}01_",
        product.dataset(),
        month.year,
        month.month
    );
    let version = filename
        .strip_prefix(&prefix)
        .and_then(|s| s.strip_suffix(".cdf"))
        .filter(|s| {
            s.strip_prefix('v')
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
        .ok_or_else(|| {
            OmniError::Invalid(format!(
                "invalid OMNI filename for selected product/month: {filename}"
            ))
        })?;
    Ok(version.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selects_numeric_version_only_for_requested_product_and_month() {
        let html = "Index of <a href='omni_hro2_1min_20240501_v02.cdf'>x</a> omni_hro2_1min_20240501_v10.cdf omni_hro_1min_20240501_v99.cdf omni_hro2_1min_20240601_v99.cdf";
        assert_eq!(
            latest_filename(
                html,
                OmniProduct::Hro2,
                Month {
                    year: 2024,
                    month: 5
                }
            )
            .unwrap()
            .unwrap(),
            "omni_hro2_1min_20240501_v10.cdf"
        );
        assert!(
            latest_filename(
                "<html>upstream error</html>",
                OmniProduct::Hro2,
                Month {
                    year: 2024,
                    month: 5
                }
            )
            .is_err()
        );
    }
    #[test]
    fn fallback_only_on_absence() {
        assert!(missing(&ionoray_store::DownloadError::UnexpectedStatus(
            404
        )));
        assert!(!missing(&ionoray_store::DownloadError::UnexpectedStatus(
            503
        )));
        assert_eq!(
            OmniProduct::candidates(None),
            vec![OmniProduct::Hro2, OmniProduct::Hro]
        );
        assert_eq!(
            OmniProduct::candidates(Some(OmniProduct::Hro2)),
            vec![OmniProduct::Hro2]
        );
    }
}
