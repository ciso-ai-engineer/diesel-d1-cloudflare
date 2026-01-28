//! Deterministic replay for transaction testing
//!
//! This module provides a transcript-based testing harness for reliably proving
//! rollback/abort behavior and guarding against regressions across platforms.
//!
//! # Transaction Transcript Format
//!
//! A transcript records:
//! - Ordered statements with bound parameters
//! - Expected per-statement result metadata (success/failure)
//! - Expected final database invariants
//!
//! # D1 Batch Semantics
//!
//! D1's `batch()` API executes as a SQL transaction with automatic rollback on failure:
//! - All statements execute atomically
//! - If any statement fails, the entire batch is rolled back
//! - No partial results are committed
//!
//! # Example
//!
//! ```
//! use diesel_d1::replay::{TransactionTranscript, TranscriptStatement, ExpectedResult};
//!
//! // Create a transcript with expected rollback behavior
//! let mut transcript = TransactionTranscript::new("test_rollback");
//!
//! // Add statements that should succeed
//! transcript.add_statement(
//!     TranscriptStatement::new("INSERT INTO users (name) VALUES (?)")
//!         .with_param("Alice")
//!         .expect_success(1),
//! );
//!
//! // Add a statement that should fail (triggering rollback)
//! transcript.add_statement(
//!     TranscriptStatement::new("INSERT INTO users (name) VALUES (?)")
//!         .with_param(None::<String>) // NULL where NOT NULL required
//!         .expect_failure("NOT NULL constraint"),
//! );
//!
//! // Add invariant: no rows should exist due to rollback
//! transcript.add_invariant("SELECT COUNT(*) FROM users", "0");
//!
//! assert!(transcript.expects_rollback());
//! ```

use std::collections::HashMap;

/// A value for transcript parameters
#[derive(Debug, Clone, PartialEq)]
pub enum TranscriptValue {
    /// Null value
    Null,
    /// Integer value
    Integer(i64),
    /// Float value
    Float(f64),
    /// Text value
    Text(String),
    /// Binary data (base64 encoded for serialization)
    Binary(Vec<u8>),
}

impl From<i64> for TranscriptValue {
    fn from(v: i64) -> Self {
        TranscriptValue::Integer(v)
    }
}

impl From<i32> for TranscriptValue {
    fn from(v: i32) -> Self {
        TranscriptValue::Integer(v as i64)
    }
}

impl From<f64> for TranscriptValue {
    fn from(v: f64) -> Self {
        TranscriptValue::Float(v)
    }
}

impl From<String> for TranscriptValue {
    fn from(v: String) -> Self {
        TranscriptValue::Text(v)
    }
}

impl From<&str> for TranscriptValue {
    fn from(v: &str) -> Self {
        TranscriptValue::Text(v.to_string())
    }
}

impl<T: Into<TranscriptValue>> From<Option<T>> for TranscriptValue {
    fn from(v: Option<T>) -> Self {
        match v {
            Some(inner) => inner.into(),
            None => TranscriptValue::Null,
        }
    }
}

/// Expected result for a statement
#[derive(Debug, Clone, PartialEq)]
pub enum ExpectedResult {
    /// Statement should succeed with the given number of affected rows
    Success {
        /// Expected number of rows affected
        rows_affected: Option<usize>,
    },
    /// Statement should fail with an error containing the given substring
    Failure {
        /// Expected error message substring
        error_contains: String,
    },
}

impl ExpectedResult {
    /// Create an expected success result
    pub fn success(rows_affected: usize) -> Self {
        ExpectedResult::Success {
            rows_affected: Some(rows_affected),
        }
    }

    /// Create an expected success result without checking row count
    pub fn success_any() -> Self {
        ExpectedResult::Success {
            rows_affected: None,
        }
    }

    /// Create an expected failure result
    pub fn failure(error_contains: impl Into<String>) -> Self {
        ExpectedResult::Failure {
            error_contains: error_contains.into(),
        }
    }

    /// Check if this is an expected failure
    pub fn is_failure(&self) -> bool {
        matches!(self, ExpectedResult::Failure { .. })
    }
}

