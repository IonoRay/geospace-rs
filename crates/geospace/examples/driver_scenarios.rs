//! Compare independent driver scenarios copied from one fixed offline baseline.
#[path = "support/automatic_fixture.rs"]
mod fixture;

use ionoray_geospace::{
    DataPolicy, Geospace, HwmRequest, IriDriverOverrides, IriRequest, MsisDriverOverrides,
    MsisRequest, PreparedHwm, PreparedIri, PreparedMsis,
    hwm::HwmGeomagneticActivity,
    igrf::{Igrf, IgrfInput, IgrfVersion},
    msis::MsisGeomagneticActivity,
};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let query = fixture::query();
    let (baseline_iri, baseline_hwm, baseline_msis) = prepare_baselines().await?;

    // Breakpoint 1: all baselines own input/evidence; Geospace and its home are gone.
    println!(
        "{}",
        json!({"model":"iri", "scenario_id":"baseline", "evaluation":baseline_iri.evaluate()?})
    );
    println!(
        "{}",
        json!({"model":"hwm", "scenario_id":"baseline", "evaluation":baseline_hwm.evaluate()?})
    );
    println!(
        "{}",
        json!({"model":"msis", "scenario_id":"baseline", "evaluation":baseline_msis.evaluate()?})
    );

    iri_scenarios(&baseline_iri);
    hwm_scenarios(&baseline_hwm);
    msis_scenarios(&baseline_msis);
    let input = IgrfInput { query };
    let result = Igrf::new(IgrfVersion::Igrf14)?.evaluate(&input)?;
    println!(
        "{}",
        json!({"model":"igrf", "scenario_id":"direct_control", "input":input, "result":result})
    );
    Ok(())
}

async fn prepare_baselines()
-> Result<(PreparedIri, PreparedHwm, PreparedMsis), Box<dyn std::error::Error>> {
    let home = tempfile::tempdir()?;
    eprintln!(
        "isolated offline home: {} (removed before scenarios)",
        home.path().display()
    );
    eprintln!(
        "Fixed 2020-07-01 12:00 UTC, 30 N, 120 E, 300 km; JSON position rad/m, F10.7 sfu, results SI. Controlled driver combinations are not observations or causal claims."
    );
    let geospace = Geospace::open(Some(home.path())).await?;
    let query = fixture::query();
    let baseline_iri = geospace
        .prepare_iri(IriRequest::new(query), DataPolicy::Offline)
        .await?;
    let baseline_hwm = geospace
        .prepare_hwm(HwmRequest::new(query), DataPolicy::Offline)
        .await?;
    let baseline_msis = geospace
        .prepare_msis(MsisRequest::new(query), DataPolicy::Offline)
        .await?;
    assert_eq!(
        baseline_msis
            .indices()
            .ap_three_hourly
            .iter()
            .map(|s| s.value.value())
            .collect::<Vec<_>>(),
        fixture::AP_HISTORY
    );
    drop(geospace);
    home.close()?;
    Ok((baseline_iri, baseline_hwm, baseline_msis))
}

fn iri_scenarios(baseline_iri: &PreparedIri) {
    // The invalid middle item must not suppress the following high-flux item.
    for (scenario_id, flux) in [("low", 70.0), ("invalid", -1.0), ("high", 150.0)] {
        let overrides = IriDriverOverrides {
            f107_daily: Some(flux),
            ..Default::default()
        };
        let scenario = baseline_iri.with_overrides(overrides);
        // Breakpoint 2: inspect scenario.input() and scenario.indices().
        let outcome = scenario.evaluate();
        // Breakpoint 3: inspect Ok/Err; every item is printed, then iteration continues.
        println!(
            "{}",
            json!({"model":"iri", "scenario_id":scenario_id, "overrides":overrides,
            "input":scenario.input(), "indices":scenario.indices(), "outcome":outcome.map_err(|e| e.to_string())})
        );
    }
}

fn hwm_scenarios(baseline_hwm: &PreparedHwm) {
    for (scenario_id, activity) in [
        ("quiet_a", HwmGeomagneticActivity::Quiet),
        (
            "invalid",
            HwmGeomagneticActivity::Disturbed { current_ap: -1.0 },
        ),
        (
            "disturbed_b",
            HwmGeomagneticActivity::Disturbed { current_ap: 40.0 },
        ),
        ("quiet_a_repeat", HwmGeomagneticActivity::Quiet),
    ] {
        let scenario = baseline_hwm.with_activity(activity);
        let outcome = scenario.evaluate();
        println!(
            "{}",
            json!({"model":"hwm", "scenario_id":scenario_id, "overrides":{"geomagnetic_activity":activity},
            "input":scenario.input(), "ap_index":scenario.ap_index(), "outcome":outcome.map_err(|e| e.to_string())})
        );
    }
}

fn msis_scenarios(baseline_msis: &PreparedMsis) {
    for (scenario_id, activity) in [
        ("daily_a", MsisGeomagneticActivity::Daily(4.0)),
        ("invalid", MsisGeomagneticActivity::Daily(-1.0)),
        (
            "storm_b",
            baseline_msis.input().drivers.geomagnetic_activity,
        ),
        ("daily_a_repeat", MsisGeomagneticActivity::Daily(4.0)),
    ] {
        let overrides = MsisDriverOverrides {
            geomagnetic_activity: Some(activity),
            ..Default::default()
        };
        let scenario = baseline_msis.with_overrides(overrides);
        let outcome = scenario.evaluate();
        println!(
            "{}",
            json!({"model":"msis", "scenario_id":scenario_id, "overrides":overrides,
            "input":scenario.input(), "indices":scenario.indices(), "outcome":outcome.map_err(|e| e.to_string())})
        );
    }
}
