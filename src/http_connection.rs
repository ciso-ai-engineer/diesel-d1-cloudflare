//! HTTP-based D1 Connection for REST API
//!
//! This module provides the D1HttpConnection type that uses the Cloudflare D1
//! REST API to interact with D1 databases in non-Workers environments.

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
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::{
    backend::D1Backend, bind_collector::D1BindCollector, http_row::D1Row,
    query_builder::D1QueryBuilder, transaction_manager::D1TransactionManager, utils::D1Error,
};

/// Configuration for D1 HTTP API connection
///
/// # Example
///
/// ```
/// use diesel_d1::D1HttpConfig;
///
/// let config = D1HttpConfig::new(
///     "your-account-id",
///     "your-database-id",
///     "your-api-token",
/// );
/// ```
#[derive(Clone)]
pub struct D1HttpConfig {
    /// Cloudflare account ID
    pub account_id: String,
    /// D1 database ID
    pub database_id: String,
    /// API token with D1 permissions
    pub api_token: String,
    /// Base URL for the API (defaults to Cloudflare API)
    pub base_url: String,
}

impl D1HttpConfig {
    /// Create a new configuration with the required parameters
    pub fn new(
        account_id: impl Into<String>,
        database_id: impl Into<String>,
        api_token: impl Into<String>,
    ) -> Self {
        Self {
            account_id: account_id.into(),
            database_id: database_id.into(),
            api_token: api_token.into(),
            base_url: "https://api.cloudflare.com/client/v4".to_string(),
        }
    }

    /// Set a custom base URL (useful for testing)
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Build the query URL for this database
    fn query_url(&self) -> String {
        format!(
            "{}/accounts/{}/d1/database/{}/query",
            self.base_url, self.account_id, self.database_id
        )
    }
}

/// D1 HTTP API request body
#[derive(Serialize, Debug)]
struct D1QueryRequest {
    sql: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    params: Vec<serde_json::Value>,
}

/// D1 HTTP API response
#[derive(Deserialize, Debug)]
struct D1ApiResponse {
    success: bool,
    errors: Vec<D1ApiError>,
    result: Option<Vec<D1QueryResult>>,
}

/// D1 API error
#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct D1ApiError {
    code: i32,
    message: String,
}

/// D1 query result
#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct D1QueryResult {
    success: bool,
    results: Option<Vec<serde_json::Value>>,
    meta: Option<D1QueryMeta>,
}

/// D1 query metadata
#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct D1QueryMeta {
    changes: Option<i64>,
    duration: Option<f64>,
    rows_read: Option<i64>,
    rows_written: Option<i64>,
}

/// D1 Connection using the HTTP REST API
///
/// This connection type allows interacting with Cloudflare D1 from any environment
/// that can make HTTP requests, not just Cloudflare Workers.
///
/// # Example
///
/// ```ignore
/// use diesel_d1::{D1HttpConfig, D1HttpConnection};
///
/// let config = D1HttpConfig::new(
///     "account-id",
///     "database-id",
///     "api-token",
/// );
///
/// let conn = D1HttpConnection::new(config);
/// ```
pub struct D1HttpConnection {
    client: Client,
    /// Connection configuration
    pub(crate) config: D1HttpConfig,
    /// Transaction manager (public for TransactionManager trait access)
    pub(crate) transaction_manager: D1TransactionManager,
    /// Instrumentation for the connection
    instrumentation: Option<Box<dyn Instrumentation>>,
}

impl D1HttpConnection {
    /// Create a new HTTP connection with the given configuration
    pub fn new(config: D1HttpConfig) -> Self {
        Self {
            client: Client::new(),
            config,
            transaction_manager: D1TransactionManager::default(),
            instrumentation: None,
        }
    }

    /// Create a new HTTP connection with a custom reqwest client
    pub fn with_client(config: D1HttpConfig, client: Client) -> Self {
        Self {
            client,
            config,
            transaction_manager: D1TransactionManager::default(),
            instrumentation: None,
        }
    }

