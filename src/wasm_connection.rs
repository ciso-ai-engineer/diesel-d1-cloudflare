//! WASM-based D1 Connection for Cloudflare Workers
//!
//! This module provides the D1Connection type that uses the WASM bindings
//! to interact with Cloudflare D1 in Workers environments.

use async_trait::async_trait;
use diesel::{
    connection::{ConnectionSealed, Instrumentation},
    query_builder::{AsQuery, QueryFragment, QueryId},
    ConnectionResult, QueryResult,
};
use diesel_async::{AsyncConnection, SimpleAsyncConnection};
use futures_util::{
    future::BoxFuture,
    stream::{self, BoxStream},
    FutureExt, StreamExt,
};
use js_sys::{Array, Object, Reflect};
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use worker::console_error;

use crate::{
    backend::D1Backend,
    bind_collector::D1BindCollector,
    binding::{D1Database, D1PreparedStatement, D1Result},
    query_builder::D1QueryBuilder,
    row::D1Row,
    transaction_manager::D1TransactionManager,
    utils::{D1Error, SendableFuture},
};

/// D1 Connection for WASM/Cloudflare Workers environment.
///
/// This connection type uses the native D1 JavaScript bindings
/// available in the Workers runtime.
///
/// # Example
///
/// ```ignore
/// use diesel_d1::D1Connection;
///
/// let conn = D1Connection::new(env, "MY_DATABASE");
/// ```
pub struct D1Connection {
    #[allow(dead_code)]
    transaction_queries: Vec<D1PreparedStatement>,
    /// Transaction manager (public for TransactionManager trait access)
    pub(crate) transaction_manager: D1TransactionManager,
    binding: D1Database,
}

impl D1Connection {
    /// Create a new D1 connection from a Workers environment.
    ///
    /// # Arguments
    ///
    /// * `env` - The Workers environment containing the D1 binding
    /// * `name` - The name of the D1 database binding
    pub fn new(env: worker::Env, name: &str) -> Self {
        let binding: D1Database = Reflect::get(&env, &name.to_owned().into()).unwrap().into();
        D1Connection {
            transaction_queries: Vec::default(),
            transaction_manager: D1TransactionManager::default(),
            binding,
        }
    }

    /// Get access to the underlying D1 binding
    pub fn binding(&self) -> &D1Database {
        &self.binding
    }
}

// SAFETY: this is safe under WASM and workers because there's no threads and therefore no race conditions (at least memory ones)
unsafe impl Send for D1Connection {}
unsafe impl Sync for D1Connection {}

#[async_trait]
impl SimpleAsyncConnection for D1Connection {
    async fn batch_execute(&mut self, query: &str) -> diesel::QueryResult<()> {
        let statements = [JsValue::from_str(query)].iter().collect::<Array>();

        match SendableFuture(JsFuture::from(self.binding.batch(statements).unwrap())).await {
            Ok(_) => Ok(()),
            Err(e) => {
                let error_msg = e.as_string().unwrap_or_else(|| "Unknown error".to_string());
                Err(diesel::result::Error::DatabaseError(
                    diesel::result::DatabaseErrorKind::Unknown,
                    Box::new(D1Error { message: error_msg }),
                ))
            }
        }
    }
}

#[async_trait]
impl AsyncConnection for D1Connection {
    type Backend = D1Backend;
    type TransactionManager = D1TransactionManager;
    type ExecuteFuture<'conn, 'query> = BoxFuture<'conn, QueryResult<usize>>;
    type LoadFuture<'conn, 'query> = BoxFuture<'conn, QueryResult<Self::Stream<'conn, 'query>>>;
    type Stream<'conn, 'query> = BoxStream<'conn, QueryResult<Self::Row<'conn, 'query>>>;
    type Row<'conn, 'query> = D1Row;

    async fn establish(_unused: &str) -> ConnectionResult<Self> {
        Err(diesel::ConnectionError::BadConnection(
            "Use D1Connection::new() with a Workers environment instead".to_string(),
        ))
    }

