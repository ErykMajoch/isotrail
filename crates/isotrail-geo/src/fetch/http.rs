use std::io::Read;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use reqwest::blocking::{Client, RequestBuilder, Response};
use reqwest::header::{CONTENT_TYPE, RETRY_AFTER};
use tracing::warn;

use super::{FetchError, Fetcher, origin, redact};

pub const USER_AGENT: &str = concat!(
    "isotrail/",
    env!("CARGO_PKG_VERSION"),
    " (+",
    env!("CARGO_PKG_REPOSITORY"),
    ")"
);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
// Longer than the Overpass query timeout, so the server gives up first and says why
const TIMEOUT: Duration = Duration::from_secs(300);
pub const MAX_RESPONSE_BYTES: u64 = 512_000_000;
const SLOW_AFTER: Duration = Duration::from_secs(10);
const ERROR_BODY_BYTES: u64 = 4096;

/// How often and how patiently to retry rate-limited or failing requests
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            max_attempts: 4,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
        }
    }
}

impl RetryPolicy {
    /// A single attempt, for callers that retry by themselves
    pub const fn once() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 1,
            base_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
        }
    }

    /// Busy Overpass servers recover in minutes rather than seconds
    pub const fn overpass() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 6,
            base_delay: Duration::from_secs(5),
            max_delay: Duration::from_secs(60),
        }
    }

    /// Wait before retry number `retry` (0-based): exponential backoff,
    /// stretched to the server's `Retry-After` and capped at `max_delay`
    pub fn delay(&self, retry: u32, retry_after: Option<Duration>) -> Duration {
        let backoff = self
            .base_delay
            .saturating_mul(1u32.checked_shl(retry).unwrap_or(u32::MAX));
        retry_after
            .map_or(backoff, |after| after.max(backoff))
            .min(self.max_delay)
    }
}

/// Fetches over HTTPS with a descriptive User-Agent, timeouts and retries
#[derive(Debug, Clone)]
pub struct HttpFetcher {
    client: Client,
    retry: RetryPolicy,
    limit: u64,
    slow_after: Duration,
}

impl HttpFetcher {
    pub fn new() -> Result<HttpFetcher, FetchError> {
        let client = Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(TIMEOUT)
            .build()
            .map_err(FetchError::Client)?;
        Ok(HttpFetcher {
            client,
            retry: RetryPolicy::default(),
            limit: MAX_RESPONSE_BYTES,
            slow_after: SLOW_AFTER,
        })
    }

    pub fn with_retry(self, retry: RetryPolicy) -> HttpFetcher {
        HttpFetcher { retry, ..self }
    }

    /// The largest answer accepted, in bytes
    pub fn with_limit(self, limit: u64) -> HttpFetcher {
        HttpFetcher { limit, ..self }
    }

    /// How long a request may take before a line says it is still waiting
    pub fn with_slow_after(self, slow_after: Duration) -> HttpFetcher {
        HttpFetcher { slow_after, ..self }
    }