    /// Execute a query against the D1 HTTP API
    async fn execute_query(
        &self,
        sql: &str,
        params: Vec<serde_json::Value>,
    ) -> Result<D1QueryResult, diesel::result::Error> {
        let request = D1QueryRequest {
            sql: sql.to_string(),
            params,
        };

        let response = self
            .client
            .post(self.config.query_url())
            .header("Authorization", format!("Bearer {}", self.config.api_token))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                diesel::result::Error::DatabaseError(
                    diesel::result::DatabaseErrorKind::Unknown,
                    Box::new(D1Error::new(format!("HTTP request failed: {}", e))),
                )
            })?;

        let status = response.status();
        let body = response.text().await.map_err(|e| {
            diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::Unknown,
                Box::new(D1Error::new(format!("Failed to read response: {}", e))),
            )
        })?;

        if !status.is_success() {
            return Err(diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::Unknown,
                Box::new(D1Error::new(format!("HTTP error {}: {}", status, body))),
            ));
        }

        let api_response: D1ApiResponse = serde_json::from_str(&body).map_err(|e| {
            diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::Unknown,
                Box::new(D1Error::new(format!("Failed to parse response: {}", e))),
            )
        })?;

        if !api_response.success {
            let error_msg = api_response
                .errors
                .first()
                .map(|e| e.message.clone())
                .unwrap_or_else(|| "Unknown error".to_string());
            return Err(diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::Unknown,
                Box::new(D1Error::new(error_msg)),
            ));
        }

        api_response
            .result
            .and_then(|r| r.into_iter().next())
            .ok_or_else(|| {
                diesel::result::Error::DatabaseError(
                    diesel::result::DatabaseErrorKind::Unknown,
                    Box::new(D1Error::new("No result returned")),
                )
            })
    }
}

// SAFETY: The HTTP client and configuration are thread-safe
unsafe impl Send for D1HttpConnection {}
unsafe impl Sync for D1HttpConnection {}

#[async_trait]
impl SimpleAsyncConnection for D1HttpConnection {
    async fn batch_execute(&mut self, query: &str) -> diesel::QueryResult<()> {
        self.execute_query(query, vec![]).await?;
        Ok(())
    }
}

#[async_trait]
impl AsyncConnection for D1HttpConnection {
    type Backend = D1Backend;
    type TransactionManager = D1TransactionManager;
    type ExecuteFuture<'conn, 'query> = BoxFuture<'conn, QueryResult<usize>>;
    type LoadFuture<'conn, 'query> = BoxFuture<'conn, QueryResult<Self::Stream<'conn, 'query>>>;
    type Stream<'conn, 'query> = BoxStream<'conn, QueryResult<Self::Row<'conn, 'query>>>;
    type Row<'conn, 'query> = D1Row;

    async fn establish(database_url: &str) -> ConnectionResult<Self> {
        // Parse the database URL in format:
        // d1://account_id:api_token@database_id
        // Note: api_token should be percent-encoded if it contains '@' or ':'
        if database_url.starts_with("d1://") {
            let url_body = database_url.strip_prefix("d1://").unwrap();

            // Find the last '@' to split auth from database_id
            // This allows '@' characters in the token if percent-encoded
            let at_pos = url_body.rfind('@');
            if at_pos.is_none() {
                return Err(diesel::ConnectionError::BadConnection(
                    "Invalid D1 URL format. Expected: d1://account_id:api_token@database_id"
                        .to_string(),
                ));
            }

            let at_pos = at_pos.unwrap();
            let auth_part = &url_body[..at_pos];
            let database_id = &url_body[at_pos + 1..];

            // Find the first ':' to split account_id from api_token
            let colon_pos = auth_part.find(':');
            if colon_pos.is_none() {
                return Err(diesel::ConnectionError::BadConnection(
                    "Invalid D1 URL format. Expected: d1://account_id:api_token@database_id"
                        .to_string(),
                ));
            }

            let colon_pos = colon_pos.unwrap();
            let account_id = &auth_part[..colon_pos];
            let api_token_encoded = &auth_part[colon_pos + 1..];

            // Decode percent-encoded characters in api_token
            let api_token = percent_decode(api_token_encoded);

            // Validate that all required fields are non-empty
            if account_id.is_empty() {
                return Err(diesel::ConnectionError::BadConnection(
                    "account_id cannot be empty in D1 URL".to_string(),
                ));
            }
            if database_id.is_empty() {
                return Err(diesel::ConnectionError::BadConnection(
                    "database_id cannot be empty in D1 URL".to_string(),
                ));
            }
            if api_token.is_empty() {
                return Err(diesel::ConnectionError::BadConnection(
                    "api_token cannot be empty in D1 URL".to_string(),
                ));
            }

            let config = D1HttpConfig::new(account_id, database_id, &api_token);
            Ok(Self::new(config))
        } else {
            Err(diesel::ConnectionError::BadConnection(
                "D1 URL must start with 'd1://'".to_string(),
            ))
        }
    }

