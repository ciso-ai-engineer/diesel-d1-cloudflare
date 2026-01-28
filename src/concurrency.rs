//! Concurrency governance for D1 backends
//!
//! This module provides concurrency control mechanisms appropriate for each backend:
//! - **WASM (Workers)**: Lightweight concurrency governor to limit simultaneous in-flight queries
//! - **HTTP**: Transport-level concurrency control with configurable limits
//!
//! # Important Notes
//!
//! D1 Workers binding is **not** a socketed connection and cannot be pooled traditionally.
//! The "pooling" abstractions in this module are designed for concurrency governance,
//! not traditional connection pooling.
//!
//! D1 REST API is rate-limited at the Cloudflare API layer and is generally intended
//! for "administrative use" rather than high-throughput production workloads.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Default maximum concurrent queries for the concurrency governor
pub const DEFAULT_MAX_CONCURRENT_QUERIES: usize = 10;

/// Default request timeout for HTTP transport
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Query concurrency policy for controlling in-flight query limits
///
/// This policy acts as a lightweight semaphore-based concurrency governor
/// to prevent request amplification under load. It does NOT imply
/// traditional database connection pooling.
///
/// # Example
///
/// ```
/// use diesel_d1::concurrency::QueryConcurrencyPolicy;
///
/// let policy = QueryConcurrencyPolicy::builder()
///     .max_concurrent_queries(5)
///     .build();
///
/// assert_eq!(policy.max_concurrent_queries(), 5);
/// ```
#[derive(Debug, Clone)]
pub struct QueryConcurrencyPolicy {
    /// Maximum number of concurrent queries allowed
    max_concurrent_queries: usize,
    /// Current number of in-flight queries (shared across clones)
    in_flight: Arc<AtomicUsize>,
}

