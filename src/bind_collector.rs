//! Bind collector for D1 backend
//!
//! This module provides the bind collector implementation that works with both
//! WASM and HTTP backends.

use diesel::{
    query_builder::BindCollector,
    serialize::{IsNull, Output},
    sql_types::HasSqlType,
};

use crate::backend::{D1Backend, D1Type};

/// Collected bind values for a query
#[derive(Default, Clone)]
pub struct D1BindCollector {
    /// The collected bind values with their types
    pub binds: Vec<(BindValue, D1Type)>,
}

/// A bind value that can be used across both WASM and HTTP backends
#[derive(Clone, Debug)]
pub enum BindValue {
    /// Null value
    Null,
    /// Integer value
    Integer(i64),
    /// Double/float value
    Double(f64),
    /// Text value
    Text(String),
    /// Binary data
    Binary(Vec<u8>),
}

impl BindValue {
    /// Convert to a WASM JsValue
    #[cfg(feature = "wasm")]
    pub fn to_js_value(&self) -> wasm_bindgen::JsValue {
        use wasm_bindgen::JsValue;
        match self {
            BindValue::Null => JsValue::null(),
            BindValue::Integer(i) => JsValue::from_f64(*i as f64),
            BindValue::Double(d) => JsValue::from_f64(*d),
            BindValue::Text(s) => JsValue::from_str(s),
            BindValue::Binary(b) => {
                let array = js_sys::Uint8Array::new_with_length(b.len() as u32);
                array.copy_from(b);
                array.into()
            }
        }
    }

    /// Convert to a serde_json Value for HTTP API
    #[cfg(feature = "http")]
    pub fn to_json_value(&self) -> serde_json::Value {
        match self {
            BindValue::Null => serde_json::Value::Null,
            BindValue::Integer(i) => serde_json::Value::Number((*i).into()),
            BindValue::Double(d) => {
                serde_json::Number::from_f64(*d)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null)
            }
            BindValue::Text(s) => serde_json::Value::String(s.clone()),
            BindValue::Binary(b) => {
                // Encode as base64 for HTTP transport
                use std::io::Write;
                let mut encoded = Vec::new();
                let mut encoder = base64_encode::Base64Encoder::new(&mut encoded);
                encoder.write_all(b).ok();
                drop(encoder);
                serde_json::Value::String(String::from_utf8_lossy(&encoded).to_string())
            }
        }
    }
}

// Simple base64 encoder for HTTP feature
#[cfg(feature = "http")]
mod base64_encode {
    use std::io::{Result, Write};

    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub struct Base64Encoder<W: Write> {
        writer: W,
        buffer: [u8; 3],
        buffer_len: usize,
    }

    impl<W: Write> Base64Encoder<W> {
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
}

impl<'bind> BindCollector<'bind, D1Backend> for D1BindCollector {
    type Buffer = BindValue;

    fn push_bound_value<T, U>(
        &mut self,
        bind: &'bind U,
        metadata_lookup: &mut <D1Backend as diesel::sql_types::TypeMetadata>::MetadataLookup,
    ) -> diesel::QueryResult<()>
    where
        D1Backend: diesel::backend::Backend + diesel::sql_types::HasSqlType<T>,
        U: diesel::serialize::ToSql<T, D1Backend> + ?Sized + 'bind,
    {
        let value = BindValue::Null;
        let mut to_sql_output = Output::new(value, metadata_lookup);
        let is_null = bind
            .to_sql(&mut to_sql_output)
            .map_err(diesel::result::Error::SerializationError)?;

        let bind = if matches!(is_null, IsNull::No) {
            to_sql_output.into_inner()
        } else {
            BindValue::Null
        };

        let metadata = D1Backend::metadata(metadata_lookup);
        self.binds.push((bind, metadata));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bind_value_null() {
        let value = BindValue::Null;
        assert!(matches!(value, BindValue::Null));
    }

    #[test]
    fn test_bind_value_integer() {
        let value = BindValue::Integer(42);
        if let BindValue::Integer(i) = value {
            assert_eq!(i, 42);
        } else {
            panic!("Expected Integer variant");
        }
    }

    #[test]
    fn test_bind_value_double() {
        let value = BindValue::Double(3.14);
        if let BindValue::Double(d) = value {
            assert!((d - 3.14).abs() < f64::EPSILON);
        } else {
            panic!("Expected Double variant");
        }
    }

    #[test]
    fn test_bind_value_text() {
        let value = BindValue::Text("hello".to_string());
        if let BindValue::Text(s) = value {
            assert_eq!(s, "hello");
        } else {
            panic!("Expected Text variant");
        }
    }

    #[test]
    fn test_bind_value_binary() {
        let value = BindValue::Binary(vec![1, 2, 3]);
        if let BindValue::Binary(b) = value {
            assert_eq!(b, vec![1, 2, 3]);
        } else {
            panic!("Expected Binary variant");
        }
    }

    #[test]
    fn test_bind_collector_default() {
        let collector = D1BindCollector::default();
        assert!(collector.binds.is_empty());
    }
}
