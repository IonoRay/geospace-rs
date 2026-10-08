use tracing::Subscriber;
use tracing_subscriber::{
    EnvFilter, Layer,
    fmt::{MakeWriter, format::FmtSpan, time::ChronoLocal},
    registry::LookupSpan,
};

pub(crate) fn json_layer<S, W>(
    writer: W,
    filter: EnvFilter,
) -> impl Layer<S> + Send + Sync + 'static
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    tracing_subscriber::fmt::layer()
        .json()
        .with_timer(ChronoLocal::rfc_3339())
        .with_writer(writer)
        .with_ansi(false)
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_current_span(true)
        .with_span_list(true)
        .with_span_events(FmtSpan::CLOSE)
        .with_filter(filter)
}

pub(crate) fn pretty_layer<S, W>(
    writer: W,
    filter: EnvFilter,
    ansi: bool,
) -> impl Layer<S> + Send + Sync + 'static
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    tracing_subscriber::fmt::layer()
        .compact()
        .with_timer(ChronoLocal::rfc_3339())
        .with_writer(writer)
        .with_ansi(ansi)
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_span_events(FmtSpan::CLOSE)
        .with_filter(filter)
}

#[cfg(test)]
mod tests {
    use std::{
        io::{self, Write},
        sync::{Arc, Mutex},
    };

    use serde_json::Value;
    use tracing_subscriber::{fmt::MakeWriter, prelude::*};

    use super::{json_layer, pretty_layer};
    use crate::run_span;

    #[derive(Clone, Default)]
    struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

    struct BufferWriter(Arc<Mutex<Vec<u8>>>);

    impl SharedBuffer {
        fn text(&self) -> String {
            let bytes = self.0.lock().expect("trace buffer lock").clone();
            String::from_utf8(bytes).expect("trace output is UTF-8")
        }
    }

    impl<'writer> MakeWriter<'writer> for SharedBuffer {
        type Writer = BufferWriter;

        fn make_writer(&'writer self) -> Self::Writer {
            BufferWriter(Arc::clone(&self.0))
        }
    }

    impl Write for BufferWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("trace buffer lock").extend(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn json_contains_local_offset_process_thread_and_elapsed_time() {
        let output = SharedBuffer::default();
        let subscriber = tracing_subscriber::registry().with(json_layer(
            output.clone(),
            tracing_subscriber::EnvFilter::new("trace"),
        ));
        tracing::subscriber::with_default(subscriber, emit_test_event);

        let text = output.text();
        let records = text
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("valid NDJSON"))
            .collect::<Vec<_>>();
        let event = records
            .iter()
            .find(|record| record["fields"]["event"] == "observability.test.event")
            .expect("test event record");
        let timestamp = event["timestamp"].as_str().expect("timestamp string");
        let offset = &timestamp[timestamp.len() - 6..];

        assert!(matches!(offset.as_bytes()[0], b'+' | b'-'));
        assert_eq!(offset.as_bytes()[3], b':');
        assert!(event.get("threadId").is_some());
        assert!(text.contains(&format!("\"pid\":{}", std::process::id())));
        assert!(text.contains("\"run_id\""));
        assert!(text.contains("\"time.busy\""));
        assert!(text.contains("\"time.idle\""));
        assert!(!text.contains('\u{1b}'));
    }

    #[test]
    fn pretty_output_uses_ansi_colors_when_enabled() {
        let output = SharedBuffer::default();
        let subscriber = tracing_subscriber::registry().with(pretty_layer(
            output.clone(),
            tracing_subscriber::EnvFilter::new("trace"),
            true,
        ));
        tracing::subscriber::with_default(subscriber, emit_test_event);

        let text = output.text();
        assert!(text.contains("\u{1b}["));
        assert!(text.contains("INFO"));
        assert!(text.contains("observability.test.event"));
        assert!(text.contains("time.busy"));
        assert!(text.contains("time.idle"));
    }

    #[test]
    fn pretty_output_assigns_distinct_colors_to_each_visible_level() {
        let output = SharedBuffer::default();
        let subscriber = tracing_subscriber::registry().with(pretty_layer(
            output.clone(),
            tracing_subscriber::EnvFilter::new("trace"),
            true,
        ));
        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!("debug event");
            tracing::info!("info event");
            tracing::warn!("warn event");
            tracing::error!("error event");
        });

        let text = output.text();
        assert!(text.contains("\u{1b}[34mDEBUG\u{1b}[0m"));
        assert!(text.contains("\u{1b}[32m INFO\u{1b}[0m"));
        assert!(text.contains("\u{1b}[33m WARN\u{1b}[0m"));
        assert!(text.contains("\u{1b}[31mERROR\u{1b}[0m"));
    }

    #[test]
    fn console_and_json_layers_receive_the_same_event() {
        let console = SharedBuffer::default();
        let file = SharedBuffer::default();
        let subscriber = tracing_subscriber::registry()
            .with(pretty_layer(
                console.clone(),
                tracing_subscriber::EnvFilter::new("trace"),
                true,
            ))
            .with(json_layer(
                file.clone(),
                tracing_subscriber::EnvFilter::new("trace"),
            ));
        tracing::subscriber::with_default(subscriber, emit_test_event);

        assert!(console.text().contains("observability.test.event"));
        assert!(console.text().contains("\u{1b}["));
        assert!(file.text().contains("observability.test.event"));
        assert!(!file.text().contains('\u{1b}'));
    }

    fn emit_test_event() {
        let span = run_span("observability.test");
        let _entered = span.enter();
        tracing::info!(event = "observability.test.event", "test event");
    }
}
