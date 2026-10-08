use super::*;
use crate::{init::open_file_writer, resource_output::delta, subscriber::json_layer};
use tracing_subscriber::prelude::*;

#[test]
fn deltas_distinguish_zero_missing_reset_and_identity() {
    use crate::resource_output::{rate_status, sample_status};
    for status in ["unavailable", "counter_reset", "identity_changed"] {
        assert_eq!(rate_status(status, 100), status);
        assert_eq!(rate_status(status, 0), status);
    }
    assert_eq!(rate_status("available", 0), "insufficient_window");
    assert_eq!(rate_status("available", 1), "available");
    assert_eq!(sample_status(true, false, 1, None), "unavailable");
    assert_eq!(sample_status(true, false, 1, Some(0)), "available");
    assert_eq!(
        crate::resource_output::background_delta(Some(5), Some(5), false),
        (None, "insufficient_window")
    );
    assert_eq!(delta(Some(5), Some(5), true), (Some(0), "available"));
    assert_eq!(delta(None, Some(5), true), (None, "unavailable"));
    assert_eq!(delta(Some(6), Some(5), true), (None, "counter_reset"));
    assert_eq!(delta(Some(5), Some(8), false), (None, "identity_changed"));
}

#[test]
fn active_close_retries_and_owner_drop_defers_flush_in_every_mode() {
    for mode in [None, Some(ProfileMode::Basic), Some(ProfileMode::Sampled)] {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("trace.ndjson");
        let (writer, file) = open_file_writer(&path).unwrap();
        let collector = Collector::new(
            mode.map(|m| (m, Duration::from_millis(100))),
            Some(file),
            Some(path.clone()),
        );
        let subscriber = tracing_subscriber::registry()
            .with(json_layer(
                writer,
                tracing_subscriber::EnvFilter::new("info"),
            ))
            .with(ResourceLayer::new(collector.clone()));
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("geospace.point",query_id=42u64,data_home=%temporary.path().display());
            assert!(collector.close().is_err());
            collector.release_owner();
            assert!(lock(&collector.0.file).is_some());
            span.in_scope(|| tracing::info!(event = "last.query.event"));
            drop(span);
        });
        assert!(collector.close().is_ok());
        assert!(lock(&collector.0.file).is_none());
        assert!(lock(&collector.0.sampler).is_none());
        let records: Vec<serde_json::Value> = std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert!(
            records
                .iter()
                .any(|r| r["fields"]["event"] == "last.query.event")
        );
        let resources: Vec<_> = records
            .iter()
            .filter(|r| r["fields"]["event"] == "resource.window")
            .collect();
        assert_eq!(resources.len(), usize::from(mode.is_some()));
        if let Some(record) = resources.first() {
            assert_eq!(record["fields"]["query_id"], 42);
            assert_eq!(record["span"]["name"], "geospace.point");
            let values: serde_json::Value =
                serde_json::from_str(record["fields"]["metrics_json"].as_str().unwrap()).unwrap();
            assert_eq!(values["cpu_time_us"]["delta"]["unit"], "us");
            assert_eq!(values["rss"]["delta"]["unit"], "bytes_signed");
        }
    }
}

#[test]
fn overlapping_windows_are_both_marked_and_have_independent_samples() {
    let collector = Collector::new(None, None, None);
    let first = collector.begin(Metadata::default()).unwrap();
    let second = collector.begin(Metadata::default()).unwrap();
    let state = lock(&collector.0.state);
    assert!(state.active.values().all(|s| s.overlaps));
    assert_eq!(state.active.len(), 2);
    drop(state);
    // Finish without a subscriber; mode off emits no parented resource events.
    first.finish(&tracing::Id::from_u64(1));
    assert!(collector.close().is_err());
    second.finish(&tracing::Id::from_u64(2));
    assert!(collector.close().is_ok());
    assert!(collector.begin(Metadata::default()).is_none());
}

#[test]
fn close_registration_race_is_atomic() {
    for _ in 0..32 {
        let collector = Collector::new(None, None, None);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let other = collector.clone();
        let other_barrier = barrier.clone();
        let thread = std::thread::spawn(move || {
            other_barrier.wait();
            other.begin(Metadata::default())
        });
        barrier.wait();
        let closed = collector.close().is_ok();
        let window = thread.join().unwrap();
        assert_eq!(closed, window.is_none());
        if let Some(window) = window {
            window.finish(&tracing::Id::from_u64(1));
        }
        assert!(collector.close().is_ok());
    }
}

#[test]
fn sampler_pauses_without_windows_and_stops_after_close() {
    let collector = Collector::new(
        Some((ProfileMode::Sampled, Duration::from_millis(200))),
        None,
        None,
    );
    assert!(lock(&collector.0.probe).as_ref().is_some());
    assert!(lock(&collector.0.state).active.is_empty());
    let window = collector.begin(Metadata::default()).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while lock(&collector.0.state).active[&window.id].count == 0
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    let stats = lock(&collector.0.state).active[&window.id].clone();
    assert!(stats.count > 0);
    assert!(stats.rss_max.is_some());
    // Avoid emitting against a fabricated parent: a real local span supplies the ID.
    let subscriber = tracing_subscriber::registry();
    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("sample.test.parent");
        window.finish(&span.id().unwrap());
    });
    assert!(collector.close().is_ok());
    assert!(lock(&collector.0.sampler).is_none());
    let off = Collector::new(None, None, None);
    assert!(lock(&off.0.probe).is_none());
    assert!(lock(&off.0.sampler).is_none());
    assert!(off.close().is_ok());
}

#[test]
fn binding_lifecycle_protects_setup_and_conversion_without_false_overlap() {
    let collector = Collector::new(None, None, None);
    let subscriber = tracing_subscriber::registry().with(ResourceLayer::new(collector.clone()));
    tracing::subscriber::with_default(subscriber, || {
        let call = tracing::info_span!("binding.call");
        assert!(collector.close().is_err());
        let point = tracing::info_span!(parent:&call,"geospace.point",query_id=1u64);
        assert!(
            lock(&collector.0.state)
                .active
                .values()
                .all(|s| !s.overlaps)
        );
        drop(point);
        assert!(collector.close().is_err());
        drop(call);
        assert!(collector.close().is_ok());
    });
}
