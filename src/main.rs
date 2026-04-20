mod blind_timer;

use axum::{extract::Json, http::StatusCode, routing::{get, post}, Router};
use blind_timer::{compute_structure, TournamentInput, TournamentStructure};
use tower_http::cors::CorsLayer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/structure", post(structure_handler))
        .layer(CorsLayer::permissive());

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn structure_handler(
    Json(input): Json<TournamentInput>,
) -> Result<Json<TournamentStructure>, (StatusCode, String)> {
    if input.players < 2 {
        return Err((StatusCode::BAD_REQUEST, "au moins 2 joueurs".into()));
    }
    if input.total_duration_minutes == 0 {
        return Err((StatusCode::BAD_REQUEST, "durée invalide".into()));
    }
    if input.case_chips.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "malette vide".into()));
    }
    Ok(Json(compute_structure(&input)))
}
