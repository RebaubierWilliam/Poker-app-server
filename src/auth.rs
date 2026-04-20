use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use subtle::ConstantTimeEq;

use crate::error::AppError;
use crate::state::AppState;

pub async fn api_key_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let header = req
        .headers()
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .ok_or(AppError::Unauthorized)?;

    let expected = state.api_key.as_bytes();
    let got = header.as_bytes();

    if got.len() != expected.len() || got.ct_eq(expected).unwrap_u8() == 0 {
        return Err(AppError::Unauthorized);
    }

    Ok(next.run(req).await)
}
