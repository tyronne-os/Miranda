#!/usr/bin/env bash
# duckdb-init.sh — creates index.duckdb with the events and entities tables
# defined in .kiro/specs/wo-memory-data-lake/design.md, with the indexes
# required for sub-100ms mood/entity/timestamp queries.
set -euo pipefail

PREFERRED_ROOT="/mnt/NOBILITY_VAULT/.miranda"
FALLBACK_ROOT="${HOME}/NOBILITY_VAULT/.miranda"

if [ -d "$PREFERRED_ROOT" ]; then
    VAULT_ROOT="$PREFERRED_ROOT"
else
    VAULT_ROOT="$FALLBACK_ROOT"
fi

DUCKDB_BIN="${DUCKDB_BIN:-$HOME/.local/bin/duckdb}"
DB_PATH="$VAULT_ROOT/datalake/indexes/index.duckdb"

mkdir -p "$VAULT_ROOT/datalake/indexes"

echo "Initializing DuckDB index at: $DB_PATH"

"$DUCKDB_BIN" "$DB_PATH" <<'SQL'
CREATE TABLE IF NOT EXISTS events (
    event_id            UUID PRIMARY KEY,
    timestamp           TIMESTAMP NOT NULL,
    event_type          VARCHAR NOT NULL,
    user_message        VARCHAR NOT NULL,
    miranda_response     VARCHAR NOT NULL,
    entities             VARCHAR,
    mood_state           VARCHAR NOT NULL,
    mood_rgb             VARCHAR,
    mood_hsl             VARCHAR
);

CREATE TABLE IF NOT EXISTS entities (
    entity_name          VARCHAR NOT NULL,
    entity_type          VARCHAR NOT NULL,
    first_mention        TIMESTAMP NOT NULL,
    last_mention         TIMESTAMP NOT NULL,
    mention_count        INTEGER NOT NULL DEFAULT 1,
    mood_contexts        VARCHAR,
    PRIMARY KEY (entity_name, entity_type)
);

CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);
CREATE INDEX IF NOT EXISTS idx_events_mood_state ON events(mood_state);
CREATE INDEX IF NOT EXISTS idx_entities_name ON entities(entity_name);
CREATE INDEX IF NOT EXISTS idx_entities_last_mention ON entities(last_mention);

.tables
SQL

echo "duckdb-init.sh complete. Schema created at $DB_PATH"
