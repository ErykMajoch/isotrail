use std::io;
use std::path::PathBuf;

/// Why a remote resource could not be fetched
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FetchError {
    /// The server answered with an error status
    #[error("{url} returned HTTP {status}")]
    Status { url: String, status: u16 },

    /// The HTTP client could not be set up
    #[error("could not set up the HTTP client")]
    Client(#[source] reqwest::Error),

    /// The request never got an answer
    #[error("could not fetch {url}")]
    Network {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    /// Reading or writing the on-disk cache failed
    #[error("cache error at {}", path.display())]
    Cache {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// Offline mode is on and the response was never cached
    #[error("{url} is not in the cache and offline mode is on")]
    Offline { url: String },

    /// A test asked for a response that has no fixture
    #[error("no recorded response for {url} (expected {})", path.display())]
    NotRecorded { url: String, path: PathBuf },

    /// The URL can't be used
    #[error("invalid URL {url}: {reason}")]
    InvalidUrl { url: String, reason: String },
}
