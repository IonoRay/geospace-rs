use serde::Serialize;

use crate::GeospaceError;

pub(super) fn json(value: &impl Serialize) -> Result<(), GeospaceError> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
