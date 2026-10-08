//! End-to-end IGRF-14 evaluation with JSON tracing and result output.

use ionoray_core::{
    Epoch, GeodeticPosition, QueryPoint,
    units::{degree, nanotesla},
};
use ionoray_igrf::{Igrf, IgrfInput, IgrfVersion};
use ionoray_observability::{init_tracing, run_span};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _tracing_guard = init_tracing()?;
    let span = run_span("igrf14.example.evaluate");
    let _entered = span.enter();
    let input = IgrfInput {
        query: QueryPoint {
            epoch: Epoch::maybe_from_gregorian_utc(2025, 1, 1, 0, 0, 0, 0)?,
            position: GeodeticPosition::from_degrees_kilometers(30.0, 120.0, 300.0)?,
        },
    };
    let model = Igrf::new(IgrfVersion::Igrf14)?;
    let result = model.evaluate(&input)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "input": input,
            "north_nt": result.field.north.get::<nanotesla>(),
            "east_nt": result.field.east.get::<nanotesla>(),
            "up_nt": result.field.up.get::<nanotesla>(),
            "magnitude_nt": result.field.magnitude.get::<nanotesla>(),
            "declination_deg": result.field.declination.get::<degree>(),
            "inclination_deg": result.field.inclination.get::<degree>(),
            "provenance": result.provenance,
        }))?
    );
    Ok(())
}
