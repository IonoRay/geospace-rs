use std::path::{Path, PathBuf};

use ionoray_store::{Store, StoreScope};
use serde::{Deserialize, Serialize};

use crate::{IndexDataset, IndexError, IndexStore};

/// Locations initialized for the independent index datasets.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IndexInitReport {
    /// Resolved runtime root.
    pub home: PathBuf,
    /// Five isolated object catalogs, in dataset order.
    pub catalogs: Vec<PathBuf>,
}

impl IndexStore {
    /// Initializes every independent index catalog without a global object pool.
    ///
    /// # Errors
    /// Returns [`IndexError`] when scoped catalog creation fails.
    pub async fn init(explicit_home: Option<&Path>) -> Result<IndexInitReport, IndexError> {
        let root = Store::open_root(explicit_home)?;
        let mut catalogs = Vec::new();
        for dataset in [
            IndexDataset::Dst,
            IndexDataset::Ae,
            IndexDataset::KpApF107,
            IndexDataset::IriIgRz,
            IndexDataset::IriApF107,
        ] {
            let scoped = root.scoped(StoreScope::new(dataset.scope_name())?).await?;
            catalogs.push(scoped.layout().object_catalog());
        }
        Ok(IndexInitReport {
            home: root.home().as_path().to_path_buf(),
            catalogs,
        })
    }
}