    fn send(&self, url: &str, request: impl Fn() -> RequestBuilder) -> Result<Vec<u8>, FetchError> {
        // Errors and logs only ever see the URL with its secrets masked
        let shown = redact(url);
        let mut attempt = 1;
        loop {
            match self.watched(&shown, request()) {
                Ok(bytes) => return Ok(bytes),
                Err(error) if error.is_transient() && attempt < self.retry.max_attempts => {
                    let delay = self.retry.delay(attempt - 1, error.retry_after());
                    warn!(
                        "{error} (attempt {attempt} of {}), retrying in {:.1}s",
                        self.retry.max_attempts,
                        delay.as_secs_f64()
                    );
                    thread::sleep(delay);
                    attempt += 1;
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// One attempt, with a line once it has taken longer than `slow_after`
    fn watched(&self, url: &str, request: RequestBuilder) -> Result<Vec<u8>, FetchError> {
        let (done, finished) = mpsc::channel::<()>();
        let slow_after = self.slow_after;
        thread::scope(|scope| {
            scope.spawn(move || {
                if let Err(RecvTimeoutError::Timeout) = finished.recv_timeout(slow_after) {
                    warn!(
                        "still waiting for {} after {:.0}s",
                        origin(url),
                        slow_after.as_secs_f64()
                    );
                }
            });
            let result = self.attempt(url, request);
            drop(done);
            result
        })
    }

    fn attempt(&self, url: &str, request: RequestBuilder) -> Result<Vec<u8>, FetchError> {
        let response = request.send().map_err(|source| FetchError::Network {
            url: url.to_owned(),
            source: source.without_url(),
        })?;
        let status = response.status();
        if !status.is_success() {
            let retry_after = response
                .headers()
                .get(RETRY_AFTER)
                .and_then(|value| value.to_str().ok()?.trim().parse().ok())
                .map(Duration::from_secs);
            let mut body = Vec::new();
            let _ = response.take(ERROR_BODY_BYTES).read_to_end(&mut body);
            return Err(FetchError::Status {
                url: url.to_owned(),
                status: status.as_u16(),
                retry_after,
                body,
            });
        }
        self.read_body(url, response)
    }

    fn read_body(&self, url: &str, response: Response) -> Result<Vec<u8>, FetchError> {
        let too_large = || FetchError::TooLarge {
            url: url.to_owned(),
            limit: self.limit,
        };
        if response
            .content_length()
            .is_some_and(|len| len > self.limit)
        {
            return Err(too_large());
        }
        let mut bytes = Vec::new();
        response
            .take(self.limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| FetchError::Body {
                url: url.to_owned(),
                source,
            })?;
        if bytes.len() as u64 > self.limit {
            return Err(too_large());
        }
        Ok(bytes)
    }
}

impl Fetcher for HttpFetcher {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        self.send(url, || self.client.get(url))
    }

    // Overpass, the only POST user, takes its query as a form field
    fn post(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, FetchError> {
        self.send(url, || {
            self.client
                .post(url)
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(body.to_vec())
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread::JoinHandle;

    use super::*;

    const FAST: RetryPolicy = RetryPolicy {
        max_attempts: 3,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(5),
    };

    #[test]
    fn backoff_doubles_up_to_the_cap() {
        let policy = RetryPolicy::default();
        let secs = |retry| policy.delay(retry, None).as_secs();
        assert_eq!([secs(0), secs(1), secs(2), secs(3)], [1, 2, 4, 8]);
        assert_eq!(secs(5), 30);
        assert_eq!(secs(40), 30);
    }

    #[test]
    fn retry_after_stretches_but_never_shortens_or_exceeds_the_cap() {
        let policy = RetryPolicy::default();
        let after = |retry, secs| {
            policy
                .delay(retry, Some(Duration::from_secs(secs)))
                .as_secs()
        };
        assert_eq!(after(0, 10), 10);
        assert_eq!(after(3, 1), 8);
        assert_eq!(after(0, 600), 30);
    }

    fn reply(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nRetry-After: 0\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    /// Answers one connection per scripted reply on 127.0.0.1 and returns
    /// the raw requests it saw
    fn serve(replies: Vec<String>) -> (String, JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/tile.png", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            replies
                .into_iter()
                .map(|reply| {
                    let (mut stream, _) = listener.accept().unwrap();
                    let request = read_request(&mut stream);
                    stream.write_all(reply.as_bytes()).unwrap();
                    request
                })
                .collect()
        });
        (url, handle)
    }

    fn read_request(stream: &mut impl Read) -> String {
        let mut data = Vec::new();
        let mut buf = [0; 1024];
        loop {
            let n = stream.read(&mut buf).unwrap();
            data.extend_from_slice(&buf[..n]);
            if n == 0 || is_complete(&data) {
                return String::from_utf8_lossy(&data).into_owned();
            }
        }
    }

    fn is_complete(data: &[u8]) -> bool {
        let text = String::from_utf8_lossy(data).to_ascii_lowercase();
        let Some(end) = text.find("\r\n\r\n") else {
            return false;
        };
        let length = text
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .map_or(0, |len| len.trim().parse::<usize>().unwrap());
        data.len() >= end + 4 + length
    }

    fn fetcher() -> HttpFetcher {
        HttpFetcher::new().unwrap().with_retry(FAST)
    }

    #[test]
    fn retries_a_server_error_then_succeeds() {
        let (url, server) = serve(vec![
            reply("503 Service Unavailable", ""),
            reply("200 OK", "tile"),
        ]);
        assert_eq!(fetcher().get(&url).unwrap(), b"tile");
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        let first = requests[0].to_ascii_lowercase();
        assert!(first.starts_with("get /tile.png "), "{first}");
        assert!(
            first.contains(&format!("user-agent: {}", USER_AGENT.to_ascii_lowercase())),
            "{first}"
        );
    }

    #[test]
    fn gives_up_after_the_last_attempt() {
        let replies = vec![reply("429 Too Many Requests", ""); 3];
        let (url, server) = serve(replies);
        let error = fetcher().get(&url).unwrap_err();
        assert!(
            matches!(error, FetchError::Status { status: 429, .. }),
            "{error:?}"
        );
        assert_eq!(server.join().unwrap().len(), 3);
    }

    #[test]
    fn client_errors_are_not_retried() {
        let (url, server) = serve(vec![reply("404 Not Found", "")]);
        let error = fetcher().get(&url).unwrap_err();
        assert!(
            matches!(error, FetchError::Status { status: 404, .. }),
            "{error:?}"
        );
        assert_eq!(server.join().unwrap().len(), 1);
    }

    #[test]
    fn a_cut_off_answer_is_retried() {
        let cut = "HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\npart";
        let (url, server) = serve(vec![cut.to_owned(), reply("200 OK", "tile")]);
        assert_eq!(fetcher().get(&url).unwrap(), b"tile");
        assert_eq!(server.join().unwrap().len(), 2);
    }

    #[test]
    fn error_answers_keep_retry_after_and_the_start_of_the_body() {
        let busy = "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 4\r\nRetry-After: 7\r\n\
                    Connection: close\r\n\r\nbusy";
        let (url, server) = serve(vec![busy.to_owned()]);
        let error = fetcher()
            .with_retry(RetryPolicy::once())
            .get(&url)
            .unwrap_err();
        assert!(error.is_transient());
        assert_eq!(error.retry_after(), Some(Duration::from_secs(7)));
        assert!(
            matches!(&error, FetchError::Status { body, .. } if body == b"busy"),
            "{error:?}"
        );
        server.join().unwrap();
    }

    #[test]
    fn answers_over_the_limit_are_refused() {
        let no_length = "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n0123456789abcdef";
        let (url, server) = serve(vec![
            reply("200 OK", "0123456789abcdef"),
            no_length.to_owned(),
        ]);
        for _ in 0..2 {
            let error = fetcher().with_limit(10).get(&url).unwrap_err();
            assert!(
                matches!(error, FetchError::TooLarge { limit: 10, .. }),
                "{error:?}"
            );
            assert!(!error.is_transient());
        }
        server.join().unwrap();
        let (url, server) = serve(vec![reply("200 OK", "0123456789")]);
        assert_eq!(fetcher().with_limit(10).get(&url).unwrap(), b"0123456789");
        server.join().unwrap();
    }

    #[test]
    fn nothing_listening_is_unreachable() {
        let port = crate::fetch::CLOSED_PORT;
        let error = fetcher()
            .with_retry(RetryPolicy::once())
            .get(&format!("http://127.0.0.1:{port}/"))
            .unwrap_err();
        assert!(error.is_unreachable() && error.is_transient(), "{error:?}");
    }

    #[test]
    fn overpass_waits_up_to_a_minute() {
        let policy = RetryPolicy::overpass();
        let secs: Vec<u64> = (0..5).map(|r| policy.delay(r, None).as_secs()).collect();
        assert_eq!(secs, [5, 10, 20, 40, 60]);
        assert_eq!(policy.max_attempts, 6);
        assert_eq!(RetryPolicy::once().max_attempts, 1);
    }

    #[test]
    fn posts_the_body_as_a_form() {
        let (url, server) = serve(vec![reply("200 OK", "{}")]);
        assert_eq!(fetcher().post(&url, b"data=node").unwrap(), b"{}");
        let request = server.join().unwrap().remove(0);
        assert!(request.starts_with("POST /tile.png "), "{request}");
        assert!(
            request
                .to_ascii_lowercase()
                .contains("content-type: application/x-www-form-urlencoded"),
            "{request}"
        );
        assert!(request.ends_with("\r\n\r\ndata=node"), "{request}");
    }
}
