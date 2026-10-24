use std::thread;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::blocking::{Client, RequestBuilder};
use reqwest::header::{CONTENT_TYPE, RETRY_AFTER};
use tracing::warn;

use super::{FetchError, Fetcher};

pub const USER_AGENT: &str = concat!(
    "isotrail/",
    env!("CARGO_PKG_VERSION"),
    " (+",
    env!("CARGO_PKG_REPOSITORY"),
    ")"
);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const TIMEOUT: Duration = Duration::from_secs(120);

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
        })
    }

    pub fn with_retry(self, retry: RetryPolicy) -> HttpFetcher {
        HttpFetcher { retry, ..self }
    }

    fn send(&self, url: &str, request: impl Fn() -> RequestBuilder) -> Result<Vec<u8>, FetchError> {
        let mut attempt = 1;
        loop {
            match attempt_once(url, request()) {
                Ok(bytes) => return Ok(bytes),
                Err(failure) if failure.retryable && attempt < self.retry.max_attempts => {
                    let delay = self.retry.delay(attempt - 1, failure.retry_after);
                    warn!(
                        "{} (attempt {attempt} of {}), retrying in {:.1} s",
                        failure.error,
                        self.retry.max_attempts,
                        delay.as_secs_f64()
                    );
                    thread::sleep(delay);
                    attempt += 1;
                }
                Err(failure) => return Err(failure.error),
            }
        }
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

struct Failure {
    error: FetchError,
    retryable: bool,
    retry_after: Option<Duration>,
}

fn attempt_once(url: &str, request: RequestBuilder) -> Result<Vec<u8>, Failure> {
    let network = |source: reqwest::Error| Failure {
        retryable: source.is_timeout() || source.is_connect(),
        retry_after: None,
        error: FetchError::Network {
            url: url.to_owned(),
            source,
        },
    };
    let response = request.send().map_err(network)?;
    let status = response.status();
    if !status.is_success() {
        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok()?.trim().parse().ok())
            .map(Duration::from_secs);
        return Err(Failure {
            error: FetchError::Status {
                url: url.to_owned(),
                status: status.as_u16(),
            },
            retryable: status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error(),
            retry_after,
        });
    }
    Ok(response.bytes().map_err(network)?.to_vec())
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
