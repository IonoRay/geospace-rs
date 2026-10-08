//! Direct evaluations of the four model families using only explicit inputs.
//!
//! This example intentionally neither opens an index store nor reads
//! `IONORAY_HOME`: every model driver is visible below.

use ionoray_geospace::{
    Epoch, GeodeticPosition, QueryPoint,
    hwm::{Hwm, HwmGeomagneticActivity, HwmInput, HwmVersion},
    igrf::{Igrf, IgrfInput, IgrfVersion},
    iri::{Iri, IriDrivers, IriInput, IriVersion},
    msis::{Msis, MsisDrivers, MsisGeomagneticActivity, MsisInput, MsisVersion},
    units::{kelvin, kilometer, meter_per_second, nanotesla},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Breakpoint 1: all four inputs are explicit, and none use index evidence.
    let igrf_input = IgrfInput {
        query: QueryPoint {
            epoch: Epoch::maybe_from_gregorian_utc(2025, 1, 1, 0, 0, 0, 0)?,
            position: GeodeticPosition::from_degrees_kilometers(30.0, 120.0, 300.0)?,
        },
    };
    let iri_input = IriInput {
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
    let hwm_input = HwmInput {
        query: QueryPoint {
            epoch: Epoch::maybe_from_gregorian_utc(1995, 5, 30, 12, 0, 0, 0)?,
            position: GeodeticPosition::from_degrees_kilometers(-45.0, -85.0, 250.0)?,
        },
        geomagnetic_activity: HwmGeomagneticActivity::Disturbed { current_ap: 80.0 },
    };
    let msis_input = MsisInput {
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

    println!("IGRF input (deg, km): {igrf_input:#?}");
    println!("IRI input (deg, km; flux in sfu): {iri_input:#?}");
    println!("HWM input (deg, km; ap): {hwm_input:#?}");
    println!("MSIS input (deg, km; flux in sfu, Ap): {msis_input:#?}");

    // Breakpoint 2: each result comes straight from its model's evaluate call.
    let igrf_result = Igrf::new(IgrfVersion::Igrf14)?.evaluate(&igrf_input)?;
    let iri_result = Iri::new(IriVersion::Iri2020).evaluate(&iri_input)?;
    let hwm_result = Hwm::new(HwmVersion::Hwm14).evaluate(&hwm_input)?;
    let msis_result = Msis::new(MsisVersion::Nrlmsis21).evaluate(&msis_input)?;

    // Breakpoint 3: SI conversions and provenance are visible just before output.
    println!(
        "IGRF magnetic field: north={:.3} nT, east={:.3} nT, up={:.3} nT, magnitude={:.3} nT; provenance={:#?}",
        igrf_result.field.north.get::<nanotesla>(),
        igrf_result.field.east.get::<nanotesla>(),
        igrf_result.field.up.get::<nanotesla>(),
        igrf_result.field.magnitude.get::<nanotesla>(),
        igrf_result.provenance,
    );
    println!(
        "IRI plasma: electron_density={:?} m^-3, electron_temperature={:?} K, F2_height={:?} km; provenance={:#?}",
        iri_result.point.electron_density_m3,
        iri_result
            .point
            .electron_temperature
            .map(|value| value.get::<kelvin>()),
        iri_result
            .peaks
            .f2
            .map(|peak| peak.height.get::<kilometer>()),
        iri_result.provenance,
    );
    println!(
        "HWM neutral wind: northward={:.3} m/s, eastward={:.3} m/s; provenance={:#?}",
        hwm_result.wind.northward.get::<meter_per_second>(),
        hwm_result.wind.eastward.get::<meter_per_second>(),
        hwm_result.provenance,
    );
    println!(
        "NRLMSIS neutral atmosphere: temperature={:.3} K, mass_density={:?} kg/m^3; provenance={:#?}",
        msis_result.atmosphere.temperature.get::<kelvin>(),
        msis_result.atmosphere.mass_density_kg_m3,
        msis_result.provenance,
    );

    Ok(())
}