/// A statement in a transcript
#[derive(Debug, Clone)]
pub struct TranscriptStatement {
    /// The SQL statement
    pub sql: String,
    /// Bound parameters
    pub params: Vec<TranscriptValue>,
    /// Expected result
    pub expected: ExpectedResult,
    /// Optional comment/description
    pub comment: Option<String>,
}

impl TranscriptStatement {
    /// Create a new statement with expected success
    pub fn new(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            params: Vec::new(),
            expected: ExpectedResult::success_any(),
            comment: None,
        }
    }

    /// Add a parameter
    pub fn with_param(mut self, param: impl Into<TranscriptValue>) -> Self {
        self.params.push(param.into());
        self
    }

    /// Add multiple parameters
    pub fn with_params(mut self, params: Vec<TranscriptValue>) -> Self {
        self.params = params;
        self
    }

    /// Set expected success with row count
    pub fn expect_success(mut self, rows_affected: usize) -> Self {
        self.expected = ExpectedResult::success(rows_affected);
        self
    }

    /// Set expected success without checking row count
    pub fn expect_success_any(mut self) -> Self {
        self.expected = ExpectedResult::success_any();
        self
    }

    /// Set expected failure
    pub fn expect_failure(mut self, error_contains: impl Into<String>) -> Self {
        self.expected = ExpectedResult::failure(error_contains);
        self
    }

    /// Add a comment
    pub fn with_comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = Some(comment.into());
        self
    }

    /// Get the parameter count
    pub fn param_count(&self) -> usize {
        self.params.len()
    }
}

/// A database invariant to check after replay
#[derive(Debug, Clone)]
pub struct DatabaseInvariant {
    /// SQL query to execute
    pub query: String,
    /// Expected result (as a string for simple comparison)
    pub expected_value: String,
    /// Optional description
    pub description: Option<String>,
}

impl DatabaseInvariant {
    /// Create a new invariant
    pub fn new(query: impl Into<String>, expected_value: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            expected_value: expected_value.into(),
            description: None,
        }
    }

    /// Add a description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// A complete transaction transcript for replay testing
#[derive(Debug, Clone)]
pub struct TransactionTranscript {
    /// Name/identifier for this transcript
    pub name: String,
    /// Description of what this transcript tests
    pub description: Option<String>,
    /// Ordered statements to execute
    pub statements: Vec<TranscriptStatement>,
    /// Invariants to check after execution
    pub invariants: Vec<DatabaseInvariant>,
    /// Metadata for the transcript
    pub metadata: HashMap<String, String>,
}

impl TransactionTranscript {
    /// Create a new empty transcript
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            statements: Vec::new(),
            invariants: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Add a description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Add a statement
    pub fn add_statement(&mut self, statement: TranscriptStatement) {
        self.statements.push(statement);
    }

    /// Add an invariant
    pub fn add_invariant(&mut self, query: impl Into<String>, expected: impl Into<String>) {
        self.invariants
            .push(DatabaseInvariant::new(query, expected));
    }

    /// Add metadata
    pub fn set_metadata(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.metadata.insert(key.into(), value.into());
    }

    /// Get the number of statements
    pub fn statement_count(&self) -> usize {
        self.statements.len()
    }

    /// Check if any statement expects a failure
    pub fn expects_rollback(&self) -> bool {
        self.statements.iter().any(|s| s.expected.is_failure())
    }

    /// Get the index of the first expected failure (if any)
    pub fn first_failure_index(&self) -> Option<usize> {
        self.statements.iter().position(|s| s.expected.is_failure())
    }

    /// Create an iterator over statements
    pub fn iter(&self) -> impl Iterator<Item = &TranscriptStatement> {
        self.statements.iter()
    }
}

/// Result of replaying a transcript
#[derive(Debug, Clone)]
pub struct ReplayResult {
    /// Name of the transcript that was replayed
    pub transcript_name: String,
    /// Whether the replay matched expectations
    pub success: bool,
    /// Per-statement results
    pub statement_results: Vec<StatementReplayResult>,
    /// Invariant check results
    pub invariant_results: Vec<InvariantResult>,
    /// Overall error message if failed
    pub error: Option<String>,
}

