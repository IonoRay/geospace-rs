use super::{
    resource_output,
    resource_probe::{Probe, Snapshot},
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use tracing::{
    Subscriber,
    field::{Field, Visit},
};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProfileMode {
    Basic,
    Sampled,
}

/// Closing was rejected because an observed query still owns resources.
#[derive(Debug)]
pub struct ProfileCloseError;
impl std::fmt::Display for ProfileCloseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("cannot close tracing while point queries are active; retry after they finish")
    }
}
impl std::error::Error for ProfileCloseError {}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Default, Clone)]
pub(crate) struct Samples {
    observed: bool,
    pub count: u64,
    pub rss_max: Option<u64>,
    pub measurement_us: u64,
    pub overlaps: bool,
}
struct State {
    active: BTreeMap<u64, Samples>,
    next: u64,
    closing: bool,
    stop: bool,
}
struct Inner {
    mode: Option<ProfileMode>,
    interval: Duration,
    state: Mutex<State>,
    wake: Condvar,
    probe: Mutex<Option<Probe>>,
    sampler: Mutex<Option<JoinHandle<()>>>,
    file: Mutex<Option<WorkerGuard>>,
    finalize: Mutex<()>,
    warned: AtomicBool,
    sampler_failed: AtomicBool,
}

#[derive(Clone)]
pub(crate) struct Collector(Arc<Inner>);
impl Collector {
    pub fn new(
        profile: Option<(ProfileMode, Duration)>,
        file: Option<WorkerGuard>,
        log: Option<PathBuf>,
    ) -> Self {
        let mode = profile.map(|v| v.0);
        let collector = Self(Arc::new(Inner {
            mode,
            interval: profile
                .map_or(Duration::from_millis(250), |v| v.1)
                .max(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL),
            state: Mutex::new(State {
                active: BTreeMap::new(),
                next: 0,
                closing: false,
                stop: false,
            }),
            wake: Condvar::new(),
            probe: Mutex::new(mode.map(|_| Probe::new(log))),
            sampler: Mutex::new(None),
            file: Mutex::new(file),
            finalize: Mutex::new(()),
            warned: AtomicBool::new(false),
            sampler_failed: AtomicBool::new(false),
        }));
        if mode == Some(ProfileMode::Sampled) {
            let shared = collector.clone();
            match thread::Builder::new()
                .name("ionoray-resource-sampler".into())
                .spawn(move || shared.sample_loop())
            {
                Ok(handle) => *lock(&collector.0.sampler) = Some(handle),
                Err(_) => {
                    collector.0.sampler_failed.store(true, Ordering::Relaxed);
                }
            }
        }
        collector
    }

    fn snapshot(&self) -> Option<Snapshot> {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            lock(&self.0.probe)
                .as_mut()
                .map(|probe| probe.snapshot(self.0.mode == Some(ProfileMode::Sampled)))
        }));
        if let Ok(snapshot) = result {
            snapshot
        } else {
            if !self.0.warned.swap(true, Ordering::Relaxed) {
                tracing::warn!(target:"ionoray_observability",event="resource.degraded","resource backend failed; query continues");
            }
            None
        }
    }

    #[cfg(test)]
    fn begin(&self, metadata: Metadata) -> Option<Window> {
        self.begin_window(metadata, true)
    }

    fn begin_window(&self, metadata: Metadata, observed: bool) -> Option<Window> {
        let mut state = lock(&self.0.state);
        if state.closing {
            return None;
        }
        let overlap = observed && state.active.values().any(|sample| sample.observed);
        for sample in state.active.values_mut().filter(|sample| sample.observed) {
            if observed {
                sample.overlaps = true;
            }
        }
        let id = state.next;
        state.next = state.next.wrapping_add(1);
        state.active.insert(
            id,
            Samples {
                observed,
                overlaps: overlap,
                ..Samples::default()
            },
        );
        self.0.wake.notify_all();
        drop(state);
        let start = if observed { self.snapshot() } else { None };
        Some(Window {
            collector: self.clone(),
            id,
            start,
            metadata,
            observed,
        })
    }

    pub fn close(&self) -> Result<(), ProfileCloseError> {
        let _finalize = lock(&self.0.finalize);
        {
            let mut state = lock(&self.0.state);
            if !state.active.is_empty() {
                return Err(ProfileCloseError);
            }
            state.closing = true;
            state.stop = true;
            self.0.wake.notify_all();
        }
        if let Some(handle) = lock(&self.0.sampler).take()
            && handle.thread().id() != thread::current().id()
        {
            let _ = handle.join();
        }
        drop(lock(&self.0.file).take());
        Ok(())
    }

    pub fn release_owner(&self) {
        lock(&self.0.state).closing = true;
        let _ = self.close();
    }

    fn sample_loop(&self) {
        loop {
            let mut state = lock(&self.0.state);
            while !state.active.values().any(|sample| sample.observed) && !state.stop {
                state = self
                    .0
                    .wake
                    .wait(state)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
            if state.stop {
                return;
            }
            let (next, timeout) = self
                .0
                .wake
                .wait_timeout(state, self.0.interval)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state = next;
            if state.stop {
                return;
            }
            if !timeout.timed_out() || !state.active.values().any(|sample| sample.observed) {
                continue;
            }
            let ids: Vec<_> = state
                .active
                .iter()
                .filter(|(_, sample)| sample.observed)
                .map(|(id, _)| *id)
                .collect();
            drop(state);
            if let Some(snapshot) = self.snapshot() {
                let mut state = lock(&self.0.state);
                for id in ids {
                    if let Some(sample) = state.active.get_mut(&id) {
                        sample.count += 1;
                        sample.measurement_us += snapshot.measurement_us;
                        if let Some(rss) = snapshot.rss {
                            sample.rss_max = Some(sample.rss_max.map_or(rss, |v| v.max(rss)));
                        }
                    }
                }
            }
        }
    }
}

