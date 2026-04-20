use std::net::SocketAddr;

use poker_blind_timer_server::{build_pool, build_router, make_state, read_config, run_migrations};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cfg = read_config()?;
    let pool = build_pool(&cfg.database_url).await?;
    run_migrations(&pool).await?;

    let state = make_state(pool, cfg.api_key);
    let router = build_router(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.port));
    tracing::info!("listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;
    Ok(())
}
