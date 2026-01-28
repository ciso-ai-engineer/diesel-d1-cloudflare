![Diesel D1](./diesel-d1-cloudflare.jpg)

A custom backend/connection for [Diesel](https://diesel.rs/) for [Cloudflare D1](https://developers.cloudflare.com/d1/), supporting both Cloudflare Workers (WASM) and standalone HTTP REST API usage.

## Features

- **WASM Binding** - Native D1 integration for Cloudflare Workers
- **HTTP REST API** - Use D1 from any Rust environment via the Cloudflare API
- **Transaction Support** - Begin, commit, and rollback operations with depth tracking
- **Concurrency Governance** - Semaphore-based query limiting for high-load scenarios
- **Statement Caching** - Best-effort LRU caching for frequently executed SQL
- **Batch Operations** - Prepared query reuse with atomic batch execution
- **Tracing Support** - Structured spans for observability and debugging
- **Full Diesel Compatibility** - Use Diesel's query DSL and type-safe operations

A production-grade extension of `diesel-d1` that enables **Cloudflare D1 access outside Workers via the HTTP REST API**, and adds **full transaction support with nested depth tracking**.

This crate allows you to use Cloudflare D1 as a first-class backend for Diesel and `diesel_async` in:

* Cloudflare Workers (WASM binding)
* Any Rust runtime (native, server, CLI, microservices) via D1 REST API

---

## ⚠️ Important: Pooling Semantics

**D1 Workers binding is NOT a socketed connection and cannot be pooled traditionally.**

The "pooling" abstractions in this crate are designed for:

- **WASM (Workers)**: A lightweight concurrency governor (semaphore-based) to limit simultaneous in-flight queries per isolate, preventing request amplification under load.
- **HTTP**: Transport-level pooling using a shared `reqwest::Client` for HTTP keep-alive and connection reuse, plus configurable request limits and timeouts.

The D1 REST API is rate-limited at the Cloudflare API layer and is generally intended for "administrative use" rather than high-throughput production workloads.

---

## ✨ Features

### 1. HTTP Backend (`http` feature)

Use D1 from any Rust environment through Cloudflare's REST API.

* `D1HttpConnection` powered by `reqwest`
* `D1HttpConfig` for account / database / token
* URL-based connection string:

```
d1://account_id:api_token@database_id
```

Example:

```rust
use diesel_d1::{D1HttpConfig, D1HttpConnection};
use diesel_async::AsyncConnection;

let config = D1HttpConfig::new("account_id", "database_id", "api_token");
let mut conn = D1HttpConnection::new(config);

// Or via URL
let mut conn = D1HttpConnection::establish("d1://account:token@database").await?;
```

---

### 2. Transaction Support (Nested, Async, Diesel-Compatible)

Full implementation of `diesel_async::TransactionManager`:

* `D1TransactionManager`
* `begin / commit / rollback`
* Nested transaction depth tracking
* Automatic savepoint handling
* Deterministic rollback on failure

Guarantees:

* Atomic multi-statement execution
* Proper isolation semantics
* Correct behavior across nested scopes

**Note:** D1's `batch()` API executes as a SQL transaction with automatic rollback on failure. If any statement fails, the entire batch is rolled back.

---

### 3. Concurrency Governance

Control simultaneous in-flight queries with `QueryConcurrencyPolicy`:

```rust
use diesel_d1::QueryConcurrencyPolicy;

// Create a policy that limits to 5 concurrent queries
let policy = QueryConcurrencyPolicy::new(5);

// Try to acquire a permit
if let Some(permit) = policy.try_acquire() {
    // Execute query while holding permit
    // Permit is released when dropped
}
```

For HTTP, use `HttpTransportPolicy` to configure transport-level settings:

```rust
use diesel_d1::HttpTransportPolicy;
use std::time::Duration;

let policy = HttpTransportPolicy::builder()
    .pool_idle_connections(20)  // Connection pool size for keep-alive reuse
    .request_timeout(Duration::from_secs(60))
    .retry_enabled(false)  // Off by default
    .build();

// Create a governed client with explicit concurrency limit (independent of pool size)
let (client, governor) = policy.create_governed_client(Some(5))?;

// Use governor to enforce true concurrency limits
if let Some(permit) = governor.try_acquire() {
    // Make request with client while holding permit
}
```

---

### 4. Statement Caching

Best-effort client-side caching for frequently executed SQL:

```rust
use diesel_d1::{StatementCache, StatementCacheConfig};

let config = StatementCacheConfig::builder()
    .max_entries(100)
    .max_bytes(16 * 1024)
    .enabled(true)
    .build();

let cache = StatementCache::new(config);

// Insert a statement
cache.insert("SELECT * FROM users WHERE id = ?", 1);

// Look up the statement
if let Some(entry) = cache.get("SELECT * FROM users WHERE id = ?") {
    // Use cached statement metadata
}

// Check cache statistics
let stats = cache.stats();
println!("Hit rate: {:.1}%", stats.hit_rate() * 100.0);
```

**Note:** Caching is best-effort and may reset on isolate eviction (WASM) or process restart.

---

### 5. Batch Operations & Prepared Query Reuse

Build batches of statements with prepared query reuse:

```rust
use diesel_d1::{BatchBuilder, BoundValue};

let mut batch = BatchBuilder::new();

// Add multiple statements with the same SQL template
let sql = "INSERT INTO users (name, email) VALUES (?, ?)";
batch.add_statement(sql, vec!["Alice".into(), "alice@example.com".into()]);
batch.add_statement(sql, vec!["Bob".into(), "bob@example.com".into()]);
batch.add_statement(sql, vec!["Charlie".into(), "charlie@example.com".into()]);

// Check reuse statistics
let stats = batch.reuse_stats();
println!("Reuse rate: {:.1}%", stats.reuse_percentage() * 100.0);
```

---

### 6. Tracing & Observability

Structured spans for query correlation and debugging:

```rust
use diesel_d1::{D1Span, SpanOperation, ErrorClass};

// Create a span for a query execution
let mut span = D1Span::new(SpanOperation::Execute)
    .with_sql("SELECT * FROM users")
    .with_backend_http();

// Record the result
span.record_success(10, std::time::Duration::from_millis(50));

// Or record an error
span.record_error(ErrorClass::SqlError, "Constraint violation");
```

Span fields include:
- `backend`: "wasm" or "http"
- `operation`: "prepare", "exec", or "batch"
- `rows_read` / `rows_written`
- `duration`
- `error_class`: auth, rate_limit, sql_error, decode, timeout, network

---

### 7. Deterministic Replay for Transaction Testing

Test rollback behavior with transaction transcripts:

```rust
use diesel_d1::{TransactionTranscript, TranscriptStatement, ExpectedResult};

let mut transcript = TransactionTranscript::new("test_rollback");

// Add a statement that should succeed
transcript.add_statement(
    TranscriptStatement::new("INSERT INTO users (name) VALUES (?)")
        .with_param("Alice")
        .expect_success(1),
);

// Add a statement that should fail (triggering rollback)
transcript.add_statement(
    TranscriptStatement::new("INSERT INTO users (id, name) VALUES (?, ?)")
        .with_param(1)
        .with_param("Bob")
        .expect_failure("UNIQUE constraint"),
);

// Add invariant: no rows should exist due to rollback
transcript.add_invariant("SELECT COUNT(*) FROM users", "0");

assert!(transcript.expects_rollback());
```

---

### 8. Feature Flags

```toml
# Cloudflare Workers (WASM binding)
diesel-d1 = { version = "0.1", features = ["wasm"] }

# Native / Server / CLI via D1 REST API
diesel-d1 = { version = "0.1", features = ["http"] }
```

Switching backends requires no query-level changes.

---

### 9. Testing & CI

* 120+ unit and integration tests
* Coverage across:

  * SQL type mapping
  * Query builder
  * HTTP transport
  * Transaction manager
  * Concurrency governance
  * Statement caching
  * Batch operations
  * Tracing support
  * Replay testing
  * Error propagation
* GitHub Actions CI matrix:

  * `wasm` feature (Workers runtime)
  * `http` feature (native async runtime)

Planned test environment support:

* Wrangler + Miniflare (local Workers simulation)
* Cloudflare Workers Vitest runtime
* Native REST API integration tests

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
diesel-d1 = { version = "0.1", features = ["wasm"] }  # For Cloudflare Workers
# OR
diesel-d1 = { version = "0.1", features = ["http"] }  # For HTTP REST API
```

## Usage

### WASM Backend (Cloudflare Workers)

Use this when building Cloudflare Workers that need to interact with D1:

```rust
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use diesel_d1::D1Connection;

// In your Worker handler
#[worker::event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    // Create connection from the D1 binding
    let mut conn = D1Connection::new(env, "MY_DATABASE");
    
    // Use Diesel queries
    let users = users::table.load::<User>(&mut conn).await?;
    
    // ...
}
```

### HTTP Backend (REST API)

Use this when building server-side applications that need to interact with D1 remotely:

```rust
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use diesel_d1::{D1HttpConfig, D1HttpConnection};

