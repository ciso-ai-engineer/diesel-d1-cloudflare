//! Utility types and functions for the D1 backend
//!
//! This module provides shared utilities used across both WASM and HTTP backends.

use diesel::result::DatabaseErrorInformation;

#[cfg(feature = "wasm")]
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// Wrapper to make JS Futures sendable in single-threaded WASM environments
///
/// JS promises are never sendable because they exist in one thread.
/// However, Cloudflare Workers are ALWAYS single-threaded, so we can make
/// every JsFuture sendable by using this wrapper.
#[cfg(feature = "wasm")]
pub struct SendableFuture<T>(pub T)
where
    T: Future;

#[cfg(feature = "wasm")]
// Safety: WebAssembly will only ever run in a single-threaded context.
unsafe impl<T: Future> Send for SendableFuture<T> {}

#[cfg(feature = "wasm")]
impl<T> Future for SendableFuture<T>
where
    T: Future,
{
    type Output = T::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Safety: We are only pinning the inner future.
        unsafe { self.map_unchecked_mut(|s| &mut s.0).poll(cx) }
    }
}

/// Error information from D1
///
/// This struct wraps error messages from D1 for use with Diesel's error system.
#[derive(Debug, Clone)]
pub struct D1Error {
    /// The error message from D1
    pub(crate) message: String,
}

impl D1Error {
    /// Create a new D1 error with the given message
    #[allow(dead_code)]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for D1Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for D1Error {}

impl DatabaseErrorInformation for D1Error {
    fn message(&self) -> &str {
        &self.message
    }

    fn details(&self) -> Option<&str> {
        None
    }

    fn hint(&self) -> Option<&str> {
        None
    }

    fn table_name(&self) -> Option<&str> {
        None
    }

    fn column_name(&self) -> Option<&str> {
        None
    }

    fn constraint_name(&self) -> Option<&str> {
        None
    }

    fn statement_position(&self) -> Option<i32> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_d1_error_new() {
        let error = D1Error::new("test error");
        assert_eq!(error.message, "test error");
    }

    #[test]
    fn test_d1_error_display() {
        let error = D1Error::new("test error");
        assert_eq!(format!("{}", error), "test error");
    }

    #[test]
    fn test_d1_error_debug() {
        let error = D1Error::new("test error");
        let debug = format!("{:?}", error);
        assert!(debug.contains("test error"));
    }

    #[test]
    fn test_database_error_information() {
        let error = D1Error::new("test message");
        assert_eq!(error.message(), "test message");
        assert!(error.details().is_none());
        assert!(error.hint().is_none());
        assert!(error.table_name().is_none());
        assert!(error.column_name().is_none());
        assert!(error.constraint_name().is_none());
        assert!(error.statement_position().is_none());
    }
}