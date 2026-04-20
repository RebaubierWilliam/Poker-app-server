# CRUD Malettes + Structures Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add persistent storage (SQLite) and REST CRUD for `malettes` and `structures` resources, gated by an API-key middleware, and update Fly.io deployment to mount a volume on a single-machine topology.

**Architecture:** Convert the crate to lib+bin: `src/lib.rs` exposes `AppState` + `build_router` + `run_migrations` + `read_config` so integration tests can import them; `src/main.rs` becomes a thin shim that loads env and binds the listener. One file per concern under `src/` (`error`, `state`, `auth`, `malettes`, `structures`). Migrations embedded via `sqlx::migrate!`. Authed routes live in a nested Axum router; `/health` stays public.

**Tech Stack:** Rust edition 2021, Axum 0.7, sqlx 0.8 (sqlite + runtime-tokio + macros + migrate + chrono), chrono 0.4, subtle 2, tower, serde, serde_json, anyhow, tracing. Tests use `tower::ServiceExt::oneshot` against `sqlite::memory:`.

**Reference spec:** `docs/superpowers/specs/2026-04-20-crud-malettes-structures-design.md`.

**Important note on JSON shape:** The `structures.result` JSON field is whatever `blind_timer::TournamentStructure` serializes to. Its actual fields are `chips_per_player: Vec<ChipDenomination>`, `starting_stack: u32`, `total_chips: u32`, `level_duration_minutes: u32`, `number_of_levels: u32`, `levels: Vec<BlindLevel>`. `BlindLevel` has `level`, `small_blind`, `big_blind`, `ante`, `duration_minutes`, `is_break`. The spec's illustrative example used simplified keys — **the implementation must match the real struct, not the spec example**.

---

## File Structure

**Create:**
- `migrations/0001_init.sql`
- `src/lib.rs`
- `src/error.rs`
- `src/state.rs`
- `src/auth.rs`
- `src/malettes.rs`
- `src/structures.rs`
- `tests/common/mod.rs`
- `tests/api.rs`

**Modify:**
- `Cargo.toml` (dependencies)
- `src/main.rs` (strip to startup shim; old `POST /structure` handler deleted)
- `src/blind_timer.rs` — **no code changes**, but visibility is already `pub` for needed types
- `Dockerfile` (add `RUN mkdir -p /data`)
- `fly.toml` (add `DATABASE_URL` env, `[[mounts]]`, change `min_machines_running`)

**Unchanged:**
- `.github/workflows/ci.yml`
- `.gitignore`

---

### Task 1: Add dependencies

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Replace `[dependencies]` block in `Cargo.toml`**

Final `[dependencies]` section:

```toml
[dependencies]
axum = "0.7"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tower = "0.5"
tower-http = { version = "0.5", features = ["cors", "trace"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
anyhow = "1"
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "sqlite", "macros", "migrate", "chrono"] }
chrono = { version = "0.4", features = ["serde"] }
subtle = "2"
```

`tower` was transitively available via `axum`, we now depend on it explicitly because tests need `tower::ServiceExt::oneshot`.

- [ ] **Step 2: Refresh lockfile and verify build**

