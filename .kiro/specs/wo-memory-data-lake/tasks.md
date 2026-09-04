# Implementation Plan: Memory Data Lake for Miranda

## Overview

Builds the bi-directional memory system: vault init, mood classifier, entity extractor, Neo4j graph, DuckDB index, Obsidian vault, event writer, retriever, LLM prompt injection, avatar color binding, and full integration testing.

## Task Dependency Graph

```json
{
  "waves": [
    {"wave": 1, "tasks": [1, 2, 3, 4, 7]},
    {"wave": 2, "tasks": [5, 6, 8]},
    {"wave": 3, "tasks": [9, 10]},
    {"wave": 4, "tasks": [11, 12]},
    {"wave": 5, "tasks": [13]},
    {"wave": 6, "tasks": [14]}
  ]
}
```

## Tasks

- [ ] 1. Vault directory structure + encryption init [CAT 1]
  - Create `/mnt/NOBILITY_VAULT/.miranda/` with `datalake/events/`, `datalake/indexes/`, `obsidian/`, `config/`
  - Generate and store local libsodium encryption keys in `config/keys.enc`
  - Write `scripts/init-vault.sh` and a vault health-check script
  - _Requirements: 1.1, 1.2, 1.4_

- [ ] 2. Mood classifier (local model) [CAT 2]
  - Select and quantize a lightweight local model (<100MB) for mood classification
  - Implement `miranda-nodes/src/memory/mood_classifier.rs` with <50ms inference
  - Test against 20 labeled inputs, verify >85% accuracy
  - _Requirements: 7.1_

- [ ] 3. Entity extractor (NER) [CAT 2]
  - Select and integrate a local NER model (spaCy small or HF `dslim/bert-base-NER`)
  - Implement `miranda-nodes/src/memory/entity_extractor.rs` with <100ms inference
  - Test against 20 labeled conversations, verify precision >80%, recall >75%
  - _Requirements: 2.2_

- [ ] 4. Neo4j local Podman container setup [CAT 1]
  - Write rootless Podman startup script for `neo4j:5.x`
  - Mount `/mnt/NOBILITY_VAULT/.miranda/neo4j-data/` for persistence
  - Write `scripts/neo4j-health-check.sh`, verify Bolt port 7687 connectivity from Rust
  - _Requirements: 2.5_

- [ ] 5. Neo4j graph schema creation [CAT 2]
  - Write Cypher DDL for Conversation, Entity, MoodState, ResearchThread nodes and MENTIONS, HAS_MOOD, RELATES_TO, FOLLOWS, INCLUDES relationships
  - Create indexes on timestamp, mood_state, entity_name
  - Write `scripts/neo4j-schema.cypher` and validation script
  - _Requirements: 2.1, 2.2, 2.3_

- [ ] 6. Neo4j writer module [CAT 3]
  - Add `neo4rs` to `miranda-nodes/Cargo.toml`
  - Implement `miranda-nodes/src/memory/neo4j_writer.rs` with async Cypher writes, <50ms latency, exponential-backoff retry
  - Unit test Cypher generation; integration test against real Neo4j container (100 events)
  - _Requirements: 2.1, 2.2, 2.3, 2.5_

- [ ] 7. DuckDB schema + initialization [CAT 1]
  - Create `index.duckdb` with `events` and `entities` tables and required indexes
  - Write `scripts/duckdb-init.sh`
  - Verify example mood/entity queries run in under 100ms
  - _Requirements: 4.3, 4.4_

- [ ] 8. Event writer (core pipeline) [CAT 3]
  - Implement `miranda-nodes/src/memory/event_writer.rs`: mood classify → entity extract → write immutable JSONL → dispatch to Neo4j/DuckDB/Obsidian via tokio channels
  - Enforce date-based JSONL file rotation and append-only durability
  - End-to-end latency <500ms; failures in one backend must not block others
  - _Requirements: 4.1, 4.2, 6.1_

- [ ] 9. Obsidian vault writer [CAT 2]
  - Implement `miranda-nodes/src/memory/obsidian_writer.rs` writing daily markdown notes
  - Generate bidirectional links for entities and mood-cluster notes
  - Test: write 50 events, verify markdown structure and link integrity
  - _Requirements: 3.1, 3.2, 3.3_

- [ ] 10. Memory retriever (query engine) [CAT 3]
  - Implement `miranda-nodes/src/memory/retriever.rs`: Neo4j entity-relationship traversal + DuckDB mood-continuity queries
  - Implement ranking (entity overlap 0.5, temporal recency 0.3, mood similarity 0.2)
  - Retrieval latency <200ms; integration test against real backends with labeled fixtures
  - _Requirements: 5.1, 5.2, 5.4, 5.5_

- [ ] 11. System prompt injection (LLM integration) [CAT 2]
  - Implement `miranda-nodes/src/memory/prompt_injection.rs` formatting retrieved contexts into natural-language system message
  - Wire into existing LLM inference call site in `miranda-nodes`
  - Test with 10 sample conversations verifying injected context appears in output; total added latency <500ms
  - _Requirements: 5.3, 5.4_

- [ ] 12. Avatar ARB color binding [CAT 2]
  - Define client-side mood-color mapping in `client-apps/web/src/memory/mood_colors.ts`
  - Add WebSocket listener for mood updates and smooth Three.js/WebGL color transitions
  - Verify color update latency <16ms across 5 sample conversations
  - _Requirements: 7.1, 7.2_

- [ ] 13. Integration tests [CAT 3]
  - Write `miranda-nodes/tests/memory_integration_tests.rs` covering write→retrieve round trip, mood/entity accuracy, ranking correctness, avatar color sync
  - Run against real Neo4j + DuckDB test containers
  - Record pass/fail and latency numbers as verification evidence
  - _Requirements: 2.4, 4.4, 5.4_

- [ ] 14. Performance benchmarks & documentation [CAT 1]
  - Write `scripts/memory-benchmarks.sh` measuring all latency targets from the design doc
  - Verify 100 events/second sustained throughput, peak memory <500MB, Neo4j container <2GB
  - Write `MEMORY_SYSTEM.md` and `docs/memory-performance-report.md` with real measured numbers
  - _Requirements: 2.4, 4.4, 5.4_

## Notes

CAT tags follow the CAT-5 Model Routing Protocol. No CAT 4/5 tasks exist in this spec — the hardest tasks (Neo4j writer, event writer, retriever, integration tests) are CAT 3, routed to Amazon Nova Pro. All tasks require real command-output verification per the build-standards rule; no task is complete on code-review confidence alone.

