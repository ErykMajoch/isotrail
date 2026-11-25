use crate::{FetchError, Retry};

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

    /// Why trying again may help, if it may. Overpass reports overload inside
    /// an HTTP 200 too, as a remark or an answer that isn't map data
    pub fn retry(&self) -> Option<Retry> {
        match self {
            OsmError::Fetch { source } => source.retry(),
            OsmError::Json { .. } => Some(Retry::Busy),
            OsmError::Overpass { remark } if is_busy_remark(remark) => Some(Retry::Busy),
            _ => None,
        }
    }

    /// What made a busy server look busy, in a few words
    pub fn busy_reason(&self) -> Option<String> {
        match self {
            OsmError::Fetch { source } => match source {
                FetchError::Status { status, .. } => Some(format!("HTTP {status}")),
                FetchError::Body { .. } => Some("the answer was cut off".to_owned()),
                _ if source.is_transient() => Some("no answer in time".to_owned()),
                _ => None,
            },
            OsmError::Json { .. } => Some("the answer was not map data".to_owned()),
            OsmError::Overpass { remark } if is_busy_remark(remark) => Some(remark.clone()),
            _ => None,
        }
    }
}

fn is_busy_remark(remark: &str) -> bool {
    let remark = remark.to_ascii_lowercase();
    [
        "timed out",
        "timeout",
        "rate limit",
        "rate_limit",
        "too many",
    ]
    .iter()
    .any(|busy| remark.contains(busy))
}