#[tokio::main]
async fn main() {
    // Create configuration
    let config = D1HttpConfig::new(
        "your-account-id",
        "your-database-id", 
        "your-api-token",
    );
    
    // Create connection
    let mut conn = D1HttpConnection::new(config);
    
    // Use Diesel queries
    let users = users::table.load::<User>(&mut conn).await.unwrap();
}
```

You can also use the connection URL format:

```rust
use diesel_async::AsyncConnection;
use diesel_d1::D1HttpConnection;

let conn = D1HttpConnection::establish("d1://account_id:api_token@database_id").await?;
```

## Transactions

Transactions are supported through Diesel's standard transaction API:

```rust
use diesel_async::AsyncConnection;

conn.transaction(|conn| async move {
    diesel::insert_into(users::table)
        .values(&new_user)
        .execute(conn)
        .await?;
    
    diesel::update(accounts::table.find(account_id))
        .set(accounts::balance.eq(accounts::balance - amount))
        .execute(conn)
        .await?;
    
    Ok(())
}).await?;
```

> **Note:** D1 uses SQLite under the hood, and transactions are emulated using the `batch()` API which executes statements atomically.

## Configuration

### WASM Configuration

In your `wrangler.toml`:

```toml
[[d1_databases]]
binding = "MY_DATABASE"
database_name = "my-database"
database_id = "your-database-id"
```

### HTTP Configuration

Set environment variables or use the `D1HttpConfig` builder:

```rust
use diesel_d1::D1HttpConfig;

