//! Rate Limiting Middleware
//!
//! Provides per-organization rate limiting with:
//! - Token bucket algorithm for smooth rate limiting
//! - Sliding window for burst protection
//! - Configurable limits per org/endpoint
//! - Redis-compatible backend abstraction

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// Rate limit configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum requests per window
    pub max_requests: u64,
    /// Window duration
    pub window: Duration,
    /// Burst capacity (for token bucket)
    pub burst_capacity: u64,
    /// Refill rate (tokens per second)
    pub refill_rate: f64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests: 1000,
            window: Duration::from_secs(60),
            burst_capacity: 100,
            refill_rate: 16.67, // ~1000 per minute
        }
    }
}

impl RateLimitConfig {
    /// Create a config for high-traffic endpoints
    pub fn high_traffic() -> Self {
        Self {
            max_requests: 10000,
            window: Duration::from_secs(60),
            burst_capacity: 500,
            refill_rate: 166.67,
        }
    }

    /// Create a config for low-traffic/expensive endpoints
    pub fn low_traffic() -> Self {
        Self {
            max_requests: 100,
            window: Duration::from_secs(60),
            burst_capacity: 10,
            refill_rate: 1.67,
        }
    }

    /// Create a strict config for sensitive endpoints
    pub fn strict() -> Self {
        Self {
            max_requests: 10,
            window: Duration::from_secs(60),
            burst_capacity: 3,
            refill_rate: 0.17,
        }
    }
}

/// Result of a rate limit check
#[derive(Debug, Clone)]
pub struct RateLimitResult {
    /// Whether the request is allowed
    pub allowed: bool,
    /// Current number of remaining requests
    pub remaining: u64,
    /// When the rate limit resets
    pub reset_at: Instant,
    /// Retry after this duration (if not allowed)
    pub retry_after: Option<Duration>,
}

impl RateLimitResult {
    /// Get headers for rate limit response
    pub fn headers(&self) -> Vec<(&'static str, String)> {
        let mut headers = vec![
            ("X-RateLimit-Remaining", self.remaining.to_string()),
            (
                "X-RateLimit-Reset",
                self.reset_at.elapsed().as_secs().to_string(),
            ),
        ];

        if let Some(retry_after) = self.retry_after {
            headers.push(("Retry-After", retry_after.as_secs().to_string()));
        }

        headers
    }
}

/// Token bucket state for rate limiting
#[derive(Debug)]
struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
    capacity: f64,
    refill_rate: f64,
}

impl TokenBucket {
    fn new(capacity: u64, refill_rate: f64) -> Self {
        Self {
            tokens: capacity as f64,
            last_refill: Instant::now(),
            capacity: capacity as f64,
            refill_rate,
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity);
        self.last_refill = now;
    }

    fn try_consume(&mut self, tokens: f64) -> bool {
        self.refill();
        if self.tokens >= tokens {
            self.tokens -= tokens;
            true
        } else {
            false
        }
    }

    fn time_until_available(&self, tokens: f64) -> Duration {
        if self.tokens >= tokens {
            Duration::ZERO
        } else {
            let needed = tokens - self.tokens;
            Duration::from_secs_f64(needed / self.refill_rate)
        }
    }
}

/// Sliding window counter for rate limiting
#[derive(Debug)]
struct SlidingWindow {
    counts: Vec<(Instant, u64)>,
    window: Duration,
    max_requests: u64,
}

impl SlidingWindow {
    fn new(window: Duration, max_requests: u64) -> Self {
        Self {
            counts: Vec::new(),
            window,
            max_requests,
        }
    }

    fn cleanup(&mut self) {
        let cutoff = Instant::now() - self.window;
        self.counts.retain(|(time, _)| *time > cutoff);
    }

    fn count(&mut self) -> u64 {
        self.cleanup();
        self.counts.iter().map(|(_, c)| c).sum()
    }

