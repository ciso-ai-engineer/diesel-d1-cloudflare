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

#[cfg(feature = "wasm")]
pub use wasm_connection::D1Connection;

#[cfg(feature = "http")]
pub use http_connection::{D1HttpConnection, D1HttpConfig};
