//! Low-level NRLMSIS 2.1 evaluation with explicitly supplied drivers.

use ionoray_core::{Epoch, GeodeticPosition, QueryPoint};
use ionoray_msis::{
    Msis, MsisDrivers, MsisGeomagneticActivity, MsisInput, MsisVersion, NRLMSIS21_NOTICE,
};
use ionoray_observability::{init_tracing, run_span};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _tracing_guard = init_tracing()?;
    let span = run_span("nrlmsis21.example.explicit_input");
    let _entered = span.enter();
    let input = MsisInput {
        query: QueryPoint {
            epoch: Epoch::maybe_from_gregorian_utc(1974, 8, 1, 10, 58, 30, 0)?,
            position: GeodeticPosition::from_degrees_kilometers(-24.4, 119.1, 399.1)?,
        },
        drivers: MsisDrivers {
            f107a: 86.5,
            f107_previous_day: 84.8,
            geomagnetic_activity: MsisGeomagneticActivity::Daily(6.0),
        },
    };
    let result = Msis::new(MsisVersion::Nrlmsis21).evaluate(&input)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "input": input,
            "atmosphere_si": result.atmosphere,
            "provenance": result.provenance,
            "license_notice": NRLMSIS21_NOTICE,
        }))?
    );
    Ok(())
}
