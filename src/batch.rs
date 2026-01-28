//! Batch operations for prepared query reuse
//!
//! This module provides batch construction that reuses a single prepared statement
//! across many binds (N executions) and runs them via batch() when possible.
//!
//! # D1 Transactional Reality
//!
//! **Important:** D1's `batch()` executes as a SQL transaction and will abort/rollback
//! the entire sequence if a statement fails. This is the key transactional guarantee
//! that D1 provides.
//!
//! # Example
//!
//! ```
//! use diesel_d1::batch::{BatchBuilder, BatchStatement};
//!
//! // Create a batch of inserts using prepared statement reuse
//! let mut batch = BatchBuilder::new();
//!
//! // Add multiple statements with the same SQL template
//! let sql = "INSERT INTO users (name, email) VALUES (?, ?)";
//! batch.add_statement(sql, vec!["Alice".into(), "alice@example.com".into()]);
//! batch.add_statement(sql, vec!["Bob".into(), "bob@example.com".into()]);
//! batch.add_statement(sql, vec!["Charlie".into(), "charlie@example.com".into()]);
//!
//! // The batch can now be executed atomically
//! assert_eq!(batch.len(), 3);
//! ```

use std::collections::HashMap;

/// A bound value for batch operations
///
/// This type represents a parameter value that can be bound to a prepared statement.
#[derive(Debug, Clone, PartialEq)]
pub enum BoundValue {
    /// Null value
    Null,
    /// Integer value
    Integer(i64),
    /// Float value
    Float(f64),
    /// Text value
    Text(String),
    /// Binary data
    Binary(Vec<u8>),
}

impl From<i64> for BoundValue {
    fn from(v: i64) -> Self {
        BoundValue::Integer(v)
    }
}

impl From<i32> for BoundValue {
    fn from(v: i32) -> Self {
        BoundValue::Integer(v as i64)
    }
}

impl From<f64> for BoundValue {
    fn from(v: f64) -> Self {
        BoundValue::Float(v)
    }
}

impl From<String> for BoundValue {
    fn from(v: String) -> Self {
        BoundValue::Text(v)
    }
}

impl From<&str> for BoundValue {
    fn from(v: &str) -> Self {
        BoundValue::Text(v.to_string())
    }
}

impl From<Vec<u8>> for BoundValue {
    fn from(v: Vec<u8>) -> Self {
        BoundValue::Binary(v)
    }
}

impl From<Option<i64>> for BoundValue {
    fn from(v: Option<i64>) -> Self {
        match v {
            Some(i) => BoundValue::Integer(i),
            None => BoundValue::Null,
        }
    }
}

impl BoundValue {
    /// Convert to a serde_json Value
    #[cfg(feature = "http")]
    pub fn to_json_value(&self) -> serde_json::Value {
        match self {
            BoundValue::Null => serde_json::Value::Null,
            BoundValue::Integer(i) => serde_json::Value::Number((*i).into()),
            BoundValue::Float(f) => serde_json::Number::from_f64(*f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            BoundValue::Text(s) => serde_json::Value::String(s.clone()),
            BoundValue::Binary(b) => {
                // Encode as base64 using shared utility
                let encoded_str = crate::utils::base64::encode(b);
                serde_json::Value::String(encoded_str)
            }
        }
    }

    /// Convert to a JsValue for WASM
    #[cfg(feature = "wasm")]
    pub fn to_js_value(&self) -> wasm_bindgen::JsValue {
        use wasm_bindgen::JsValue;
        match self {
            BoundValue::Null => JsValue::null(),
            BoundValue::Integer(i) => JsValue::from_f64(*i as f64),
            BoundValue::Float(f) => JsValue::from_f64(*f),
            BoundValue::Text(s) => JsValue::from_str(s),
            BoundValue::Binary(b) => {
                let array = js_sys::Uint8Array::new_with_length(b.len() as u32);
                array.copy_from(b);
                array.into()
            }
        }
    }
}

/// A single statement in a batch with its bound parameters
#[derive(Debug, Clone)]
pub struct BatchStatement {
    /// The SQL string
    pub sql: String,
    /// Bound parameter values
    pub params: Vec<BoundValue>,
}

impl BatchStatement {
    /// Create a new batch statement
    pub fn new(sql: impl Into<String>, params: Vec<BoundValue>) -> Self {
        Self {
            sql: sql.into(),
            params,
        }
    }

    /// Get the SQL string
    pub fn sql(&self) -> &str {
        &self.sql
    }