    fn try_increment(&mut self) -> bool {
        self.cleanup();
        let current = self.count();
        if current < self.max_requests {
            self.counts.push((Instant::now(), 1));
            true
        } else {
            false
        }
    }

    fn remaining(&mut self) -> u64 {
        self.max_requests.saturating_sub(self.count())
    }

    fn reset_at(&self) -> Instant {
        self.counts
            .first()
            .map(|(time, _)| *time + self.window)
            .unwrap_or_else(|| Instant::now() + self.window)
    }
}

/// Rate limiter state for a single key (org + endpoint)
#[derive(Debug)]
struct RateLimiterState {
    token_bucket: TokenBucket,
    sliding_window: SlidingWindow,
}

impl RateLimiterState {
    fn new(config: &RateLimitConfig) -> Self {
        Self {
            token_bucket: TokenBucket::new(config.burst_capacity, config.refill_rate),
            sliding_window: SlidingWindow::new(config.window, config.max_requests),
        }
    }

    fn check(&mut self) -> RateLimitResult {
        // First check token bucket for burst protection
        if !self.token_bucket.try_consume(1.0) {
            let retry_after = self.token_bucket.time_until_available(1.0);
            return RateLimitResult {
                allowed: false,
                remaining: 0,
                reset_at: self.sliding_window.reset_at(),
                retry_after: Some(retry_after),
            };
        }

        // Then check sliding window for overall rate
        if !self.sliding_window.try_increment() {
            // Refund the token since we're rejecting
            self.token_bucket.tokens += 1.0;

            let reset_at = self.sliding_window.reset_at();
            let retry_after = reset_at.saturating_duration_since(Instant::now());

            return RateLimitResult {
                allowed: false,
                remaining: 0,
                reset_at,
                retry_after: Some(retry_after),
            };
        }

        RateLimitResult {
            allowed: true,
            remaining: self.sliding_window.remaining(),
            reset_at: self.sliding_window.reset_at(),
            retry_after: None,
        }
    }
}

/// Key for rate limiting (combines org and optional endpoint)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RateLimitKey {
    pub org_id: String,
    pub endpoint: Option<String>,
}

impl RateLimitKey {
    pub fn org(org_id: impl Into<String>) -> Self {
        Self {
            org_id: org_id.into(),
            endpoint: None,
        }
    }

    pub fn endpoint(org_id: impl Into<String>, endpoint: impl Into<String>) -> Self {
        Self {
            org_id: org_id.into(),
            endpoint: Some(endpoint.into()),
        }
    }
}

/// In-memory rate limiter
pub struct RateLimiter {
    states: Arc<Mutex<HashMap<RateLimitKey, RateLimiterState>>>,
    default_config: RateLimitConfig,
    endpoint_configs: HashMap<String, RateLimitConfig>,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new(RateLimitConfig::default())
    }
}

impl RateLimiter {
    pub fn new(default_config: RateLimitConfig) -> Self {
        Self {
            states: Arc::new(Mutex::new(HashMap::new())),
            default_config,
            endpoint_configs: HashMap::new(),
        }
    }

    /// Add a custom config for a specific endpoint
    pub fn with_endpoint_config(mut self, endpoint: impl Into<String>, config: RateLimitConfig) -> Self {
        self.endpoint_configs.insert(endpoint.into(), config);
        self
    }

    /// Check rate limit for a request
    pub fn check(&self, key: RateLimitKey) -> RateLimitResult {
        let config = key
            .endpoint
            .as_ref()
            .and_then(|e| self.endpoint_configs.get(e))
            .unwrap_or(&self.default_config);

        let mut states = self.states.lock();
        let state = states
            .entry(key)
            .or_insert_with(|| RateLimiterState::new(config));

        state.check()
    }

    /// Check rate limit for an org (global limit)
    pub fn check_org(&self, org_id: impl Into<String>) -> RateLimitResult {
        self.check(RateLimitKey::org(org_id))
    }

