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
