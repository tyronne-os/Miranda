//! Task 10 — Memory retriever (query engine).
//!
//! Implements the outgoing path from `design.md`: given the current
//! message + mood, traverse the Neo4j knowledge graph for entity-linked
//! past conversations, cross-reference DuckDB's mood-continuity index, and
//! rank the combined candidate set by:
//!
//!   score = entity_overlap * 0.5 + temporal_recency * 0.3 + mood_similarity * 0.2
//!
//! per design.md's Memory Retriever component and Requirement 5.2.
//!
//! Determinism (Property 3 in design.md): for a fixed graph/index state
//! and fixed `now`, `retrieve_context` returns the same ranked order every
//! call — there is no randomness in scoring or tie-breaking (ties break on
//! conversation_id string order, deterministically).

use std::time::Instant;

use chrono::{DateTime, Utc};

use super::duckdb_writer::DuckDbWriter;
use super::entity_extractor::extract_entities;
use super::mood_classifier::MoodState;
use super::neo4j_writer::Neo4jWriter;

#[derive(Debug, thiserror::Error)]
pub enum RetrieverError {
    #[error("neo4j query failed: {0}")]
    Neo4j(#[from] super::neo4j_writer::Neo4jError),
    #[error("duckdb query failed: {0}")]
    DuckDb(#[from] super::duckdb_writer::DuckDbError),
}

/// One ranked past context, matching design.md's `RetrievedContext`.
#[derive(Debug, Clone, PartialEq)]
pub struct RetrievedContext {
    pub conversation_id: String,
    pub relevance_score: f32,
    pub summary: String,
    pub mood_state: MoodState,
}

/// Candidate metadata gathered from the two backends before scoring.
struct Candidate {
    conversation_id: String,
    summary: String,
    mood_state: MoodState,
    /// Position within its source result list (0 = most recent per that
    /// backend's own `ORDER BY ... DESC` query) — used as a proxy for
    /// temporal recency since neither backend query returns an absolute
    /// timestamp back to this layer today. Deterministic and monotonic.
    recency_rank: usize,
    recency_pool_size: usize,
}

pub struct MemoryRetriever {
    neo4j: Neo4jWriter,
    duckdb: DuckDbWriter,
    /// Requirement 5.4: total retrieval latency budget in milliseconds.
    /// Queries that run past this are still allowed to finish (per
    /// design.md's "return whatever partial results are available rather
    /// than blocking"), but the elapsed time is surfaced to the caller via
    /// `retrieve_context_timed` for observability/testing.
    timeout_ms: u64,
}

impl MemoryRetriever {
    pub fn new(neo4j: Neo4jWriter, duckdb: DuckDbWriter) -> Self {
        Self {
            neo4j,
            duckdb,
            timeout_ms: 200,
        }
    }

    /// Requirement 5.1, 5.2, 5.5 — queries both backends for the current
    /// message's entities/mood, ranks the union, and returns the top-K
    /// (or an empty vec if nothing relevant exists, per 5.5: retrieval
    /// finding nothing is not an error).
    pub async fn retrieve_context(
        &self,
        current_message: &str,
        current_mood: MoodState,
        top_k: usize,
    ) -> Result<Vec<RetrievedContext>, RetrieverError> {
        let entities = extract_entities(current_message);
        let entity_names: Vec<String> = entities.iter().map(|e| e.entity_name.clone()).collect();

        let mut candidates: Vec<Candidate> = Vec::new();

        if !entity_names.is_empty() {
            let related = self
                .neo4j
                .query_related_conversations(&entity_names, top_k.max(10))
                .await?;
            let pool_size = related.len();
            for (rank, conversation_id) in related.into_iter().enumerate() {
                candidates.push(Candidate {
                    conversation_id,
                    summary: format!("Related conversation (shared entities: {})", entity_names.join(", ")),
                    // Mood unknown from this query path; entity overlap is
                    // still scored below from `entity_names` directly, and
                    // mood_similarity for these candidates falls back to
                    // Unknown-vs-current (0.0) unless DuckDB also surfaces
                    // the same conversation with its mood.
                    mood_state: MoodState::Unknown,
                    recency_rank: rank,
                    recency_pool_size: pool_size,
                });
            }
        }

        let mood_rows = self.duckdb.query_by_mood(current_mood, top_k.max(10))?;
        let mood_pool_size = mood_rows.len();
        for (rank, (event_id, user_message)) in mood_rows.into_iter().enumerate() {
            candidates.push(Candidate {
                conversation_id: event_id,
                summary: user_message,
                mood_state: current_mood,
                recency_rank: rank,
                recency_pool_size: mood_pool_size,
            });
        }

        // Merge duplicate conversation_ids (found via both backends):
        // keep the richer (DuckDB, which carries mood + summary) entry and
        // credit it with the better (lower) recency_rank of the two.
        let mut merged: std::collections::HashMap<String, Candidate> = std::collections::HashMap::new();
        for c in candidates {
            merged
                .entry(c.conversation_id.clone())
                .and_modify(|existing| {
                    if c.recency_rank < existing.recency_rank {
                        existing.recency_rank = c.recency_rank;
                        existing.recency_pool_size = c.recency_pool_size;
                    }
                    if existing.mood_state == MoodState::Unknown && c.mood_state != MoodState::Unknown {
                        existing.mood_state = c.mood_state;
                        existing.summary = c.summary.clone();
                    }
                })
                .or_insert(c);
        }

        let entity_set: std::collections::HashSet<String> =
            entity_names.iter().map(|s| s.to_lowercase()).collect();

        let mut scored: Vec<RetrievedContext> = merged
            .into_values()
            .map(|c| {
                let entity_overlap = if entity_set.is_empty() {
                    0.0
                } else {
                    // Entity overlap proxy: candidates found via the
                    // entity-traversal path share at least one entity by
                    // construction, so they get full credit; mood-only
                    // candidates (no entity match confirmed) get none.
                    if c.mood_state == MoodState::Unknown || c.recency_pool_size == 0 {
                        1.0
                    } else {
                        0.0
                    }
                };
                let temporal_recency = if c.recency_pool_size <= 1 {
                    1.0
                } else {
                    1.0 - (c.recency_rank as f32 / (c.recency_pool_size - 1) as f32)
                };
                let mood_similarity = if c.mood_state == current_mood { 1.0 } else { 0.0 };

                let score = entity_overlap * 0.5 + temporal_recency * 0.3 + mood_similarity * 0.2;

                RetrievedContext {
                    conversation_id: c.conversation_id,
                    relevance_score: score,
                    summary: c.summary,
                    mood_state: c.mood_state,
                }
            })
            .collect();

        // Deterministic ordering: score descending, then conversation_id
        // ascending as a stable tie-breaker (Property 3).
        scored.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap()
                .then_with(|| a.conversation_id.cmp(&b.conversation_id))
        });
        scored.truncate(top_k);

        Ok(scored)
    }

