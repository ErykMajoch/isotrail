use std::io;
use std::path::PathBuf;
use std::time::Duration;

/// Why a remote resource could not be fetched
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FetchError {
    /// The server answered with an error status
    #[error("{url} returned HTTP {status}")]
    Status {
        url: String,
        status: u16,
        /// How long the server asked to wait before trying again
        retry_after: Option<Duration>,
        /// The start of the server's answer, which often says why
        body: Vec<u8>,
    },

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

    /// The connection broke while the answer was arriving
    #[error("the answer from {url} was cut off")]
    Body {
        url: String,
        #[source]
        source: io::Error,
    },

    /// The answer is larger than isotrail is willing to hold in memory
    #[error("the answer from {url} is larger than {} MB", limit / 1_000_000)]
    TooLarge { url: String, limit: u64 },

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

impl FetchError {
    /// An error status with no body, as fakes in tests answer
    pub fn status(url: &str, status: u16) -> FetchError {
        FetchError::Status {
            url: url.to_owned(),
            status,
            retry_after: None,
            body: Vec::new(),
        }
    }

    /// Whether the same request may well succeed a little later
    pub fn is_transient(&self) -> bool {
        match self {
            FetchError::Status { status, .. } => *status == 429 || (500..600).contains(status),
            FetchError::Network { source, .. } => source.is_timeout() || source.is_connect(),
            FetchError::Body { .. } => true,
            _ => false,
        }
    }

    /// Nothing answered at all: no server at that address, or no network
    pub fn is_unreachable(&self) -> bool {
        matches!(self, FetchError::Network { source, .. } if source.is_connect() && !source.is_timeout())
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            FetchError::Status { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}
