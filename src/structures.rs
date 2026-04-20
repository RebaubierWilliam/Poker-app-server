use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::blind_timer::{
    compute_structure, ChipDenomination, TournamentInput, TournamentStructure,
};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/structures", get(list).post(create))
        .route(
            "/structures/:id",
            get(get_one).put(update).delete(delete_one),
        )
}

#[derive(Debug, Deserialize)]
pub struct StructureInput {
    pub malette_id: i64,
    pub players: u32,
    pub total_duration_minutes: u32,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub malette_id: Option<i64>,
}

#[derive(Debug, sqlx::FromRow)]
struct StructureRow {
    id: i64,
    malette_id: i64,
    players: i64,
    total_duration_minutes: i64,
    result: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct StructureOut {
    pub id: i64,
    pub malette_id: i64,
    pub players: u32,
    pub total_duration_minutes: u32,
    pub result: JsonValue,
    pub created_at: String,
    pub updated_at: String,
}

impl TryFrom<StructureRow> for StructureOut {
    type Error = AppError;
    fn try_from(r: StructureRow) -> Result<Self, Self::Error> {
        let result: JsonValue = serde_json::from_str(&r.result)
            .map_err(|e| AppError::Validation(format!("stored result JSON invalid: {e}")))?;
        Ok(StructureOut {
            id: r.id,
            malette_id: r.malette_id,
            players: r.players as u32,
            total_duration_minutes: r.total_duration_minutes as u32,
            result,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

fn validate_input(input: &StructureInput) -> AppResult<()> {
    if input.players < 2 {
        return Err(AppError::Validation("players must be >= 2".into()));
    }
    if input.total_duration_minutes == 0 {
        return Err(AppError::Validation(
            "total_duration_minutes must be > 0".into(),
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
struct StoredChip {
    value: u32,
    count: u32,
}

async fn load_malette_chips(
    pool: &sqlx::SqlitePool,
    malette_id: i64,
) -> AppResult<Vec<ChipDenomination>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT chips FROM malettes WHERE id = ?1")
        .bind(malette_id)
        .fetch_optional(pool)
        .await?;
    let chips_json = row.ok_or(AppError::MaletteNotFound(malette_id))?.0;
    let parsed: Vec<StoredChip> = serde_json::from_str(&chips_json)
        .map_err(|e| AppError::Validation(format!("malette chips JSON corrupted: {e}")))?;
    Ok(parsed
        .into_iter()
        .map(|c| ChipDenomination {
            value: c.value,
            count: c.count,
        })
        .collect())
}

fn compute_and_serialize(
    input: &StructureInput,
    chips: Vec<ChipDenomination>,
) -> AppResult<String> {
    let ti = TournamentInput {
        players: input.players,
        total_duration_minutes: input.total_duration_minutes,
        case_chips: chips,
    };
    let out: TournamentStructure = compute_structure(&ti);
    let json = serde_json::to_string(&out)
        .map_err(|e| AppError::Validation(format!("result serialization: {e}")))?;
    Ok(json)
}

async fn create(
    State(state): State<AppState>,
    Json(input): Json<StructureInput>,
) -> AppResult<Response> {
    validate_input(&input)?;
    let chips = load_malette_chips(&state.pool, input.malette_id).await?;
    let result_json = compute_and_serialize(&input, chips)?;

    let row: StructureRow = sqlx::query_as(
        r#"
        INSERT INTO structures (malette_id, players, total_duration_minutes, result)
        VALUES (?1, ?2, ?3, ?4)
        RETURNING id, malette_id, players, total_duration_minutes, result, created_at, updated_at
        "#,
    )
    .bind(input.malette_id)
    .bind(input.players as i64)
    .bind(input.total_duration_minutes as i64)
    .bind(&result_json)
    .fetch_one(&state.pool)
    .await?;

    let out = StructureOut::try_from(row)?;
    let location = format!("/structures/{}", out.id);
    Ok((StatusCode::CREATED, [("location", location)], Json(out)).into_response())
}

async fn list(
    State(_state): State<AppState>,
    Query(_q): Query<ListQuery>,
) -> AppResult<Json<Vec<StructureOut>>> {
    Err(AppError::Validation("list not implemented".into()))
}

async fn get_one(
    State(_state): State<AppState>,
    Path(_id): Path<i64>,
) -> AppResult<Json<StructureOut>> {
    Err(AppError::Validation("get_one not implemented".into()))
}

async fn update(
    State(_state): State<AppState>,
    Path(_id): Path<i64>,
    Json(_input): Json<StructureInput>,
) -> AppResult<Json<StructureOut>> {
    Err(AppError::Validation("update not implemented".into()))
}

async fn delete_one(
    State(_state): State<AppState>,
    Path(_id): Path<i64>,
) -> AppResult<StatusCode> {
    Err(AppError::Validation("delete_one not implemented".into()))
}
