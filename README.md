![Diesel D1](./diesel-d1-cloudflare.jpg)

A custom backend/connection for [Diesel](https://diesel.rs/) for [Cloudflare D1](https://developers.cloudflare.com/d1/), supporting both Cloudflare Workers (WASM) and standalone HTTP REST API usage.

## Features

- **WASM Binding** - Native D1 integration for Cloudflare Workers
- **HTTP REST API** - Use D1 from any Rust environment via the Cloudflare API
- **Transaction Support** - Begin, commit, and rollback operations with depth tracking
- **Full Diesel Compatibility** - Use Diesel's query DSL and type-safe operations

A production-grade extension of `diesel-d1` that enables **Cloudflare D1 access outside Workers via the HTTP REST API**, and adds **full transaction support with nested depth tracking**.

This crate allows you to use Cloudflare D1 as a first-class backend for Diesel and `diesel_async` in:

* Cloudflare Workers (WASM binding)
* Any Rust runtime (native, server, CLI, microservices) via D1 REST API

---

## ✨ Features

### 1. HTTP Backend (`http` feature)

Use D1 from any Rust environment through Cloudflare’s REST API.

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

---

### 3. Feature Flags

```toml
# Cloudflare Workers (WASM binding)
diesel-d1 = { version = "0.1", features = ["wasm"] }

# Native / Server / CLI via D1 REST API
diesel-d1 = { version = "0.1", features = ["http"] }
```

Switching backends requires no query-level changes.

---

### 4. Testing & CI

* 48+ unit and integration tests
* Coverage across:

  * SQL type mapping
  * Query builder
  * HTTP transport
  * Transaction manager
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

* Connection pooling
* Statement caching
* Prepared query reuse
* Tracing + OpenTelemetry spans
* Deterministic replay for transaction testing
* Cloudflare Zero Trust auth integration

## TO-DO List

- [x] Proper transaction support
- [x] HTTP API backend
- [ ] Make it more SQLite compatible (RETURNING clause)
- [ ] Durable Object sync SQLite support

## License

MIT License - see [LICENSE](LICENSE) for details.

