use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

use crate::state::AppState;

pub async fn api_key_middleware(
    State(_state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    next.run(req).await
}
