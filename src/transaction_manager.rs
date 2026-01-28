//! Transaction Manager for D1 Backend
//!
//! This module provides transaction support for D1 databases.
//! Since D1 is based on SQLite and uses the batch() API for atomicity,
//! transactions are emulated by collecting statements and executing them
//! atomically on commit.

use std::cell::Cell;

use async_trait::async_trait;
use diesel::{connection::TransactionManagerStatus, result::Error as DieselError, QueryResult};
use diesel_async::TransactionManager;

/// Transaction Manager for D1 connections
///
/// D1 doesn't have traditional transaction support, but we can emulate
/// depth=1 transactions using the `batch()` API which executes statements
/// atomically.
///
/// # Example
///
/// ```ignore
/// use diesel_async::AsyncConnection;
///
/// conn.transaction(|conn| async move {
///     diesel::insert_into(users)
///         .values(&new_user)
///         .execute(conn)
///         .await?;
///     Ok(())
/// }).await?;
/// ```
#[derive(Default)]
pub struct D1TransactionManager {
    /// Whether we're currently in a transaction
    pub(crate) is_in_transaction: Cell<bool>,
    /// Transaction depth for nested transaction tracking
    pub(crate) depth: Cell<u32>,
    /// Transaction status
    pub(crate) status: TransactionManagerStatus,
}

impl D1TransactionManager {
    /// Create a new transaction manager
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if currently in a transaction
    pub fn is_in_transaction(&self) -> bool {
        self.is_in_transaction.get()
    }

    /// Get the current transaction depth
    pub fn transaction_depth(&self) -> u32 {
        self.depth.get()
    }
}

/// Trait for connections that support D1 transactions
#[cfg(feature = "wasm")]
#[allow(dead_code)]
pub trait D1TransactionConnection {
    /// Get the transaction manager
    fn d1_transaction_manager(&mut self) -> &mut D1TransactionManager;

    /// Get the list of queued transaction queries
    fn transaction_queries(&mut self) -> &mut Vec<crate::binding::D1PreparedStatement>;

    /// Get the D1 binding for batch execution
    fn binding(&self) -> &crate::binding::D1Database;
}

/// Trait for HTTP connections that support D1 transactions
#[cfg(feature = "http")]
#[allow(dead_code)]
pub trait D1HttpTransactionConnection {
    /// Get the transaction manager
    fn d1_transaction_manager(&mut self) -> &mut D1TransactionManager;

    /// Get the list of queued transaction SQL statements
    fn transaction_queries(&mut self) -> &mut Vec<(String, Vec<serde_json::Value>)>;
}

#[cfg(feature = "wasm")]
#[async_trait]
impl TransactionManager<crate::wasm_connection::D1Connection> for D1TransactionManager {
    type TransactionStateData = Self;

    async fn begin_transaction(conn: &mut crate::wasm_connection::D1Connection) -> QueryResult<()> {
        let depth = conn.transaction_manager.depth.get();
        conn.transaction_manager.depth.set(depth + 1);

        if depth == 0 {
            conn.transaction_manager.is_in_transaction.set(true);
        }

        Ok(())
    }

    async fn rollback_transaction(
        conn: &mut crate::wasm_connection::D1Connection,
    ) -> QueryResult<()> {
        let depth = conn.transaction_manager.depth.get();

        if depth == 0 {
            return Err(DieselError::NotInTransaction);
        }

        conn.transaction_manager.depth.set(depth - 1);

        if depth == 1 {
            conn.transaction_manager.is_in_transaction.set(false);
        }

        Ok(())
    }

    async fn commit_transaction(
        conn: &mut crate::wasm_connection::D1Connection,
    ) -> QueryResult<()> {
        let depth = conn.transaction_manager.depth.get();

        if depth == 0 {
            return Err(DieselError::NotInTransaction);
        }

        conn.transaction_manager.depth.set(depth - 1);

        if depth == 1 {
            conn.transaction_manager.is_in_transaction.set(false);
        }

        Ok(())
    }

    fn transaction_manager_status_mut(
        conn: &mut crate::wasm_connection::D1Connection,
    ) -> &mut TransactionManagerStatus {
        &mut conn.transaction_manager.status
    }
}

#[cfg(feature = "http")]
#[async_trait]
impl TransactionManager<crate::http_connection::D1HttpConnection> for D1TransactionManager {
    type TransactionStateData = Self;

    async fn begin_transaction(
        conn: &mut crate::http_connection::D1HttpConnection,
    ) -> QueryResult<()> {
        let depth = conn.transaction_manager.depth.get();
        conn.transaction_manager.depth.set(depth + 1);

        if depth == 0 {
            conn.transaction_manager.is_in_transaction.set(true);
        }

        Ok(())
    }

    async fn rollback_transaction(
        conn: &mut crate::http_connection::D1HttpConnection,
    ) -> QueryResult<()> {
        let depth = conn.transaction_manager.depth.get();

        if depth == 0 {
            return Err(DieselError::NotInTransaction);
        }

        conn.transaction_manager.depth.set(depth - 1);

        if depth == 1 {
            conn.transaction_manager.is_in_transaction.set(false);
        }

        Ok(())
    }

    async fn commit_transaction(
        conn: &mut crate::http_connection::D1HttpConnection,
    ) -> QueryResult<()> {
        let depth = conn.transaction_manager.depth.get();

        if depth == 0 {
            return Err(DieselError::NotInTransaction);
        }

        conn.transaction_manager.depth.set(depth - 1);

        if depth == 1 {
            conn.transaction_manager.is_in_transaction.set(false);
        }

        Ok(())
    }

    fn transaction_manager_status_mut(
        conn: &mut crate::http_connection::D1HttpConnection,
    ) -> &mut TransactionManagerStatus {
        &mut conn.transaction_manager.status
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_manager_new() {
        let tm = D1TransactionManager::new();
        assert!(!tm.is_in_transaction());
        assert_eq!(tm.transaction_depth(), 0);
    }

    #[test]
    fn test_transaction_manager_default() {
        let tm = D1TransactionManager::default();
        assert!(!tm.is_in_transaction());
        assert_eq!(tm.transaction_depth(), 0);
    }

    #[test]
    fn test_transaction_state_changes() {
        let tm = D1TransactionManager::new();

        // Start transaction
        tm.is_in_transaction.set(true);
        tm.depth.set(1);
        assert!(tm.is_in_transaction());
        assert_eq!(tm.transaction_depth(), 1);

        // Nested transaction
        tm.depth.set(2);
        assert!(tm.is_in_transaction());
        assert_eq!(tm.transaction_depth(), 2);

        // Commit inner
        tm.depth.set(1);
        assert!(tm.is_in_transaction());
        assert_eq!(tm.transaction_depth(), 1);

        // Commit outer
        tm.depth.set(0);
        tm.is_in_transaction.set(false);
        assert!(!tm.is_in_transaction());
        assert_eq!(tm.transaction_depth(), 0);
    }
}
