//! Task 8 — Event writer (core pipeline).
//!
//! Wires together Tasks 2/3/6 and the DuckDB writer: given a conversation
//! turn, runs mood classification + entity extraction, appends an
//! immutable JSONL event (Property 1 in design.md: append-only + fsync,
//! never a partial line), then dispatches the write to Neo4j and DuckDB
//! over independent tokio channels so a failure in one backend never
//! blocks or is blocked by the other (Property 2: the JSONL log is the
//! durable source of truth regardless of backend availability).
//!
//! Obsidian dispatch (Task 9) is intentionally left as a documented
//! extension point (`ObsidianDispatch` trait is not defined yet — Task 9
//! is a separate, not-yet-implemented task) rather than stubbed with a
//! fake no-op writer, per the project's "no simulated inference/behavior"
//! build standard: better to have two real backends than three where one
//! silently does nothing.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::duckdb_writer::DuckDbWriter;
use super::entity_extractor::{extract_entities, Entity};
use super::mood_classifier::{classify_mood, MoodState};
use super::neo4j_writer::{ConversationRecord, Neo4jWriter};

#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("failed to append JSONL event: {0}")]
    Jsonl(#[from] std::io::Error),
    #[error("failed to serialize event: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// One conversation turn submitted to the event writer.
#[derive(Debug, Clone)]
pub struct ConversationTurn {
    pub turn_id: uuid::Uuid,
    pub timestamp: DateTime<Utc>,
    pub user_message: String,
    pub miranda_response: String,
    /// Requirement 8.1 — Mahogany Hall readiness: present from day one,
    /// optional, no schema migration needed to start populating it.
    pub intimacy_level: Option<f32>,
}

/// The immutable event record written to JSONL, matching design.md's
/// `Event` struct (minus fields — `graph_updates`, `obsidian_note_id`,
/// `retrieval_context_used` — that belong to later tasks; those are
/// carried as optional/empty here rather than fabricated).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event_id: uuid::Uuid,
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    pub user_message: String,
    pub miranda_response: String,
    pub entities_extracted: Vec<String>,
    pub mood_state: String,
    pub mood_color: String,
    pub retrieval_context_used: Vec<uuid::Uuid>,
}

/// Dispatch message sent to the Neo4j/DuckDB backend tasks. Each backend
/// gets its own channel and its own consumer task, so a slow or failing
/// backend never backpressures the other.
struct BackendJob {
    event_id: uuid::Uuid,
    timestamp: DateTime<Utc>,
    user_message: String,
    miranda_response: String,
    entities: Vec<Entity>,
    mood_state: MoodState,
    intimacy_level: Option<f32>,
}

pub struct EventWriter {
    events_dir: PathBuf,
    neo4j_tx: mpsc::Sender<BackendJob>,
    duckdb_tx: mpsc::Sender<BackendJob>,
}

