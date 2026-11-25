use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

#[cfg(target_arch = "wasm32")]
use super::RequestFailure;
use super::{ERROR_BODY_BYTES, FetchError, Fetcher, cache_key};

/// A request as the pipeline makes it
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Request {
    pub url: String,
    /// None for a GET
    pub body: Option<Vec<u8>>,
}

impl Request {
    pub fn get(url: impl Into<String>) -> Request {
        Request {
            url: url.into(),
            body: None,
        }
    }

    pub fn post(url: impl Into<String>, body: Vec<u8>) -> Request {
        Request {
            url: url.into(),
            body: Some(body),
        }
    }

    pub fn method(&self) -> &'static str {
        match self.body {
            None => "GET",
            Some(_) => "POST",
        }
    }

    /// The key the on-disk cache stores the answer under
    pub fn key(&self) -> String {
        cache_key(
            self.method(),
            &self.url,
            self.body.as_deref().unwrap_or_default(),
        )
    }
}

/// What came back for a request
#[derive(Debug, Clone)]
pub enum Answer {
    Body(Vec<u8>),
    Status {
        status: u16,
        body: Vec<u8>,
    },
    #[cfg(target_arch = "wasm32")]
    Failed(RequestFailure),
}

/// Answers held in memory, fetched by someone else. A request without one
/// is noted and fails as offline, so a run can be repeated once the missing
/// answers are held
#[derive(Debug, Default)]
pub struct ReplayFetcher {
    held: RefCell<HashMap<String, Answer>>,
    missed: RefCell<Vec<Request>>,
    forgotten: RefCell<HashSet<String>>,
}

impl ReplayFetcher {
    pub fn new() -> ReplayFetcher {
        ReplayFetcher::default()
    }

    /// Holds an answer under a request's key
    pub fn hold(&self, key: String, answer: Answer) {
        self.held.borrow_mut().insert(key, answer);
    }

    pub fn holds(&self, request: &Request) -> bool {
        self.held.borrow().contains_key(&request.key())
    }

    /// The requests asked for since the last call that had no answer, each once
    pub fn take_missed(&self) -> Vec<Request> {
        std::mem::take(&mut *self.missed.borrow_mut())
    }

    fn answer(&self, request: Request) -> Result<Vec<u8>, FetchError> {
        let key = request.key();
        let url = request.url.clone();
        match self.held.borrow().get(&key) {
            Some(Answer::Body(body)) => return Ok(body.clone()),
            Some(Answer::Status { status, body }) => {
                let kept = body.len().min(ERROR_BODY_BYTES as usize);
                return Err(FetchError::Status {
                    url,
                    status: *status,
                    retry_after: None,
                    body: body[..kept].to_vec(),
                });
            }
            #[cfg(target_arch = "wasm32")]
            Some(Answer::Failed(failure)) => {
                return Err(FetchError::Network {
                    url,
                    source: failure.clone(),
                });
            }
            None => {}
        }
        let mut missed = self.missed.borrow_mut();
        if !missed.contains(&request) {
            missed.push(request);
        }
        Err(FetchError::Offline { url })
    }
}

impl Fetcher for ReplayFetcher {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        self.answer(Request::get(url))
    }

    fn post(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, FetchError> {
        self.answer(Request::post(url, body.to_vec()))
    }

    // Only the first time, so an answer that is still damaged when fetched
    // again fails as it does from the disk cache
    fn forget(&self, url: &str, body: Option<&[u8]>) -> Result<(), FetchError> {
        let request = Request {
            url: url.to_owned(),
            body: body.map(<[u8]>::to_vec),
        };
        let key = request.key();
        if self.forgotten.borrow_mut().insert(key.clone()) {
            self.held.borrow_mut().remove(&key);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://example.org/tile.png";

    #[test]
    fn keys_are_the_disk_cache_keys() {
        assert_eq!(
            Request::get("https://e.org/a?apikey=s3cret").key(),
            cache_key("GET", "https://e.org/a?apikey=redacted", b"")
        );
        assert_ne!(
            Request::post(URL, b"q".to_vec()).key(),
            Request::get(URL).key()
        );
    }

    #[test]
    fn misses_are_noted_once_and_fail_as_offline() {
        let replay = ReplayFetcher::new();
        assert!(matches!(replay.get(URL), Err(FetchError::Offline { .. })));
        assert!(replay.get(URL).is_err());
        assert!(replay.post(URL, b"q").is_err());
        assert_eq!(
            replay.take_missed(),
            [Request::get(URL), Request::post(URL, b"q".to_vec())]
        );
        assert!(replay.take_missed().is_empty());
    }

    #[test]
    fn held_answers_are_given_back() {
        let replay = ReplayFetcher::new();
        replay.hold(Request::get(URL).key(), Answer::Body(b"png".to_vec()));
        replay.hold(
            Request::post(URL, b"q".to_vec()).key(),
            Answer::Status {
                status: 502,
                body: vec![b'x'; 10_000],
            },
        );
        assert_eq!(replay.get(URL).unwrap(), b"png");
        match replay.post(URL, b"q") {
            Err(FetchError::Status { status, body, .. }) => {
                assert_eq!(status, 502);
                assert_eq!(body.len(), ERROR_BODY_BYTES as usize);
            }
            other => panic!("{other:?}"),
        }
        assert!(replay.take_missed().is_empty());
    }

    #[test]
    fn a_forgotten_answer_is_fetched_again_once() {
        let replay = ReplayFetcher::new();
        let key = Request::get(URL).key();
        replay.hold(key.clone(), Answer::Body(b"bad".to_vec()));
        replay.forget(URL, None).unwrap();
        assert!(replay.get(URL).is_err());
        assert_eq!(replay.take_missed(), [Request::get(URL)]);
        replay.hold(key, Answer::Body(b"still bad".to_vec()));
        replay.forget(URL, None).unwrap();
        assert_eq!(replay.get(URL).unwrap(), b"still bad");
    }
}
