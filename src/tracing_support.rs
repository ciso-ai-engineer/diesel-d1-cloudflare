//! Tracing and OpenTelemetry support for D1 backends
//!
//! This module provides structured tracing with OpenTelemetry-compatible spans
//! for correlating latency, errors, and query behavior across Workers and HTTP deployments.
//!
//! # Features
//!
//! - Consistent span fields across both backends
//! - Error classification for different failure types
//! - Query execution metadata (duration, rows)
//! - Optional OpenTelemetry export (via `otel` feature)
//!
//! # Span Fields
//!
//! All query execution spans include:
//! - `backend`: "wasm" or "http"
//! - `operation`: "prepare", "exec", or "batch"
//! - `db.system`: "d1"
//! - `db.statement`: SQL query (optional, can be disabled for security)
//!
//! # Example
//!
//! ```
//! use diesel_d1::tracing_support::{D1Span, SpanOperation, ErrorClass};
//!
//! // Create a span for a query execution
//! let mut span = D1Span::new(SpanOperation::Execute)
//!     .with_sql("SELECT * FROM users")
//!     .with_backend_http();
//!
//! // Record the result
//! span.record_success(10, std::time::Duration::from_millis(50));
//! ```

use std::time::Duration;

/// The backend type for tracing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendType {
    /// WASM/Cloudflare Workers backend
    Wasm,
    /// HTTP REST API backend
    Http,
}

impl BackendType {
    /// Get the string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            BackendType::Wasm => "wasm",
            BackendType::Http => "http",
        }
    }
}

impl std::fmt::Display for BackendType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// The operation type for spans
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanOperation {
    /// Preparing a statement
    Prepare,
    /// Executing a single query
    Execute,
    /// Executing a batch of queries
    Batch,
}

impl SpanOperation {
    /// Get the string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            SpanOperation::Prepare => "prepare",
            SpanOperation::Execute => "exec",
            SpanOperation::Batch => "batch",
        }
    }
}

impl std::fmt::Display for SpanOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Error classification for D1 operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    /// Authentication or authorization error
    Auth,
    /// Rate limiting error (e.g., 429)
    RateLimit,
    /// SQL syntax or execution error
    SqlError,
    /// Response decoding error
    Decode,
    /// Request timeout
    Timeout,
    /// Network or connection error
    Network,
    /// Unknown error
    Unknown,
}

impl ErrorClass {
    /// Get the string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorClass::Auth => "auth",
            ErrorClass::RateLimit => "rate_limit",
            ErrorClass::SqlError => "sql_error",
            ErrorClass::Decode => "decode",
            ErrorClass::Timeout => "timeout",
            ErrorClass::Network => "network",
            ErrorClass::Unknown => "unknown",
        }
    }

    /// Classify an error from an HTTP status code
    pub fn from_http_status(status: u16) -> Self {
        match status {
            401 | 403 => ErrorClass::Auth,
            429 => ErrorClass::RateLimit,
            400 => ErrorClass::SqlError,
            408 | 504 => ErrorClass::Timeout,
            502 | 503 | 520..=530 => ErrorClass::Network,
            _ => ErrorClass::Unknown,
        }
    }

    /// Classify an error from an error message
    pub fn from_error_message(message: &str) -> Self {
        let msg_lower = message.to_lowercase();

        if msg_lower.contains("unauthorized")
            || msg_lower.contains("forbidden")
            || msg_lower.contains("auth")
        {
            ErrorClass::Auth
        } else if msg_lower.contains("rate limit") || msg_lower.contains("too many requests") {
            ErrorClass::RateLimit
        } else if msg_lower.contains("syntax")
            || msg_lower.contains("sql")
            || msg_lower.contains("constraint")
            || msg_lower.contains("duplicate")
        {
            ErrorClass::SqlError
        } else if msg_lower.contains("parse")
            || msg_lower.contains("decode")
            || msg_lower.contains("deserialize")
        {
            ErrorClass::Decode
        } else if msg_lower.contains("timeout") || msg_lower.contains("timed out") {
            ErrorClass::Timeout
        } else if msg_lower.contains("network")
            || msg_lower.contains("connection")
            || msg_lower.contains("connect")
        {
            ErrorClass::Network
        } else {
            ErrorClass::Unknown
        }
    }
}