impl ReplayResult {
    /// Create a new successful replay result
    pub fn success(transcript_name: impl Into<String>) -> Self {
        Self {
            transcript_name: transcript_name.into(),
            success: true,
            statement_results: Vec::new(),
            invariant_results: Vec::new(),
            error: None,
        }
    }

    /// Create a new failed replay result
    pub fn failure(transcript_name: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            transcript_name: transcript_name.into(),
            success: false,
            statement_results: Vec::new(),
            invariant_results: Vec::new(),
            error: Some(error.into()),
        }
    }

    /// Add a statement result
    pub fn add_statement_result(&mut self, result: StatementReplayResult) {
        if !result.matched_expectation {
            self.success = false;
        }
        self.statement_results.push(result);
    }

    /// Add an invariant result
    pub fn add_invariant_result(&mut self, result: InvariantResult) {
        if !result.matched {
            self.success = false;
        }
        self.invariant_results.push(result);
    }

    /// Get a summary of the replay
    pub fn summary(&self) -> String {
        let statements_passed = self
            .statement_results
            .iter()
            .filter(|r| r.matched_expectation)
            .count();
        let invariants_passed = self.invariant_results.iter().filter(|r| r.matched).count();

        format!(
            "Transcript '{}': {} - Statements: {}/{}, Invariants: {}/{}",
            self.transcript_name,
            if self.success { "PASSED" } else { "FAILED" },
            statements_passed,
            self.statement_results.len(),
            invariants_passed,
            self.invariant_results.len()
        )
    }
}

/// Result of replaying a single statement
#[derive(Debug, Clone)]
pub struct StatementReplayResult {
    /// The statement index
    pub index: usize,
    /// Whether the actual result matched the expected result
    pub matched_expectation: bool,
    /// The expected result
    pub expected: ExpectedResult,
    /// Whether the statement actually succeeded
    pub actual_success: bool,
    /// Actual rows affected (if succeeded)
    pub actual_rows_affected: Option<usize>,
    /// Actual error (if failed)
    pub actual_error: Option<String>,
    /// Details about the mismatch (if any)
    pub mismatch_detail: Option<String>,
}

impl StatementReplayResult {
    /// Create a result for a matched expectation
    pub fn matched(
        index: usize,
        expected: ExpectedResult,
        actual_success: bool,
        actual_rows_affected: Option<usize>,
        actual_error: Option<String>,
    ) -> Self {
        Self {
            index,
            matched_expectation: true,
            expected,
            actual_success,
            actual_rows_affected,
            actual_error,
            mismatch_detail: None,
        }
    }

    /// Create a result for a mismatched expectation
    pub fn mismatched(
        index: usize,
        expected: ExpectedResult,
        actual_success: bool,
        actual_rows_affected: Option<usize>,
        actual_error: Option<String>,
        mismatch_detail: impl Into<String>,
    ) -> Self {
        Self {
            index,
            matched_expectation: false,
            expected,
            actual_success,
            actual_rows_affected,
            actual_error,
            mismatch_detail: Some(mismatch_detail.into()),
        }
    }
}

/// Result of checking an invariant
#[derive(Debug, Clone)]
pub struct InvariantResult {
    /// The invariant query
    pub query: String,
    /// Expected value
    pub expected: String,
    /// Actual value
    pub actual: String,
    /// Whether they matched
    pub matched: bool,
}

impl InvariantResult {
    /// Create a matched invariant result
    pub fn matched(query: impl Into<String>, expected: impl Into<String>) -> Self {
        let expected = expected.into();
        Self {
            query: query.into(),
            expected: expected.clone(),
            actual: expected,
            matched: true,
        }
    }

    /// Create a mismatched invariant result
    pub fn mismatched(
        query: impl Into<String>,
        expected: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        Self {
            query: query.into(),
            expected: expected.into(),
            actual: actual.into(),
            matched: false,
        }
    }
}

