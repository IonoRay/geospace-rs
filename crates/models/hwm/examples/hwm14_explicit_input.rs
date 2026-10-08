//! Low-level HWM14 backend evaluation with an explicitly supplied ap driver.

use ionoray_core::{Epoch, GeodeticPosition, QueryPoint, units::meter_per_second};
use ionoray_hwm::{HWM14_CITATION, Hwm, HwmGeomagneticActivity, HwmInput, HwmVersion};
use ionoray_observability::{init_tracing, run_span};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _tracing_guard = init_tracing()?;
    let span = run_span("hwm14.example.explicit_input");
    let _entered = span.enter();
    let input = HwmInput {
        query: QueryPoint {
            epoch: Epoch::maybe_from_gregorian_utc(1995, 5, 30, 12, 0, 0, 0)?,
            position: GeodeticPosition::from_degrees_kilometers(-45.0, -85.0, 250.0)?,
        },
        geomagnetic_activity: HwmGeomagneticActivity::Disturbed { current_ap: 80.0 },
    };
    let model = Hwm::new(HwmVersion::Hwm14);
    // evaluate initializes automatically; initialize() is only an optional preflight.
    let result = model.evaluate(&input)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "input": input,
            "northward_m_s": result.wind.northward.get::<meter_per_second>(),
            "eastward_m_s": result.wind.eastward.get::<meter_per_second>(),
            "provenance": result.provenance,
            "citation": HWM14_CITATION,
        }))?
    );
    Ok(())
}
