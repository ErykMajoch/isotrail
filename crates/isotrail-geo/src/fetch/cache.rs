use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;
use serde::Serialize;
use tracing::{debug, warn};

use super::{FetchError, Fetcher, sha256_hex};

const DATA_EXT: &str = "bin";
const META_EXT: &str = "json";
const TEMP_EXT: &str = "tmp";

/// Raw responses on disk, keyed by a SHA-256 of method, URL and body
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cache {
    dir: PathBuf,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub files: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Copy)]
struct Request<'a> {
    method: &'static str,
    url: &'a str,
    body: &'a [u8],
}

impl Request<'_> {
    fn key(&self) -> String {
        sha256_hex(&[
            self.method.as_bytes(),
            b"\n",
            self.url.as_bytes(),
            b"\n",
            self.body,
        ])
    }
}

#[derive(Serialize)]
struct Metadata<'a> {
    method: &'a str,
    url: &'a str,
    body_sha256: Option<String>,
    bytes: usize,
    fetched_unix_s: u64,
}

impl Cache {
    pub fn new(dir: impl Into<PathBuf>) -> Cache {
        Cache { dir: dir.into() }
    }

    /// The per-user cache directory, e.g. `%LOCALAPPDATA%\isotrail\cache`
    /// on Windows or `~/.cache/isotrail` on Linux
    pub fn default_dir() -> Option<PathBuf> {
        ProjectDirs::from("", "", "isotrail").map(|dirs| dirs.cache_dir().to_path_buf())
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn stats(&self) -> Result<CacheStats, FetchError> {
        Ok(self
            .entries()?
            .iter()
            .fold(CacheStats::default(), |stats, (_, bytes)| CacheStats {
                files: stats.files + 1,
                bytes: stats.bytes + bytes,
            }))
    }

    /// Deletes the cached responses but leaves the directory and any
    /// foreign files in it alone
    pub fn clear(&self) -> Result<CacheStats, FetchError> {
        let mut stats = CacheStats::default();
        for (path, bytes) in self.entries()? {
            fs::remove_file(&path).map_err(|source| FetchError::Cache { path, source })?;
            stats.files += 1;
            stats.bytes += bytes;
        }
        Ok(stats)
    }

    fn entries(&self) -> Result<Vec<(PathBuf, u64)>, FetchError> {
        let error = |source| FetchError::Cache {
            path: self.dir.clone(),
            source,
        };
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(error(err)),
        };
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry.map_err(error)?;
            let path = entry.path();
            let ours = path.extension().is_some_and(|ext| {
                [DATA_EXT, META_EXT, TEMP_EXT]
                    .iter()
                    .any(|ours| ext == *ours)
            });
            let meta = entry.metadata().map_err(error)?;
            if ours && meta.is_file() {
                files.push((path, meta.len()));
            }
        }
        Ok(files)
    }

    fn get(&self, request: &Request) -> Result<Option<Vec<u8>>, FetchError> {
        let path = self.dir.join(format!("{}.{DATA_EXT}", request.key()));
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(FetchError::Cache { path, source }),
        }
    }

    fn put(&self, request: &Request, bytes: &[u8]) -> Result<(), FetchError> {
        let key = request.key();
        let metadata = Metadata {
            method: request.method,
            url: request.url,
            body_sha256: (!request.body.is_empty()).then(|| sha256_hex(&[request.body])),
            bytes: bytes.len(),
            fetched_unix_s: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| since.as_secs()),
        };
        let metadata = serde_json::to_vec_pretty(&metadata).map_err(io::Error::from);
        let temp = self
            .dir
            .join(format!("{key}.{}.{TEMP_EXT}", std::process::id()));
        let data = self.dir.join(format!("{key}.{DATA_EXT}"));
        let meta = self.dir.join(format!("{key}.{META_EXT}"));
        // Renaming into place means an interrupted write never looks like a hit
        fs::create_dir_all(&self.dir)
            .and_then(|()| fs::write(&temp, bytes))
            .and_then(|()| fs::rename(&temp, &data))
            .and_then(|()| fs::write(&meta, metadata?))
            .map_err(|source| FetchError::Cache {
                path: self.dir.clone(),
                source,
            })
    }
}