/// Golden test transcript builder for common rollback scenarios
pub mod golden_transcripts {
    use super::*;

    /// Create a transcript that tests basic rollback on constraint violation
    pub fn rollback_on_constraint_violation(table_name: &str) -> TransactionTranscript {
        let mut transcript = TransactionTranscript::new("rollback_on_constraint_violation");
        transcript.description = Some(format!(
            "Tests that batch() rolls back all changes when a constraint violation occurs in {}",
            table_name
        ));

        // First insert should succeed
        transcript.add_statement(
            TranscriptStatement::new(format!(
                "INSERT INTO {} (id, name) VALUES (?, ?)",
                table_name
            ))
            .with_param(1i64)
            .with_param("Alice")
            .expect_success(1)
            .with_comment("First insert should succeed"),
        );

        // Second insert with duplicate key should fail
        transcript.add_statement(
            TranscriptStatement::new(format!(
                "INSERT INTO {} (id, name) VALUES (?, ?)",
                table_name
            ))
            .with_param(1i64) // Duplicate key
            .with_param("Bob")
            .expect_failure("UNIQUE constraint failed")
            .with_comment("Duplicate key should trigger rollback"),
        );

        // Invariant: no rows should exist due to rollback
        transcript.add_invariant(format!("SELECT COUNT(*) FROM {}", table_name), "0");

        transcript
    }

    /// Create a transcript that tests successful batch execution
    pub fn successful_batch(table_name: &str) -> TransactionTranscript {
        let mut transcript = TransactionTranscript::new("successful_batch");
        transcript.description = Some(format!(
            "Tests that batch() commits all changes when all statements succeed in {}",
            table_name
        ));

        transcript.add_statement(
            TranscriptStatement::new(format!(
                "INSERT INTO {} (id, name) VALUES (?, ?)",
                table_name
            ))
            .with_param(1i64)
            .with_param("Alice")
            .expect_success(1),
        );

        transcript.add_statement(
            TranscriptStatement::new(format!(
                "INSERT INTO {} (id, name) VALUES (?, ?)",
                table_name
            ))
            .with_param(2i64)
            .with_param("Bob")
            .expect_success(1),
        );

        // Invariant: both rows should exist
        transcript.add_invariant(format!("SELECT COUNT(*) FROM {}", table_name), "2");

        transcript
    }

