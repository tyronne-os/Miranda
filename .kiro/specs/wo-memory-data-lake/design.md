# Design Document

## Overview

The Memory Data Lake gives Miranda a bi-directional, local-first memory system built on three coordinated backends: a Neo4j knowledge graph for associative reasoning, an Obsidian vault for human-readable browsing, and a DuckDB-indexed JSONL event log for fast analytics. Every conversation turn is written once (incoming path) and queried before every future response (outgoing path), so memory actively shapes conversation rather than just recording it.

## Architecture

```
User/Miranda Turn
      │
      ▼
Entity Extractor ──┐
                    │
Mood Classifier ────┼──► Event Writer (JSONL, immutable)
                    │           │
                    │   ┌───────┼────────┐
                    │   ▼       ▼        ▼
                    │ Neo4j  Obsidian  DuckDB
                    │ (graph) (notes)  (index)
                    │
                    ▼
            Memory Retriever (queries Neo4j + DuckDB)
                    │
                    ▼
            System Prompt Injection
                    │
                    ▼
              LLM Inference
```

Incoming path: user_message + miranda_response → entity extraction + mood classification → immutable JSONL event → parallel writes to Neo4j, Obsidian, DuckDB.

Outgoing path: new user_message → Neo4j graph traversal + DuckDB mood/entity lookup → ranked top-K contexts → formatted into system prompt → passed to LLM before inference.

## Components and Interfaces

### Entity Extractor
- Input: raw turn text
- Output: `Vec<(entity_name: String, entity_type: EntityType, confidence: f32)>`
- Interface: `extract_entities(text: &str) -> Vec<Entity>`

### Mood Classifier
- Input: raw turn text
- Output: `MoodState` enum + confidence
- Interface: `classify_mood(text: &str) -> (MoodState, f32)`

### Event Writer
- Input: `ConversationTurn { user_message, miranda_response, turn_id, timestamp }`
- Output: immutable JSONL line appended to `datalake/events/{date}.jsonl`, plus dispatch to Neo4j/Obsidian/DuckDB writers via tokio channels
- Interface: `write_event(turn: ConversationTurn) -> Result<EventId, WriteError>`

### Neo4j Writer/Reader
- Interface: `write_conversation_node(event: &Event) -> Result<(), Neo4jError>`
- Interface: `query_related_conversations(entities: &[String], before: Timestamp, limit: usize) -> Vec<ConversationNode>`

### Obsidian Writer
- Interface: `append_daily_note(date: Date, entry: NoteEntry) -> Result<(), IoError>`

### DuckDB Index
- Interface: `index_event(event: &Event) -> Result<(), DbError>`
- Interface: `query_by_mood(mood: MoodState, since: Timestamp, limit: usize) -> Vec<EventRow>`

### Memory Retriever
- Interface: `retrieve_context(current_message: &str, current_mood: MoodState) -> Vec<RetrievedContext>`
- Ranking: weighted score = entity_overlap * 0.5 + temporal_recency * 0.3 + mood_similarity * 0.2

## Data Models

```rust
struct Event {
    event_id: Uuid,
    timestamp: DateTime<Utc>,
    event_type: String,
    user_message: String,
    miranda_response: String,
    entities_extracted: Vec<String>,
    mood_state: MoodState,
    mood_color: String,      // hex RGB
    graph_updates: Vec<GraphUpdate>,
    obsidian_note_id: Uuid,
    retrieval_context_used: Vec<Uuid>,
}

enum MoodState {
    Research, Curiosity, Disappointment, Casual, Intimate, Frustrated, Excited,
}

struct RetrievedContext {
    conversation_id: Uuid,
    relevance_score: f32,
    summary: String,
    mood_state: MoodState,
    timestamp: DateTime<Utc>,
}
```

Neo4j schema: `Conversation`, `Entity`, `MoodState`, `ResearchThread` nodes; `MENTIONS`, `HAS_MOOD`, `RELATES_TO`, `FOLLOWS`, `INCLUDES` relationships.

DuckDB schema: `events(event_id, timestamp, event_type, user_message, miranda_response, entities, mood_state, mood_rgb, mood_hsl)` and `entities(entity_name, entity_type, first_mention, last_mention, mention_count, mood_contexts)`, indexed on timestamp, mood_state, and entities.

## Correctness Properties

### Property 1: Write durability
Every completed conversation turn produces exactly one immutable JSONL event; a crash mid-write never produces a partial/corrupt line (write-then-rename or append-only with fsync).

**Validates: Requirements 4.1**

### Property 2: No data loss on backend failure
If Neo4j or DuckDB is unreachable, the event is still durably recorded in JSONL and queued for retry — the JSONL log is the source of truth.

**Validates: Requirements 2.5**

### Property 3: Retrieval determinism
Given the same graph/index state, the same query returns the same ranked results.

**Validates: Requirements 5.2**

### Property 4: Privacy invariant
No code path in the memory system makes an outbound network call to a non-local address.

**Validates: Requirements 1.3**

## Error Handling

- Neo4j/DuckDB/Obsidian write failures are retried with exponential backoff (max 5 attempts); after exhaustion, the failure is logged and the event remains in JSONL for manual replay.
- Malformed or unparseable entity/mood extraction results in the event being stored with `entities_extracted: []` or a `Unknown` mood rather than blocking the write.
- Retrieval queries that exceed a 200ms timeout return whatever partial results are available rather than blocking LLM inference.

## Testing Strategy

- Unit tests per component (entity extractor, mood classifier, writers, retriever ranking logic) using mocked backends.
- Integration tests against real local Neo4j (Podman) and DuckDB instances: write N events, verify graph/index state, verify retrieval ranking against labeled fixtures.
- Load test: sustain 100 events/second, verify no dropped writes and retrieval latency stays under 200ms.
- Privacy test: network traffic capture during a full test run verifies zero outbound connections beyond localhost.

