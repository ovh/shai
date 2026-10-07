use std::fmt;
use std::time::Duration;

/// An HTTP error response from a provider that may carry a `Retry-After` hint.
///
/// Produced by providers where shai-llm owns the HTTP layer (Anthropic, and the
/// `ChatClient`-based providers such as Mistral). Providers going through
/// `openai_dive` directly cannot surface the header and keep the generic error.
#[derive(Debug, Clone)]
pub struct RateLimitedError {
    /// HTTP status code (typically 429, 503 or 529).
    pub status: u16,
    /// Delay announced by the server via the `Retry-After` header, if any.
    pub retry_after: Option<Duration>,
    /// Response body / error message from the provider.
    pub message: String,
}

impl fmt::Display for RateLimitedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HTTP {}: {}", self.status, self.message)
    }
}

impl std::error::Error for RateLimitedError {}

impl RateLimitedError {
    pub fn new(status: u16, retry_after: Option<Duration>, message: String) -> Self {
        Self {
            status,
            retry_after,
            message,
        }
    }
}

/// Parse a `Retry-After` header value.
///
/// Supports both formats from RFC 9110:
/// - delta-seconds (e.g. `120`)
/// - HTTP-date (e.g. `Fri, 31 Dec 2026 23:59:59 GMT`)
///
/// Returns `None` for invalid values. A date in the past yields `Duration::ZERO`.
pub fn parse_retry_after(raw: &str) -> Option<Duration> {
    let raw = raw.trim();

    if let Ok(secs) = raw.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }

    if let Ok(date) = chrono::DateTime::parse_from_rfc2822(raw) {
        let delta = date
            .with_timezone(&chrono::Utc)
            .signed_duration_since(chrono::Utc::now());
        return Some(delta.to_std().unwrap_or(Duration::ZERO));
    }

    None
}

/// Extract and parse the `Retry-After` header from a response header map.
pub fn retry_after_from_headers(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()
        .and_then(parse_retry_after)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};

    #[test]
    fn test_parse_retry_after_seconds() {
        assert_eq!(parse_retry_after("120"), Some(Duration::from_secs(120)));
        assert_eq!(parse_retry_after(" 2 "), Some(Duration::from_secs(2)));
        assert_eq!(parse_retry_after("0"), Some(Duration::ZERO));
    }

    #[test]
    fn test_parse_retry_after_http_date() {
        let future = chrono::Utc::now() + chrono::Duration::seconds(30);
        let header = future.format("%a, %d %b %Y %H:%M:%S GMT").to_string();
        let parsed = parse_retry_after(&header).expect("valid HTTP-date should parse");
        assert!(parsed <= Duration::from_secs(31) && parsed >= Duration::from_secs(28));

        let past = "Fri, 01 Jan 2021 00:00:00 GMT";
        assert_eq!(parse_retry_after(past), Some(Duration::ZERO));
    }

    #[test]
    fn test_parse_retry_after_invalid() {
        assert_eq!(parse_retry_after("garbage"), None);
        assert_eq!(parse_retry_after("-5"), None);
        assert_eq!(parse_retry_after(""), None);
    }

    #[test]
    fn test_retry_after_from_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("7"));
        assert_eq!(
            retry_after_from_headers(&headers),
            Some(Duration::from_secs(7))
        );

        let empty = HeaderMap::new();
        assert_eq!(retry_after_from_headers(&empty), None);
    }

    #[test]
    fn test_rate_limited_error_display() {
        let err = RateLimitedError::new(429, Some(Duration::from_secs(2)), "slow down".into());
        assert_eq!(err.to_string(), "HTTP 429: slow down");
    }
}