impl std::fmt::Display for ErrorClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Configuration for tracing
#[derive(Debug, Clone)]
pub struct TracingConfig {
    /// Whether to include SQL statements in spans
    pub include_sql: bool,
    /// Whether to include parameter values (potentially sensitive)
    pub include_params: bool,
    /// Maximum SQL length to include in spans
    pub max_sql_length: usize,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            include_sql: true,
            include_params: false,
            max_sql_length: 1000,
        }
    }
}

/// Span metadata for D1 operations
///
/// This struct captures all the relevant information for a single
/// D1 operation span.
#[derive(Debug, Clone)]
pub struct D1Span {
    /// The operation type
    pub operation: SpanOperation,
    /// The backend type
    pub backend: Option<BackendType>,
    /// SQL statement (if captured)
    pub sql: Option<String>,
    /// Number of rows read
    pub rows_read: Option<usize>,
    /// Number of rows written
    pub rows_written: Option<usize>,
    /// Execution duration
    pub duration: Option<Duration>,
    /// Error class (if failed)
    pub error_class: Option<ErrorClass>,
    /// Error message (if failed)
    pub error_message: Option<String>,
    /// HTTP request ID (for correlation)
    pub request_id: Option<String>,
    /// HTTP response status
    pub response_status: Option<u16>,
    /// Number of retry attempts
    pub retry_count: Option<u32>,
}

impl D1Span {
    /// Create a new span for an operation
    pub fn new(operation: SpanOperation) -> Self {
        Self {
            operation,
            backend: None,
            sql: None,
            rows_read: None,
            rows_written: None,
            duration: None,
            error_class: None,
            error_message: None,
            request_id: None,
            response_status: None,
            retry_count: None,
        }
    }

    /// Set the backend to WASM
    pub fn with_backend_wasm(mut self) -> Self {
        self.backend = Some(BackendType::Wasm);
        self
    }

    /// Set the backend to HTTP
    pub fn with_backend_http(mut self) -> Self {
        self.backend = Some(BackendType::Http);
        self
    }

    /// Set the SQL statement
    pub fn with_sql(mut self, sql: impl Into<String>) -> Self {
        self.sql = Some(sql.into());
        self
    }

    /// Set the request ID for correlation
    pub fn with_request_id(mut self, id: impl Into<String>) -> Self {
        self.request_id = Some(id.into());
        self
    }

    /// Record a successful execution
    pub fn record_success(&mut self, rows_affected: usize, duration: Duration) {
        self.rows_written = Some(rows_affected);
        self.duration = Some(duration);
    }

    /// Record a successful query with read rows
    pub fn record_query_success(
        &mut self,
        rows_read: usize,
        rows_written: usize,
        duration: Duration,
    ) {
        self.rows_read = Some(rows_read);
        self.rows_written = Some(rows_written);
        self.duration = Some(duration);
    }

    /// Record an error
    pub fn record_error(&mut self, error_class: ErrorClass, message: impl Into<String>) {
        self.error_class = Some(error_class);
        self.error_message = Some(message.into());
    }

    /// Record HTTP response details
    pub fn record_http_response(&mut self, status: u16, request_id: Option<String>) {
        self.response_status = Some(status);
        if let Some(id) = request_id {
            self.request_id = Some(id);
        }
    }

    /// Record retry information
    pub fn record_retry(&mut self, attempt: u32) {
        self.retry_count = Some(attempt);
    }

    /// Check if the span represents a failure
    pub fn is_error(&self) -> bool {
        self.error_class.is_some()
    }

    /// Get a summary string for the span
    pub fn summary(&self) -> String {
        let mut parts = vec![format!("op={}", self.operation)];

        if let Some(ref backend) = self.backend {
            parts.push(format!("backend={}", backend));
        }

        if let Some(ref duration) = self.duration {
            parts.push(format!("duration={:?}", duration));
        }

        if let Some(rows) = self.rows_read {
            parts.push(format!("rows_read={}", rows));
        }

        if let Some(rows) = self.rows_written {
            parts.push(format!("rows_written={}", rows));
        }

        if let Some(ref error) = self.error_class {
            parts.push(format!("error={}", error));
        }

        parts.join(" ")
    }
}

/// Trait for types that can emit spans
pub trait SpanEmitter {
    /// Emit a D1 span
    fn emit_span(&self, span: &D1Span);
}

