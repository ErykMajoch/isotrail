mod cache;
mod error;
mod http;
mod recorded;

use sha2::{Digest, Sha256};

pub use cache::{Cache, CacheStats, CachedFetcher};
pub use error::FetchError;
pub use http::{HttpFetcher, RetryPolicy, USER_AGENT};
pub use recorded::RecordedFetcher;

/// Where remote data comes from, so tests can swap the network for fixtures
pub trait Fetcher {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError>;
    fn post(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, FetchError>;
}

impl<F: Fetcher + ?Sized> Fetcher for &F {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        (**self).get(url)
    }

    fn post(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, FetchError> {
        (**self).post(url, body)
    }
}

impl<F: Fetcher + ?Sized> Fetcher for Box<F> {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        (**self).get(url)
    }

    fn post(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, FetchError> {
        (**self).post(url, body)
    }
}

fn sha256_hex(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