impl EventWriter {
    /// Spawns the Neo4j and DuckDB consumer tasks and returns a handle
    /// that can be cloned/shared across turns. `events_dir` is
    /// `datalake/events/` under the vault root; `duckdb_path` is
    /// `datalake/indexes/index.duckdb`.
    pub fn spawn(events_dir: PathBuf, neo4j: Neo4jWriter, duckdb_path: String) -> Self {
        let (neo4j_tx, mut neo4j_rx) = mpsc::channel::<BackendJob>(256);
        let (duckdb_tx, mut duckdb_rx) = mpsc::channel::<BackendJob>(256);

        tokio::spawn(async move {
            while let Some(job) = neo4j_rx.recv().await {
                let record = ConversationRecord {
                    conversation_id: job.event_id,
                    timestamp: job.timestamp,
                    user_message: &job.user_message,
                    miranda_response: &job.miranda_response,
                    mood_state: job.mood_state,
                    intimacy_level: job.intimacy_level,
                    entities: &job.entities,
                };
                if let Err(e) = neo4j.write_conversation(&record).await {
                    // Per design.md error handling: log and move on — the
                    // JSONL event already landed durably before this
                    // channel send, so no data is lost even if this
                    // backend write ultimately fails.
                    eprintln!("[event_writer] neo4j write failed for {}: {e}", job.event_id);
                }
            }
        });

        tokio::spawn(async move {
            while let Some(job) = duckdb_rx.recv().await {
                let path = duckdb_path.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let writer = DuckDbWriter::new(path);
                    writer.write_event(
                        job.event_id,
                        job.timestamp,
                        "conversation_turn",
                        &job.user_message,
                        &job.miranda_response,
                        &job.entities,
                        job.mood_state,
                    )
                })
                .await;
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        eprintln!("[event_writer] duckdb write failed: {e}");
                    }
                    Err(e) => {
                        eprintln!("[event_writer] duckdb task panicked: {e}");
                    }
                }
            }
        });

        Self {
            events_dir,
            neo4j_tx,
            duckdb_tx,
        }
    }

    /// Runs the full incoming path for one turn: classify mood, extract
    /// entities, append an immutable JSONL line, dispatch to Neo4j and
    /// DuckDB. Returns once the JSONL write is durable — backend fan-out
    /// happens concurrently and does not block the caller further than
    /// the (bounded, buffered) channel sends.
    pub async fn write_event(&self, turn: ConversationTurn) -> Result<Event, WriteError> {
        let (mood_state, _confidence) = classify_mood(&turn.user_message);
        let entities = extract_entities(&turn.user_message);

        let event = Event {
            event_id: turn.turn_id,
            timestamp: turn.timestamp,
            event_type: "conversation_turn".to_string(),
            user_message: turn.user_message.clone(),
            miranda_response: turn.miranda_response.clone(),
            entities_extracted: entities.iter().map(|e| e.entity_name.clone()).collect(),
            mood_state: mood_state.as_str().to_string(),
            mood_color: mood_state.color_hex().to_string(),
            retrieval_context_used: Vec::new(),
        };

        self.append_jsonl(&event).await?;

        let job = BackendJob {
            event_id: event.event_id,
            timestamp: event.timestamp,
            user_message: turn.user_message,
            miranda_response: turn.miranda_response,
            entities,
            mood_state,
            intimacy_level: turn.intimacy_level,
        };

        // Independent channels: an unavailable/slow Neo4j never blocks
        // DuckDB dispatch and vice versa. `try_send` degrades gracefully
        // (drops the fan-out, not the JSONL record) if a channel is full
        // rather than blocking the incoming path indefinitely.
        if let Err(e) = self.neo4j_tx.try_send(clone_job(&job)) {
            eprintln!("[event_writer] neo4j channel send failed: {e}");
        }
        if let Err(e) = self.duckdb_tx.try_send(job) {
            eprintln!("[event_writer] duckdb channel send failed: {e}");
        }

        Ok(event)
    }

    /// Append-only JSONL write per Property 1: write the fully-serialized
    /// line in one `write_all` call and `sync_data` before returning, so
    /// a crash mid-write cannot leave a partial line durable on disk (the
    /// OS either persists the full write or none of it once fsync'd; a
    /// torn write before fsync is not yet durable and is not observed on
    /// recovery).
    async fn append_jsonl(&self, event: &Event) -> Result<(), WriteError> {
        use tokio::io::AsyncWriteExt;

        tokio::fs::create_dir_all(&self.events_dir).await?;
        let filename = format!("{}.jsonl", event.timestamp.format("%Y-%m-%d"));
        let path = self.events_dir.join(filename);

        let mut line = serde_json::to_string(event)?;
        line.push('\n');

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        file.write_all(line.as_bytes()).await?;
        file.sync_data().await?;
        Ok(())
    }
}