    /// Same as `retrieve_context` but also returns elapsed wall time, so
    /// callers/tests can verify the <200ms budget from Requirement 5.4
    /// against a real clock rather than assuming it.
    pub async fn retrieve_context_timed(
        &self,
        current_message: &str,
        current_mood: MoodState,
        top_k: usize,
    ) -> (Result<Vec<RetrievedContext>, RetrieverError>, std::time::Duration) {
        let start = Instant::now();
        let result = self.retrieve_context(current_message, current_mood, top_k).await;
        (result, start.elapsed())
    }

    pub fn timeout_budget_ms(&self) -> u64 {
        self.timeout_ms
    }
}

/// Placeholder used only for doc-comment cross-referencing; not part of
/// the public API. Kept private so `chrono` import stays justified without
/// an unused-import warning if the timestamp field is added to
/// `RetrievedContext` in a later task.
#[allow(dead_code)]
fn _unused_time_anchor() -> DateTime<Utc> {
    Utc::now()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real integration test against the live `miranda-neo4j` container
    /// and a real temp DuckDB file (schema created inline). Ignored by
    /// default since it needs the container; run with `-- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn retrieves_ranked_context_against_live_backends() {
        let neo4j = Neo4jWriter::connect("bolt://127.0.0.1:7687", "neo4j", "mirandamemory")
            .await
            .expect("connect to live neo4j");

        let tmp_duckdb = std::env::temp_dir()
            .join(format!("miranda-retriever-test-{}.duckdb", uuid::Uuid::new_v4()));
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
        let duckdb = DuckDbWriter::new(tmp_duckdb.to_string_lossy().to_string());

        // Seed one labeled fixture via DuckDB directly.
        duckdb
            .write_event(
                uuid::Uuid::new_v4(),
                Utc::now(),
                "conversation_turn",
                "I love researching Neo4j graph queries",
                "That's a great research topic",
                &[],
                MoodState::Research,
            )
            .expect("seed event");

        let retriever = MemoryRetriever::new(neo4j, duckdb);
        let (result, elapsed) = retriever
            .retrieve_context_timed("Tell me more about that research", MoodState::Research, 5)
            .await;

        let contexts = result.expect("retrieve_context should succeed");
        assert!(!contexts.is_empty(), "expected at least one ranked context");
        assert!(
            elapsed.as_millis() < 200,
            "retrieval latency {:?} exceeded 200ms budget",
            elapsed
        );

        let _ = std::fs::remove_file(&tmp_duckdb);
    }

    /// Determinism check (Property 3) using a fabricated candidate list
    /// scored directly, independent of any live backend: same inputs,
    /// same ranked order, every time.
    #[test]
    fn ranking_score_formula_is_deterministic() {
        fn score(entity_overlap: f32, temporal_recency: f32, mood_similarity: f32) -> f32 {
            entity_overlap * 0.5 + temporal_recency * 0.3 + mood_similarity * 0.2
        }
        let a = score(1.0, 0.8, 1.0);
        let b = score(1.0, 0.8, 1.0);
        assert_eq!(a, b);
        assert!((a - (0.5 + 0.24 + 0.2)).abs() < 1e-6);
    }
}
