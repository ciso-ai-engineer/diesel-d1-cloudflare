# D1 Diesel Examples

This directory contains examples for using the diesel-d1 crate.

## HTTP REST API Example

The `http_example.rs` demonstrates using D1 via the Cloudflare HTTP REST API.

### Prerequisites

1. Create a D1 database in the Cloudflare dashboard
2. Create an API token with D1 permissions
3. Set environment variables:
   - `CF_ACCOUNT_ID`: Your Cloudflare account ID
   - `CF_DATABASE_ID`: Your D1 database ID
   - `CF_API_TOKEN`: Your Cloudflare API token

### Running

```bash
cargo run --example http_example --features http
```

## WASM/Workers Example

For Cloudflare Workers, see the documentation in the main README and the code example below.

### Example Code for Workers

```rust
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use diesel_d1::D1Connection;
use worker::{event, Context, Env, Request, Response, Result};

// Define your schema
table! {
    users (id) {
        id -> Integer,
        name -> Text,
        email -> Text,
    }
}

#[derive(Queryable)]
struct User {
    id: i32,
    name: String,
    email: String,
}

#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    // Create connection from the D1 binding
    let mut conn = D1Connection::new(env, "MY_DATABASE");
    
    // Query users
    let users = users::table.load::<User>(&mut conn).await
        .map_err(|e| worker::Error::from(format!("{:?}", e)))?;
    
    Response::ok(format!("Found {} users", users.len()))
}
```

### wrangler.toml

```toml
name = "my-worker"
main = "src/lib.rs"
compatibility_date = "2024-01-01"

[[d1_databases]]
binding = "MY_DATABASE"
database_name = "my-database"
database_id = "your-database-id"
```

### Cargo.toml

```toml
[dependencies]
diesel = { version = "2.2", default-features = false }
diesel-async = "0.5"
diesel-d1 = { version = "0.1", features = ["wasm"] }
worker = "0.4"
```
