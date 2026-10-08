//! Low-level IRI-2020 evaluation with explicitly supplied climatological drivers.

use ionoray_core::{Epoch, GeodeticPosition, QueryPoint, units::kelvin};
use ionoray_iri::{IRI2020_CITATION, Iri, IriDrivers, IriInput, IriVersion};
use ionoray_observability::{init_tracing, run_span};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _tracing_guard = init_tracing()?;
    let span = run_span("iri2020.example.explicit_input");
    let _entered = span.enter();
    let input = IriInput {
        query: QueryPoint {
            epoch: Epoch::maybe_from_gregorian_utc(2020, 3, 20, 12, 0, 0, 0)?,
            position: GeodeticPosition::from_degrees_kilometers(0.0, 0.0, 300.0)?,
        },
        drivers: IriDrivers {
            sunspot_number_12_month: 10.0,
            ionospheric_index_12_month: 10.0,
            f107_daily: 70.0,
            f107_81_day: 70.0,
        },
    };
    let model = Iri::new(IriVersion::Iri2020);
    let result = model.evaluate(&input)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "input": input,
            "point": result.point,
            "electron_density_m3": result.point.electron_density_m3,
            "electron_temperature_k": result
                .point
                .electron_temperature
                .map(|value| value.get::<kelvin>()),
            "peaks": result.peaks,
            "provenance": result.provenance,
            "citation": IRI2020_CITATION,
        }))?
    );
    Ok(())
}
