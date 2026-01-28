//! D1 Backend implementation for Diesel
//!
//! This module provides the backend definition that works with both WASM and HTTP backends.

use diesel::{
    backend::{
        sql_dialect::{self, returning_clause::DoesNotSupportReturningClause},
        Backend, DieselReserveSpecialization, SqlDialect, TrustedBackend,
    },
    sql_types::TypeMetadata,
};

use crate::{bind_collector::D1BindCollector, query_builder::D1QueryBuilder};

#[cfg(feature = "wasm")]
use crate::value::D1Value;

#[cfg(feature = "http")]
use crate::http_value::D1Value;

// When neither feature is enabled, provide a placeholder type
#[cfg(not(any(feature = "wasm", feature = "http")))]
pub struct D1Value;

/// The D1 backend for Diesel
///
/// This backend is compatible with Cloudflare D1 which uses SQLite under the hood.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, Default)]
pub struct D1Backend;

/// D1 data types
///
/// These correspond to the SQLite type affinities used by D1.
#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub enum D1Type {
    /// Binary/blob data
    Binary,
    /// Text/string data
    Text,
    /// Double precision floating point
    Double,
    /// Integer (64-bit)
    Integer,
}

impl Backend for D1Backend {
    type QueryBuilder = D1QueryBuilder;
    type RawValue<'a> = D1Value;
    type BindCollector<'a> = D1BindCollector;
}

impl TypeMetadata for D1Backend {
    type TypeMetadata = D1Type;
    type MetadataLookup = ();
}

impl SqlDialect for D1Backend {
    type ReturningClause = DoesNotSupportReturningClause;
    type OnConflictClause = SqliteOnConflictClause;
    type InsertWithDefaultKeyword =
        sql_dialect::default_keyword_for_insert::DoesNotSupportDefaultKeyword;
    type BatchInsertSupport = SqliteBatchInsert;
    type ConcatClause = sql_dialect::concat_clause::ConcatWithPipesClause;
    type DefaultValueClauseForInsert = sql_dialect::default_value_clause::AnsiDefaultValueClause;
    type EmptyFromClauseSyntax = sql_dialect::from_clause_syntax::AnsiSqlFromClauseSyntax;
    type SelectStatementSyntax = sql_dialect::select_statement_syntax::AnsiSqlSelectStatement;
    type ExistsSyntax = sql_dialect::exists_syntax::AnsiSqlExistsSyntax;
    type ArrayComparison = sql_dialect::array_comparison::AnsiSqlArrayComparison;
    type AliasSyntax = sql_dialect::alias_syntax::AsAliasSyntax;
}

impl DieselReserveSpecialization for D1Backend {}
impl TrustedBackend for D1Backend {}

/// SQLite-compatible ON CONFLICT clause support
#[derive(Debug, Copy, Clone)]
pub struct SqliteOnConflictClause;

impl sql_dialect::on_conflict_clause::SupportsOnConflictClause for SqliteOnConflictClause {}
impl sql_dialect::on_conflict_clause::PgLikeOnConflictClause for SqliteOnConflictClause {}

/// SQLite-compatible batch insert support
#[derive(Debug, Copy, Clone)]
pub struct SqliteBatchInsert;

/// SQLite-compatible RETURNING clause support (for future use)
#[derive(Debug, Copy, Clone)]
pub struct SqliteReturningClause;

impl sql_dialect::returning_clause::SupportsReturningClause for SqliteReturningClause {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_d1_type_equality() {
        assert_eq!(D1Type::Integer, D1Type::Integer);
        assert_ne!(D1Type::Integer, D1Type::Text);
    }

    #[test]
    fn test_d1_backend_default() {
        let _backend = D1Backend::default();
    }

    #[test]
    fn test_d1_type_debug() {
        let type_int = D1Type::Integer;
        assert_eq!(format!("{:?}", type_int), "Integer");
    }
}