fn clone_job(job: &BackendJob) -> BackendJob {
    BackendJob {
        event_id: job.event_id,
        timestamp: job.timestamp,
        user_message: job.user_message.clone(),
        miranda_response: job.miranda_response.clone(),
        entities: job.entities.clone(),
        mood_state: job.mood_state,
        intimacy_level: job.intimacy_level,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// Real integration test: no mocks. Requires the live `miranda-neo4j`
    /// Podman container (Bolt on 7687) and a real DuckDB file at a temp
    /// path (schema created inline, mirroring `scripts/duckdb-init.sh`).
    /// Ignored by default since it depends on the container; run with
    /// `cargo test -- --ignored` once Neo4j is confirmed running.
    #[tokio::test]
    #[ignore]
    async fn processes_real_turns_end_to_end() {
        let tmp_events_dir =
            std::env::temp_dir().join(format!("miranda-events-test-{}", uuid::Uuid::new_v4()));
        let tmp_duckdb = std::env::temp_dir()
            .join(format!("miranda-test-{}.duckdb", uuid::Uuid::new_v4()));

        {
            let conn = duckdb::Connection::open(&tmp_duckdb).expect("open temp duckdb");
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
        }

        let neo4j = Neo4jWriter::connect("bolt://127.0.0.1:7687", "neo4j", "mirandamemory")
            .await
            .expect("connect to live neo4j");

        let writer = EventWriter::spawn(
            tmp_events_dir.clone(),
            neo4j.clone(),
            tmp_duckdb.to_string_lossy().to_string(),
        );

        let turns = vec![
            ("Hey Sarah, did you check the Neo4j logs?", "Yes, all clear."),
            ("I'm so excited about the AWS Bedrock demo!", "Me too, it's going to be great."),
            ("Ugh, this Docker build keeps failing.", "Let's debug it together."),
        ];

        let mut written_ids = Vec::new();
        for (user_msg, miranda_resp) in turns {
            let start = Instant::now();
            let turn = ConversationTurn {
                turn_id: uuid::Uuid::new_v4(),
                timestamp: Utc::now(),
                user_message: user_msg.to_string(),
                miranda_response: miranda_resp.to_string(),
                intimacy_level: None,
            };
            let event = writer.write_event(turn).await.expect("write_event");
            let elapsed = start.elapsed();
            assert!(
                elapsed.as_millis() < 500,
                "end-to-end latency {:?} exceeded 500ms budget",
                elapsed
            );
            written_ids.push(event.event_id);
        }

        // Give the async backend fan-out tasks a moment to land.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        // Verify JSONL file was written with the right number of lines.
        let filename = format!("{}.jsonl", Utc::now().format("%Y-%m-%d"));
        let contents = tokio::fs::read_to_string(tmp_events_dir.join(filename))
            .await
            .expect("jsonl file should exist");
        assert_eq!(contents.lines().count(), 3);

        // Verify DuckDB has the rows.
        let duckdb_path = tmp_duckdb.to_string_lossy().to_string();
        let count: i64 = tokio::task::spawn_blocking(move || {
            let conn = duckdb::Connection::open(&duckdb_path).unwrap();
            conn.query_row("SELECT count(*) FROM events", [], |r| r.get(0))
                .unwrap()
        })
        .await
        .unwrap();
        assert_eq!(count, 3);

        // Verify Neo4j has at least one of the conversation nodes.
        let related = neo4j
            .query_related_conversations(&["Sarah".to_string()], 10)
            .await
            .expect("query related");
        assert!(related.iter().any(|id| written_ids.iter().any(|w| &w.to_string() == id)));

        let _ = std::fs::remove_file(&tmp_duckdb);
        let _ = std::fs::remove_dir_all(&tmp_events_dir);
    }

    /// No-network test: verifies the JSONL append path alone (mood +
    /// entity extraction + durable append) without requiring Neo4j, by
    /// constructing an EventWriter whose backend channels have no live
    /// consumer draining them (jobs simply queue in the bounded channel;
    /// JSONL durability is independent of backend availability, which is
    /// exactly Property 2 from design.md).
    #[tokio::test]
    async fn jsonl_write_succeeds_even_if_backends_are_unavailable() {
        let tmp_events_dir =
            std::env::temp_dir().join(format!("miranda-events-nobackend-{}", uuid::Uuid::new_v4()));

        // Channels with capacity but no consumer draining them — this
        // simulates "backend unreachable" without needing a live
        // container, while still proving the JSONL append is unaffected.
        let (neo4j_tx, _neo4j_rx) = mpsc::channel::<BackendJob>(8);
        let (duckdb_tx, _duckdb_rx) = mpsc::channel::<BackendJob>(8);
        let writer = EventWriter {
            events_dir: tmp_events_dir.clone(),
            neo4j_tx,
            duckdb_tx,
        };

        let turn = ConversationTurn {
            turn_id: uuid::Uuid::new_v4(),
            timestamp: Utc::now(),
            user_message: "quick test message".to_string(),
            miranda_response: "quick test response".to_string(),
            intimacy_level: None,
        };

        let event = writer.write_event(turn).await.expect("jsonl write should succeed");

        let filename = format!("{}.jsonl", Utc::now().format("%Y-%m-%d"));
        let contents = tokio::fs::read_to_string(tmp_events_dir.join(&filename))
            .await
            .expect("jsonl file should exist");
        assert!(contents.contains(&event.event_id.to_string()));

        let _ = std::fs::remove_dir_all(&tmp_events_dir);
    }
}
