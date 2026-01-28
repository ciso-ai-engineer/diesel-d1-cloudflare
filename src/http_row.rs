//! HTTP row type for D1 backend
//!
//! This module provides the D1Row type for iterating over query results
//! when using the HTTP REST API.

use std::{
    cell::{Ref, RefCell},
    rc::Rc,
};

use diesel::row::{Field, PartialRow, Row, RowIndex, RowSealed};
use serde_json::Value as JsonValue;

use crate::{backend::D1Backend, http_value::D1Value};

/// A row from a D1 query result (HTTP version)
pub struct D1Row {
    data: Rc<RefCell<JsonValue>>,
    field_vec: Vec<String>,
}

// SAFETY: The data is reference-counted and properly synchronized
unsafe impl Send for D1Row {}
unsafe impl Sync for D1Row {}

impl D1Row {
    /// Create a new row from a JSON value and field names
    pub fn new(json_value: JsonValue, field_vec: Vec<String>) -> Self {
        Self {
            data: Rc::new(RefCell::new(json_value)),
            field_vec,
        }
    }
}

impl RowSealed for D1Row {}

impl<'stmt> Row<'stmt, D1Backend> for D1Row {
    type Field<'f> = D1Field<'f> where 'stmt: 'f, Self: 'f;
    type InnerPartialRow = Self;

    fn field_count(&self) -> usize {
        self.field_vec.len()
    }

    fn get<'b, I>(&'b self, idx: I) -> Option<Self::Field<'b>>
    where
        'stmt: 'b,
        Self: diesel::row::RowIndex<I>,
    {
        let index = self.idx(idx)?;
        let name = self.field_vec.get(index)?;
        Some(D1Field {
            name: name.to_string(),
            row: self.data.borrow(),
        })
    }

    fn partial_row(
        &self,
        range: std::ops::Range<usize>,
    ) -> diesel::row::PartialRow<'_, Self::InnerPartialRow> {
        PartialRow::new(self, range)
    }
}

impl RowIndex<usize> for D1Row {
    fn idx(&self, idx: usize) -> Option<usize> {
        if idx < self.field_vec.len() {
            Some(idx)
        } else {
            None
        }
    }
}

impl RowIndex<&str> for D1Row {
    fn idx(&self, field: &str) -> Option<usize> {
        self.field_vec.iter().position(|i| i == field)
    }
}

/// A field from a D1 row (HTTP version)
pub struct D1Field<'stmt> {
    row: Ref<'stmt, JsonValue>,
    name: String,
}

impl<'stmt> Field<'stmt, D1Backend> for D1Field<'stmt> {
    fn field_name(&self) -> Option<&str> {
        Some(&self.name)
    }

    fn value(&self) -> Option<D1Value> {
        if let Some(obj) = self.row.as_object() {
            obj.get(&self.name).map(|v| D1Value::new(v.clone()))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_d1_row_field_count() {
        let row = D1Row::new(
            json!({"id": 1, "name": "test"}),
            vec!["id".to_string(), "name".to_string()],
        );
        assert_eq!(row.field_count(), 2);
    }

    #[test]
    fn test_d1_row_index_by_position() {
        let row = D1Row::new(
            json!({"id": 1, "name": "test"}),
            vec!["id".to_string(), "name".to_string()],
        );
        assert_eq!(row.idx(0), Some(0));
        assert_eq!(row.idx(1), Some(1));
        assert_eq!(row.idx(2), None);
    }

    #[test]
    fn test_d1_row_index_by_name() {
        let row = D1Row::new(
            json!({"id": 1, "name": "test"}),
            vec!["id".to_string(), "name".to_string()],
        );
        assert_eq!(row.idx("id"), Some(0));
        assert_eq!(row.idx("name"), Some(1));
        assert_eq!(row.idx("unknown"), None);
    }

    #[test]
    fn test_d1_row_get_field() {
        let row = D1Row::new(
            json!({"id": 1, "name": "test"}),
            vec!["id".to_string(), "name".to_string()],
        );
        
        let field = row.get(0usize).unwrap();
        assert_eq!(field.field_name(), Some("id"));
        
        let value = field.value().unwrap();
        assert!((value.read_number() - 1.0).abs() < f64::EPSILON);
    }
}
