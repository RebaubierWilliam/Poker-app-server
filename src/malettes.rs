use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/malettes", get(list).post(create))
        .route("/malettes/:id", get(get_one).put(update))
}

#[derive(Debug, Deserialize)]
pub struct MaletteInput {
    pub name: String,
    pub chips: Vec<ChipInput>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ChipInput {
    pub value: u32,
    pub count: u32,
}

#[derive(Debug, sqlx::FromRow)]
pub struct MaletteRow {
    pub id: i64,
    pub name: String,
    pub chips: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct MaletteOut {
    pub id: i64,
    pub name: String,
    pub chips: JsonValue,
    pub created_at: String,
    pub updated_at: String,
}

impl TryFrom<MaletteRow> for MaletteOut {
    type Error = AppError;
    fn try_from(r: MaletteRow) -> Result<Self, Self::Error> {
        let chips: JsonValue = serde_json::from_str(&r.chips)
            .map_err(|e| AppError::Validation(format!("stored chips JSON invalid: {e}")))?;
        Ok(MaletteOut {
            id: r.id,
            name: r.name,
            chips,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

fn validate_input(input: &MaletteInput) -> AppResult<()> {
    if input.name.trim().is_empty() {
        return Err(AppError::Validation("name must not be empty".into()));
    }
    if input.chips.is_empty() {
        return Err(AppError::Validation("chips must not be empty".into()));
    }
    for c in &input.chips {
        if c.value == 0 {
            return Err(AppError::Validation("chip value must be > 0".into()));
        }
        if c.count == 0 {
            return Err(AppError::Validation("chip count must be > 0".into()));
        }
    }
    Ok(())
}

async fn create(
    State(state): State<AppState>,
    Json(input): Json<MaletteInput>,
) -> AppResult<Response> {
    validate_input(&input)?;
    let chips_json = serde_json::to_string(&input.chips)
        .map_err(|e| AppError::Validation(format!("chips serialization: {e}")))?;

    let row: MaletteRow = sqlx::query_as(
        r#"
        INSERT INTO malettes (name, chips)
        VALUES (?1, ?2)
        RETURNING id, name, chips, created_at, updated_at
        "#,
    )
    .bind(&input.name)
    .bind(&chips_json)
    .fetch_one(&state.pool)
    .await?;

    let out = MaletteOut::try_from(row)?;
    let location = format!("/malettes/{}", out.id);
    Ok((StatusCode::CREATED, [("location", location)], Json(out)).into_response())
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<MaletteOut>> {
    let row: Option<MaletteRow> = sqlx::query_as(
        "SELECT id, name, chips, created_at, updated_at FROM malettes WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let row = row.ok_or(AppError::NotFound)?;
    Ok(Json(MaletteOut::try_from(row)?))
}

async fn list(State(state): State<AppState>) -> AppResult<Json<Vec<MaletteOut>>> {
    let rows: Vec<MaletteRow> = sqlx::query_as(
        "SELECT id, name, chips, created_at, updated_at FROM malettes ORDER BY id ASC",
    )
    .fetch_all(&state.pool)
    .await?;
    let out: Result<Vec<MaletteOut>, AppError> =
        rows.into_iter().map(MaletteOut::try_from).collect();
    Ok(Json(out?))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<MaletteInput>,
) -> AppResult<Json<MaletteOut>> {
    validate_input(&input)?;
    let chips_json = serde_json::to_string(&input.chips)
        .map_err(|e| AppError::Validation(format!("chips serialization: {e}")))?;

    let row: Option<MaletteRow> = sqlx::query_as(
        r#"
        UPDATE malettes
        SET name = ?1, chips = ?2, updated_at = datetime('now')
        WHERE id = ?3
        RETURNING id, name, chips, created_at, updated_at
        "#,
    )
    .bind(&input.name)
    .bind(&chips_json)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;

    let row = row.ok_or(AppError::NotFound)?;
    Ok(Json(MaletteOut::try_from(row)?))
}
