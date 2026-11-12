use crate::FetchError;

use super::TileId;

/// Why elevation data could not be prepared
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DemError {
    /// An elevation tile could not be downloaded or read from the cache
    #[error("could not fetch elevation tile {tile}")]
    Fetch {
        tile: TileId,
        #[source]
        source: FetchError,
    },

    /// The tile is not a readable PNG
    #[error("could not decode elevation tile {tile}")]
    Decode {
        tile: TileId,
        #[source]
        source: png::DecodingError,
    },

    /// The tile decoded but isn't a 256 x 256 RGB(A) terrarium tile
    #[error("elevation tile {tile} has an unexpected format: {reason}")]
    Format { tile: TileId, reason: String },

    /// The tile could not be read, and fetching it again did not help
    #[error("elevation tile {tile} could not be read, and fetching it again did not help")]
    Damaged {
        tile: TileId,
        #[source]
        source: Box<DemError>,
    },
}
