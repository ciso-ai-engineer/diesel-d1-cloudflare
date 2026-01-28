//! Example: Using Diesel D1 with HTTP REST API
//!
//! This example demonstrates how to use the diesel-d1 crate to interact
//! with Cloudflare D1 via the HTTP REST API in non-Workers environments.
//!
//! ## Prerequisites
//!
//! 1. Create a D1 database in the Cloudflare dashboard
//! 2. Create an API token with D1 permissions
//! 3. Set the required environment variables:
//!    - `CF_ACCOUNT_ID`: Your Cloudflare account ID
//!    - `CF_DATABASE_ID`: Your D1 database ID
//!    - `CF_API_TOKEN`: Your Cloudflare API token
//!
//! ## Running
//!
//! ```bash
//! cargo run --example http_example --features http
//! ```

use diesel_async::SimpleAsyncConnection;
use diesel_d1::{D1HttpConfig, D1HttpConnection};
use std::env;

#[tokio::main]
async fn main() {
    // Read configuration from environment variables
    let account_id = env::var("CF_ACCOUNT_ID").expect("CF_ACCOUNT_ID must be set");
    let database_id = env::var("CF_DATABASE_ID").expect("CF_DATABASE_ID must be set");
    let api_token = env::var("CF_API_TOKEN").expect("CF_API_TOKEN must be set");

    // Create configuration
    let config = D1HttpConfig::new(account_id, database_id, api_token);

    // Create connection
    let mut conn = D1HttpConnection::new(config);

    println!("Connected to D1 database via HTTP API");

    // Example: Create the users table (if it doesn't exist)
    let create_table_sql = r#"
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            email TEXT NOT NULL
        )
    "#;

    match conn.batch_execute(create_table_sql).await {
        Ok(_) => println!("Table created or already exists"),
        Err(e) => eprintln!("Error creating table: {:?}", e),
    }

    // Example: Insert a user
    let insert_sql = "INSERT INTO users (name, email) VALUES ('John Doe', 'john@example.com')";
    match conn.batch_execute(insert_sql).await {
        Ok(_) => println!("User inserted"),
        Err(e) => eprintln!("Error inserting user: {:?}", e),
    }

    // Example: Query users
    let select_sql = "SELECT * FROM users";
    match conn.batch_execute(select_sql).await {
        Ok(_) => println!("Query executed successfully"),
        Err(e) => eprintln!("Error querying users: {:?}", e),
    }

    println!("Example complete!");
    println!();
    println!("Note: This example uses batch_execute for simplicity.");
    println!("For type-safe queries with the Diesel DSL, define your schema");
    println!("with diesel::table! and use diesel_async::RunQueryDsl.");
}
