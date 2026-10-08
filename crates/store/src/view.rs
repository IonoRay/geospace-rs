use std::path::{Path, PathBuf};

use crate::StoreError;

pub(crate) fn browse_collision(path: &Path, sequence: u32) -> Result<PathBuf, StoreError> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| StoreError::InvalidBrowsePath(path.to_path_buf()))?;
    Ok(path.with_file_name(format!("{name}.{sequence}")))
}
