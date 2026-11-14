mod cache;
mod error;
mod http;
mod recorded;

use std::borrow::Cow;

use reqwest::Url;
use sha2::{Digest, Sha256};

pub use cache::{Cache, CacheStats, CachedFetcher};
pub use error::FetchError;
pub use http::{HttpFetcher, MAX_RESPONSE_BYTES, RetryPolicy, USER_AGENT};
pub use recorded::RecordedFetcher;

// For tests that need nothing listening. Port 1 lies below every system's ephemeral range, so no test server or
// connection is ever given it, and with no socket on it every system refuses at once. A freed port can be handed
// to a parallel test, and macOS drops (rather than refuses) connections to a socket that is bound but not listening
#[cfg(test)]
pub(crate) const CLOSED_PORT: u16 = 1;

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

/// Query parameters that hold a secret
const SECRET_PARAMS: [&str; 5] = ["apikey", "api_key", "key", "access_token", "token"];

/// The URL with the values of secret query parameters masked, as it may be
/// shown, logged or used as a cache key
pub fn redact(url: &str) -> Cow<'_, str> {
    let Some(query_at) = url.find('?') else {
        return Cow::Borrowed(url);
    };
    let (base, query) = url.split_at(query_at + 1);
    let mut changed = false;
    let pairs: Vec<Cow<'_, str>> = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((name, value)) if SECRET_PARAMS.contains(&name) && !value.is_empty() => {
                changed = true;
                Cow::Owned(format!("{name}=redacted"))
            }
            _ => Cow::Borrowed(pair),
        })
        .collect();
    if changed {
        Cow::Owned(format!("{base}{}", pairs.join("&")))
    } else {
        Cow::Borrowed(url)
    }
}

/// The scheme, host and port of a URL, without credentials, path or query
pub fn origin(url: &str) -> String {
    match Url::parse(url) {
        Ok(parsed) if parsed.has_host() => parsed.origin().ascii_serialization(),
        _ => "an unreadable address".to_owned(),
    }
}

pub fn sha256_hex(parts: &[&[u8]]) -> String {
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

    #[test]
    fn secret_parameters_are_masked() {
        let plain = "https://example.org/v1/archive?latitude=1&longitude=2";
        assert!(matches!(redact(plain), Cow::Borrowed(_)));
        assert_eq!(
            redact("https://example.org/v1/archive?latitude=1&apikey=s3cret&x=y"),
            "https://example.org/v1/archive?latitude=1&apikey=redacted&x=y"
        );
        assert_eq!(
            redact("https://e.org/a?apikey=k"),
            "https://e.org/a?apikey=redacted"
        );
        assert_eq!(
            redact("https://e.org/a?myapikey=k"),
            "https://e.org/a?myapikey=k"
        );
        assert_eq!(
            redact("https://tiles.example/{z}/{x}/{y}.png?key=abc"),
            "https://tiles.example/{z}/{x}/{y}.png?key=redacted"
        );
    }
}
