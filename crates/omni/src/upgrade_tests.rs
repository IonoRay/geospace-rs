use super::{
    tests::{Server, range},
    *,
};
use crate::{SelectionChange, SelectionReason};

fn fixtures() -> Option<(Vec<u8>, Vec<u8>)> {
    let mut bodies = Vec::new();
    for (env, default) in [
        ("OMNI_TEST_HRO_CDF", "/tmp/geospace-omni-hro-202405.cdf"),
        ("OMNI_TEST_CDF", "/tmp/geospace-omni-hro2-202405.cdf"),
    ] {
        let path = std::env::var_os(env).map_or_else(|| PathBuf::from(default), PathBuf::from);
        if !path.try_exists().unwrap() {
            eprintln!("not executed: missing {}", path.display());
            return None;
        }
        bodies.push(fs::read(path).unwrap());
    }
    let hro2 = bodies.pop().unwrap();
    Some((bodies.pop().unwrap(), hro2))
}

fn listing(product: OmniProduct, version: &str) -> (u16, Vec<u8>) {
    (
        200,
        format!("Index of omni_{}_20240501_{version}.cdf", product.dataset()).into_bytes(),
    )
}

async fn read_identity(store: &OmniStore, product: Option<OmniProduct>) -> crate::FileIdentity {
    let report = store
        .read(range(), product, |row| {
            assert_eq!(row.cadence, "1min");
            assert_eq!(row.partition, "2024-05");
            assert!(
                row.filename
                    .ends_with(&format!("_{}.cdf", row.file_version))
            );
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        report.selections[0].source_check,
        SourceCheck::OfflineNotChecked
    );
    assert_eq!(report.selections[0].change, None);
    assert_eq!(report.files[0].identity, report.selections[0].selected);
    report.files.into_iter().next().unwrap().identity
}

#[tokio::test]
#[ignore = "requires official May 2024 HRO/HRO2 CDFs; see docs/omni.md"]
async fn hro_upgrades_to_hro2_and_never_downgrades_on_absence() {
    let Some((hro, hro2)) = fixtures() else {
        return;
    };
    let server = Server::start(vec![
        // First run: HRO2 absent, so HRO is accepted.
        (404, vec![]),
        listing(OmniProduct::Hro, "v01"),
        (200, hro.clone()),
        // Second run: HRO2 appears and is accepted despite complete local HRO.
        listing(OmniProduct::Hro2, "v01"),
        (200, hro2.clone()),
        // Third run: matching HEAD metadata transfers no CDF body.
        listing(OmniProduct::Hro2, "v01"),
        (200, hro2.clone()),
        // Fourth run: HRO2 directory disappears; do not try HRO.
        (404, vec![]),
        // Fifth run: directory lists HRO2 but its file is absent (forced GET).
        listing(OmniProduct::Hro2, "v01"),
        (404, vec![]),
        // Explicit HRO remains selectable without changing automatic priority.
        listing(OmniProduct::Hro, "v01"),
        (200, hro),
    ]);
    let home = tempfile::tempdir().unwrap();
    let store = OmniStore::open_at(home.path(), &server.base).await.unwrap();
    let first = store.download(range(), None, false).await.unwrap();
    let original = &first.files[0];
    assert_eq!(original.identity.product, OmniProduct::Hro);
    assert_eq!(
        first.selections[0].change,
        Some(SelectionChange::FirstAcquired)
    );
    assert_eq!(
        first.selections[0].selection_reason,
        SelectionReason::FallbackProduct
    );
    assert_eq!(read_identity(&store, None).await, original.identity);

    let upgraded = store.download(range(), None, false).await.unwrap();
    let transition = &upgraded.selections[0];
    assert_eq!(transition.change, Some(SelectionChange::ProductUpgraded));
    assert_eq!(
        transition.previous_selected.as_ref(),
        Some(&original.identity)
    );
    assert_eq!(transition.selected.product, OmniProduct::Hro2);
    assert_eq!(
        transition.selection_reason,
        SelectionReason::PreferredProduct
    );
    assert_eq!(transition.source_check, SourceCheck::Checked);
    assert_eq!(read_identity(&store, None).await, transition.selected);
    assert!(original.artifact.path.is_file());
    assert!(original.browse_path.as_ref().unwrap().is_file());
    assert_eq!(
        read_identity(&store, Some(OmniProduct::Hro)).await,
        original.identity
    );

    let unchanged = store.download(range(), None, false).await.unwrap();
    assert_eq!(
        unchanged.selections[0].change,
        Some(SelectionChange::Unchanged)
    );
    for force in [false, true] {
        let retained = store.download(range(), None, force).await.unwrap();
        assert_eq!(
            retained.selections[0].source_check,
            SourceCheck::UpstreamMissing
        );
        assert_eq!(
            retained.selections[0].change,
            Some(SelectionChange::Unchanged)
        );
        assert_eq!(retained.files[0].identity, transition.selected);
        assert_eq!(read_identity(&store, None).await, transition.selected);
    }
    let explicit = store
        .download(range(), Some(OmniProduct::Hro), false)
        .await
        .unwrap();
    assert_eq!(
        explicit.selections[0].selection_reason,
        SelectionReason::ExplicitProduct
    );
    assert_eq!(explicit.files[0].identity, original.identity);
    assert_eq!(read_identity(&store, None).await, transition.selected);
    let paths = server.finish();
    assert_eq!(paths.len(), 12);
    assert!(paths[6].starts_with("HEAD "));
    assert!(paths[11].starts_with("HEAD "));
}

#[tokio::test]
#[ignore = "requires official May 2024 HRO/HRO2 CDFs; see docs/omni.md"]
async fn failed_hro2_upgrade_keeps_accepted_hro_readable() {
    let Some((hro, _)) = fixtures() else {
        return;
    };
    let server = Server::start(vec![
        listing(OmniProduct::Hro, "v01"),
        (200, hro),
        listing(OmniProduct::Hro2, "v01"),
        (200, b"not a cdf".to_vec()),
        (503, vec![]),
        (503, vec![]),
        (503, vec![]),
    ]);
    let home = tempfile::tempdir().unwrap();
    let store = OmniStore::open_at(home.path(), &server.base).await.unwrap();
    let first = store
        .download(range(), Some(OmniProduct::Hro), false)
        .await
        .unwrap();
    assert!(matches!(
        store.download(range(), None, false).await,
        Err(OmniError::Cdf(_))
    ));
    assert_eq!(read_identity(&store, None).await, first.files[0].identity);
    assert!(matches!(
        store.download(range(), None, false).await,
        Err(OmniError::Download(_))
    ));
    assert_eq!(read_identity(&store, None).await, first.files[0].identity);
    assert_eq!(server.finish().len(), 7);
}

#[tokio::test]
#[ignore = "requires official May 2024 HRO/HRO2 CDFs; see docs/omni.md"]
async fn revisions_keep_exact_version_and_accepted_request_after_failure() {
    let Some((_, original)) = fixtures() else {
        return;
    };
    // Explicitly modified real-file fixture: change only a copyright text byte
    // to simulate same-URL upstream revision. No scientific sample is synthesized.
    let mut revised = original.clone();
    let copyright = revised.windows(4).position(|w| w == b"NASA").unwrap();
    revised[copyright] = b'n';
    let server = Server::start(vec![
        listing(OmniProduct::Hro2, "v01"),
        (200, original.clone()),
        listing(OmniProduct::Hro2, "v02"),
        (200, original),
        listing(OmniProduct::Hro2, "v02"),
        (200, revised),
        listing(OmniProduct::Hro2, "v03"),
        (200, b"invalid v03".to_vec()),
    ]);
    let home = tempfile::tempdir().unwrap();
    let store = OmniStore::open_at(home.path(), &server.base).await.unwrap();
    let first = store.download(range(), None, true).await.unwrap();
    let version = store.download(range(), None, true).await.unwrap();
    assert_eq!(
        version.selections[0].change,
        Some(SelectionChange::ProductRevised)
    );
    assert_eq!(version.files[0].identity.file_version, "v02");
    assert_eq!(
        version.files[0].identity.sha256,
        first.files[0].identity.sha256
    );
    assert!(
        version.files[0]
            .artifact
            .original_filename
            .ends_with("_v02.cdf")
    );
    assert_eq!(read_identity(&store, None).await, version.files[0].identity);
    let bytes = store.download(range(), None, true).await.unwrap();
    assert_eq!(
        bytes.selections[0].change,
        Some(SelectionChange::ProductRevised)
    );
    assert_ne!(
        bytes.files[0].identity.sha256,
        version.files[0].identity.sha256
    );
    assert!(store.download(range(), None, true).await.is_err());
    let still_selected = read_identity(&store, None).await;
    assert_eq!(still_selected, bytes.files[0].identity);
    assert!(still_selected.source_url.ends_with("_v02.cdf"));
    assert!(first.files[0].artifact.path.is_file());
    assert!(bytes.files[0].artifact.path.is_file());
    assert_eq!(server.finish().len(), 8);
}
