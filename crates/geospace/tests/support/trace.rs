//! Per-future capture of production read/prepare spans, without global subscribers.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use tracing::{
    Event, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};

#[derive(Clone, Default)]
pub struct Calls {
    entries: Arc<Mutex<Vec<(String, String)>>>,
    next: Arc<AtomicU64>,
}

impl Calls {
    pub fn reads(&self) -> Vec<String> {
        self.of("index.read")
    }
    pub fn preparations(&self) -> Vec<String> {
        self.of("dataset.prepare")
    }
    fn of(&self, name: &str) -> Vec<String> {
        self.entries
            .lock()
            .unwrap()
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, value)| value.clone())
            .collect()
    }
}
struct Selected(String);
impl Visit for Selected {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if matches!(field.name(), "index" | "fields") {
            format!("{value:?}")
                .trim_matches('"')
                .clone_into(&mut self.0);
        }
    }
}
impl Subscriber for Calls {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, attrs: &Attributes<'_>) -> Id {
        let mut selected = Selected(String::new());
        attrs.record(&mut selected);
        self.entries
            .lock()
            .unwrap()
            .push((attrs.metadata().name().to_owned(), selected.0));
        Id::from_u64(self.next.fetch_add(1, Ordering::Relaxed) + 1)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, _: &Event<'_>) {}
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}
