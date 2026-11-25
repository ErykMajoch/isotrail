use std::time::Duration;

/// Why a failed request is worth trying again
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retry {
    /// The server answered with a busy status, slowly or not in full
    Busy,
    /// Nothing answered at all: no server at that address, or no network
    Unreachable,
}

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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn overpass_waits_up_to_a_minute() {
        let policy = RetryPolicy::overpass();
        let secs: Vec<u64> = (0..5).map(|r| policy.delay(r, None).as_secs()).collect();
        assert_eq!(secs, [5, 10, 20, 40, 60]);
        assert_eq!(policy.max_attempts, 6);
        assert_eq!(RetryPolicy::once().max_attempts, 1);
    }
}
