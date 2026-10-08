use crate::IndexDataset;

use super::cache_manifest::{
    GFZ_END_YEAR, GFZ_SHA256, GFZ_START_YEAR, IRI_APF107_END_YEAR, IRI_APF107_SHA256,
    IRI_APF107_START_YEAR, IRI_IG_RZ_END_YEAR, IRI_IG_RZ_SHA256, IRI_IG_RZ_START_YEAR,
    SNAPSHOT_AT_UTC_MS,
};

include!(concat!(env!("OUT_DIR"), "/optional_cache.rs"));

pub(crate) struct BundledSource {
    pub(crate) bytes: &'static [u8],
    pub(crate) sha256: &'static str,
    pub(crate) source_modified_at_utc_ms: i64,
}

pub(crate) fn for_dataset(dataset: IndexDataset, year: u16) -> Option<BundledSource> {
    match dataset {
        IndexDataset::KpApF107 if (GFZ_START_YEAR..=GFZ_END_YEAR).contains(&year) => {
            Some(BundledSource {
                bytes: GFZ_BYTES?,
                sha256: GFZ_SHA256,
                source_modified_at_utc_ms: SNAPSHOT_AT_UTC_MS,
            })
        }
        IndexDataset::IriIgRz if (IRI_IG_RZ_START_YEAR..=IRI_IG_RZ_END_YEAR).contains(&year) => {
            Some(BundledSource {
                bytes: IG_RZ_BYTES?,
                sha256: IRI_IG_RZ_SHA256,
                source_modified_at_utc_ms: SNAPSHOT_AT_UTC_MS,
            })
        }
        IndexDataset::IriApF107
            if (IRI_APF107_START_YEAR..=IRI_APF107_END_YEAR).contains(&year) =>
        {
            Some(BundledSource {
                bytes: APF107_BYTES?,
                sha256: IRI_APF107_SHA256,
                source_modified_at_utc_ms: SNAPSHOT_AT_UTC_MS,
            })
        }
        IndexDataset::KpApF107
        | IndexDataset::Dst
        | IndexDataset::Ae
        | IndexDataset::IriIgRz
        | IndexDataset::IriApF107 => None,
    }
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::*;

    #[test]
    fn manifest_hashes_match_packaged_bytes() {
        for dataset in [
            IndexDataset::KpApF107,
            IndexDataset::IriIgRz,
            IndexDataset::IriApF107,
        ] {
            let Some(cache) = for_dataset(dataset, 2020) else {
                continue;
            };
            let actual = ionoray_core::Sha256Digest::from_bytes(Sha256::digest(cache.bytes).into());
            assert_eq!(actual.to_hex(), cache.sha256);
        }
    }

    #[test]
    fn coverage_excludes_partial_snapshot_years() {
        assert_eq!(
            for_dataset(IndexDataset::KpApF107, 2025).is_some(),
            GFZ_BYTES.is_some()
        );
        assert!(for_dataset(IndexDataset::KpApF107, 2026).is_none());
        assert_eq!(
            for_dataset(IndexDataset::IriIgRz, IRI_IG_RZ_END_YEAR).is_some(),
            IG_RZ_BYTES.is_some()
        );
        assert!(for_dataset(IndexDataset::IriIgRz, IRI_IG_RZ_END_YEAR + 1).is_none());
        assert_eq!(
            for_dataset(IndexDataset::IriApF107, IRI_APF107_END_YEAR).is_some(),
            APF107_BYTES.is_some()
        );
        assert!(for_dataset(IndexDataset::IriApF107, IRI_APF107_END_YEAR + 1).is_none());
        assert!(for_dataset(IndexDataset::Dst, 2020).is_none());
    }
}
