//! SQL type implementations for D1 backend
//!
//! This module provides type mappings between Diesel SQL types and D1 types.

use diesel::{
    deserialize::{self, FromSql},
    serialize::{self, IsNull, Output, ToSql},
    sql_types::{self, HasSqlType},
};

use crate::{
    backend::{D1Backend, D1Type},
    bind_collector::BindValue,
};

// Value type for deserialization - feature-specific implementations in separate modules
#[cfg(feature = "wasm")]
use crate::value::D1Value;

#[cfg(feature = "http")]
use crate::http_value::D1Value;

// Boolean
impl HasSqlType<sql_types::Bool> for D1Backend {
    fn metadata(_lookup: &mut ()) -> D1Type {
        D1Type::Integer
    }
}

#[cfg(any(feature = "wasm", feature = "http"))]
impl FromSql<sql_types::Bool, D1Backend> for bool {
    fn from_sql(value: D1Value) -> deserialize::Result<Self> {
        let bool_number = value.read_number();
        if !(bool_number == 0.0 || bool_number == 1.0) {
            return Err("Invalid boolean value".into());
        }
        Ok(bool_number != 0.0)
    }
}

impl ToSql<sql_types::Bool, D1Backend> for bool {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, D1Backend>) -> serialize::Result {
        out.set_value(BindValue::Integer(if *self { 1 } else { 0 }));
        Ok(IsNull::No)
    }
}

// SMALL INT
impl HasSqlType<sql_types::SmallInt> for D1Backend {
    fn metadata(_lookup: &mut ()) -> D1Type {
        D1Type::Integer
    }
}

#[cfg(any(feature = "wasm", feature = "http"))]
impl FromSql<sql_types::SmallInt, D1Backend> for i16 {
    fn from_sql(value: D1Value) -> deserialize::Result<Self> {
        let num = value.read_number();
        Ok(num as i16)
    }
}

impl ToSql<sql_types::SmallInt, D1Backend> for i16 {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, D1Backend>) -> serialize::Result {
        out.set_value(BindValue::Integer(*self as i64));
        Ok(IsNull::No)
    }
}

// Int
impl HasSqlType<sql_types::Integer> for D1Backend {
    fn metadata(_lookup: &mut ()) -> D1Type {
        D1Type::Integer
    }
}

#[cfg(any(feature = "wasm", feature = "http"))]
impl FromSql<sql_types::Integer, D1Backend> for i32 {
    fn from_sql(value: D1Value) -> deserialize::Result<Self> {
        let num = value.read_number();
        Ok(num as i32)
    }
}

impl ToSql<sql_types::Integer, D1Backend> for i32 {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, D1Backend>) -> serialize::Result {
        out.set_value(BindValue::Integer(*self as i64));
        Ok(IsNull::No)
    }
}

// BigInt
impl HasSqlType<sql_types::BigInt> for D1Backend {
    fn metadata(_lookup: &mut ()) -> D1Type {
        D1Type::Integer
    }
}

#[cfg(any(feature = "wasm", feature = "http"))]
impl FromSql<sql_types::BigInt, D1Backend> for i64 {
    fn from_sql(value: D1Value) -> deserialize::Result<Self> {
        let num = value.read_number();
        Ok(num as i64)
    }
}

impl ToSql<sql_types::BigInt, D1Backend> for i64 {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, D1Backend>) -> serialize::Result {
        out.set_value(BindValue::Integer(*self));
        Ok(IsNull::No)
    }
}

// Float
impl HasSqlType<sql_types::Float> for D1Backend {
    fn metadata(_lookup: &mut ()) -> D1Type {
        D1Type::Double
    }
}

#[cfg(any(feature = "wasm", feature = "http"))]
impl FromSql<sql_types::Float, D1Backend> for f32 {
    fn from_sql(value: D1Value) -> deserialize::Result<Self> {
        let num = value.read_number();
        Ok(num as f32)
    }
}