    fn load<'conn, 'query, T>(&'conn mut self, source: T) -> Self::LoadFuture<'conn, 'query>
    where
        T: AsQuery + 'query,
        T::Query: QueryFragment<Self::Backend> + QueryId + 'query,
    {
        let source = source.as_query();
        let (sql, params) = build_query_with_params(source);

        async move {
            let result = self.execute_query(&sql, params).await?;

            let results = result.results.unwrap_or_default();

            if results.is_empty() {
                return Ok(stream::iter(vec![]).boxed());
            }

            // Get field names from first result
            // Sort keys to ensure consistent field ordering regardless of JSON object iteration order
            let field_keys: Vec<String> = if let Some(first) = results.first() {
                if let Some(obj) = first.as_object() {
                    let mut keys: Vec<String> = obj.keys().cloned().collect();
                    keys.sort();
                    keys
                } else {
                    vec![]
                }
            } else {
                vec![]
            };

            let rows: Vec<QueryResult<D1Row>> = results
                .into_iter()
                .map(|val| Ok(D1Row::new(val, field_keys.clone())))
                .collect();

            Ok(stream::iter(rows).boxed())
        }
        .boxed()
    }

    fn execute_returning_count<'conn, 'query, T>(
        &'conn mut self,
        source: T,
    ) -> Self::ExecuteFuture<'conn, 'query>
    where
        T: QueryFragment<Self::Backend> + QueryId + 'query,
    {
        let (sql, params) = build_query_with_params(source);

        async move {
            let result = self.execute_query(&sql, params).await?;

            let changes = result.meta.and_then(|m| m.changes).unwrap_or(0);

            Ok(changes as usize)
        }
        .boxed()
    }

    fn transaction_state(&mut self) -> &mut D1TransactionManager {
        &mut self.transaction_manager
    }

    fn instrumentation(&mut self) -> &mut dyn Instrumentation {
        self.instrumentation
            .get_or_insert_with(|| Box::new(NoopInstrumentation))
            .as_mut()
    }

    fn set_instrumentation(&mut self, instrumentation: impl Instrumentation) {
        self.instrumentation = Some(Box::new(instrumentation));
    }
}

impl ConnectionSealed for D1HttpConnection {}

struct NoopInstrumentation;

impl Instrumentation for NoopInstrumentation {
    fn on_connection_event(&mut self, _event: diesel::connection::InstrumentationEvent<'_>) {
        // No-op
    }
}

/// Build SQL and parameters from a query
fn build_query_with_params<T>(source: T) -> (String, Vec<serde_json::Value>)
where
    T: QueryFragment<D1Backend>,
{
    let mut query_builder = D1QueryBuilder::default();
    source.to_sql(&mut query_builder, &D1Backend).unwrap();

    let mut bind_collector = D1BindCollector::default();
    source
        .collect_binds(&mut bind_collector, &mut (), &D1Backend)
        .unwrap();

    let params: Vec<serde_json::Value> = bind_collector
        .binds
        .iter()
        .map(|(bind, _)| bind.to_json_value())
        .collect();

    (query_builder.sql, params)
}

/// Simple percent-decode for URL parsing
/// Handles ASCII percent-encoding (sufficient for API tokens which are typically alphanumeric with some symbols)
fn percent_decode(input: &str) -> String {
    let mut bytes = Vec::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if hex.len() == 2 {
                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                    bytes.push(byte);
                    continue;
                }
            }
            // If decoding failed, keep original sequence as bytes
            bytes.push(b'%');
            bytes.extend(hex.as_bytes());
        } else {
            // For ASCII characters, this is safe; for UTF-8, push the bytes
            let mut buf = [0u8; 4];
            bytes.extend(c.encode_utf8(&mut buf).as_bytes());
        }
    }

    // Convert to string, replacing invalid UTF-8 sequences
    String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_d1_http_config_new() {
        let config = D1HttpConfig::new("account", "database", "token");
        assert_eq!(config.account_id, "account");
        assert_eq!(config.database_id, "database");
        assert_eq!(config.api_token, "token");
        assert!(config.base_url.contains("cloudflare.com"));
    }

    #[test]
    fn test_d1_http_config_query_url() {
        let config = D1HttpConfig::new("acc123", "db456", "token");
        let url = config.query_url();
        assert!(url.contains("acc123"));
        assert!(url.contains("db456"));
        assert!(url.ends_with("/query"));
    }

    #[test]
    fn test_d1_http_config_custom_base_url() {
        let config = D1HttpConfig::new("account", "database", "token")
            .with_base_url("http://localhost:8080");
        assert_eq!(config.base_url, "http://localhost:8080");
    }

    #[tokio::test]
    async fn test_establish_valid_url() {
        let result = D1HttpConnection::establish("d1://account:token@database").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_establish_invalid_url() {
        let result = D1HttpConnection::establish("invalid://url").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_establish_malformed_url() {
        let result = D1HttpConnection::establish("d1://missing-at-sign").await;
        assert!(result.is_err());
    }

    #[test]
    fn test_percent_decode_simple() {
        assert_eq!(percent_decode("hello"), "hello");
        assert_eq!(percent_decode("hello%20world"), "hello world");
    }

    #[test]
    fn test_percent_decode_special_chars() {
        assert_eq!(percent_decode("token%40example"), "token@example");
        assert_eq!(percent_decode("a%3Ab"), "a:b");
    }

    #[test]
    fn test_percent_decode_edge_cases() {
        // Incomplete sequence at end
        assert_eq!(percent_decode("test%2"), "test%2");
        assert_eq!(percent_decode("test%"), "test%");

        // Invalid hex digits
        assert_eq!(percent_decode("test%GG"), "test%GG");
        assert_eq!(percent_decode("test%2G"), "test%2G");

        // Empty string
        assert_eq!(percent_decode(""), "");

        // Only percent sign
        assert_eq!(percent_decode("%"), "%");

        // UTF-8 sequences
        assert_eq!(percent_decode("%C3%A9"), "é");
    }

    #[tokio::test]
    async fn test_establish_url_with_encoded_token() {
        // Token with @ and : characters encoded
        let result =
            D1HttpConnection::establish("d1://account:token%40with%3Aspecial@database").await;
        assert!(result.is_ok());
        let conn = result.unwrap();
        assert_eq!(conn.config.api_token, "token@with:special");
    }

    #[tokio::test]
    async fn test_establish_url_with_empty_fields() {
        // Empty account_id
        let result = D1HttpConnection::establish("d1://:token@database").await;
        assert!(result.is_err());

        // Empty database_id
        let result = D1HttpConnection::establish("d1://account:token@").await;
        assert!(result.is_err());

        // Empty api_token
        let result = D1HttpConnection::establish("d1://account:@database").await;
        assert!(result.is_err());
    }
}