    /// Check rate limit for a specific endpoint
    pub fn check_endpoint(
        &self,
        org_id: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> RateLimitResult {
        self.check(RateLimitKey::endpoint(org_id, endpoint))
    }

    /// Clean up old entries to prevent memory growth
    pub fn cleanup(&self) {
        let mut states = self.states.lock();
        states.retain(|_, state| {
            // Keep if there's been activity in the last hour
            state.sliding_window.cleanup();
            !state.sliding_window.counts.is_empty()
        });
    }
}

/// Rate limiter that can be shared across async boundaries
pub type SharedRateLimiter = Arc<RateLimiter>;

/// Create a shared rate limiter
pub fn create_shared_limiter(config: RateLimitConfig) -> SharedRateLimiter {
    Arc::new(RateLimiter::new(config))
}

/// Middleware-style rate limit check
pub async fn rate_limit_middleware(
    limiter: &RateLimiter,
    org_id: &str,
    endpoint: Option<&str>,
) -> Result<RateLimitResult, RateLimitError> {
    let result = if let Some(ep) = endpoint {
        limiter.check_endpoint(org_id, ep)
    } else {
        limiter.check_org(org_id)
    };

    if result.allowed {
        Ok(result)
    } else {
        Err(RateLimitError::Exceeded(result))
    }
}

/// Error returned when rate limit is exceeded
#[derive(Debug, thiserror::Error)]
pub enum RateLimitError {
    #[error("Rate limit exceeded")]
    Exceeded(RateLimitResult),
}

impl RateLimitError {
    pub fn result(&self) -> &RateLimitResult {
        match self {
            RateLimitError::Exceeded(r) => r,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_bucket() {
        let mut bucket = TokenBucket::new(10, 1.0);

        // Should allow initial burst
        for _ in 0..10 {
            assert!(bucket.try_consume(1.0));
        }

        // Should reject when empty
        assert!(!bucket.try_consume(1.0));
    }

    #[test]
    fn test_sliding_window() {
        let mut window = SlidingWindow::new(Duration::from_secs(1), 5);

        // Should allow up to max
        for _ in 0..5 {
            assert!(window.try_increment());
        }

        // Should reject after max
        assert!(!window.try_increment());
        assert_eq!(window.remaining(), 0);
    }

    #[test]
    fn test_rate_limiter() {
        let config = RateLimitConfig {
            max_requests: 10,
            window: Duration::from_secs(1),
            burst_capacity: 5,
            refill_rate: 10.0,
        };

        let limiter = RateLimiter::new(config);

        // Should allow initial requests
        for i in 0..5 {
            let result = limiter.check_org("org-1");
            assert!(result.allowed, "Request {} should be allowed", i);
        }

        // Should eventually reject
        let mut rejected = false;
        for _ in 0..20 {
            let result = limiter.check_org("org-1");
            if !result.allowed {
                rejected = true;
                break;
            }
        }
        assert!(rejected, "Should eventually reject requests");
    }

    #[test]
    fn test_endpoint_specific_limits() {
        let limiter = RateLimiter::new(RateLimitConfig::default())
            .with_endpoint_config("/api/expensive", RateLimitConfig::strict());

        // Regular endpoint should be allowed
        let result = limiter.check_endpoint("org-1", "/api/normal");
        assert!(result.allowed);

        // Expensive endpoint should be limited quickly
        for _ in 0..5 {
            let result = limiter.check_endpoint("org-1", "/api/expensive");
            if !result.allowed {
                return; // Test passes
            }
        }
        panic!("Strict endpoint should reject quickly");
    }

    #[test]
    fn test_rate_limit_headers() {
        let limiter = RateLimiter::new(RateLimitConfig::default());
        let result = limiter.check_org("org-1");

        let headers = result.headers();
        assert!(headers.iter().any(|(k, _)| *k == "X-RateLimit-Remaining"));
        assert!(headers.iter().any(|(k, _)| *k == "X-RateLimit-Reset"));
    }
}