impl ToSql<sql_types::Float, D1Backend> for f32 {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, D1Backend>) -> serialize::Result {
        out.set_value(BindValue::Double(*self as f64));
        Ok(IsNull::No)
    }
}

// Double
impl HasSqlType<sql_types::Double> for D1Backend {
    fn metadata(_lookup: &mut ()) -> D1Type {
        D1Type::Double
    }
}

#[cfg(any(feature = "wasm", feature = "http"))]
impl FromSql<sql_types::Double, D1Backend> for f64 {
    fn from_sql(value: D1Value) -> deserialize::Result<Self> {
        let num = value.read_number();
        Ok(num)
    }
}

impl ToSql<sql_types::Double, D1Backend> for f64 {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, D1Backend>) -> serialize::Result {
        out.set_value(BindValue::Double(*self));
        Ok(IsNull::No)
    }
}

// Text
impl HasSqlType<sql_types::Text> for D1Backend {
    fn metadata(_lookup: &mut ()) -> D1Type {
        D1Type::Text
    }
}

#[cfg(any(feature = "wasm", feature = "http"))]
impl FromSql<sql_types::Text, D1Backend> for String {
    fn from_sql(value: D1Value) -> deserialize::Result<Self> {
        let text = value.read_string();
        Ok(text)
    }
}

impl ToSql<sql_types::Text, D1Backend> for str {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, D1Backend>) -> serialize::Result {
        out.set_value(BindValue::Text(self.to_string()));
        Ok(IsNull::No)
    }
}

// Blob/Binary
impl HasSqlType<sql_types::Binary> for D1Backend {
    fn metadata(_lookup: &mut ()) -> D1Type {
        D1Type::Binary
    }
}

#[cfg(any(feature = "wasm", feature = "http"))]
impl FromSql<sql_types::Binary, D1Backend> for Vec<u8> {
    fn from_sql(value: D1Value) -> deserialize::Result<Self> {
        let blob = value.read_blob();
        Ok(blob)
    }
}

impl ToSql<sql_types::Binary, D1Backend> for [u8] {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, D1Backend>) -> serialize::Result {
        out.set_value(BindValue::Binary(self.to_vec()));
        Ok(IsNull::No)
    }
}

// Time-related types (stored as text in SQLite/D1)
impl HasSqlType<sql_types::Date> for D1Backend {
    fn metadata(_lookup: &mut ()) -> D1Type {
        D1Type::Text
    }
}

impl HasSqlType<sql_types::Time> for D1Backend {
    fn metadata(_lookup: &mut ()) -> D1Type {
        D1Type::Text
    }
}

impl HasSqlType<sql_types::Timestamp> for D1Backend {
    fn metadata(_lookup: &mut ()) -> D1Type {
        D1Type::Text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bool_metadata() {
        let metadata: D1Type = <D1Backend as HasSqlType<sql_types::Bool>>::metadata(&mut ());
        assert_eq!(metadata, D1Type::Integer);
    }

    #[test]
    fn test_integer_metadata() {
        let metadata: D1Type = <D1Backend as HasSqlType<sql_types::Integer>>::metadata(&mut ());
        assert_eq!(metadata, D1Type::Integer);
    }

    #[test]
    fn test_text_metadata() {
        let metadata: D1Type = <D1Backend as HasSqlType<sql_types::Text>>::metadata(&mut ());
        assert_eq!(metadata, D1Type::Text);
    }

    #[test]
    fn test_double_metadata() {
        let metadata: D1Type = <D1Backend as HasSqlType<sql_types::Double>>::metadata(&mut ());
        assert_eq!(metadata, D1Type::Double);
    }

    #[test]
    fn test_binary_metadata() {
        let metadata: D1Type = <D1Backend as HasSqlType<sql_types::Binary>>::metadata(&mut ());
        assert_eq!(metadata, D1Type::Binary);
    }
}