Run: `cargo check`
Expected: `Finished` with no errors. Warnings about unused deps are fine (they'll be used by upcoming tasks).

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "Add sqlx, chrono, subtle, tower deps"
```

---

### Task 2: Initial migration SQL

**Files:**
- Create: `migrations/0001_init.sql`

- [ ] **Step 1: Create `migrations/0001_init.sql`**

```sql
CREATE TABLE malettes (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT    NOT NULL,
    chips      TEXT    NOT NULL CHECK (json_valid(chips)),
    created_at TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE structures (
    id                     INTEGER PRIMARY KEY AUTOINCREMENT,
    malette_id             INTEGER NOT NULL REFERENCES malettes(id) ON DELETE CASCADE,
    players                INTEGER NOT NULL CHECK (players >= 2),
    total_duration_minutes INTEGER NOT NULL CHECK (total_duration_minutes > 0),
    result                 TEXT    NOT NULL CHECK (json_valid(result)),
    created_at             TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at             TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_structures_malette ON structures(malette_id);
```

Note: `PRAGMA foreign_keys = ON` is **not** in the migration file — it is a connection-level setting applied when the pool is created (Task 3).

- [ ] **Step 2: Commit**

```bash
git add migrations/0001_init.sql
git commit -m "Add initial migration: malettes + structures tables"
```

---

### Task 3: Convert to lib+bin, strip main.rs

**Files:**
- Create: `src/lib.rs`
- Create: `src/error.rs` (stub)
- Create: `src/state.rs`
- Create: `src/auth.rs` (stub)
- Create: `src/malettes.rs` (stub)
- Create: `src/structures.rs` (stub)
- Modify: `src/main.rs`

The current `src/main.rs` contains both startup and the old `POST /structure` handler. We split it: `src/lib.rs` holds reusable pieces, `src/main.rs` just wires env → listener.

- [ ] **Step 1: Create `src/lib.rs` with the full public API skeleton**

```rust
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
        .route_layer(axum::middleware::from_fn_with_state(
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
```

- [ ] **Step 2: Write stub module files so `lib.rs` compiles**

`src/state.rs`:
```rust
use sqlx::SqlitePool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub api_key: Arc<str>,
}
```

`src/error.rs`:
```rust
// Full implementation in Task 4.
```

`src/auth.rs` (passthrough stub — will be replaced in Task 5):
```rust
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
```

`src/malettes.rs`:
```rust
use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
}
```

`src/structures.rs`:
```rust
use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
}
```

- [ ] **Step 3: Rewrite `src/main.rs` as a thin startup shim**

Overwrite `src/main.rs` entirely with:

```rust
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
```

The old `TournamentInput` / `structure_handler` in `main.rs` is deleted (the old `POST /structure` endpoint is gone, per spec decision).

- [ ] **Step 4: Verify the tree still builds**

Run: `cargo check`
Expected: `Finished`. There may be an unused-import warning on `State` inside the `auth.rs` stub; ignore until Task 5.

- [ ] **Step 5: Run the existing algo tests to confirm nothing regressed**

Run: `cargo test blind_timer`
Expected: 5 passed.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/main.rs src/auth.rs src/error.rs src/state.rs src/malettes.rs src/structures.rs
git commit -m "Split into lib+bin; remove legacy POST /structure"
```

---

### Task 4: Implement `AppError`

**Files:**
- Modify: `src/error.rs`

- [ ] **Step 1: Overwrite `src/error.rs` with the full implementation**

```rust
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
    Unauthorized,
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
            AppError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                json!({"error": "invalid or missing API key"}),
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
```

- [ ] **Step 2: Verify**

Run: `cargo check`
Expected: `Finished`.

- [ ] **Step 3: Commit**

```bash
git add src/error.rs
git commit -m "Implement AppError with IntoResponse"
```

---

### Task 5: Implement auth middleware

**Files:**
- Modify: `src/auth.rs`

- [ ] **Step 1: Overwrite `src/auth.rs`**

```rust
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
```

- [ ] **Step 2: Verify**

Run: `cargo check`
Expected: `Finished`, no warnings.

- [ ] **Step 3: Commit**

```bash
git add src/auth.rs
git commit -m "Implement X-API-Key middleware (constant-time compare)"
```

---

### Task 6: Integration test harness + /health smoke test

**Files:**
- Create: `tests/common/mod.rs`
- Create: `tests/api.rs`

- [ ] **Step 1: Create `tests/common/mod.rs`**

```rust
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
```

Note: SqlitePool for `sqlite::memory:` must be pinned at `max_connections=1`, otherwise each connection has its own empty in-memory DB and tests become flaky.

- [ ] **Step 2: Create `tests/api.rs` with one smoke test and the shared helpers**

```rust
mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::ServiceExt;

pub async fn with_api_key(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
) -> axum::http::Response<Body> {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-api-key", common::TEST_API_KEY);
    if body.is_some() {
        req = req.header("content-type", "application/json");
    }
    let body = match body {
        Some(v) => Body::from(serde_json::to_vec(&v).unwrap()),
        None => Body::empty(),
    };
    app.oneshot(req.body(body).unwrap()).await.unwrap()
}

pub async fn read_json(res: axum::http::Response<Body>) -> serde_json::Value {
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn health_is_public_and_returns_ok() {
    let (app, _pool) = common::test_app().await;
    let res = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}
```

The `json!` macro and the two helpers will be used by every subsequent test.

- [ ] **Step 3: Run and verify PASS**

Run: `cargo test --test api`
Expected: 1 passed.

- [ ] **Step 4: Commit**

```bash
git add tests/common/mod.rs tests/api.rs
git commit -m "Add integration test harness + /health smoke test"
```

---

### Task 7: Malettes — POST /malettes

**Files:**
- Modify: `src/malettes.rs`
- Modify: `tests/api.rs`

- [ ] **Step 1: Failing test — POST creates and returns 201**

Append to `tests/api.rs`:

```rust
#[tokio::test]
async fn post_malettes_creates_and_returns_the_row() {
    let (app, _pool) = common::test_app().await;
    let body = json!({
        "name": "Starter 300",
        "chips": [
            {"value": 25, "count": 100},
            {"value": 100, "count": 80}
        ]
    });
    let res = with_api_key(app, "POST", "/malettes", Some(body.clone())).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    assert!(res.headers().contains_key("location"));
    let got: serde_json::Value = read_json(res).await;
    assert!(got["id"].is_number());
    assert_eq!(got["name"], "Starter 300");
    assert_eq!(got["chips"], body["chips"]);
    assert!(got["created_at"].is_string());
    assert!(got["updated_at"].is_string());
}
```

- [ ] **Step 2: Run test to confirm FAIL**

Run: `cargo test --test api post_malettes`
Expected: FAIL (route not found — 401 from middleware if present, or 404).

- [ ] **Step 3: Implement the handler**

Overwrite `src/malettes.rs`:

```rust
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
    Router::new().route("/malettes", post(create))
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
```

Note: `Path` and `get` are imported now even though only `post` is used in `router()` yet — Tasks 8-11 will use them immediately.

- [ ] **Step 4: Run test to confirm PASS**

Run: `cargo test --test api post_malettes`
Expected: 1 passed.

- [ ] **Step 5: Commit**

```bash
git add src/malettes.rs tests/api.rs
git commit -m "malettes: POST /malettes handler + test"
```

---

### Task 8: Malettes — GET /malettes/:id

**Files:**
- Modify: `src/malettes.rs`
- Modify: `tests/api.rs`

- [ ] **Step 1: Failing tests**

Append to `tests/api.rs`:

```rust
#[tokio::test]
async fn get_malette_by_id_returns_stored_row() {
    let (app, _pool) = common::test_app().await;

    let body = json!({
        "name": "M1",
        "chips": [{"value": 25, "count": 100}]
    });
    let created = with_api_key(app.clone(), "POST", "/malettes", Some(body)).await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created: serde_json::Value = read_json(created).await;
    let id = created["id"].as_i64().unwrap();

    let res = with_api_key(app, "GET", &format!("/malettes/{id}"), None).await;
    assert_eq!(res.status(), StatusCode::OK);
    let got: serde_json::Value = read_json(res).await;
    assert_eq!(got["id"], id);
    assert_eq!(got["name"], "M1");
}

#[tokio::test]
async fn get_malette_missing_returns_404() {
    let (app, _pool) = common::test_app().await;
    let res = with_api_key(app, "GET", "/malettes/9999", None).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
```

- [ ] **Step 2: Run, confirm FAIL**

Run: `cargo test --test api get_malette`
Expected: 2 failed.

- [ ] **Step 3: Implement**

Add at the bottom of `src/malettes.rs`:

```rust
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
```

Update the `router()` function in `src/malettes.rs`:

```rust
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/malettes", post(create))
        .route("/malettes/:id", get(get_one))
}
```

- [ ] **Step 4: Run, confirm PASS**

Run: `cargo test --test api get_malette`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add src/malettes.rs tests/api.rs
git commit -m "malettes: GET /malettes/:id"
```

---

### Task 9: Malettes — GET list

**Files:**
- Modify: `src/malettes.rs`
- Modify: `tests/api.rs`

- [ ] **Step 1: Failing test**

Append to `tests/api.rs`:

```rust
#[tokio::test]
async fn list_malettes_returns_all_rows_ordered_by_id() {
    let (app, _pool) = common::test_app().await;

    for name in ["A", "B", "C"] {
        let body = json!({"name": name, "chips": [{"value": 25, "count": 10}]});
        let r = with_api_key(app.clone(), "POST", "/malettes", Some(body)).await;
        assert_eq!(r.status(), StatusCode::CREATED);
    }

    let res = with_api_key(app, "GET", "/malettes", None).await;
    assert_eq!(res.status(), StatusCode::OK);
    let list: serde_json::Value = read_json(res).await;
    let arr = list.as_array().expect("array");
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[0]["name"], "A");
    assert_eq!(arr[2]["name"], "C");
}
```

- [ ] **Step 2: Run, confirm FAIL**

Run: `cargo test --test api list_malettes`
Expected: FAIL (GET /malettes returns 405 Method Not Allowed or similar).

- [ ] **Step 3: Implement**

Add at the bottom of `src/malettes.rs`:

```rust
async fn list(State(state): State<AppState>) -> AppResult<Json<Vec<MaletteOut>>> {
    let rows: Vec<MaletteRow> = sqlx::query_as(
        "SELECT id, name, chips, created_at, updated_at FROM malettes ORDER BY id ASC",
    )
    .fetch_all(&state.pool)
    .await?;
    let out: Result<Vec<MaletteOut>, AppError> = rows.into_iter().map(MaletteOut::try_from).collect();
    Ok(Json(out?))
}
```

Update `router()`:

```rust
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/malettes", get(list).post(create))
        .route("/malettes/:id", get(get_one))
}
```

- [ ] **Step 4: Run, confirm PASS**

Run: `cargo test --test api list_malettes`
Expected: 1 passed.

- [ ] **Step 5: Commit**

```bash
git add src/malettes.rs tests/api.rs
git commit -m "malettes: GET /malettes list"
```

---

### Task 10: Malettes — PUT /malettes/:id

**Files:**
- Modify: `src/malettes.rs`
- Modify: `tests/api.rs`

- [ ] **Step 1: Failing tests**

Append to `tests/api.rs`:

```rust
#[tokio::test]
async fn put_malette_updates_row_and_bumps_updated_at() {
    let (app, _pool) = common::test_app().await;

    let body = json!({"name": "old", "chips": [{"value": 25, "count": 10}]});
    let created = with_api_key(app.clone(), "POST", "/malettes", Some(body)).await;
    let created: serde_json::Value = read_json(created).await;
    let id = created["id"].as_i64().unwrap();
    let old_updated = created["updated_at"].as_str().unwrap().to_string();

    // SQLite datetime('now') has second resolution — sleep > 1s to guarantee a tick.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    let new_body = json!({"name": "new", "chips": [{"value": 100, "count": 50}]});
    let res = with_api_key(app.clone(), "PUT", &format!("/malettes/{id}"), Some(new_body)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let got: serde_json::Value = read_json(res).await;
    assert_eq!(got["name"], "new");
    assert_eq!(got["chips"][0]["value"], 100);
    assert_ne!(got["updated_at"].as_str().unwrap(), old_updated);
}

#[tokio::test]
async fn put_malette_missing_returns_404() {
    let (app, _pool) = common::test_app().await;
    let body = json!({"name": "x", "chips": [{"value": 25, "count": 10}]});
    let res = with_api_key(app, "PUT", "/malettes/9999", Some(body)).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
```

- [ ] **Step 2: Run, confirm FAIL**

Run: `cargo test --test api put_malette`
Expected: 2 failed (405 Method Not Allowed).

- [ ] **Step 3: Implement**

Add at the bottom of `src/malettes.rs`:

```rust
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
```

Update `router()`:

```rust
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/malettes", get(list).post(create))
        .route("/malettes/:id", get(get_one).put(update))
}
```

- [ ] **Step 4: Run, confirm PASS**

Run: `cargo test --test api put_malette`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add src/malettes.rs tests/api.rs
git commit -m "malettes: PUT /malettes/:id"
```

---

### Task 11: Malettes — DELETE /malettes/:id

**Files:**
- Modify: `src/malettes.rs`
- Modify: `tests/api.rs`

- [ ] **Step 1: Failing tests**

Append to `tests/api.rs`:

```rust
#[tokio::test]
async fn delete_malette_returns_204_then_404_on_get() {
    let (app, _pool) = common::test_app().await;

    let body = json!({"name": "x", "chips": [{"value": 25, "count": 10}]});
    let created = with_api_key(app.clone(), "POST", "/malettes", Some(body)).await;
    let id = read_json(created).await["id"].as_i64().unwrap();

    let res = with_api_key(app.clone(), "DELETE", &format!("/malettes/{id}"), None).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let res = with_api_key(app, "GET", &format!("/malettes/{id}"), None).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_malette_missing_returns_404() {
    let (app, _pool) = common::test_app().await;
    let res = with_api_key(app, "DELETE", "/malettes/9999", None).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
```

- [ ] **Step 2: Run, confirm FAIL**

Run: `cargo test --test api delete_malette`
Expected: 2 failed.

- [ ] **Step 3: Implement**

Add at the bottom of `src/malettes.rs`:

```rust
async fn delete_one(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let res = sqlx::query("DELETE FROM malettes WHERE id = ?1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}
```

Update `router()`:

```rust
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/malettes", get(list).post(create))
        .route("/malettes/:id", get(get_one).put(update).delete(delete_one))
}
```

- [ ] **Step 4: Run tests, confirm PASS**

Run: `cargo test --test api delete_malette`
Expected: 2 passed.

- [ ] **Step 5: Full-suite regression check**

Run: `cargo test`
Expected: all passed.

- [ ] **Step 6: Commit**

```bash
git add src/malettes.rs tests/api.rs
git commit -m "malettes: DELETE /malettes/:id"
```

---

### Task 12: Structures — POST /structures (+ 422)

**Files:**
- Modify: `src/structures.rs`
- Modify: `tests/api.rs`

- [ ] **Step 1: Failing tests**

Append to `tests/api.rs`:

```rust
#[tokio::test]
async fn post_structures_creates_and_returns_result() {
    let (app, _pool) = common::test_app().await;

    let malette_body = json!({
        "name": "M",
        "chips": [
            {"value": 25, "count": 100},
            {"value": 100, "count": 100}
        ]
    });
    let created = with_api_key(app.clone(), "POST", "/malettes", Some(malette_body)).await;
    let malette_id = read_json(created).await["id"].as_i64().unwrap();

    let body = json!({
        "malette_id": malette_id,
        "players": 9,
        "total_duration_minutes": 240
    });
    let res = with_api_key(app, "POST", "/structures", Some(body)).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    assert!(res.headers().contains_key("location"));
    let got: serde_json::Value = read_json(res).await;
    assert!(got["id"].is_number());
    assert_eq!(got["malette_id"], malette_id);
    assert_eq!(got["players"], 9);
    assert_eq!(got["total_duration_minutes"], 240);
    assert!(got["result"]["levels"].is_array());
    assert!(got["result"]["starting_stack"].is_number());
}

#[tokio::test]
async fn post_structures_with_unknown_malette_returns_422() {
    let (app, _pool) = common::test_app().await;
    let body = json!({
        "malette_id": 9999,
        "players": 9,
        "total_duration_minutes": 240
    });
    let res = with_api_key(app, "POST", "/structures", Some(body)).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
}
```

- [ ] **Step 2: Run, confirm FAIL**

Run: `cargo test --test api post_structures`
Expected: 2 failed.

- [ ] **Step 3: Implement `src/structures.rs`**

Overwrite `src/structures.rs`:

```rust
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

// Stubs for tasks 13–16 so the router compiles.
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
    Err(AppError::Validation("delete not implemented".into()))
}
```

- [ ] **Step 4: Run, confirm PASS**

Run: `cargo test --test api post_structures`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add src/structures.rs tests/api.rs
git commit -m "structures: POST /structures (+ 422 on bad malette_id)"
```

---

### Task 13: Structures — GET /structures/:id

**Files:**
- Modify: `src/structures.rs`
- Modify: `tests/api.rs`

- [ ] **Step 1: Failing tests**

Append to `tests/api.rs`:

```rust
#[tokio::test]
async fn get_structure_by_id_returns_stored_result() {
    let (app, _pool) = common::test_app().await;

    let malette_body = json!({"name": "M", "chips": [{"value": 25, "count": 100}]});
    let created = with_api_key(app.clone(), "POST", "/malettes", Some(malette_body)).await;
    let mid = read_json(created).await["id"].as_i64().unwrap();

    let s_body = json!({"malette_id": mid, "players": 4, "total_duration_minutes": 120});
    let s = with_api_key(app.clone(), "POST", "/structures", Some(s_body)).await;
    let sid = read_json(s).await["id"].as_i64().unwrap();

    let res = with_api_key(app, "GET", &format!("/structures/{sid}"), None).await;
    assert_eq!(res.status(), StatusCode::OK);
    let got: serde_json::Value = read_json(res).await;
    assert_eq!(got["id"], sid);
    assert_eq!(got["malette_id"], mid);
    assert_eq!(got["players"], 4);
}

#[tokio::test]
async fn get_structure_missing_returns_404() {
    let (app, _pool) = common::test_app().await;
    let res = with_api_key(app, "GET", "/structures/9999", None).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
```

- [ ] **Step 2: Run, confirm FAIL**

Run: `cargo test --test api get_structure`
Expected: 2 failed (stub returns 400).

- [ ] **Step 3: Implement**

Replace the `get_one` stub in `src/structures.rs` with:

```rust
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
```

- [ ] **Step 4: Run, confirm PASS**

Run: `cargo test --test api get_structure`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add src/structures.rs tests/api.rs
git commit -m "structures: GET /structures/:id"
```

---

### Task 14: Structures — GET list with `?malette_id=`

**Files:**
- Modify: `src/structures.rs`
- Modify: `tests/api.rs`

- [ ] **Step 1: Failing test**

Append to `tests/api.rs`:

```rust
#[tokio::test]
async fn list_structures_filters_by_malette_id() {
    let (app, _pool) = common::test_app().await;

    let m1 = with_api_key(
        app.clone(),
        "POST",
        "/malettes",
        Some(json!({"name": "A", "chips": [{"value": 25, "count": 100}]})),
    )
    .await;
    let m1_id = read_json(m1).await["id"].as_i64().unwrap();
    let m2 = with_api_key(
        app.clone(),
        "POST",
        "/malettes",
        Some(json!({"name": "B", "chips": [{"value": 25, "count": 100}]})),
    )
    .await;
    let m2_id = read_json(m2).await["id"].as_i64().unwrap();

    for mid in [m1_id, m1_id, m2_id] {
        let body = json!({"malette_id": mid, "players": 4, "total_duration_minutes": 120});
        with_api_key(app.clone(), "POST", "/structures", Some(body)).await;
    }

    let all = with_api_key(app.clone(), "GET", "/structures", None).await;
    assert_eq!(all.status(), StatusCode::OK);
    assert_eq!(read_json(all).await.as_array().unwrap().len(), 3);

    let filt = with_api_key(
        app,
        "GET",
        &format!("/structures?malette_id={m1_id}"),
        None,
    )
    .await;
    assert_eq!(filt.status(), StatusCode::OK);
    let v = read_json(filt).await;
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    for s in arr {
        assert_eq!(s["malette_id"], m1_id);
    }
}
```

- [ ] **Step 2: Run, confirm FAIL**

Run: `cargo test --test api list_structures`
Expected: FAIL (stub returns 400).

- [ ] **Step 3: Implement**

Replace the `list` stub in `src/structures.rs`:

```rust
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
```

- [ ] **Step 4: Run, confirm PASS**

Run: `cargo test --test api list_structures`
Expected: 1 passed.

- [ ] **Step 5: Commit**

```bash
git add src/structures.rs tests/api.rs
git commit -m "structures: GET list with optional malette_id filter"
```

---

### Task 15: Structures — PUT /structures/:id regenerates

**Files:**
- Modify: `src/structures.rs`
- Modify: `tests/api.rs`

- [ ] **Step 1: Failing tests**

Append to `tests/api.rs`:

```rust
#[tokio::test]
async fn put_structure_regenerates_result_and_bumps_updated_at() {
    let (app, _pool) = common::test_app().await;

    let m = with_api_key(
        app.clone(),
        "POST",
        "/malettes",
        Some(json!({"name": "M", "chips": [{"value": 25, "count": 100}]})),
    )
    .await;
    let mid = read_json(m).await["id"].as_i64().unwrap();

    let s = with_api_key(
        app.clone(),
        "POST",
        "/structures",
        Some(json!({"malette_id": mid, "players": 4, "total_duration_minutes": 120})),
    )
    .await;
    let created: serde_json::Value = read_json(s).await;
    let sid = created["id"].as_i64().unwrap();
    let old_updated = created["updated_at"].as_str().unwrap().to_string();

    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    let res = with_api_key(
        app,
        "PUT",
        &format!("/structures/{sid}"),
        Some(json!({"malette_id": mid, "players": 8, "total_duration_minutes": 240})),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let got: serde_json::Value = read_json(res).await;
    assert_eq!(got["id"], sid);
    assert_eq!(got["players"], 8);
    assert_eq!(got["total_duration_minutes"], 240);
    assert_ne!(got["updated_at"].as_str().unwrap(), old_updated);
}

#[tokio::test]
async fn put_structure_missing_returns_404() {
    let (app, _pool) = common::test_app().await;
    let m = with_api_key(
        app.clone(),
        "POST",
        "/malettes",
        Some(json!({"name": "M", "chips": [{"value": 25, "count": 100}]})),
    )
    .await;
    let mid = read_json(m).await["id"].as_i64().unwrap();

    let res = with_api_key(
        app,
        "PUT",
        "/structures/9999",
        Some(json!({"malette_id": mid, "players": 4, "total_duration_minutes": 120})),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn put_structure_with_unknown_malette_returns_422() {
    let (app, _pool) = common::test_app().await;
    let m = with_api_key(
        app.clone(),
        "POST",
        "/malettes",
        Some(json!({"name": "M", "chips": [{"value": 25, "count": 100}]})),
    )
    .await;
    let mid = read_json(m).await["id"].as_i64().unwrap();
    let s = with_api_key(
        app.clone(),
        "POST",
        "/structures",
        Some(json!({"malette_id": mid, "players": 4, "total_duration_minutes": 120})),
    )
    .await;
    let sid = read_json(s).await["id"].as_i64().unwrap();

    let res = with_api_key(
        app,
        "PUT",
        &format!("/structures/{sid}"),
        Some(json!({"malette_id": 9999, "players": 4, "total_duration_minutes": 120})),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
}
```

- [ ] **Step 2: Run, confirm FAIL**

Run: `cargo test --test api put_structure`
Expected: 3 failed.

- [ ] **Step 3: Implement**

Replace the `update` stub in `src/structures.rs`:

```rust
async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<StructureInput>,
) -> AppResult<Json<StructureOut>> {
    validate_input(&input)?;

    // Distinguish 404 (unknown structure) from 422 (unknown malette) by checking existence first.
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
```

- [ ] **Step 4: Run, confirm PASS**

Run: `cargo test --test api put_structure`
Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add src/structures.rs tests/api.rs
git commit -m "structures: PUT /structures/:id regenerates result"
```

---

### Task 16: Structures — DELETE /structures/:id + cascade

**Files:**
- Modify: `src/structures.rs`
- Modify: `tests/api.rs`

- [ ] **Step 1: Failing tests**

Append to `tests/api.rs`:

```rust
#[tokio::test]
async fn delete_structure_returns_204_then_404() {
    let (app, _pool) = common::test_app().await;
    let m = with_api_key(
        app.clone(),
        "POST",
        "/malettes",
        Some(json!({"name": "M", "chips": [{"value": 25, "count": 100}]})),
    )
    .await;
    let mid = read_json(m).await["id"].as_i64().unwrap();
    let s = with_api_key(
        app.clone(),
        "POST",
        "/structures",
        Some(json!({"malette_id": mid, "players": 4, "total_duration_minutes": 120})),
    )
    .await;
    let sid = read_json(s).await["id"].as_i64().unwrap();

    let res = with_api_key(app.clone(), "DELETE", &format!("/structures/{sid}"), None).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let res = with_api_key(app, "GET", &format!("/structures/{sid}"), None).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deleting_malette_cascades_to_structures() {
    let (app, _pool) = common::test_app().await;

    let m = with_api_key(
        app.clone(),
        "POST",
        "/malettes",
        Some(json!({"name": "M", "chips": [{"value": 25, "count": 100}]})),
    )
    .await;
    let mid = read_json(m).await["id"].as_i64().unwrap();
    for _ in 0..3 {
        with_api_key(
            app.clone(),
            "POST",
            "/structures",
            Some(json!({"malette_id": mid, "players": 4, "total_duration_minutes": 120})),
        )
        .await;
    }

    let all = with_api_key(app.clone(), "GET", "/structures", None).await;
    assert_eq!(read_json(all).await.as_array().unwrap().len(), 3);

    let d = with_api_key(app.clone(), "DELETE", &format!("/malettes/{mid}"), None).await;
    assert_eq!(d.status(), StatusCode::NO_CONTENT);

    let after = with_api_key(app, "GET", "/structures", None).await;
    assert_eq!(read_json(after).await.as_array().unwrap().len(), 0);
}
```

- [ ] **Step 2: Run, confirm FAIL**

Run: `cargo test --test api`
Expected: the two new tests fail (stub returns 400 / cascade fails because FK not enforced if you forgot `foreign_keys(true)` on the test pool).

- [ ] **Step 3: Implement**

Replace the `delete_one` stub in `src/structures.rs`:

```rust
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
```

Cascade-on-malette-delete is handled at the schema level (`ON DELETE CASCADE`) and requires FKs enabled at connection time — which `build_pool` and `test_pool` already do via `.foreign_keys(true)`. No extra code needed here.

- [ ] **Step 4: Run, confirm PASS**

Run: `cargo test --test api`
Expected: all tests passed.

- [ ] **Step 5: Commit**

```bash
git add src/structures.rs tests/api.rs
git commit -m "structures: DELETE /structures/:id (+ verify cascade from malette)"
```

---

### Task 17: Auth negative tests

**Files:**
- Modify: `tests/api.rs`

- [ ] **Step 1: Add tests**

Append to `tests/api.rs`:

```rust
#[tokio::test]
async fn missing_api_key_on_protected_route_returns_401() {
    let (app, _pool) = common::test_app().await;
    let res = app
        .oneshot(
            Request::builder()
                .uri("/malettes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn wrong_api_key_returns_401() {
    let (app, _pool) = common::test_app().await;
    let res = app
        .oneshot(
            Request::builder()
                .uri("/malettes")
                .header("x-api-key", "not-the-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn health_is_reachable_without_api_key() {
    let (app, _pool) = common::test_app().await;
    let res = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}
```

- [ ] **Step 2: Run, confirm PASS (no code change needed — auth is already wired)**

Run: `cargo test --test api api_key`
Expected: 2 passed.

Run: `cargo test --test api health`
Expected: 2 passed (the existing smoke test + this new one).

- [ ] **Step 3: Final full-suite run**

Run: `cargo test`
Expected: all passed (5 `blind_timer` unit tests + all API integration tests).

- [ ] **Step 4: Commit**

```bash
git add tests/api.rs
git commit -m "Add auth negative tests (401 without / wrong key)"
```

---

### Task 18: Update Dockerfile

**Files:**
- Modify: `Dockerfile`

- [ ] **Step 1: Add `RUN mkdir -p /data` to the runtime stage**

The final runtime stage should read:

```dockerfile
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/poker-blind-timer-server /usr/local/bin/poker-blind-timer-server

RUN mkdir -p /data

ENV PORT=8080
EXPOSE 8080
CMD ["/usr/local/bin/poker-blind-timer-server"]
```

Migrations are embedded in the binary by `sqlx::migrate!()` at compile time, so no migration tooling is needed in the image.

- [ ] **Step 2: Commit**

```bash
git add Dockerfile
git commit -m "Dockerfile: ensure /data mountpoint exists in runtime"
```

---

### Task 19: Update fly.toml

**Files:**
- Modify: `fly.toml`

- [ ] **Step 1: Overwrite `fly.toml`**

```toml
app = "poker-blind-timer"
primary_region = "cdg"

[build]

[env]
  RUST_LOG = "info"
  DATABASE_URL = "sqlite:///data/poker.db"

[http_service]
  internal_port = 8080
  force_https = true
  auto_stop_machines = "stop"
  auto_start_machines = true
  min_machines_running = 1
  processes = ["app"]

  [[http_service.checks]]
    grace_period = "10s"
    interval = "30s"
    method = "GET"
    timeout = "5s"
    path = "/health"

[[mounts]]
  source = "poker_data"
  destination = "/data"

[[vm]]
  size = "shared-cpu-1x"
  memory = "256mb"
  cpu_kind = "shared"
  cpus = 1
```

- [ ] **Step 2: Commit — but DO NOT push yet**

```bash
git add fly.toml
git commit -m "fly: add /data volume mount and DATABASE_URL, scale to 1 machine"
```

Push is held until the user runs the Fly one-shot commands in Task 20; otherwise the deploy job in CI will fail (missing volume or missing `API_KEY` env on the running machines).

---

### Task 20: Manual Fly operations + first deploy

These commands run on the **user's machine** using the local `flyctl` binary (`C:\Users\Rebau\.fly\bin\flyctl.exe` on this machine). An agent executing this plan should present these commands to the user and wait for confirmation before continuing to Step 4.

- [ ] **Step 1: Create the volume (one-time)**

```bash
flyctl volumes create poker_data --region cdg --size 1 --yes
```

Expected: volume created, 1 GB in `cdg` region.

- [ ] **Step 2: Set `API_KEY` secret**

Generate a strong random value locally (e.g. `openssl rand -hex 32`) then:

```bash
flyctl secrets set API_KEY="<paste-the-generated-value>" --app poker-blind-timer
```

Record the key in a password manager — clients will need it for `X-API-Key`. The secret-set triggers a redeploy; that deploy will fail (no code pushed yet with volume mount + new env). This is OK; the next push will succeed.

- [ ] **Step 3: Inventory current machines**

```bash
flyctl machines list --app poker-blind-timer
```

The deploy in Step 4 will replace existing machines with new ones that mount `poker_data`. If after Step 4 there are still two machines running, destroy the one not holding the volume:

```bash
flyctl machines destroy <id> --app poker-blind-timer --force
```

SQLite is single-writer — having two app machines pointing at the same file volume causes lock contention and corruption risk. `min_machines_running = 1` + destroying extras enforces this.

- [ ] **Step 4: Push and let CI deploy**

```bash
git push origin main
```

The GitHub Actions `CI & Deploy` workflow runs `cargo test --all` then `flyctl deploy --remote-only`. Watch at:
- https://github.com/RebaubierWilliam/Poker-app-server/actions
- https://fly.io/apps/poker-blind-timer/monitoring

- [ ] **Step 5: Smoke-test the deployed app**

```bash
# Health (no auth required)
curl -s -w "\nHTTP %{http_code}\n" https://poker-blind-timer.fly.dev/health

# Protected (should 401)
curl -s -w "\nHTTP %{http_code}\n" https://poker-blind-timer.fly.dev/malettes

# With key (should 200, empty array)
curl -s -w "\nHTTP %{http_code}\n" \
  -H "X-API-Key: <same-key-you-set>" \
  https://poker-blind-timer.fly.dev/malettes

# Create a malette
curl -s -w "\nHTTP %{http_code}\n" \
  -H "X-API-Key: <same-key-you-set>" \
  -H "Content-Type: application/json" \
  -d '{"name":"Test","chips":[{"value":25,"count":100}]}' \
  https://poker-blind-timer.fly.dev/malettes
```

Expected: 200, 401, 200 (`[]`), 201 (returns the created malette JSON with `id: 1`).

- [ ] **Step 6: Verify persistence**

```bash
# List — should show the malette created above
curl -s -w "\nHTTP %{http_code}\n" \
  -H "X-API-Key: <same-key-you-set>" \
  https://poker-blind-timer.fly.dev/malettes
```

Expected: 200, array with one item. Scale the machine down and up to confirm data survives a restart:

```bash
flyctl machines list --app poker-blind-timer
flyctl machines stop <machine-id> --app poker-blind-timer
flyctl machines start <machine-id> --app poker-blind-timer
# re-run the list call above — data should still be present
```

If any smoke-test step fails, roll back:
```bash
flyctl releases --app poker-blind-timer
flyctl deploy --image <previous-image-tag> --app poker-blind-timer
```

---

## Done

All 20 tasks completed means: `cargo test` is green locally and in CI, the deployed app at `https://poker-blind-timer.fly.dev` serves `/health` publicly and `/malettes` + `/structures` under `X-API-Key`, a SQLite file persists at `/data/poker.db` on the Fly volume, and subsequent commits to `main` auto-deploy via the existing CI workflow.
