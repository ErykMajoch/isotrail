use crate::FetchError;

/// Why map features could not be prepared
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum OsmError {
    /// The Overpass request failed, or offline mode is on and it isn't cached
    #[error("could not fetch OpenStreetMap features")]
    Fetch {
        #[source]
        source: FetchError,
    },

    /// The response isn't Overpass JSON
    #[error("could not read the Overpass response")]
    Json {
        #[source]
        source: serde_json::Error,
    },

    /// Overpass answered but reported a problem such as a timeout, so the
    /// data may be incomplete
    #[error("Overpass reported an error: {remark}")]
    Overpass { remark: String },

    /// The server stayed busy or out of reach through every retry
    #[error("Overpass still failed after {attempts} attempts")]
    GaveUp {
        attempts: u32,
        #[source]
        last: Box<OsmError>,
    },
}

impl OsmError {
    /// Offline mode is on and the map data was never cached
    pub fn is_offline(&self) -> bool {
        matches!(
            self,
            OsmError::Fetch {
                source: FetchError::Offline { .. }
            }
        )
    }
}
