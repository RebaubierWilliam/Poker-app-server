# Design: CRUD Malettes + Structures de tournoi

**Date**: 2026-04-20
**Status**: Approved, ready for implementation plan
**Project**: poker-blind-timer (`Poker-app-server` GitHub repo)

## Problème

Le serveur expose aujourd'hui un endpoint stateless `POST /structure` qui calcule une structure de blinds à partir d'un input complet. Rien n'est persisté : impossible de retrouver ou partager une malette, impossible de retrouver une structure déjà calculée.

Objectif : introduire une base de données et deux ressources REST, pour pouvoir gérer les malettes (composition de jetons) et les structures calculées de manière persistante.

## Décisions clés

| Décision | Choix |
|----------|-------|
| Base de données | **SQLite** + volume Fly (1 machine, plus de HA) |
| Update sur `structures` | **PUT régénère** (CRUD complet ; 404 si id absent, pas d'upsert) |
| Endpoint existant `POST /structure` | **Supprimé** (remplacé par `/structures` DB-backed) |
| Auth | **API key** statique via header `X-API-Key` (hors `/health`) |

## Stack

Crates ajoutés (`Cargo.toml`) :
- `sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "macros", "migrate", "chrono"] }`
- `chrono = { version = "0.4", features = ["serde"] }`
- `subtle = "2"` — comparaison constant-time pour l'API key

Pas d'ORM (SeaORM/Diesel) : on reste proche du SQL.

Choix : `sqlx::query_as::<_, T>(...)` **runtime** plutôt que les macros `sqlx::query!` compile-time. Trade-off assumé : on perd la vérification SQL au build, on gagne une CI simple (pas besoin de `DATABASE_URL` au `cargo build`, pas de `.sqlx/` à committer).

## Architecture

### Layout `src/` (flat)

```
src/
├── main.rs            startup: migrations + router bind
├── state.rs           AppState { pool: SqlitePool, api_key: Arc<str> }
├── auth.rs            middleware X-API-Key
├── error.rs           AppError enum + IntoResponse
├── malettes.rs        5 handlers CRUD
├── structures.rs      5 handlers CRUD (appelle blind_timer::compute)
└── blind_timer.rs     algo existant, inchangé

migrations/
└── 0001_init.sql      tables malettes + structures

tests/
└── api.rs             tests d'intégration bout-en-bout (SQLite mémoire)
```

### Flux d'une requête

```
Request → AxumRouter → api_key_middleware → handler (malettes.rs | structures.rs)
                                               ↓
                                         sqlx → SQLite file @ /data/poker.db
                                               ↓
                            pour POST/PUT /structures : blind_timer::compute()
                                               ↓
                                         AppError (map vers status+JSON)
                                               ↓
                                         Response
```

## Schéma DB

Fichier unique `migrations/0001_init.sql`, appliqué au démarrage via `sqlx::migrate!("./migrations")` (embarqué dans le binaire).

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

**Conventions** :
- `chips` stocké en JSON texte (pas de table `malette_chips` séparée). Atomique, simple, OK pour ~10 puces max. Normalisation future si besoin.
- `result` stocké en JSON texte (sortie de `blind_timer::compute()`).
- `ON DELETE CASCADE` : suppression d'une malette supprime ses structures.
- `CHECK` constraints pour cohérence défensive (defense-in-depth vs bug applicatif).
- `PRAGMA foreign_keys = ON` activé au montage du pool (sinon SQLite ignore silencieusement les FK).
- Timestamps en ISO 8601 text (`datetime('now')`). Suffisant, pas de fuseau horaire.

## Routes REST

Toutes les routes sous `/malettes` et `/structures` passent par le middleware API-key. `/health` reste public.

| Verbe | Route | 2xx | 4xx |
|-------|---------------------------|---------|-----|
| GET | `/health` | 200 | — |
| GET | `/malettes` | 200 | — |
| POST | `/malettes` | 201 + `Location` | 400 |
| GET | `/malettes/:id` | 200 | 404 |
| PUT | `/malettes/:id` | 200 | 400, 404 |
| DELETE | `/malettes/:id` | 204 | 404 |
| GET | `/structures` | 200 | — |
| GET | `/structures?malette_id=N` | 200 | — |
| POST | `/structures` | 201 + `Location` | 400, 422 |
| GET | `/structures/:id` | 200 | 404 |
| PUT | `/structures/:id` | 200 | 400, 404, 422 |
| DELETE | `/structures/:id` | 204 | 404 |

Pas de pagination sur les listes (`GET /malettes`, `GET /structures`) → YAGNI ; à ajouter quand la DB grossit. Filtre optionnel `?malette_id=N` sur `/structures`.

## Payloads

### Malette

`POST /malettes` et `PUT /malettes/:id` (body) :
```json
{
  "name": "Malette 300 jetons",
  "chips": [
    {"value": 25, "count": 100},
    {"value": 100, "count": 80},
    {"value": 500, "count": 60},
    {"value": 1000, "count": 40},
    {"value": 5000, "count": 20}
  ]
}
```

Réponse (`POST`, `PUT`, `GET /malettes/:id`) :
```json
{
  "id": 1,
  "name": "Malette 300 jetons",
  "chips": [...],
  "created_at": "2026-04-20T16:05:00",
  "updated_at": "2026-04-20T16:05:00"
}
```

### Structure

`POST /structures` et `PUT /structures/:id` (body) :
```json
{
  "malette_id": 1,
  "players": 9,
  "total_duration_minutes": 240
}
```

Réponse (`POST`, `PUT`, `GET /structures/:id`) :
```json
{
  "id": 1,
  "malette_id": 1,
  "players": 9,
  "total_duration_minutes": 240,
  "result": {
    "chips_per_player": 1111,
    "starting_stack": 3000,
    "total_chips": 10000,
    "level_duration_minutes": 20,
    "levels": [
      {"number": 1, "small_blind": 25, "big_blind": 50, "ante": 0}
    ]
  },
  "created_at": "2026-04-20T16:07:12",
  "updated_at": "2026-04-20T16:07:12"
}
```

## Erreurs

Enum `AppError` dans `src/error.rs`, impl `IntoResponse` renvoie toujours du JSON :

| Variante | HTTP | Body |
|----------|------|------|
| `NotFound` | 404 | `{"error": "not found"}` |
| `Validation(String)` | 400 | `{"error": "validation failed", "details": "..."}` |
| `MaletteNotFound(i64)` | 422 | `{"error": "malette_id references nonexistent malette"}` |
| `Unauthorized` | 401 | `{"error": "invalid or missing API key"}` |
| `Db(sqlx::Error)` | 500 | `{"error": "internal server error"}` + `tracing::error!` |
| `Compute(anyhow::Error)` | 500 | `{"error": "internal server error"}` + `tracing::error!` |

Les 500 masquent les détails côté client ; le détail est dans les logs Fly.

## Authentification

- `AppState` contient `api_key: Arc<str>`.
- Au démarrage, `main.rs` lit `std::env::var("API_KEY")` : **panic si absente** (fail fast).
- Middleware `api_key_middleware` via `axum::middleware::from_fn_with_state` :
  - Lit header `X-API-Key`.
  - Compare avec `subtle::ConstantTimeEq::ct_eq` (prévient l'attaque par timing).
  - Absent / incorrect → retourne `AppError::Unauthorized`.
- Middleware appliqué sur un sous-routeur qui couvre `/malettes` + `/structures`. `/health` reste dans le routeur racine non protégé (utile pour les healthchecks Fly).

## Tests

### Unit (inchangé)
`src/blind_timer.rs` garde ses 5 tests existants.

### Integration (nouveau fichier `tests/api.rs`)
- Chaque test crée son propre pool SQLite **en mémoire** (`sqlite::memory:`) + applique les migrations, donc tests indépendants.
- Utilise `tower::ServiceExt::oneshot` + `axum::body::Body` pour faire de vraies requêtes HTTP sans ouvrir de port.
- Cas couverts (~8-10 tests) :
  - `POST /malettes` + `GET /malettes/:id` round-trip
  - `PUT /malettes/:id` modifie et bump `updated_at`
  - `DELETE /malettes/:id` → 404 sur le `GET` suivant
  - `POST /structures` avec `malette_id` invalide → 422
  - `PUT /structures/:id` régénère avec nouveaux inputs, même id
  - `DELETE` malette → structures cascade-supprimées
  - Sans header `X-API-Key` → 401
  - Avec header incorrect → 401
  - `GET /structures?malette_id=N` filtre bien

CI existante (`cargo test --all`) picke ces tests automatiquement, aucun changement de workflow.

## Déploiement Fly

### `fly.toml` — diff

```toml
[env]
  RUST_LOG = "info"
  DATABASE_URL = "sqlite:///data/poker.db"    # AJOUT

[http_service]
  # ...
  min_machines_running = 1                    # CHANGE: 0 -> 1

[[mounts]]                                    # AJOUT
  source = "poker_data"
  destination = "/data"
```

Note : `min_machines_running = 1` + `auto_stop_machines = "stop"` → la machine peut s'endormir, mais une est toujours allouée (pas zéro) → redémarre avec le même volume (cold start ~2-3 s).

### `Dockerfile` — ajout

```dockerfile
# dans la stage runtime
RUN mkdir -p /data
```

Les migrations sont embarquées dans le binaire (`sqlx::migrate!("./migrations")` lit les `.sql` au compile time), donc pas besoin d'installer `sqlx-cli` dans l'image.

### One-shot avant le premier push du code

```bash
fly volumes create poker_data --region cdg --size 1
fly secrets set API_KEY="<string aléatoire costaude>"
fly machines list
fly machines destroy <id_de_la_2e_machine>
```

Le `secrets set` déclenche un redeploy tout seul ; le volume existe dès sa création ; on supprime la 2e machine car SQLite = 1 writer.

### CI

Aucune modification du workflow GitHub Actions. Le job `deploy` actuel picke le nouveau `fly.toml` et déploie normalement dès qu'un commit atterrit sur `main`.

## Hors scope (explicitement)

- Pagination des listes (à ajouter quand nécessaire)
- Plusieurs API keys / vrais users
- Admin UI
- `POST /structures/preview` (compute sans persister)
- Normalisation de `chips` en table séparée
- Historique / versioning des structures
- Observabilité au-delà de `tracing` vers stdout
