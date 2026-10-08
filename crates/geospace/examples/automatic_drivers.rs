//! Resolve missing drivers once, inspect their evidence, then evaluate repeatedly.
#[path = "support/automatic_fixture.rs"]
mod fixture;

use ionoray_geospace::{
    DataPolicy, Geospace, HwmRequest, IriRequest, MsisRequest,
    igrf::{Igrf, IgrfInput, IgrfVersion},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = tempfile::tempdir()?;
    println!("isolated home: {} (removed on exit)", home.path().display());
    println!(
        "Offline fixed snapshot: 2020-07-01 12:00 UTC, 30 deg, 120 deg, 300 km. JSON uses rad/m for position, sfu for F10.7; results use T, m/s, K, m^-3, kg/m^3."
    );
    let geospace = Geospace::open(Some(home.path())).await?;
    let query = fixture::query();
    let policy = DataPolicy::Offline;

    // Breakpoint 1: step into prepare_iri to inspect override selection.
    let prepared_iri = geospace.prepare_iri(IriRequest::new(query), policy).await?;
    let prepared_hwm = geospace.prepare_hwm(HwmRequest::new(query), policy).await?;
    let prepared_msis = geospace
        .prepare_msis(MsisRequest::new(query), policy)
        .await?;
    assert_eq!(prepared_msis.indices().ap_three_hourly.len(), 20);
    let ap: Vec<_> = prepared_msis
        .indices()
        .ap_three_hourly
        .iter()
        .map(|s| s.value.value())
        .collect();
    assert_eq!(ap, fixture::AP_HISTORY);
    println!(
        "IRI resolved input/evidence: {}",
        serde_json::to_string(&(prepared_iri.input(), prepared_iri.indices()))?
    );
    println!(
        "HWM resolved input/evidence: {}",
        serde_json::to_string(&(prepared_hwm.input(), prepared_hwm.ap_index()))?
    );
    println!(
        "MSIS resolved input/evidence: {}",
        serde_json::to_string(&(prepared_msis.input(), prepared_msis.indices()))?
    );

    // Breakpoint 2: prepared inputs and evidence are complete; evaluate borrows them.
    let iri_result = prepared_iri.evaluate()?;
    let hwm_result = prepared_hwm.evaluate()?;
    let msis_result = prepared_msis.evaluate()?;
    assert_eq!(iri_result, prepared_iri.evaluate()?);
    assert_eq!(hwm_result, prepared_hwm.evaluate()?);
    assert_eq!(msis_result, prepared_msis.evaluate()?);
    assert_eq!(
        iri_result,
        geospace
            .evaluate_iri(IriRequest::new(query), policy)
            .await?
    );
    assert_eq!(
        hwm_result,
        geospace
            .evaluate_hwm(HwmRequest::new(query), policy)
            .await?
    );
    assert_eq!(
        msis_result,
        geospace
            .evaluate_msis(MsisRequest::new(query), policy)
            .await?
    );
    let igrf_result = Igrf::new(IgrfVersion::Igrf14)?.evaluate(&IgrfInput { query })?;

    // Breakpoint 3: actual model returns, including input/evidence/provenance.
    println!(
        "IRI result: {}",
        serde_json::to_string_pretty(&iri_result.result)?
    );
    println!(
        "HWM result: {}",
        serde_json::to_string_pretty(&hwm_result.result)?
    );
    println!(
        "MSIS result: {}",
        serde_json::to_string_pretty(&msis_result.result)?
    );
    println!(
        "IGRF direct result: {}",
        serde_json::to_string_pretty(&igrf_result)?
    );
    println!("Repeated and one-step evaluations match for IRI/HWM/MSIS.");
    Ok(())
}
