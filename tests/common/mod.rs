use axum::Router;
use poker_blind_timer_server::{build_router, make_state};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::ConnectOptions;
use sqlx::SqlitePool;
use std::str::FromStr;

pub const TEST_API_KEY: &str = "test-secret-key";

pub async fn test_pool() -> SqlitePool {
    let opts = SqliteConnectOptions::from_str("sqlite::memory:")
        .unwrap()
        .foreign_keys(true)
        .disable_statement_logging();
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .expect("connect in-memory sqlite");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("apply migrations");
    pool
}

pub async fn test_app() -> (Router, SqlitePool) {
    let pool = test_pool().await;
    let state = make_state(pool.clone(), TEST_API_KEY);
    let router = build_router(state);
    (router, pool)
}
