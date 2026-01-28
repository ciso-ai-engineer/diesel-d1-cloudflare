//! The D1 query builder
//!
//! This module provides the query builder implementation for the D1 backend,
//! which generates SQLite-compatible SQL.

use super::backend::D1Backend;
use diesel::query_builder::QueryBuilder;
use diesel::result::QueryResult;

mod limit_offset;
mod returning;

/// Constructs SQL queries for use with the D1 backend
///
/// This query builder generates SQLite-compatible SQL queries that can be
/// executed against Cloudflare D1.
#[derive(Default)]
pub struct D1QueryBuilder {
    /// The SQL string being built
    pub(crate) sql: String,
}

impl D1QueryBuilder {
    /// Construct a new query builder with an empty query
    pub fn new() -> Self {
        D1QueryBuilder::default()
    }

    /// Get the current SQL string
    pub fn sql(&self) -> &str {
        &self.sql
    }
}

impl QueryBuilder<D1Backend> for D1QueryBuilder {
    fn push_sql(&mut self, sql: &str) {
        self.sql.push_str(sql);
    }

    fn push_identifier(&mut self, identifier: &str) -> QueryResult<()> {
        self.push_sql("`");
        self.push_sql(&identifier.replace('`', "``"));
        self.push_sql("`");
        Ok(())
    }

    fn push_bind_param(&mut self) {
        self.push_sql("?");
    }

    fn finish(self) -> String {
        self.sql
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_builder_new() {
        let qb = D1QueryBuilder::new();
        assert!(qb.sql().is_empty());
    }

    #[test]
    fn test_push_sql() {
        let mut qb = D1QueryBuilder::new();
        qb.push_sql("SELECT * FROM users");
        assert_eq!(qb.sql(), "SELECT * FROM users");
    }

    #[test]
    fn test_push_identifier() {
        let mut qb = D1QueryBuilder::new();
        qb.push_identifier("user_name").unwrap();
        assert_eq!(qb.sql(), "`user_name`");
    }

    #[test]
    fn test_push_identifier_with_backticks() {
        let mut qb = D1QueryBuilder::new();
        qb.push_identifier("user`name").unwrap();
        assert_eq!(qb.sql(), "`user``name`");
    }

    #[test]
    fn test_push_bind_param() {
        let mut qb = D1QueryBuilder::new();
        qb.push_bind_param();
        assert_eq!(qb.sql(), "?");
    }

    #[test]
    fn test_finish() {
        let mut qb = D1QueryBuilder::new();
        qb.push_sql("SELECT ");
        qb.push_identifier("id").unwrap();
        qb.push_sql(" FROM users WHERE id = ");
        qb.push_bind_param();
        assert_eq!(qb.finish(), "SELECT `id` FROM users WHERE id = ?");
    }
}