impl Default for QueryConcurrencyPolicy {
    fn default() -> Self {
        Self {
            max_concurrent_queries: DEFAULT_MAX_CONCURRENT_QUERIES,
            in_flight: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl QueryConcurrencyPolicy {
    /// Create a new policy with the specified maximum concurrent queries
    pub fn new(max_concurrent_queries: usize) -> Self {
        Self {
            max_concurrent_queries,
            in_flight: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Create a builder for configuring the policy
    pub fn builder() -> QueryConcurrencyPolicyBuilder {
        QueryConcurrencyPolicyBuilder::default()
    }

    /// Get the maximum number of concurrent queries allowed
    pub fn max_concurrent_queries(&self) -> usize {
        self.max_concurrent_queries
    }

    /// Get the current number of in-flight queries
    pub fn current_in_flight(&self) -> usize {
        self.in_flight.load(Ordering::SeqCst)
    }

    /// Check if a new query can be started
    ///
    /// Returns `true` if the current in-flight count is below the maximum.
    pub fn can_acquire(&self) -> bool {
        self.in_flight.load(Ordering::SeqCst) < self.max_concurrent_queries
    }

    /// Try to acquire a permit for a new query
    ///
    /// Returns a `ConcurrencyPermit` if successful, or `None` if the
    /// maximum concurrent queries limit has been reached.
    ///
    /// # Example
    ///
    /// ```
    /// use diesel_d1::concurrency::QueryConcurrencyPolicy;
    ///
    /// let policy = QueryConcurrencyPolicy::new(2);
    ///
    /// // Acquire first permit
    /// let permit1 = policy.try_acquire().expect("should acquire first permit");
    /// assert_eq!(policy.current_in_flight(), 1);
    ///
    /// // Acquire second permit
    /// let permit2 = policy.try_acquire().expect("should acquire second permit");
    /// assert_eq!(policy.current_in_flight(), 2);
    ///
    /// // Third acquisition should fail
    /// assert!(policy.try_acquire().is_none());
    ///
    /// // Drop a permit and try again
    /// drop(permit1);
    /// assert!(policy.try_acquire().is_some());
    /// ```
    pub fn try_acquire(&self) -> Option<ConcurrencyPermit> {
        loop {
            let current = self.in_flight.load(Ordering::SeqCst);
            if current >= self.max_concurrent_queries {
                return None;
            }
            if self
                .in_flight
                .compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return Some(ConcurrencyPermit {
                    in_flight: Arc::clone(&self.in_flight),
                });
            }
            // CAS failed, retry
        }
    }

    /// Acquire a permit, waiting asynchronously if necessary
    ///
    /// This method will yield to allow other tasks to make progress until
    /// a permit becomes available. In single-threaded WASM environments,
    /// this uses a cooperative yielding strategy.
    #[cfg(feature = "wasm")]
    pub async fn acquire(&self) -> ConcurrencyPermit {
        use std::future::Future;
        use std::pin::Pin;
        use std::task::{Context, Poll};

        /// A future that yields control once and then completes
        struct YieldNow {
            yielded: bool,
        }

        impl Future for YieldNow {
            type Output = ();

            fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
                if self.yielded {
                    Poll::Ready(())
                } else {
                    self.yielded = true;
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            }
        }

        loop {
            if let Some(permit) = self.try_acquire() {
                return permit;
            }
            // Yield to allow other tasks to make progress
            YieldNow { yielded: false }.await;
        }
    }

    /// Acquire a permit, waiting asynchronously if necessary
    ///
    /// This method uses a brief sleep to avoid busy-waiting while waiting
    /// for a permit to become available.
    #[cfg(all(feature = "http", not(feature = "wasm")))]
    pub async fn acquire(&self) -> ConcurrencyPermit {
        use std::time::Duration;
        loop {
            if let Some(permit) = self.try_acquire() {
                return permit;
            }
            // Brief sleep to avoid busy-waiting
            tokio::time::sleep(Duration::from_micros(100)).await;
        }
    }
}

/// Builder for QueryConcurrencyPolicy
#[derive(Debug, Default)]
pub struct QueryConcurrencyPolicyBuilder {
    max_concurrent_queries: Option<usize>,
}

impl QueryConcurrencyPolicyBuilder {
    /// Set the maximum number of concurrent queries
    pub fn max_concurrent_queries(mut self, max: usize) -> Self {
        self.max_concurrent_queries = Some(max);
        self
    }

    /// Build the policy
    pub fn build(self) -> QueryConcurrencyPolicy {
        QueryConcurrencyPolicy::new(
            self.max_concurrent_queries
                .unwrap_or(DEFAULT_MAX_CONCURRENT_QUERIES),
        )
    }
}

/// A permit that allows a single query to execute
///
/// The permit is released when dropped, allowing another query to proceed.
/// This provides RAII-style cleanup for concurrency permits.
#[derive(Debug)]
pub struct ConcurrencyPermit {
    in_flight: Arc<AtomicUsize>,
}

impl Drop for ConcurrencyPermit {
    fn drop(&mut self) {
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

/// HTTP transport policy for configuring HTTP-based D1 connections
///
/// This policy controls transport-level settings for the HTTP backend,
/// enabling HTTP keep-alive and connection reuse (transport pooling).
///
/// **Note:** The `pool_idle_connections` setting configures the connection pool's
/// idle connection limit, not a concurrency cap. To enforce true request concurrency
/// limits, use a `QueryConcurrencyPolicy` alongside this transport policy.
///
/// # Example
///
/// ```
/// use diesel_d1::concurrency::HttpTransportPolicy;
/// use std::time::Duration;
///
/// let policy = HttpTransportPolicy::builder()
///     .pool_idle_connections(20)
///     .request_timeout(Duration::from_secs(60))
///     .build();
/// ```
#[cfg(feature = "http")]
#[derive(Debug, Clone)]
pub struct HttpTransportPolicy {
    /// Maximum number of idle connections per host in the connection pool.
    /// This configures keep-alive connection reuse, NOT a concurrency limit.
    /// Use `QueryConcurrencyPolicy` for actual request concurrency control.
    pool_idle_connections: usize,
    /// Request timeout duration
    request_timeout: Duration,
    /// Whether retry/backoff is enabled (off by default)
    retry_enabled: bool,
    /// Maximum number of retry attempts (if retries are enabled)
    max_retries: u32,
    /// Base delay for exponential backoff
    retry_base_delay: Duration,
}

#[cfg(feature = "http")]
impl Default for HttpTransportPolicy {
    fn default() -> Self {
        Self {
            pool_idle_connections: DEFAULT_MAX_CONCURRENT_QUERIES,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            retry_enabled: false, // Explicitly off by default
            max_retries: 3,
            retry_base_delay: Duration::from_millis(100),
        }
    }
}

#[cfg(feature = "http")]
impl HttpTransportPolicy {
    /// Create a builder for configuring the HTTP transport policy
    pub fn builder() -> HttpTransportPolicyBuilder {
        HttpTransportPolicyBuilder::default()
    }

    /// Get the maximum number of idle connections per host.
    ///
    /// This controls connection pool sizing for keep-alive reuse,
    /// NOT the number of concurrent requests. Use `QueryConcurrencyPolicy`
    /// for actual concurrency limits.
    pub fn pool_idle_connections(&self) -> usize {
        self.pool_idle_connections
    }

    /// Get the request timeout duration
    pub fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    /// Check if retries are enabled
    pub fn retry_enabled(&self) -> bool {
        self.retry_enabled
    }

    /// Get the maximum number of retry attempts
    pub fn max_retries(&self) -> u32 {
        self.max_retries
    }

    /// Get the base delay for exponential backoff
    pub fn retry_base_delay(&self) -> Duration {
        self.retry_base_delay
    }

    /// Create a configured reqwest Client based on this policy.
    ///
    /// **Note:** The returned client does not enforce request concurrency limits.
    /// Use a `QueryConcurrencyPolicy` to limit concurrent in-flight requests.
    pub fn create_client(&self) -> Result<reqwest::Client, reqwest::Error> {
        reqwest::Client::builder()
            .timeout(self.request_timeout)
            .pool_max_idle_per_host(self.pool_idle_connections)
            .build()
    }

    /// Create a concurrency-governed client.
    ///
    /// Returns both a configured reqwest Client and a `QueryConcurrencyPolicy`
    /// that should be used to limit concurrent requests.
    ///
    /// # Example
    ///
    /// ```
    /// use diesel_d1::concurrency::HttpTransportPolicy;
    ///
    /// let policy = HttpTransportPolicy::builder()
    ///     .pool_idle_connections(10)
    ///     .build();
    ///
    /// let (client, governor) = policy.create_governed_client().unwrap();
    ///
    /// // Use governor to limit concurrent requests
    /// if let Some(permit) = governor.try_acquire() {
    ///     // Make request with client while holding permit
    /// }
    /// ```
    pub fn create_governed_client(
        &self,
    ) -> Result<(reqwest::Client, QueryConcurrencyPolicy), reqwest::Error> {
        let client = self.create_client()?;
        let governor = QueryConcurrencyPolicy::new(self.pool_idle_connections);
        Ok((client, governor))
    }
}

/// Builder for HttpTransportPolicy
#[cfg(feature = "http")]
#[derive(Debug, Default)]
pub struct HttpTransportPolicyBuilder {
    pool_idle_connections: Option<usize>,
    request_timeout: Option<Duration>,
    retry_enabled: Option<bool>,
    max_retries: Option<u32>,
    retry_base_delay: Option<Duration>,
}

#[cfg(feature = "http")]
impl HttpTransportPolicyBuilder {
    /// Set the maximum number of idle connections per host in the connection pool.
    ///
    /// This configures keep-alive connection reuse for better performance,
    /// but does NOT limit concurrent requests. Use `QueryConcurrencyPolicy`
    /// for actual concurrency control.
    pub fn pool_idle_connections(mut self, max: usize) -> Self {
        self.pool_idle_connections = Some(max);
        self
    }

    /// Set the request timeout duration
    pub fn request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = Some(timeout);
        self
    }

    /// Enable or disable retry/backoff policy
    pub fn retry_enabled(mut self, enabled: bool) -> Self {
        self.retry_enabled = Some(enabled);
        self
    }

    /// Set the maximum number of retry attempts
    pub fn max_retries(mut self, max: u32) -> Self {
        self.max_retries = Some(max);
        self
    }

    /// Set the base delay for exponential backoff
    pub fn retry_base_delay(mut self, delay: Duration) -> Self {
        self.retry_base_delay = Some(delay);
        self
    }

    /// Build the HTTP transport policy
    pub fn build(self) -> HttpTransportPolicy {
        let default = HttpTransportPolicy::default();
        HttpTransportPolicy {
            pool_idle_connections: self
                .pool_idle_connections
                .unwrap_or(default.pool_idle_connections),
            request_timeout: self.request_timeout.unwrap_or(default.request_timeout),
            retry_enabled: self.retry_enabled.unwrap_or(default.retry_enabled),
            max_retries: self.max_retries.unwrap_or(default.max_retries),
            retry_base_delay: self.retry_base_delay.unwrap_or(default.retry_base_delay),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_concurrency_policy_default() {
        let policy = QueryConcurrencyPolicy::default();
        assert_eq!(
            policy.max_concurrent_queries(),
            DEFAULT_MAX_CONCURRENT_QUERIES
        );
        assert_eq!(policy.current_in_flight(), 0);
    }

    #[test]
    fn test_query_concurrency_policy_new() {
        let policy = QueryConcurrencyPolicy::new(5);
        assert_eq!(policy.max_concurrent_queries(), 5);
    }

    #[test]
    fn test_query_concurrency_policy_builder() {
        let policy = QueryConcurrencyPolicy::builder()
            .max_concurrent_queries(3)
            .build();
        assert_eq!(policy.max_concurrent_queries(), 3);
    }

    #[test]
    fn test_try_acquire_success() {
        let policy = QueryConcurrencyPolicy::new(2);

        let permit1 = policy.try_acquire();
        assert!(permit1.is_some());
        assert_eq!(policy.current_in_flight(), 1);

        let permit2 = policy.try_acquire();
        assert!(permit2.is_some());
        assert_eq!(policy.current_in_flight(), 2);
    }

    #[test]
    fn test_try_acquire_at_limit() {
        let policy = QueryConcurrencyPolicy::new(1);

        let _permit = policy.try_acquire();
        assert_eq!(policy.current_in_flight(), 1);

        // Should fail - at limit
        assert!(policy.try_acquire().is_none());
    }

    #[test]
    fn test_permit_drop_releases() {
        let policy = QueryConcurrencyPolicy::new(1);

        {
            let _permit = policy.try_acquire();
            assert_eq!(policy.current_in_flight(), 1);
            assert!(policy.try_acquire().is_none());
        }

        // After drop, should be able to acquire again
        assert_eq!(policy.current_in_flight(), 0);
        assert!(policy.try_acquire().is_some());
    }

    #[test]
    fn test_can_acquire() {
        let policy = QueryConcurrencyPolicy::new(1);

        assert!(policy.can_acquire());

        let _permit = policy.try_acquire();
        assert!(!policy.can_acquire());

        drop(_permit);
        assert!(policy.can_acquire());
    }

    #[test]
    fn test_policy_clone_shares_state() {
        let policy1 = QueryConcurrencyPolicy::new(2);
        let policy2 = policy1.clone();

        let _permit = policy1.try_acquire();
        assert_eq!(policy1.current_in_flight(), 1);
        assert_eq!(policy2.current_in_flight(), 1);

        let _permit2 = policy2.try_acquire();
        assert_eq!(policy1.current_in_flight(), 2);
        assert_eq!(policy2.current_in_flight(), 2);
    }

    #[cfg(feature = "http")]
    mod http_tests {
        use super::*;

        #[test]
        fn test_http_transport_policy_default() {
            let policy = HttpTransportPolicy::default();
            assert_eq!(
                policy.pool_idle_connections(),
                DEFAULT_MAX_CONCURRENT_QUERIES
            );
            assert_eq!(policy.request_timeout(), DEFAULT_REQUEST_TIMEOUT);
            assert!(!policy.retry_enabled());
        }

        #[test]
        fn test_http_transport_policy_builder() {
            let policy = HttpTransportPolicy::builder()
                .pool_idle_connections(20)
                .request_timeout(Duration::from_secs(60))
                .retry_enabled(true)
                .max_retries(5)
                .retry_base_delay(Duration::from_millis(200))
                .build();

            assert_eq!(policy.pool_idle_connections(), 20);
            assert_eq!(policy.request_timeout(), Duration::from_secs(60));
            assert!(policy.retry_enabled());
            assert_eq!(policy.max_retries(), 5);
            assert_eq!(policy.retry_base_delay(), Duration::from_millis(200));
        }

        #[test]
        fn test_http_transport_policy_create_client() {
            let policy = HttpTransportPolicy::default();
            let client = policy.create_client();
            assert!(client.is_ok());
        }

        #[test]
        fn test_http_transport_policy_create_governed_client() {
            let policy = HttpTransportPolicy::builder()
                .pool_idle_connections(5)
                .build();
            let result = policy.create_governed_client();
            assert!(result.is_ok());

            let (_client, governor) = result.unwrap();
            assert_eq!(governor.max_concurrent_queries(), 5);
        }
    }
}