    fn load<'conn, 'query, T>(&'conn mut self, source: T) -> Self::LoadFuture<'conn, 'query>
    where
        T: AsQuery + 'query,
        T::Query: QueryFragment<Self::Backend> + QueryId + 'query,
    {
        let source = source.as_query();
        let result = prepare_statement_sql(source, &self.binding);

        SendableFuture(async move {
            let promise = match result.all() {
                Ok(res) => res,
                Err(err) => {
                    console_error!("{:?}", err);
                    return Err(diesel::result::Error::DatabaseError(
                        diesel::result::DatabaseErrorKind::Unknown,
                        Box::new(D1Error {
                            message: "Failed to execute query".to_string(),
                        }),
                    ));
                }
            };

            let result = match SendableFuture(JsFuture::from(promise)).await {
                Ok(res) => res,
                Err(err) => {
                    console_error!("{:?}", err);
                    return Err(diesel::result::Error::DatabaseError(
                        diesel::result::DatabaseErrorKind::Unknown,
                        Box::new(D1Error {
                            message: "Query execution failed".to_string(),
                        }),
                    ));
                }
            };

            let result: D1Result = result.into();

            let error = result.error().unwrap();

            if let Some(error_str) = error {
                return Err(diesel::result::Error::DatabaseError(
                    diesel::result::DatabaseErrorKind::Unknown,
                    Box::new(D1Error { message: error_str }),
                ));
            }

            let array = result.results().unwrap().unwrap().to_vec();

            if array.is_empty() {
                return Ok(stream::iter(vec![]).boxed());
            }

            let field_keys: Vec<String> = js_sys::Object::keys(&Object::from(array[0].clone()))
                .to_vec()
                .iter()
                .map(|val| val.as_string().unwrap())
                .collect();

            let rows: Vec<QueryResult<D1Row>> = array
                .iter()
                .map(|val| Ok(D1Row::new(val.clone(), field_keys.clone())))
                .collect();
            let iter = stream::iter(rows).boxed();
            Ok(iter)
        })
        .boxed()
    }

    fn execute_returning_count<'conn, 'query, T>(
        &'conn mut self,
        source: T,
    ) -> Self::ExecuteFuture<'conn, 'query>
    where
        T: QueryFragment<Self::Backend> + QueryId + 'query,
    {
        let result = prepare_statement_sql(source, &self.binding);
        SendableFuture(async move {
            let promise = match result.all() {
                Ok(res) => res,
                Err(err) => {
                    console_error!("{:?}", err);
                    return Err(diesel::result::Error::DatabaseError(
                        diesel::result::DatabaseErrorKind::Unknown,
                        Box::new(D1Error {
                            message: "Failed to execute query".to_string(),
                        }),
                    ));
                }
            };

            let result = match SendableFuture(JsFuture::from(promise)).await {
                Ok(res) => res,
                Err(err) => {
                    console_error!("{:?}", err);
                    return Err(diesel::result::Error::DatabaseError(
                        diesel::result::DatabaseErrorKind::Unknown,
                        Box::new(D1Error {
                            message: "Query execution failed".to_string(),
                        }),
                    ));
                }
            };

            let result: D1Result = result.into();

            let error = result.error().unwrap();

            if let Some(error_str) = error {
                return Err(diesel::result::Error::DatabaseError(
                    diesel::result::DatabaseErrorKind::Unknown,
                    Box::new(D1Error { message: error_str }),
                ));
            }

            let meta = result.meta().unwrap();
            let value = js_sys::Reflect::get(&meta, &"changes".to_owned().into())
                .unwrap()
                .as_f64()
                .unwrap();

            Ok(value as usize)
        })
        .boxed()
    }

    fn transaction_state(&mut self) -> &mut D1TransactionManager {
        &mut self.transaction_manager
    }

    #[allow(static_mut_refs)]
    fn instrumentation(&mut self) -> &mut dyn Instrumentation {
        // Return a no-op instrumentation
        static mut NOOP: NoopInstrumentation = NoopInstrumentation;
        unsafe { &mut NOOP }
    }

    fn set_instrumentation(&mut self, _instrumentation: impl Instrumentation) {
        // No-op for now
    }
}

impl ConnectionSealed for D1Connection {}

struct NoopInstrumentation;

impl Instrumentation for NoopInstrumentation {
    fn on_connection_event(&mut self, _event: diesel::connection::InstrumentationEvent<'_>) {
        // No-op
    }
}

fn construct_bind_data<T>(query: &T) -> Result<Array, diesel::result::Error>
where
    T: QueryFragment<D1Backend>,
{
    let mut bind_collector = D1BindCollector::default();

    query.collect_binds(&mut bind_collector, &mut (), &D1Backend)?;

    let array = bind_collector
        .binds
        .iter()
        .map(|(bind, _)| bind.to_js_value())
        .collect::<Array>();
    Ok(array)
}

fn prepare_statement_sql<'conn, 'query, T>(source: T, binding: &D1Database) -> D1PreparedStatement
where
    T: QueryFragment<D1Backend> + QueryId + 'query,
{
    let mut query_builder = D1QueryBuilder::default();
    source.to_sql(&mut query_builder, &D1Backend).unwrap();
    let result = match binding.prepare(&query_builder.sql) {
        Ok(res) => res,
        Err(err) => {
            console_error!("{:?}", err);
            panic!("Failed to prepare statement");
        }
    };

    let binds = construct_bind_data(&source).unwrap();

    match result.bind(binds) {
        Ok(res) => res,
        Err(err) => {
            console_error!("{:?}", err);
            panic!("Failed to bind parameters");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noop_instrumentation() {
        let mut noop = NoopInstrumentation;
        // Just verify it doesn't panic
        noop.on_connection_event(diesel::connection::InstrumentationEvent::StartQuery {
            query: &diesel::debug_query::<D1Backend, _>(&diesel::sql_query("SELECT 1")),
        });
    }
}