    /// Get the parameter count
    pub fn param_count(&self) -> usize {
        self.params.len()
    }
}

/// Builder for creating batches of statements with prepared query reuse
///
/// This builder collects statements and tracks SQL string reuse for efficient
/// batch execution. When executed via D1's `batch()` API, all statements
/// run as a SQL transaction with automatic rollback on failure.
///
/// # D1 Transactional Semantics
///
/// - All statements in a batch execute atomically
/// - If any statement fails, the entire batch is rolled back
/// - Results are returned in-order for all successful executions
///
/// # Example
///
/// ```
/// use diesel_d1::batch::{BatchBuilder, BoundValue};
///
/// let mut batch = BatchBuilder::new();
///
/// // Add statements - SQL string is reused internally
/// batch.add_statement(
///     "INSERT INTO items (name) VALUES (?)",
///     vec![BoundValue::Text("Item 1".into())],
/// );
/// batch.add_statement(
///     "INSERT INTO items (name) VALUES (?)",
///     vec![BoundValue::Text("Item 2".into())],
/// );
///
/// // Check reuse statistics
/// let stats = batch.reuse_stats();
/// assert_eq!(stats.total_statements, 2);
/// assert_eq!(stats.unique_sql_strings, 1);
/// ```
#[derive(Debug, Clone, Default)]
pub struct BatchBuilder {
    /// All statements in order
    statements: Vec<BatchStatement>,
    /// Track SQL string occurrences for reuse statistics
    sql_counts: HashMap<String, usize>,
}

impl BatchBuilder {
    /// Create a new empty batch builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a statement to the batch
    pub fn add_statement(&mut self, sql: impl Into<String>, params: Vec<BoundValue>) {
        let sql_string = sql.into();
        *self.sql_counts.entry(sql_string.clone()).or_insert(0) += 1;
        self.statements
            .push(BatchStatement::new(sql_string, params));
    }

    /// Add a raw SQL statement (no parameters)
    pub fn add_raw(&mut self, sql: impl Into<String>) {
        self.add_statement(sql, vec![]);
    }

    /// Get the number of statements in the batch
    pub fn len(&self) -> usize {
        self.statements.len()
    }

    /// Check if the batch is empty
    pub fn is_empty(&self) -> bool {
        self.statements.is_empty()
    }

    /// Get all statements
    pub fn statements(&self) -> &[BatchStatement] {
        &self.statements
    }

    /// Consume the builder and return the statements
    pub fn into_statements(self) -> Vec<BatchStatement> {
        self.statements
    }

    /// Get reuse statistics for the batch
    pub fn reuse_stats(&self) -> BatchReuseStats {
        let total_reused = self.sql_counts.values().filter(|&&c| c > 1).sum::<usize>();
        BatchReuseStats {
            total_statements: self.statements.len(),
            unique_sql_strings: self.sql_counts.len(),
            reused_statements: total_reused,
        }
    }

    /// Clear all statements from the batch
    pub fn clear(&mut self) {
        self.statements.clear();
        self.sql_counts.clear();
    }
}

/// Statistics about SQL string reuse in a batch
#[derive(Debug, Clone, Copy, Default)]
pub struct BatchReuseStats {
    /// Total number of statements
    pub total_statements: usize,
    /// Number of unique SQL strings
    pub unique_sql_strings: usize,
    /// Number of statements that reused an existing SQL string
    pub reused_statements: usize,
}

impl BatchReuseStats {
    /// Get the reuse percentage (0.0 to 1.0)
    pub fn reuse_percentage(&self) -> f64 {
        if self.total_statements <= 1 {
            0.0
        } else {
            self.reused_statements as f64 / self.total_statements as f64
        }
    }
}

/// Result metadata from a batch execution
#[derive(Debug, Clone, Default)]
pub struct BatchResult {
    /// Number of statements that succeeded
    pub successful_statements: usize,
    /// Number of rows affected across all statements
    pub total_rows_affected: usize,
    /// Results per statement (in order)
    pub statement_results: Vec<StatementResult>,
    /// Whether the batch completed successfully
    pub success: bool,
}

/// Result from a single statement in a batch
#[derive(Debug, Clone)]
pub struct StatementResult {
    /// Whether this statement succeeded
    pub success: bool,
    /// Number of rows affected
    pub rows_affected: usize,
    /// Error message if failed
    pub error: Option<String>,
}

impl StatementResult {
    /// Create a successful result
    pub fn success(rows_affected: usize) -> Self {
        Self {
            success: true,
            rows_affected,
            error: None,
        }
    }

