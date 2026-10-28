//! Decides whether a failed call is safe to repeat.
//!
//! Repeating a read is harmless. Repeating a write is only safe when the server said it
//! refused the call, because a write that failed in transit may still have been applied,
//! and a repeated message reaches the user twice. So writes retry only on a 429 that says
//! when to come back, and never after a transport failure or a 5xx.
//!
//! This is per-process and doesn't throttle: it can't coordinate separate invocations in
//! a shell loop.

use std::time::Duration;

use crate::request::Method;

/// Total tries per request, including the first.
pub const MAX_ATTEMPTS: u32 = 3;
/// The longest wait worth holding a command open for. Past this, fail and let the caller
/// decide, since a silent long wait looks like a hang.
pub const MAX_WAIT: Duration = Duration::from_secs(30);
/// The wait before the second try; it doubles each time.
pub const BASE_BACKOFF: Duration = Duration::from_secs(1);

/// How a try failed, for the kinds of failure worth classifying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The connection was never established.
    Unreachable,
    /// The server answered with this status.
    Status {
        status: u16,
        retry_after: Option<Duration>,
    },
}

/// The server errors that say nothing about the request itself, so a read may repeat it.
pub const fn is_server_error(status: u16) -> bool {
    matches!(status, 500 | 502 | 503 | 504)
}

/// How long to wait before repeating try number `attempt`, or `None` to stop.
pub fn delay(
    method: Method,
    attempt: u32,
    failure: Failure,
    base_backoff: Duration,
) -> Option<Duration> {
    if attempt >= MAX_ATTEMPTS {
        return None;
    }
    let is_read = matches!(method, Method::Get);
    let backoff = base_backoff.saturating_mul(2u32.saturating_pow(attempt - 1));
    let wait = match failure {
        Failure::Unreachable => is_read.then_some(backoff),
        Failure::Status {
            status: 429,
            retry_after: Some(wait),
        } => Some(wait),
        Failure::Status {
            status,
            retry_after,
        } if is_read && (status == 429 || is_server_error(status)) => {
            Some(retry_after.unwrap_or(backoff))
        }
        Failure::Status { .. } => None,
    };
    wait.filter(|wait| *wait <= MAX_WAIT)
}

/// A `Retry-After` in delta-seconds. Anything else counts as no hint, including the
/// HTTP-date form, so a value that can't be understood never licenses a repeated write.
pub fn parse_retry_after(value: &str) -> Option<Duration> {
    let value = value.trim();
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse().ok().map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: Duration = BASE_BACKOFF;

    fn status(status: u16, retry_after: Option<u64>) -> Failure {
        Failure::Status {
            status,
            retry_after: retry_after.map(Duration::from_secs),
        }
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn reads_retry_on_transient_failures_with_doubling_backoff() {
        for failure in [
            Failure::Unreachable,
            status(429, None),
            status(500, None),
            status(502, None),
            status(503, None),
            status(504, None),
        ] {
            assert_eq!(
                delay(Method::Get, 1, failure, BASE),
                Some(secs(1)),
                "{failure:?}"
            );
            assert_eq!(
                delay(Method::Get, 2, failure, BASE),
                Some(secs(2)),
                "{failure:?}"
            );
        }
    }

    #[test]
    fn reads_prefer_the_server_hint() {
        assert_eq!(
            delay(Method::Get, 1, status(429, Some(7)), BASE),
            Some(secs(7))
        );
        assert_eq!(
            delay(Method::Get, 1, status(503, Some(7)), BASE),
            Some(secs(7))
        );
    }

    #[test]
    fn writes_retry_only_on_a_429_that_says_when() {
        for method in [Method::Post, Method::Delete] {
            assert_eq!(delay(method, 1, status(429, Some(5)), BASE), Some(secs(5)));
            for failure in [
                Failure::Unreachable,
                status(429, None),
                status(500, None),
                status(500, Some(5)),
                status(503, Some(5)),
            ] {
                assert_eq!(delay(method, 1, failure, BASE), None, "{failure:?}");
            }
        }
    }

    #[test]
    fn other_statuses_never_retry() {
        for code in [200, 301, 400, 401, 403, 404, 501] {
            for method in [Method::Get, Method::Post] {
                assert_eq!(delay(method, 1, status(code, Some(1)), BASE), None);
            }
        }
    }

    #[test]
    fn attempts_and_waits_are_bounded() {
        assert_eq!(
            delay(Method::Get, MAX_ATTEMPTS, status(429, None), BASE),
            None
        );
        assert_eq!(
            delay(Method::Post, MAX_ATTEMPTS, status(429, Some(1)), BASE),
            None
        );
        assert_eq!(
            delay(Method::Get, 1, status(429, Some(30)), BASE),
            Some(secs(30))
        );
        assert_eq!(delay(Method::Get, 1, status(429, Some(31)), BASE), None);
        assert_eq!(delay(Method::Post, 1, status(429, Some(31)), BASE), None);
    }

    #[test]
    fn retry_after_accepts_only_delta_seconds() {
        assert_eq!(parse_retry_after("0"), Some(secs(0)));
        assert_eq!(parse_retry_after(" 120 "), Some(secs(120)));
        for bad in [
            "",
            "  ",
            "-1",
            "+1",
            "1.5",
            "soon",
            "Wed, 21 Oct 2026 07:28:00 GMT",
            "99999999999999999999999",
        ] {
            assert_eq!(parse_retry_after(bad), None, "{bad:?}");
        }
    }
}
