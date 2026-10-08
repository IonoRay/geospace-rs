use crate::IndexError;
use std::{future::Future, time::Instant};
use tracing::Instrument;

pub(crate) async fn read<T>(
    index: &'static str,
    operation: impl Future<Output = Result<T, IndexError>>,
) -> Result<T, IndexError> {
    let span = tracing::debug_span!(
        "index.read",
        scope = "operation",
        index,
        read_count = 1u64,
        elapsed_us = tracing::field::Empty,
        status = tracing::field::Empty
    );
    let started = Instant::now();
    let result = operation.instrument(span.clone()).await;
    span.record(
        "elapsed_us",
        u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
    );
    span.record(
        "status",
        if result.is_ok() {
            "succeeded"
        } else {
            "failed"
        },
    );
    result
}
