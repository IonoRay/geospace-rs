use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn unix_time_millis() -> i64 {
    system_time_millis(SystemTime::now())
}

pub(crate) fn system_time_millis(time: SystemTime) -> i64 {
    let millis = time
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    i64::try_from(millis).unwrap_or(i64::MAX)
}

pub(crate) fn system_time_nanos(time: SystemTime) -> i64 {
    let nanos = time
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    i64::try_from(nanos).unwrap_or(i64::MAX)
}