    /// Create a failed result
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            success: false,
            rows_affected: 0,
            error: Some(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bound_value_from_integer() {
        let v: BoundValue = 42i64.into();
        assert_eq!(v, BoundValue::Integer(42));
    }

    #[test]
    fn test_bound_value_from_i32() {
        let v: BoundValue = 42i32.into();
        assert_eq!(v, BoundValue::Integer(42));
    }

    #[test]
    fn test_bound_value_from_float() {
        let v: BoundValue = 3.14f64.into();
        if let BoundValue::Float(f) = v {
            assert!((f - 3.14).abs() < f64::EPSILON);
        } else {
            panic!("Expected Float variant");
        }
    }

    #[test]
    fn test_bound_value_from_string() {
        let v: BoundValue = "hello".into();
        assert_eq!(v, BoundValue::Text("hello".to_string()));
    }

    #[test]
    fn test_bound_value_from_vec_u8() {
        let v: BoundValue = vec![1u8, 2, 3].into();
        assert_eq!(v, BoundValue::Binary(vec![1, 2, 3]));
    }

    #[test]
    fn test_bound_value_from_option() {
        let v: BoundValue = Some(42i64).into();
        assert_eq!(v, BoundValue::Integer(42));

        let v: BoundValue = None::<i64>.into();
        assert_eq!(v, BoundValue::Null);
    }

    #[test]
    fn test_batch_statement_new() {
        let stmt = BatchStatement::new("SELECT ?", vec![BoundValue::Integer(1)]);
        assert_eq!(stmt.sql(), "SELECT ?");
        assert_eq!(stmt.param_count(), 1);
    }

    #[test]
    fn test_batch_builder_new() {
        let batch = BatchBuilder::new();
        assert!(batch.is_empty());
        assert_eq!(batch.len(), 0);
    }

    #[test]
    fn test_batch_builder_add_statement() {
        let mut batch = BatchBuilder::new();
        batch.add_statement("SELECT ?", vec![BoundValue::Integer(1)]);

        assert_eq!(batch.len(), 1);
        assert!(!batch.is_empty());
    }

    #[test]
    fn test_batch_builder_add_raw() {
        let mut batch = BatchBuilder::new();
        batch.add_raw("SELECT 1");

        assert_eq!(batch.len(), 1);
        assert_eq!(batch.statements()[0].param_count(), 0);
    }

    #[test]
    fn test_batch_builder_reuse_stats() {
        let mut batch = BatchBuilder::new();
        batch.add_statement("INSERT INTO t VALUES (?)", vec![1.into()]);
        batch.add_statement("INSERT INTO t VALUES (?)", vec![2.into()]);
        batch.add_statement("INSERT INTO t VALUES (?)", vec![3.into()]);
        batch.add_raw("SELECT 1");

        let stats = batch.reuse_stats();
        assert_eq!(stats.total_statements, 4);
        assert_eq!(stats.unique_sql_strings, 2);
        assert_eq!(stats.reused_statements, 3); // 3 inserts reused same SQL
    }

    #[test]
    fn test_batch_builder_into_statements() {
        let mut batch = BatchBuilder::new();
        batch.add_statement("SELECT 1", vec![]);
        batch.add_statement("SELECT 2", vec![]);

        let statements = batch.into_statements();
        assert_eq!(statements.len(), 2);
    }

    #[test]
    fn test_batch_builder_clear() {
        let mut batch = BatchBuilder::new();
        batch.add_statement("SELECT 1", vec![]);
        batch.add_statement("SELECT 2", vec![]);

        batch.clear();

        assert!(batch.is_empty());
    }

    #[test]
    fn test_batch_reuse_stats_percentage() {
        let stats = BatchReuseStats {
            total_statements: 10,
            unique_sql_strings: 2,
            reused_statements: 8,
        };

        assert!((stats.reuse_percentage() - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn test_batch_reuse_stats_percentage_empty() {
        let stats = BatchReuseStats::default();
        assert!((stats.reuse_percentage() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_statement_result_success() {
        let result = StatementResult::success(5);
        assert!(result.success);
        assert_eq!(result.rows_affected, 5);
        assert!(result.error.is_none());
    }

    #[test]
    fn test_statement_result_failure() {
        let result = StatementResult::failure("Constraint violation");
        assert!(!result.success);
        assert_eq!(result.rows_affected, 0);
        assert_eq!(result.error, Some("Constraint violation".to_string()));
    }

    #[cfg(feature = "http")]
    #[test]
    fn test_bound_value_to_json() {
        let null = BoundValue::Null;
        assert_eq!(null.to_json_value(), serde_json::Value::Null);

        let int = BoundValue::Integer(42);
        assert_eq!(int.to_json_value(), serde_json::json!(42));

        let float = BoundValue::Float(3.14);
        let json_float = float.to_json_value();
        if let serde_json::Value::Number(n) = json_float {
            assert!((n.as_f64().unwrap() - 3.14).abs() < 0.001);
        } else {
            panic!("Expected Number");
        }

        let text = BoundValue::Text("hello".to_string());
        assert_eq!(text.to_json_value(), serde_json::json!("hello"));
    }
}
