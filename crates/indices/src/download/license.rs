//! Provider terms follow raw and embedded data, independent of the software license.
use crate::IndexDataset;
use std::sync::Once;
static GFZ: Once = Once::new();
static IRI: Once = Once::new();
pub(super) fn notice(dataset: IndexDataset) {
    match dataset {
        IndexDataset::KpApF107 => GFZ.call_once(|| {
            tracing::info!(
                license = "CC BY 4.0; included SN: CC BY-NC 4.0",
                source = "https://kp.gfz.de/en/data",
                "GFZ data retains provider attribution and SN terms"
            );
        }),
        IndexDataset::IriIgRz | IndexDataset::IriApF107 => IRI.call_once(|| {
            tracing::info!(
                license = "IRI upstream terms",
                source = "https://irimodel.org/IRI-2020/00_iri-License.txt",
                "IRI indices retain provider terms and scientific acknowledgement"
            );
        }),
        IndexDataset::Dst | IndexDataset::Ae => {}
    }
}