let config = D1HttpConfig::new(
    std::env::var("CF_ACCOUNT_ID").unwrap(),
    std::env::var("CF_DATABASE_ID").unwrap(),
    std::env::var("CF_API_TOKEN").unwrap(),
);

// Optional: Use a custom base URL (e.g., for testing)
let config = config.with_base_url("http://localhost:8080");
```

## Examples

See the `examples/` directory for complete examples:

- `examples/wasm_example.rs` - Cloudflare Workers example
- `examples/http_example.rs` - HTTP REST API example

## Supported SQL Types

| Diesel Type | D1/SQLite Type |
|-------------|----------------|
| `Bool` | INTEGER (0/1) |
| `SmallInt` | INTEGER |
| `Integer` | INTEGER |
| `BigInt` | INTEGER |
| `Float` | REAL |
| `Double` | REAL |
| `Text` | TEXT |
| `Binary` | BLOB |
| `Date` | TEXT |
| `Time` | TEXT |
| `Timestamp` | TEXT |

## Compatibility

- **Rust**: 1.83+
- **Diesel**: 2.2.x
- **diesel-async**: 0.5.x

## Roadmap

- [x] Statement caching
- [x] Prepared query reuse
- [x] Tracing + OpenTelemetry spans
- [x] Deterministic replay for transaction testing
- [ ] Cloudflare Zero Trust auth integration

## TO-DO List

- [x] Proper transaction support
- [x] HTTP API backend
- [x] Concurrency governance
- [x] Statement caching
- [x] Batch operations
- [x] Tracing support
- [ ] Make it more SQLite compatible (RETURNING clause)
- [ ] Durable Object sync SQLite support

## License

MIT License - see [LICENSE](LICENSE) for details.
