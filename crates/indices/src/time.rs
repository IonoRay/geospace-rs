use ionoray_core::Epoch;

use crate::IndexError;

pub(crate) fn epoch_millis(epoch: Epoch) -> Result<i64, IndexError> {
    let milliseconds = epoch.to_unix_milliseconds();
    if !milliseconds.is_finite() {
        return Err(IndexError::InvalidNumber("epoch milliseconds"));
    }
    Ok(epoch_millis_f64(milliseconds))
}

#[allow(clippy::cast_possible_truncation)]
fn epoch_millis_f64(milliseconds: f64) -> i64 {
    milliseconds.round() as i64
}

#[allow(clippy::cast_precision_loss)]
pub(crate) fn epoch_from_millis(milliseconds: i64) -> Epoch {
    Epoch::from_unix_milliseconds(milliseconds as f64)
}
