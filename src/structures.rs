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
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<Vec<StructureOut>>> {
    let rows: Vec<StructureRow> = if let Some(mid) = q.malette_id {
        sqlx::query_as(
            "SELECT id, malette_id, players, total_duration_minutes, result, created_at, updated_at \
             FROM structures WHERE malette_id = ?1 ORDER BY id ASC",
        )
        .bind(mid)
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT id, malette_id, players, total_duration_minutes, result, created_at, updated_at \
             FROM structures ORDER BY id ASC",
        )
        .fetch_all(&state.pool)
        .await?
    };
    let out: Result<Vec<StructureOut>, AppError> =
        rows.into_iter().map(StructureOut::try_from).collect();
    Ok(Json(out?))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<StructureOut>> {
    let row: Option<StructureRow> = sqlx::query_as(
        "SELECT id, malette_id, players, total_duration_minutes, result, created_at, updated_at \
         FROM structures WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let row = row.ok_or(AppError::NotFound)?;
    Ok(Json(StructureOut::try_from(row)?))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<StructureInput>,
) -> AppResult<Json<StructureOut>> {
    validate_input(&input)?;

    let exists: Option<(i64,)> = sqlx::query_as("SELECT id FROM structures WHERE id = ?1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }

    let chips = load_malette_chips(&state.pool, input.malette_id).await?;
    let result_json = compute_and_serialize(&input, chips)?;

    let row: StructureRow = sqlx::query_as(
        r#"
        UPDATE structures
        SET malette_id = ?1,
            players = ?2,
            total_duration_minutes = ?3,
            result = ?4,
            updated_at = datetime('now')
        WHERE id = ?5
        RETURNING id, malette_id, players, total_duration_minutes, result, created_at, updated_at
        "#,
    )
    .bind(input.malette_id)
    .bind(input.players as i64)
    .bind(input.total_duration_minutes as i64)
    .bind(&result_json)
    .bind(id)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(StructureOut::try_from(row)?))
}

async fn delete_one(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let res = sqlx::query("DELETE FROM structures WHERE id = ?1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}
