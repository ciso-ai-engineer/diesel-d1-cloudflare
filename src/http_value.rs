//! HTTP value type for D1 backend
//!
//! This module provides the D1Value type for deserializing values from D1 query results
//! when using the HTTP REST API.

use serde_json::Value as JsonValue;

/// A value from a D1 query result (HTTP version)
///
/// This wraps a serde_json Value and provides type-safe access to the data.
pub struct D1Value {
    value: JsonValue,
}

impl D1Value {
    /// Create a new D1Value from a JSON value
    pub fn new(value: JsonValue) -> Self {
        Self { value }
    }

    /// Read the value as a string
    pub(crate) fn read_string(&self) -> String {
        match &self.value {
            JsonValue::String(s) => s.clone(),
            JsonValue::Number(n) => n.to_string(),
            JsonValue::Bool(b) => b.to_string(),
            JsonValue::Null => String::new(),
            _ => self.value.to_string(),
        }
    }

    /// Read the value as a boolean
    #[allow(dead_code)]
    pub(crate) fn read_bool(&self) -> bool {
        match &self.value {
            JsonValue::Bool(b) => *b,
            JsonValue::Number(n) => n.as_i64().map(|i| i != 0).unwrap_or(false),
            _ => false,
        }
    }

    /// Read the value as a number (f64)
    pub(crate) fn read_number(&self) -> f64 {
        match &self.value {
            JsonValue::Number(n) => n.as_f64().unwrap_or(0.0),
            JsonValue::String(s) => s.parse().unwrap_or(0.0),
            JsonValue::Bool(b) => if *b { 1.0 } else { 0.0 },
            _ => 0.0,
        }
    }

    /// Check if the value is null
    #[allow(dead_code)]
    pub(crate) fn is_null(&self) -> bool {
        self.value.is_null()
    }

    /// Read the value as a blob (binary data)
    ///
    /// Expects the value to be a base64-encoded string
    pub(crate) fn read_blob(&self) -> Vec<u8> {
        match &self.value {
            JsonValue::String(s) => {
                // Decode base64
                base64_decode(s).unwrap_or_default()
            }
            JsonValue::Array(arr) => {
                // Array of numbers
                arr.iter()
                    .filter_map(|v| v.as_u64().map(|n| n as u8))
                    .collect()
            }
            _ => Vec::new(),
        }
    }
}

/// Simple base64 decoder
fn base64_decode(input: &str) -> Result<Vec<u8>, ()> {
    const DECODE_TABLE: [i8; 256] = {
        let mut table = [-1i8; 256];
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut i = 0;
        while i < 64 {
            table[alphabet[i] as usize] = i as i8;
            i += 1;
        }
        table
    };

    let input = input.trim_end_matches('=');
    let len = input.len();
    let mut output = Vec::with_capacity(len * 3 / 4);

    let mut buffer = 0u32;
    let mut bits_collected = 0;

    for byte in input.bytes() {
        let value = DECODE_TABLE[byte as usize];
        if value < 0 {
            return Err(());
        }
        buffer = (buffer << 6) | (value as u32);
        bits_collected += 6;

        if bits_collected >= 8 {
            bits_collected -= 8;
            output.push((buffer >> bits_collected) as u8);
            buffer &= (1 << bits_collected) - 1;
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_string() {
        let value = D1Value::new(JsonValue::String("hello".to_string()));
        assert_eq!(value.read_string(), "hello");
    }

    #[test]
    fn test_read_string_from_number() {
        let value = D1Value::new(serde_json::json!(42));
        assert_eq!(value.read_string(), "42");
    }

    #[test]
    fn test_read_bool_true() {
        let value = D1Value::new(JsonValue::Bool(true));
        assert!(value.read_bool());
    }

    #[test]
    fn test_read_bool_false() {
        let value = D1Value::new(JsonValue::Bool(false));
        assert!(!value.read_bool());
    }

    #[test]
    fn test_read_bool_from_number() {
        let value = D1Value::new(serde_json::json!(1));
        assert!(value.read_bool());

        let value = D1Value::new(serde_json::json!(0));
        assert!(!value.read_bool());
    }

    #[test]
    fn test_read_number_integer() {
        let value = D1Value::new(serde_json::json!(42));
        assert!((value.read_number() - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_read_number_float() {
        let value = D1Value::new(serde_json::json!(3.14));
        assert!((value.read_number() - 3.14).abs() < 0.001);
    }

    #[test]
    fn test_is_null() {
        let value = D1Value::new(JsonValue::Null);
        assert!(value.is_null());

        let value = D1Value::new(serde_json::json!(42));
        assert!(!value.is_null());
    }

    #[test]
    fn test_read_blob_from_base64() {
        // "hello" in base64 is "aGVsbG8="
        let value = D1Value::new(JsonValue::String("aGVsbG8=".to_string()));
        assert_eq!(value.read_blob(), b"hello");
    }

    #[test]
    fn test_read_blob_from_array() {
        let value = D1Value::new(serde_json::json!([104, 101, 108, 108, 111]));
        assert_eq!(value.read_blob(), b"hello");
    }

    #[test]
    fn test_base64_decode() {
        assert_eq!(base64_decode("SGVsbG8=").unwrap(), b"Hello");
        assert_eq!(base64_decode("V29ybGQ=").unwrap(), b"World");
        assert_eq!(base64_decode("").unwrap(), b"");
    }
}
