//! WO-Memory — Miranda's bi-directional local memory system.
//!
//! Per `.kiro/specs/wo-memory-data-lake/design.md`. This module currently
//! hosts the mood classifier and entity extractor (Tasks 2-3). Later tasks
//! (Neo4j/DuckDB/Obsidian writers, retriever, prompt injection) add
//! sibling submodules here without touching these two.

pub mod duckdb_writer;
pub mod entity_extractor;
pub mod event_writer;
pub mod mood_classifier;
pub mod neo4j_writer;
