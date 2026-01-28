//! Utility types and functions for the D1 backend
//!
//! This module provides shared utilities used across both WASM and HTTP backends.

use diesel::result::DatabaseErrorInformation;

// Base64 encoding for HTTP feature
#[cfg(feature = "http")]
pub mod base64 {
    //! Simple base64 encoder for HTTP transport
    //!
    //! This module provides a streaming base64 encoder that can be used
    //! to encode binary data for transmission over HTTP.

    use std::io::{Result, Write};

    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    /// A streaming base64 encoder that writes encoded data to an underlying writer.
    ///
    /// The encoder buffers input in 3-byte chunks and outputs 4-byte base64
    /// encoded chunks. Padding is automatically added when the encoder is dropped.
    pub struct Base64Encoder<W: Write> {
        writer: W,
        buffer: [u8; 3],
        buffer_len: usize,
    }

    impl<W: Write> Base64Encoder<W> {
        /// Create a new base64 encoder that writes to the given writer.
        pub fn new(writer: W) -> Self {
            Self {
                writer,
                buffer: [0; 3],
                buffer_len: 0,
            }
        }
    }

    impl<W: Write> Write for Base64Encoder<W> {
        fn write(&mut self, buf: &[u8]) -> Result<usize> {
            let mut written = 0;
            for &byte in buf {
                self.buffer[self.buffer_len] = byte;
                self.buffer_len += 1;
                written += 1;

                if self.buffer_len == 3 {
                    let out = [
                        ALPHABET[(self.buffer[0] >> 2) as usize],
                        ALPHABET[(((self.buffer[0] & 0x03) << 4) | (self.buffer[1] >> 4)) as usize],
                        ALPHABET[(((self.buffer[1] & 0x0f) << 2) | (self.buffer[2] >> 6)) as usize],
                        ALPHABET[(self.buffer[2] & 0x3f) as usize],
                    ];
                    self.writer.write_all(&out)?;
                    self.buffer_len = 0;
                }
            }
            Ok(written)
        }

        fn flush(&mut self) -> Result<()> {
            self.writer.flush()
        }
    }

    impl<W: Write> Drop for Base64Encoder<W> {
        fn drop(&mut self) {
            if self.buffer_len > 0 {
                let out = match self.buffer_len {
                    1 => [
                        ALPHABET[(self.buffer[0] >> 2) as usize],
                        ALPHABET[((self.buffer[0] & 0x03) << 4) as usize],
                        b'=',
                        b'=',
                    ],
                    2 => [
                        ALPHABET[(self.buffer[0] >> 2) as usize],
                        ALPHABET[(((self.buffer[0] & 0x03) << 4) | (self.buffer[1] >> 4)) as usize],
                        ALPHABET[((self.buffer[1] & 0x0f) << 2) as usize],
                        b'=',
                    ],
                    _ => return,
                };
                let _ = self.writer.write_all(&out);
            }
        }
    }

    /// Encode binary data to a base64 string.
    ///
    /// This is a convenience function that encodes the entire input at once.
    pub fn encode(data: &[u8]) -> String {
        use std::io::Write;
        let mut encoded = Vec::new();
        let mut encoder = Base64Encoder::new(&mut encoded);
        let _ = encoder.write_all(data);
        drop(encoder);
        String::from_utf8(encoded).unwrap_or_default()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_encode_empty() {
            assert_eq!(encode(b""), "");
        }

        #[test]
        fn test_encode_hello() {
            assert_eq!(encode(b"hello"), "aGVsbG8=");
        }

        #[test]
        fn test_encode_hello_world() {
            assert_eq!(encode(b"Hello World"), "SGVsbG8gV29ybGQ=");
        }

        #[test]
        fn test_encode_single_byte() {
            assert_eq!(encode(b"a"), "YQ==");
        }

        #[test]
        fn test_encode_two_bytes() {
            assert_eq!(encode(b"ab"), "YWI=");
        }

        #[test]
        fn test_encode_three_bytes() {
            assert_eq!(encode(b"abc"), "YWJj");
        }
    }
}

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
