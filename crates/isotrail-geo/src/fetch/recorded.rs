use std::fs;
use std::io;
use std::path::PathBuf;

use reqwest::Url;

use super::{FetchError, Fetcher, sha256_hex};

/// Serves recorded responses from a fixtures directory, laid out as
/// `<host>/<url path>`. Requests with a query string or a POST body get a
/// `-<hash>` suffix on the file name so different queries don't collide
#[derive(Debug, Clone)]
pub struct RecordedFetcher {
    dir: PathBuf,
}

impl RecordedFetcher {
    pub fn new(dir: impl Into<PathBuf>) -> RecordedFetcher {
        RecordedFetcher { dir: dir.into() }
    }

    /// The fixture file that answers a GET (`body: None`) or a POST
    pub fn path_for(&self, url: &str, body: Option<&[u8]>) -> Result<PathBuf, FetchError> {
        let invalid = |reason: &str| FetchError::InvalidUrl {
            url: url.to_owned(),
            reason: reason.to_owned(),
        };
        let parsed = Url::parse(url).map_err(|err| invalid(&err.to_string()))?;
        let host = parsed.host_str().ok_or_else(|| invalid("no host"))?;
        let segments: Vec<&str> = parsed
            .path_segments()
            .ok_or_else(|| invalid("no path"))?
            .collect();
        let safe = |segment: &str| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && !segment.contains(['\\', ':'])
        };
        if !segments.iter().all(|segment| safe(segment)) {
            return Err(invalid("the path does not map to a fixture file"));
        }

        let mut path = self.dir.join(host);
        path.extend(&segments);
        if parsed.query().is_some() || body.is_some() {
            let query = parsed.query().unwrap_or_default().as_bytes();
            let hash = sha256_hex(&[query, b"\n", body.unwrap_or_default()]);
            let mut name = path.file_name().unwrap_or_default().to_owned();
            name.push(format!("-{}", &hash[..16]));
            path.set_file_name(name);
        }
        Ok(path)
    }

    fn serve(&self, url: &str, body: Option<&[u8]>) -> Result<Vec<u8>, FetchError> {
        let path = self.path_for(url, body)?;
        fs::read(&path).map_err(|source| match source.kind() {
            io::ErrorKind::NotFound => FetchError::NotRecorded {
                url: url.to_owned(),
                path,
            },
            _ => FetchError::Cache { path, source },
        })
    }
}

impl Fetcher for RecordedFetcher {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        self.serve(url, None)
    }

    fn post(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, FetchError> {
        self.serve(url, Some(body))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn relative(fetcher: &RecordedFetcher, url: &str, body: Option<&[u8]>) -> PathBuf {
        let path = fetcher.path_for(url, body).unwrap();
        path.strip_prefix(&fetcher.dir).unwrap().to_path_buf()
    }

    #[test]
    fn get_maps_host_and_path() {
        let fetcher = RecordedFetcher::new("fixtures");
        assert_eq!(
            relative(
                &fetcher,
                "https://tiles.example.com/terrarium/12/2200/1400.png",
                None
            ),
            Path::new("tiles.example.com/terrarium/12/2200/1400.png")
        );
    }

    #[test]
    fn queries_and_bodies_get_a_hash_suffix() {
        let fetcher = RecordedFetcher::new("fixtures");
        let url = "https://api.example.com/interpreter";
        let query = relative(&fetcher, "https://api.example.com/interpreter?data=a", None);
        let post_a = relative(&fetcher, url, Some(b"data=a"));
        let post_b = relative(&fetcher, url, Some(b"data=b"));
        for path in [&query, &post_a, &post_b] {
            let name = path.file_name().unwrap().to_str().unwrap();
            assert_eq!(name.len(), "interpreter-".len() + 16, "{name}");
            assert!(name.starts_with("interpreter-"), "{name}");
            assert_eq!(path.parent().unwrap(), Path::new("api.example.com"));
        }
        assert_ne!(query, post_a);
        assert_ne!(post_a, post_b);
        assert_eq!(post_a, relative(&fetcher, url, Some(b"data=a")));
    }

    #[test]
    fn serves_recorded_files_and_names_missing_ones() {
        let temp = tempfile::tempdir().unwrap();
        let fetcher = RecordedFetcher::new(temp.path());
        let url = "https://tiles.example.com/z/1/2.png";
        let path = fetcher.path_for(url, None).unwrap();

        match fetcher.get(url).unwrap_err() {
            FetchError::NotRecorded { path: expected, .. } => assert_eq!(expected, path),
            error => panic!("{error:?}"),
        }

        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"png").unwrap();
        assert_eq!(fetcher.get(url).unwrap(), b"png");
        assert!(matches!(
            fetcher.post(url, b""),
            Err(FetchError::NotRecorded { .. })
        ));
    }

    #[test]
    fn rejects_paths_that_escape_or_are_not_files() {
        let fetcher = RecordedFetcher::new("fixtures");
        for url in [
            "https://example.com/",
            "https://example.com/dir/",
            "https://example.com/c:x",
            "data:text/plain,hi",
        ] {
            assert!(
                matches!(
                    fetcher.path_for(url, None),
                    Err(FetchError::InvalidUrl { .. })
                ),
                "{url}"
            );
        }
        let escaped = relative(&fetcher, "https://example.com/a/../../b", None);
        assert_eq!(escaped, Path::new("example.com/b"));
    }
}
