CREATE TABLE IF NOT EXISTS keys (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    token_hash TEXT NOT NULL,
    token_prefix TEXT NOT NULL,
    budget_micros INTEGER,
    spent_micros INTEGER NOT NULL DEFAULT 0,
    rpm INTEGER,
    tpm INTEGER,
    max_parallel INTEGER,
    model_allowlist TEXT,
    expires_at_ms INTEGER,
    revoked INTEGER NOT NULL DEFAULT 0,
    parent_id TEXT,
    created_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS reservations (
    id TEXT PRIMARY KEY,
    key_id TEXT NOT NULL REFERENCES keys(id),
    estimate_micros INTEGER NOT NULL,
    created_at_ms INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_reservations_key_id ON reservations(key_id)
