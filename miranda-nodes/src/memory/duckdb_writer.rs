//! Task 8 support — DuckDB index writer.
//!
//! Inserts rows into the `events` and `entities` tables created by
//! `scripts/duckdb-init.sh` (per `design.md`'s DuckDB schema), so the
//! analytics/retrieval path (Task 10) has real indexed data to query
//! instead of only the raw JSONL log.
//!
//! DuckDB's Rust bindings are synchronous (`duckdb::Connection` is not
//! `Send` across an `.await` in a way that plays well with a shared async
//! pool), so writes are dispatched onto a blocking thread via
//! `tokio::task::spawn_blocking` from the event writer's channel consumer
//! rather than held open across awaits here.

use chrono::{DateTime, Utc};
use duckdb::{params, Connection, types::Value};

use super::entity_extractor::Entity;
use super::mood_classifier::MoodState;

#[derive(Debug, thiserror::Error)]
pub enum DuckDbError {
    #[error("duckdb error: {0}")]
    Duck(#[from] duckdb::Error),
}

pub struct DuckDbWriter {
    db_path: String,
}

impl DuckDbWriter {
    pub fn new(db_path: impl Into<String>) -> Self {
        Self {
            db_path: db_path.into(),
        }
    }

    /// Opens a fresh connection per call. DuckDB connections are cheap
    /// relative to the <500ms end-to-end budget and this avoids sharing a
    /// non-`Send` `Connection` across the tokio channel/task boundary.
    fn connect(&self) -> Result<Connection, DuckDbError> {
        Ok(Connection::open(&self.db_path)?)
    }

    /// Inserts one event row plus upserts each extracted entity's summary
    /// row, matching design.md's `events` / `entities` table shapes.
    pub fn write_event(
        &self,
        event_id: uuid::Uuid,
        timestamp: DateTime<Utc>,
        event_type: &str,
        user_message: &str,
        miranda_response: &str,
        entities: &[Entity],
        mood_state: MoodState,
    ) -> Result<(), DuckDbError> {
        let conn = self.connect()?;

        let entity_names: Vec<String> = entities.iter().map(|e| e.entity_name.clone()).collect();
        let entity_names_value = Value::List(entity_names.into_iter().map(Value::Text).collect());

        conn.execute(
            "INSERT INTO events (event_id, timestamp, event_type, user_message, \
             miranda_response, entities, mood_state, mood_rgb, mood_hsl) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (event_id) DO NOTHING",
            params![
                event_id.to_string(),
                timestamp.to_rfc3339(),
                event_type,
                user_message,
                miranda_response,
                entity_names_value,
                mood_state.as_str(),
                mood_state.color_hex(),
                Option::<String>::None,
            ],
        )?;

        for entity in entities {
            conn.execute(
                "INSERT INTO entities (entity_name, entity_type, first_mention, \
                 last_mention, mention_count, mood_contexts) \
                 VALUES (?, ?, ?, ?, 1, ?) \
                 ON CONFLICT (entity_name, entity_type) DO UPDATE SET \
                     last_mention = excluded.last_mention, \
                     mention_count = entities.mention_count + 1",
                params![
                    entity.entity_name,
                    entity.entity_type.as_str(),
                    timestamp.to_rfc3339(),
                    timestamp.to_rfc3339(),
                    Value::List(vec![Value::Text(mood_state.as_str().to_string())]),
                ],
            )?;
        }

        Ok(())
    }

    /// Requirement 4.4 — mood/date-range query used by Task 10's
    /// retriever, expected under 100ms.
    pub fn query_by_mood(
        &self,
        mood: MoodState,
        limit: usize,
    ) -> Result<Vec<(String, String)>, DuckDbError> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT event_id, user_message FROM events WHERE mood_state = ? \
             ORDER BY timestamp DESC LIMIT ?",
        )?;
        let mut rows = stmt.query(params![mood.as_str(), limit as i64])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let msg: String = row.get(1)?;
            out.push((id, msg));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::entity_extractor::EntityType;

    /// Real integration test against a real temp DuckDB file, using the
    /// exact schema `scripts/duckdb-init.sh` creates (recreated here
    /// in-line so the test doesn't depend on the live vault path).
    #[test]
    fn writes_and_queries_real_duckdb_rows() {
        let tmp = std::env::temp_dir().join(format!("miranda-test-{}.duckdb", uuid::Uuid::new_v4()));
        let conn = Connection::open(&tmp).expect("open temp duckdb");
        conn.execute_batch(
            "CREATE TABLE events (
                event_id VARCHAR PRIMARY KEY,
                timestamp TIMESTAMP NOT NULL,
                event_type VARCHAR NOT NULL,
                user_message VARCHAR NOT NULL,
                miranda_response VARCHAR NOT NULL,
                entities VARCHAR[],
                mood_state VARCHAR NOT NULL,
                mood_rgb VARCHAR,
                mood_hsl VARCHAR
            );
            CREATE TABLE entities (
                entity_name VARCHAR NOT NULL,
                entity_type VARCHAR NOT NULL,
                first_mention TIMESTAMP NOT NULL,
                last_mention TIMESTAMP NOT NULL,
                mention_count INTEGER NOT NULL DEFAULT 1,
                mood_contexts VARCHAR[],
                PRIMARY KEY (entity_name, entity_type)
            );",
        )
        .expect("create schema");
        drop(conn);

        let writer = DuckDbWriter::new(tmp.to_string_lossy().to_string());
        let entities = vec![Entity {
            entity_name: "TestPerson".to_string(),
            entity_type: EntityType::Person,
            confidence: 0.8,
        }];

        writer
            .write_event(
                uuid::Uuid::new_v4(),
                Utc::now(),
                "conversation_turn",
                "hello there",
                "hi back",
                &entities,
                MoodState::Casual,
            )
            .expect("write_event should succeed");

        let results = writer
            .query_by_mood(MoodState::Casual, 10)
            .expect("query_by_mood should succeed");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, "hello there");

        let _ = std::fs::remove_file(&tmp);
    }
}
