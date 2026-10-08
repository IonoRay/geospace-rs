use std::path::Path;

use clap::Subcommand;
use ionoray_store::Store;
use serde::Serialize;

use crate::GeospaceError;

use super::output;

#[derive(Debug, Subcommand)]
pub(super) enum DataCommand {
    /// Explicitly create the local layout and current object catalog.
    Init,
    /// Report the resolved home and current object-catalog schema.
    Status,
}

pub(super) async fn execute(
    home: Option<&Path>,
    command: DataCommand,
) -> Result<(), GeospaceError> {
    match command {
        DataCommand::Init => {
            #[cfg(feature = "indices")]
            output::json(&ionoray_indices::IndexStore::init(home).await?)?;
            #[cfg(not(feature = "indices"))]
            output::json(&Store::init(home).await?)?;
        }
        DataCommand::Status => {
            #[cfg(feature = "indices")]
            {
                let root = Store::open_root(home)?;
                let scopes = ["dst", "ae", "kp-ap-f107", "iri-ig-rz", "iri-apf107"];
                let catalogs: Vec<_> = scopes
                    .iter()
                    .map(|scope| {
                        let path = root
                            .home()
                            .as_path()
                            .join("objects")
                            .join(scope)
                            .join("catalog.db");
                        ScopedCatalog {
                            scope,
                            exists: path.is_file(),
                            path,
                        }
                    })
                    .collect();
                output::json(&ScopedStatus {
                    home: root.home().as_path(),
                    catalogs,
                })?;
            }
            #[cfg(not(feature = "indices"))]
            {
                let store = Store::open(home).await?;
                output::json(&DataStatus {
                    home: store.home().as_path(),
                    status: store.status(),
                })?;
            }
        }
    }
    Ok(())
}

#[derive(Serialize)]
#[cfg(not(feature = "indices"))]
struct DataStatus<'a> {
    home: &'a Path,
    status: &'a ionoray_store::StoreStatus,
}

#[cfg(feature = "indices")]
#[derive(Serialize)]
struct ScopedStatus<'a> {
    home: &'a Path,
    catalogs: Vec<ScopedCatalog<'a>>,
}

#[cfg(feature = "indices")]
#[derive(Serialize)]
struct ScopedCatalog<'a> {
    scope: &'a str,
    path: std::path::PathBuf,
    exists: bool,
}
