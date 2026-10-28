use super::*;

// Synthetic identity metadata only; these are not scientific data samples.
fn identity(product: OmniProduct, version: &str, digest: u8) -> FileIdentity {
    FileIdentity {
        product,
        cadence: "1min",
        partition: "2024-05".into(),
        file_version: version.into(),
        filename: format!("omni_{}_20240501_{version}.cdf", product.dataset()),
        sha256: Sha256Digest::from_bytes([digest; 32]),
        source_id: "test".into(),
        source_url: "https://example.test/input.cdf".into(),
        downloaded_at_utc_ms: 1,
        accepted_at_utc_ms: 2,
    }
}

#[test]
fn identifies_upgrade_revision_and_unchanged_inputs() {
    let old = identity(OmniProduct::Hro, "v99", 1);
    let new = identity(OmniProduct::Hro2, "v01", 2);
    assert_eq!(change(None, &old), SelectionChange::FirstAcquired);
    assert_eq!(change(Some(&old), &new), SelectionChange::ProductUpgraded);
    let revised_version = identity(OmniProduct::Hro2, "v02", 2);
    let revised_bytes = identity(OmniProduct::Hro2, "v02", 3);
    assert_eq!(
        change(Some(&new), &revised_version),
        SelectionChange::ProductRevised
    );
    assert_eq!(
        change(Some(&revised_version), &revised_bytes),
        SelectionChange::ProductRevised
    );
    let mut rechecked = revised_bytes.clone();
    rechecked.accepted_at_utc_ms += 1;
    assert_eq!(
        change(Some(&revised_bytes), &rechecked),
        SelectionChange::Unchanged
    );
}

#[test]
fn offline_selection_does_not_invent_an_update_or_upstream_check() {
    let old = identity(OmniProduct::Hro, "v01", 1);
    let report = selection_report(&old, None, None, SourceCheck::OfflineNotChecked);
    assert_eq!(report.selection_reason, SelectionReason::FallbackProduct);
    assert_eq!(report.change, None);
    assert_eq!(report.previous_selected, None);
    let report = selection_report(
        &old,
        Some(old.clone()),
        Some(OmniProduct::Hro),
        SourceCheck::Checked,
    );
    assert_eq!(report.selection_reason, SelectionReason::ExplicitProduct);
    assert_eq!(report.change, Some(SelectionChange::Unchanged));
}