/// Serves responses from a [`Cache`] and fills it from `inner` on a miss.
/// In offline mode a miss is an error and `inner` is never called
#[derive(Debug)]
pub struct CachedFetcher<F> {
    inner: F,
    cache: Cache,
    offline: bool,
}

impl<F: Fetcher> CachedFetcher<F> {
    pub fn new(inner: F, cache: Cache) -> CachedFetcher<F> {
        CachedFetcher {
            inner,
            cache,
            offline: false,
        }
    }

    pub fn offline(self, offline: bool) -> CachedFetcher<F> {
        CachedFetcher { offline, ..self }
    }

    pub fn cache(&self) -> &Cache {
        &self.cache
    }

    fn fetch(
        &self,
        request: Request,
        send: impl FnOnce() -> Result<Vec<u8>, FetchError>,
    ) -> Result<Vec<u8>, FetchError> {
        let url = request.url;
        if let Some(bytes) = self.cache.get(&request)? {
            debug!("cache hit: {} {url}", request.method);
            return Ok(bytes);
        }
        if self.offline {
            return Err(FetchError::Offline {
                url: url.to_owned(),
            });
        }
        debug!("cache miss: {} {url}", request.method);
        let bytes = send()?;
        if let Err(err) = self.cache.put(&request, &bytes) {
            warn!("could not cache {url}: {err}");
        }
        Ok(bytes)
    }
}