#[derive(Default)]
struct Metadata {
    query_id: u64,
    home: Option<PathBuf>,
}
impl Visit for Metadata {
    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "query_id" {
            self.query_id = value;
        }
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "data_home" {
            self.home = Some(value.into());
        }
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "data_home" {
            self.home = Some(format!("{value:?}").trim_matches('"').into());
        }
    }
}
struct Window {
    collector: Collector,
    id: u64,
    start: Option<Snapshot>,
    metadata: Metadata,
    observed: bool,
}
impl Window {
    fn finish(self, parent: &tracing::Id) {
        let end = if self.observed {
            self.collector.snapshot()
        } else {
            None
        };
        let samples = lock(&self.collector.0.state)
            .active
            .get(&self.id)
            .cloned()
            .unwrap_or_default();
        if self.observed && self.collector.0.mode.is_some() {
            let filesystems = lock(&self.collector.0.probe)
                .as_ref()
                .map(|p| p.filesystems(self.metadata.home.as_deref()));
            resource_output::emit(
                parent,
                self.metadata.query_id,
                self.start.as_ref(),
                end.as_ref(),
                &samples,
                self.collector.0.interval,
                self.collector.0.mode == Some(ProfileMode::Sampled),
                self.collector.0.sampler_failed.load(Ordering::Relaxed),
                filesystems,
            );
        }
        let mut state = lock(&self.collector.0.state);
        state.active.remove(&self.id);
        let close = state.closing && state.active.is_empty();
        self.collector.0.wake.notify_all();
        drop(state);
        if close {
            let _ = self.collector.close();
        }
    }
}

/// Composable resource layer. Build with [`crate::resource_layer_from_env`].
pub struct ResourceLayer {
    collector: Collector,
}
impl ResourceLayer {
    pub(crate) const fn new(collector: Collector) -> Self {
        Self { collector }
    }
}
impl<S> Layer<S> for ResourceLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attributes: &tracing::span::Attributes<'_>,
        id: &tracing::Id,
        context: Context<'_, S>,
    ) {
        if matches!(
            attributes.metadata().name(),
            "geospace.point" | "binding.call"
        ) && let Some(span) = context.span(id)
        {
            let mut metadata = Metadata::default();
            attributes.record(&mut metadata);
            if let Some(window) = self
                .collector
                .begin_window(metadata, attributes.metadata().name() == "geospace.point")
            {
                span.extensions_mut().insert(window);
            }
        }
    }
    fn on_close(&self, id: tracing::Id, context: Context<'_, S>) {
        if let Some(span) = context.span(&id) {
            let window = span.extensions_mut().remove::<Window>();
            if let Some(window) = window {
                window.finish(&id);
            }
        }
    }
}

#[cfg(test)]
mod tests;
