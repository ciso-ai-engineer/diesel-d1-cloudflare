//! WASM value type for D1 backend
//!
//! This module provides the D1Value type for deserializing values from D1 query results
//! when using the WASM binding.

use js_sys::Uint8Array;
use wasm_bindgen::{JsCast, JsValue};

/// A value from a D1 query result (WASM version)
///
/// This wraps a JavaScript value and provides type-safe access to the data.
pub struct D1Value {
    _row: JsValue,
}

impl D1Value {
    /// Create a new D1Value from a JsValue
    pub fn new(row: JsValue) -> Self {
        Self { _row: row }
    }

    /// Read the value as a string
    ///
    /// Note: Returns an empty string for NULL values. Use `is_null()` to check for NULL.
    pub(crate) fn read_string(&self) -> String {
        self._row.as_string().unwrap_or_default()
    }

    /// Read the value as a boolean
    ///
    /// Note: Returns `false` for NULL values. Use `is_null()` to check for NULL.
    #[allow(dead_code)]
    pub(crate) fn read_bool(&self) -> bool {
        self._row.as_bool().unwrap_or(false)
    }

    /// Read the value as a number (f64)
    ///
    /// Note: JS numbers are always f64, which might cause precision issues
    /// when crossing boundaries with i64.
    /// Note: Returns 0.0 for NULL values. Use `is_null()` to check for NULL.
    pub(crate) fn read_number(&self) -> f64 {
        self._row.as_f64().unwrap_or(0.0)
    }

    /// Check if the value is null or undefined
    #[allow(dead_code)]
    pub(crate) fn is_null(&self) -> bool {
        self._row.is_null() || self._row.is_undefined()
    }

    /// Read the value as a blob (binary data)
    ///
    /// Note: Returns an empty Vec for NULL values or non-Uint8Array types.
    /// Use `is_null()` to check for NULL.
    pub(crate) fn read_blob(&self) -> Vec<u8> {
        if !self._row.is_instance_of::<Uint8Array>() {
            return Vec::new();
        }
        Uint8Array::from(self._row.clone()).to_vec()
    }
}

#[cfg(test)]
mod tests {
    // Tests would require a WASM environment to run
}
