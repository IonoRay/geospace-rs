//! R04 source/time contract for the pinned real IRI index snapshot.
#![cfg(feature = "standard")]

use ionoray_geospace::{
    DataPolicy, Duration, Epoch, GeodeticPosition, Geospace, IriRequest, QueryPoint,
};
use ionoray_indices::QualityFlag;

#[tokio::test]
async fn iri_drivers_preserve_source_values_and_utc_day() {
    let home = tempfile::tempdir().unwrap();
    let gs = Geospace::open(Some(home.path())).await.unwrap();
    let query = QueryPoint {
        epoch: "2020-07-01T12:00:00 UTC".parse().unwrap(),
        position: GeodeticPosition::from_degrees_kilometers(30.0, 120.0, 300.0).unwrap(),
    };
    let prepared = gs
        .prepare_iri(IriRequest::new(query), DataPolicy::Offline)
        .await
        .unwrap();
    let input = prepared.input().drivers;
    // Raw ig_rz.dat: June/July 2020 IG=-6.7/-4.5, Rz=7.9/9.0.
    // July 1 is 16/30 of the interval June 15 -> July 15. Since Jan 2014,
    // official read_ig_rz scales new Rz by 0.7; no clipping of negative IG.
    let weight = 16.0 / 30.0;
    assert!((input.ionospheric_index_12_month - (-6.7 + 2.2 * weight)).abs() < 1e-12);
    assert!((input.sunspot_number_12_month - (7.9 + 1.1 * weight) * 0.7).abs() < 1e-12);
    // apf107.dat line 22828, byte columns 39..44 and 44..49 (zero-based).
    assert!((input.f107_daily - 71.2).abs() < 1e-12);
    assert!((input.f107_81_day - 72.1).abs() < 1e-12);
    let evidence = prepared.indices();
    let rz = evidence.rz12.as_ref().unwrap();
    let ig = evidence.ig12.as_ref().unwrap();
    let daily = evidence.f107_daily.as_ref().unwrap();
    let mean = evidence.f107_81_day.as_ref().unwrap();
    let midnight: Epoch = "2020-07-01T00:00:00 UTC".parse().unwrap();
    for interval in [rz.interval, ig.interval, daily.interval, mean.interval] {
        assert_eq!(interval.start, midnight);
        assert_eq!(interval.end, midnight + Duration::from_days(1.0));
    }
    assert_eq!(rz.artifact, ig.artifact);
    assert_eq!(daily.artifact, mean.artifact);
    assert_eq!(ig.quality, QualityFlag::Final);
    assert!(!ig.release_id.is_empty() && !daily.release_id.is_empty());
    assert_eq!(
        ig.artifact.to_string(),
        "sha256:48e6ac1a501c39ad9842fcf75d63a86303e09852406ee4153275d4b82f9e8804"
    );
    assert_eq!(
        daily.artifact.to_string(),
        "sha256:25bb20ff10c9cc9bf3ec9b80774e856119145d0ce5bbf65dfd3f2bd046ef9262"
    );
    // The interval is the daily sample validity, not the 81-day source window.
    let mut late = query;
    late.epoch = midnight + Duration::from_seconds(86_399.0);
    let repeated = gs
        .prepare_iri(IriRequest::new(late), DataPolicy::Offline)
        .await
        .unwrap();
    assert_eq!(prepared.input().drivers, repeated.input().drivers);
    assert_eq!(prepared.indices(), repeated.indices());
}
