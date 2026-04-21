use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Debug)]
pub enum AppError {
    NotFound,
    Validation(String),
    MaletteNotFound(i64),
    Db(sqlx::Error),
    Compute(anyhow::Error),
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError::Db(e)
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Compute(e)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            AppError::NotFound => (
                StatusCode::NOT_FOUND,
                json!({"error": "not found"}),
            ),
            AppError::Validation(msg) => (
                StatusCode::BAD_REQUEST,
                json!({"error": "validation failed", "details": msg}),
            ),
            AppError::MaletteNotFound(id) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                json!({
                    "error": "malette_id references nonexistent malette",
                    "malette_id": id
                }),
            ),
            AppError::Db(e) => {
                tracing::error!(error = ?e, "database error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({"error": "internal server error"}),
                )
            }
            AppError::Compute(e) => {
                tracing::error!(error = ?e, "compute error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({"error": "internal server error"}),
                )
            }
        };
        (status, Json(body)).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