/// A no-op span emitter for when tracing is disabled
#[derive(Debug, Clone, Default)]
pub struct NoopSpanEmitter;

impl SpanEmitter for NoopSpanEmitter {
    fn emit_span(&self, _span: &D1Span) {
        // No-op
    }
}

/// A span emitter that logs to a Vec for testing
#[derive(Debug, Default)]
pub struct TestSpanEmitter {
    spans: std::sync::Mutex<Vec<D1Span>>,
}

impl TestSpanEmitter {
    /// Create a new test span emitter
    pub fn new() -> Self {
        Self::default()
    }

    /// Get all captured spans
    pub fn get_spans(&self) -> Vec<D1Span> {
        self.spans.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Clear all captured spans
    pub fn clear(&self) {
        if let Ok(mut spans) = self.spans.lock() {
            spans.clear();
        }
    }

    /// Get the number of captured spans
    pub fn len(&self) -> usize {
        self.spans.lock().map(|s| s.len()).unwrap_or(0)
    }

    /// Check if no spans have been captured
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl SpanEmitter for TestSpanEmitter {
    fn emit_span(&self, span: &D1Span) {
        if let Ok(mut spans) = self.spans.lock() {
            spans.push(span.clone());
        }
    }
}

/// Helper to time an operation and create a span
pub struct SpanTimer {
    start: std::time::Instant,
    span: D1Span,
}

impl SpanTimer {
    /// Start timing an operation
    pub fn start(operation: SpanOperation) -> Self {
        Self {
            start: std::time::Instant::now(),
            span: D1Span::new(operation),
        }
    }

    /// Get mutable access to the span
    pub fn span_mut(&mut self) -> &mut D1Span {
        &mut self.span
    }

    /// Finish timing and record success
    pub fn finish_success(mut self, rows_affected: usize) -> D1Span {
        let duration = self.start.elapsed();
        self.span.record_success(rows_affected, duration);
        self.span
    }

    /// Finish timing and record an error
    pub fn finish_error(mut self, error_class: ErrorClass, message: impl Into<String>) -> D1Span {
        self.span.duration = Some(self.start.elapsed());
        self.span.record_error(error_class, message);
        self.span
    }

    /// Get the elapsed duration so far
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_type_as_str() {
        assert_eq!(BackendType::Wasm.as_str(), "wasm");
        assert_eq!(BackendType::Http.as_str(), "http");
    }

    #[test]
    fn test_span_operation_as_str() {
        assert_eq!(SpanOperation::Prepare.as_str(), "prepare");
        assert_eq!(SpanOperation::Execute.as_str(), "exec");
        assert_eq!(SpanOperation::Batch.as_str(), "batch");
    }

    #[test]
    fn test_error_class_as_str() {
        assert_eq!(ErrorClass::Auth.as_str(), "auth");
        assert_eq!(ErrorClass::RateLimit.as_str(), "rate_limit");
        assert_eq!(ErrorClass::SqlError.as_str(), "sql_error");
        assert_eq!(ErrorClass::Decode.as_str(), "decode");
        assert_eq!(ErrorClass::Timeout.as_str(), "timeout");
        assert_eq!(ErrorClass::Network.as_str(), "network");
        assert_eq!(ErrorClass::Unknown.as_str(), "unknown");
    }

    #[test]
    fn test_error_class_from_http_status() {
        assert_eq!(ErrorClass::from_http_status(401), ErrorClass::Auth);
        assert_eq!(ErrorClass::from_http_status(403), ErrorClass::Auth);
        assert_eq!(ErrorClass::from_http_status(429), ErrorClass::RateLimit);
        assert_eq!(ErrorClass::from_http_status(400), ErrorClass::SqlError);
        assert_eq!(ErrorClass::from_http_status(408), ErrorClass::Timeout);
        assert_eq!(ErrorClass::from_http_status(504), ErrorClass::Timeout);
        assert_eq!(ErrorClass::from_http_status(502), ErrorClass::Network);
        assert_eq!(ErrorClass::from_http_status(500), ErrorClass::Unknown);
    }

    #[test]
    fn test_error_class_from_error_message() {
        assert_eq!(
            ErrorClass::from_error_message("Unauthorized access"),
            ErrorClass::Auth
        );
        assert_eq!(
            ErrorClass::from_error_message("Rate limit exceeded"),
            ErrorClass::RateLimit
        );
        assert_eq!(
            ErrorClass::from_error_message("SQL syntax error"),
            ErrorClass::SqlError
        );
        assert_eq!(
            ErrorClass::from_error_message("Failed to parse response"),
            ErrorClass::Decode
        );
        assert_eq!(
            ErrorClass::from_error_message("Request timed out"),
            ErrorClass::Timeout
        );
        assert_eq!(
            ErrorClass::from_error_message("Connection refused"),
            ErrorClass::Network
        );
        assert_eq!(
            ErrorClass::from_error_message("Something went wrong"),
            ErrorClass::Unknown
        );
    }

    #[test]
    fn test_d1_span_new() {
        let span = D1Span::new(SpanOperation::Execute);
        assert_eq!(span.operation, SpanOperation::Execute);
        assert!(span.backend.is_none());
        assert!(span.sql.is_none());
    }

    #[test]
    fn test_d1_span_with_backend() {
        let span = D1Span::new(SpanOperation::Execute).with_backend_wasm();
        assert_eq!(span.backend, Some(BackendType::Wasm));

        let span = D1Span::new(SpanOperation::Execute).with_backend_http();
        assert_eq!(span.backend, Some(BackendType::Http));
    }

    #[test]
    fn test_d1_span_with_sql() {
        let span = D1Span::new(SpanOperation::Execute).with_sql("SELECT * FROM users");
        assert_eq!(span.sql, Some("SELECT * FROM users".to_string()));
    }

    #[test]
    fn test_d1_span_record_success() {
        let mut span = D1Span::new(SpanOperation::Execute);
        span.record_success(10, Duration::from_millis(50));

        assert_eq!(span.rows_written, Some(10));
        assert_eq!(span.duration, Some(Duration::from_millis(50)));
        assert!(!span.is_error());
    }

    #[test]
    fn test_d1_span_record_error() {
        let mut span = D1Span::new(SpanOperation::Execute);
        span.record_error(ErrorClass::SqlError, "Constraint violation");

        assert_eq!(span.error_class, Some(ErrorClass::SqlError));
        assert_eq!(span.error_message, Some("Constraint violation".to_string()));
        assert!(span.is_error());
    }

    #[test]
    fn test_d1_span_summary() {
        let mut span = D1Span::new(SpanOperation::Execute)
            .with_backend_http()
            .with_sql("SELECT 1");
        span.record_success(5, Duration::from_millis(100));

        let summary = span.summary();
        assert!(summary.contains("op=exec"));
        assert!(summary.contains("backend=http"));
        assert!(summary.contains("rows_written=5"));
    }

    #[test]
    fn test_tracing_config_default() {
        let config = TracingConfig::default();
        assert!(config.include_sql);
        assert!(!config.include_params);
        assert_eq!(config.max_sql_length, 1000);
    }

    #[test]
    fn test_test_span_emitter() {
        let emitter = TestSpanEmitter::new();
        assert!(emitter.is_empty());

        let span = D1Span::new(SpanOperation::Execute);
        emitter.emit_span(&span);

        assert_eq!(emitter.len(), 1);
        assert!(!emitter.is_empty());

        let spans = emitter.get_spans();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].operation, SpanOperation::Execute);

        emitter.clear();
        assert!(emitter.is_empty());
    }

    #[test]
    fn test_span_timer() {
        let timer = SpanTimer::start(SpanOperation::Execute);
        std::thread::sleep(Duration::from_millis(10));

        let span = timer.finish_success(5);
        assert!(span.duration.unwrap() >= Duration::from_millis(10));
        assert_eq!(span.rows_written, Some(5));
    }

    #[test]
    fn test_span_timer_error() {
        let timer = SpanTimer::start(SpanOperation::Execute);
        let span = timer.finish_error(ErrorClass::SqlError, "Test error");

        assert!(span.is_error());
        assert_eq!(span.error_class, Some(ErrorClass::SqlError));
    }

    #[test]
    fn test_noop_span_emitter() {
        let emitter = NoopSpanEmitter;
        let span = D1Span::new(SpanOperation::Execute);
        // Just verify it doesn't panic
        emitter.emit_span(&span);
    }
}
