mod cache;
mod error;
mod http;
mod recorded;

use reqwest::Url;
use sha2::{Digest, Sha256};

pub use cache::{Cache, CacheStats, CachedFetcher};
pub use error::FetchError;
pub use http::{HttpFetcher, MAX_RESPONSE_BYTES, RetryPolicy, USER_AGENT};
pub use recorded::RecordedFetcher;

/// Where remote data comes from, so tests can swap the network for fixtures
pub trait Fetcher {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError>;
    fn post(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, FetchError>;

    /// Drops a stored response that turned out to be unusable, so the next
    /// request fetches it again. `body: None` means a GET
    fn forget(&self, _url: &str, _body: Option<&[u8]>) -> Result<(), FetchError> {
        Ok(())
    }
}

impl<F: Fetcher + ?Sized> Fetcher for &F {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        (**self).get(url)
    }

    fn post(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, FetchError> {
        (**self).post(url, body)
    }

    fn forget(&self, url: &str, body: Option<&[u8]>) -> Result<(), FetchError> {
        (**self).forget(url, body)
    }
}

impl<F: Fetcher + ?Sized> Fetcher for Box<F> {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        (**self).get(url)
    }

    fn post(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, FetchError> {
        (**self).post(url, body)
    }

    fn forget(&self, url: &str, body: Option<&[u8]>) -> Result<(), FetchError> {
        (**self).forget(url, body)
    }
}

/// The scheme, host and port of a URL, without credentials, path or query
pub fn origin(url: &str) -> String {
    match Url::parse(url) {
        Ok(parsed) if parsed.has_host() => parsed.origin().ascii_serialization(),
        _ => "an unreadable address".to_owned(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_leave_out_credentials_and_paths() {
        assert_eq!(
            origin("https://user:secret@overpass.example.org/api/interpreter?x=1"),
            "https://overpass.example.org"
        );
        assert_eq!(origin("http://127.0.0.1:9/api"), "http://127.0.0.1:9");
        assert_eq!(origin("https://example.org:443/a"), "https://example.org");
        assert_eq!(origin("not a url"), "an unreadable address");
    }
}