impl<F: Fetcher> Fetcher for CachedFetcher<F> {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        let request = Request {
            method: "GET",
            url,
            body: &[],
        };
        self.fetch(request, || self.inner.get(url))
    }

    fn post(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, FetchError> {
        let request = Request {
            method: "POST",
            url,
            body,
        };
        self.fetch(request, || self.inner.post(url, body))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    const URL: &str = "https://example.com/tiles/1/2/3.png";

    #[derive(Default)]
    struct Fake {
        calls: Cell<u32>,
        fail: bool,
    }

    impl Fake {
        fn answer(&self, method: &str, url: &str, body: &[u8]) -> Result<Vec<u8>, FetchError> {
            self.calls.set(self.calls.get() + 1);
            if self.fail {
                return Err(FetchError::Status {
                    url: url.to_owned(),
                    status: 503,
                });
            }
            Ok(format!("{method} {url} {}", String::from_utf8_lossy(body)).into_bytes())
        }
    }

    impl Fetcher for Fake {
        fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
            self.answer("GET", url, &[])
        }

        fn post(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, FetchError> {
            self.answer("POST", url, body)
        }
    }

    fn setup() -> (tempfile::TempDir, Cache) {
        let temp = tempfile::tempdir().unwrap();
        let cache = Cache::new(temp.path().join("cache"));
        (temp, cache)
    }

    #[test]
    fn a_miss_fills_the_cache_and_the_next_call_hits() {
        let (_temp, cache) = setup();
        let fetcher = CachedFetcher::new(Fake::default(), cache.clone());
        let first = fetcher.get(URL).unwrap();
        let second = fetcher.get(URL).unwrap();
        assert_eq!(first, second);
        assert_eq!(fetcher.inner.calls.get(), 1);
        assert_eq!(cache.stats().unwrap().files, 2);
    }

    #[test]
    fn metadata_records_the_request() {
        let (_temp, cache) = setup();
        CachedFetcher::new(Fake::default(), cache.clone())
            .post(URL, b"data=x")
            .unwrap();
        let key = Request {
            method: "POST",
            url: URL,
            body: b"data=x",
        }
        .key();
        let meta = fs::read_to_string(cache.dir().join(format!("{key}.json"))).unwrap();
        let meta: serde_json::Value = serde_json::from_str(&meta).unwrap();
        assert_eq!(meta["method"], "POST");
        assert_eq!(meta["url"], URL);
        assert_eq!(meta["bytes"], format!("POST {URL} data=x").len());
        assert_eq!(meta["body_sha256"].as_str().unwrap().len(), 64);
    }

    #[test]
    fn offline_serves_hits_and_refuses_misses() {
        let (_temp, cache) = setup();
        let online = CachedFetcher::new(Fake::default(), cache.clone());
        let cached = online.get(URL).unwrap();

        let offline = CachedFetcher::new(Fake::default(), cache).offline(true);
        assert_eq!(offline.get(URL).unwrap(), cached);
        let other = "https://example.com/tiles/1/2/4.png";
        let error = offline.get(other).unwrap_err();
        assert!(
            matches!(&error, FetchError::Offline { url } if url == other),
            "{error:?}"
        );
        assert_eq!(
            error.to_string(),
            format!("{other} is not in the cache and offline mode is on")
        );
        assert_eq!(offline.inner.calls.get(), 0);
    }

    #[test]
    fn failures_are_not_cached() {
        let (_temp, cache) = setup();
        let fake = Fake {
            fail: true,
            ..Fake::default()
        };
        let fetcher = CachedFetcher::new(fake, cache.clone());
        assert!(fetcher.get(URL).is_err());
        assert!(fetcher.get(URL).is_err());
        assert_eq!(fetcher.inner.calls.get(), 2);
        assert_eq!(cache.stats().unwrap(), CacheStats::default());
    }

    #[test]
    fn method_and_body_are_part_of_the_key() {
        let (_temp, cache) = setup();
        let fetcher = CachedFetcher::new(Fake::default(), cache);
        let fetch_all = || {
            [
                fetcher.get(URL).unwrap(),
                fetcher.post(URL, b"").unwrap(),
                fetcher.post(URL, b"a").unwrap(),
                fetcher.post(URL, b"b").unwrap(),
            ]
        };
        let first = fetch_all();
        assert_eq!(fetch_all(), first);
        assert_eq!(fetcher.inner.calls.get(), 4);
        for (i, a) in first.iter().enumerate() {
            assert!(first[i + 1..].iter().all(|b| a != b));
        }
    }

    #[test]
    fn leftovers_without_data_are_misses() {
        let (_temp, cache) = setup();
        let key = Request {
            method: "GET",
            url: URL,
            body: &[],
        }
        .key();
        fs::create_dir_all(cache.dir()).unwrap();
        fs::write(cache.dir().join(format!("{key}.123.tmp")), "partial").unwrap();
        fs::write(cache.dir().join(format!("{key}.json")), "{}").unwrap();

        let fetcher = CachedFetcher::new(Fake::default(), cache).offline(true);
        assert!(matches!(fetcher.get(URL), Err(FetchError::Offline { .. })));
    }

    #[test]
    fn clear_removes_cached_files_only() {
        let (_temp, cache) = setup();
        let fetcher = CachedFetcher::new(Fake::default(), cache.clone());
        fetcher.get(URL).unwrap();
        fetcher.post(URL, b"q").unwrap();
        fs::write(cache.dir().join("notes.txt"), "keep me").unwrap();

        let before = cache.stats().unwrap();
        assert_eq!(before.files, 4);
        assert!(before.bytes > 0);
        assert_eq!(cache.clear().unwrap(), before);
        assert_eq!(cache.stats().unwrap(), CacheStats::default());
        assert!(cache.dir().join("notes.txt").exists());

        fetcher.get(URL).unwrap();
        assert_eq!(fetcher.inner.calls.get(), 3);
    }

    #[test]
    fn a_missing_directory_is_an_empty_cache() {
        let (_temp, cache) = setup();
        assert_eq!(cache.stats().unwrap(), CacheStats::default());
        assert_eq!(cache.clear().unwrap(), CacheStats::default());
    }

    #[test]
    fn default_dir_is_named_after_the_app() {
        let dir = Cache::default_dir().unwrap();
        assert!(
            dir.components().any(|part| part.as_os_str() == "isotrail"),
            "{}",
            dir.display()
        );
    }
}
