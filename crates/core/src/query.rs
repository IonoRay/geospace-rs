use serde::{Deserialize, Serialize};

use crate::{Epoch, GeodeticPosition};

/// A point in time and geodetic space at which models are evaluated.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueryPoint {
    /// Physical epoch, including its time scale.
    pub epoch: Epoch,
    /// Geodetic model position.
    pub position: GeodeticPosition,
}
