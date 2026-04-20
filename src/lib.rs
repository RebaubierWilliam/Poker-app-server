pub mod auth;
pub mod error;
pub mod malettes;
pub mod state;
pub mod structures;

mod blind_timer;

use anyhow::Context;
use axum::{routing::get, Router};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{ConnectOptions, SqlitePool};
use std::str::FromStr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;

pub use state::AppState;

pub struct Config {
    pub database_url: String,
    pub api_key: String,
    pub port: u16,
}

pub fn read_config() -> anyhow::Result<Config> {
    let database_url = std::env::var("DATABASE_URL")
        .context("DATABASE_URL env var is required")?;
    let api_key = std::env::var("API_KEY")
        .context("API_KEY env var is required")?;
    if api_key.trim().is_empty() {
        anyhow::bail!("API_KEY must not be empty");
    }
    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    Ok(Config { database_url, api_key, port })
}

pub async fn build_pool(database_url: &str) -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .disable_statement_logging();
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await
        .context("connect sqlite")?;
    Ok(pool)
}

pub async fn run_migrations(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .context("apply migrations")?;
    Ok(())
}

pub fn build_router(state: AppState) -> Router {
    let authed = Router::new()
        .merge(malettes::router())
        .merge(structures::router())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::api_key_middleware,
        ));

    Router::new()
        .route("/health", get(|| async { "ok" }))
        .merge(authed)
        .layer(CorsLayer::permissive())
        .with_state(state)
}

pub fn make_state(pool: SqlitePool, api_key: impl Into<Arc<str>>) -> AppState {
    AppState { pool, api_key: api_key.into() }
}
