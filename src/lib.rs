//! # Diesel D1 Backend
//!
//! This crate provides a Diesel backend for Cloudflare D1, supporting both WASM (Workers)
//! and HTTP (REST API) backends.
//!
//! ## Features
//!
//! - `wasm` - Enable WASM bindings for Cloudflare Workers (requires wasm32 target)
//! - `http` - Enable HTTP REST API backend for non-Workers environments
//!
//! ## Concurrency & "Pooling" Semantics
//!
//! **Important:** D1 Workers binding is **not** a socketed connection and cannot be pooled
//! traditionally. The "pooling" abstractions in this crate are designed for:
//!
//! - **WASM (Workers)**: A lightweight concurrency governor (semaphore-based) to limit
//!   simultaneous in-flight queries per isolate, preventing request amplification under load.
//! - **HTTP**: Transport-level pooling using a shared `reqwest::Client` for HTTP keep-alive
//!   and connection reuse, plus configurable request limits and timeouts.
//!
//! The D1 REST API is rate-limited at the Cloudflare API layer and is generally intended
//! for "administrative use" rather than high-throughput production workloads.
//!
//! ## Statement Caching
//!
//! The crate provides best-effort client-side statement caching to reduce overhead for
//! frequently executed SQL. Caching may reset on isolate eviction (WASM) or process restart.
//!
//! ## Batch Operations & Transactions
//!
//! D1's `batch()` API executes statements as a SQL transaction with automatic rollback on
//! failure. This crate provides batch construction utilities that support prepared statement
//! reuse across multiple binds.
//!
//! ## Usage
//!
//! ### WASM Backend (Cloudflare Workers)
//!
//! ```toml
//! [dependencies]
//! diesel-d1 = { version = "0.1", features = ["wasm"] }
//! ```
//!
//! ### HTTP Backend (REST API)
//!
//! ```toml
//! [dependencies]
//! diesel-d1 = { version = "0.1", features = ["http"] }
//! ```

pub mod backend;
mod bind_collector;
mod query_builder;
mod transaction_manager;
mod types;
mod utils;

// New feature modules
pub mod batch;
pub mod cache;
pub mod concurrency;
pub mod replay;
pub mod tracing_support;

// WASM-specific modules
#[cfg(feature = "wasm")]
mod binding;
#[cfg(feature = "wasm")]
mod row;
#[cfg(feature = "wasm")]
mod value;
#[cfg(feature = "wasm")]
mod wasm_connection;

// HTTP-specific modules
#[cfg(feature = "http")]
mod http_connection;
#[cfg(feature = "http")]
mod http_row;
#[cfg(feature = "http")]
mod http_value;

// Re-exports
pub use backend::D1Backend;
pub use transaction_manager::D1TransactionManager;

// Concurrency and caching re-exports
pub use cache::{StatementCache, StatementCacheConfig};
pub use concurrency::QueryConcurrencyPolicy;

#[cfg(feature = "http")]
pub use concurrency::HttpTransportPolicy;

// Batch operations re-exports
pub use batch::{BatchBuilder, BatchStatement, BoundValue};

// Tracing re-exports
pub use tracing_support::{D1Span, ErrorClass, SpanOperation};

// Replay testing re-exports
pub use replay::{ExpectedResult, TransactionTranscript, TranscriptStatement};

#[cfg(feature = "wasm")]
pub use wasm_connection::D1Connection;

#[cfg(feature = "http")]
pub use http_connection::{D1HttpConfig, D1HttpConnection};