    /// Create a transcript for testing NULL constraint violations
    pub fn rollback_on_null_violation(table_name: &str) -> TransactionTranscript {
        let mut transcript = TransactionTranscript::new("rollback_on_null_violation");
        transcript.description = Some(
            "Tests that batch() rolls back when a NOT NULL constraint is violated".to_string(),
        );

        transcript.add_statement(
            TranscriptStatement::new(format!(
                "INSERT INTO {} (id, name) VALUES (?, ?)",
                table_name
            ))
            .with_param(1i64)
            .with_param("Alice")
            .expect_success(1),
        );

        transcript.add_statement(
            TranscriptStatement::new(format!(
                "INSERT INTO {} (id, name) VALUES (?, ?)",
                table_name
            ))
            .with_param(2i64)
            .with_param(None::<String>) // NULL where NOT NULL required
            .expect_failure("NOT NULL constraint failed"),
        );

        transcript.add_invariant(format!("SELECT COUNT(*) FROM {}", table_name), "0");

        transcript
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transcript_value_from_types() {
        let v: TranscriptValue = 42i64.into();
        assert_eq!(v, TranscriptValue::Integer(42));

        let v: TranscriptValue = 3.14f64.into();
        if let TranscriptValue::Float(f) = v {
            assert!((f - 3.14).abs() < f64::EPSILON);
        } else {
            panic!("Expected Float");
        }

        let v: TranscriptValue = "hello".into();
        assert_eq!(v, TranscriptValue::Text("hello".to_string()));

        let v: TranscriptValue = None::<i64>.into();
        assert_eq!(v, TranscriptValue::Null);
    }

    #[test]
    fn test_expected_result() {
        let success = ExpectedResult::success(5);
        assert!(!success.is_failure());

        let failure = ExpectedResult::failure("constraint");
        assert!(failure.is_failure());
    }

    #[test]
    fn test_transcript_statement() {
        let stmt = TranscriptStatement::new("INSERT INTO users VALUES (?)")
            .with_param(1i64)
            .with_param("Alice")
            .expect_success(1)
            .with_comment("Test insert");

        assert_eq!(stmt.sql, "INSERT INTO users VALUES (?)");
        assert_eq!(stmt.param_count(), 2);
        assert!(!stmt.expected.is_failure());
        assert_eq!(stmt.comment, Some("Test insert".to_string()));
    }

    #[test]
    fn test_database_invariant() {
        let inv = DatabaseInvariant::new("SELECT COUNT(*) FROM users", "0")
            .with_description("No users should exist");

        assert_eq!(inv.query, "SELECT COUNT(*) FROM users");
        assert_eq!(inv.expected_value, "0");
        assert_eq!(inv.description, Some("No users should exist".to_string()));
    }

    #[test]
    fn test_transaction_transcript() {
        let mut transcript =
            TransactionTranscript::new("test_transcript").with_description("Test description");

        transcript.add_statement(TranscriptStatement::new("SELECT 1").expect_success_any());
        transcript.add_statement(TranscriptStatement::new("BAD SQL").expect_failure("syntax"));
        transcript.add_invariant("SELECT 1", "1");
        transcript.set_metadata("version", "1.0");

        assert_eq!(transcript.name, "test_transcript");
        assert_eq!(transcript.statement_count(), 2);
        assert!(transcript.expects_rollback());
        assert_eq!(transcript.first_failure_index(), Some(1));
        assert_eq!(transcript.metadata.get("version"), Some(&"1.0".to_string()));
    }

    #[test]
    fn test_replay_result() {
        let mut result = ReplayResult::success("test");
        assert!(result.success);

        result.add_statement_result(StatementReplayResult::mismatched(
            0,
            ExpectedResult::success(1),
            false,
            None,
            Some("error".to_string()),
            "Expected success but got failure",
        ));

        assert!(!result.success);
        assert!(result.summary().contains("FAILED"));
    }

    #[test]
    fn test_statement_replay_result() {
        let matched =
            StatementReplayResult::matched(0, ExpectedResult::success(1), true, Some(1), None);
        assert!(matched.matched_expectation);

        let mismatched = StatementReplayResult::mismatched(
            1,
            ExpectedResult::success(1),
            false,
            None,
            Some("error".to_string()),
            "Row count mismatch",
        );
        assert!(!mismatched.matched_expectation);
    }

    #[test]
    fn test_invariant_result() {
        let matched = InvariantResult::matched("SELECT 1", "1");
        assert!(matched.matched);

        let mismatched = InvariantResult::mismatched("SELECT COUNT(*)", "0", "5");
        assert!(!mismatched.matched);
    }

    #[test]
    fn test_golden_transcript_rollback() {
        let transcript = golden_transcripts::rollback_on_constraint_violation("test_table");

        assert!(transcript.expects_rollback());
        assert_eq!(transcript.statement_count(), 2);
        assert_eq!(transcript.first_failure_index(), Some(1));
        assert!(!transcript.invariants.is_empty());
    }

    #[test]
    fn test_golden_transcript_success() {
        let transcript = golden_transcripts::successful_batch("test_table");

        assert!(!transcript.expects_rollback());
        assert_eq!(transcript.statement_count(), 2);
        assert!(transcript.first_failure_index().is_none());
    }

    #[test]
    fn test_transcript_iter() {
        let mut transcript = TransactionTranscript::new("test");
        transcript.add_statement(TranscriptStatement::new("SELECT 1"));
        transcript.add_statement(TranscriptStatement::new("SELECT 2"));

        let sqls: Vec<&str> = transcript.iter().map(|s| s.sql.as_str()).collect();
        assert_eq!(sqls, vec!["SELECT 1", "SELECT 2"]);
    }
}
