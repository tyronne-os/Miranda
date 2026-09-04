# Requirements Document

## Introduction

Miranda requires a bi-directional, unrestricted memory system that outperforms CSM 1B Maya in context retention, relationship mapping, and conversational coherence. The system stores and retrieves conversation history, emotional states, research threads, and entity relationships using a local-first knowledge graph and data lake architecture, with zero cloud transmission and zero content restrictions on stored data.

## Glossary

- **Data Lake**: Unified, immutable, timestamped event log of all conversation and memory operations.
- **Knowledge Graph**: Neo4j-backed graph of entities, conversations, and relationships used for associative retrieval.
- **Mood State**: A classified emotional/conversational category (research, curiosity, disappointment, casual, intimate, frustrated, excited) with an associated color mapping.
- **Bi-directional memory**: Memory that both records new conversation (incoming) and actively informs future responses via retrieval (outgoing).
- **Vault**: The local, encrypted storage root at `/mnt/NOBILITY_VAULT/.miranda/`.

## Requirements

### Requirement 1: Local-First Privacy Architecture

**User Story:** As the user, I want all of Miranda's memory stored locally and encrypted, so that no conversation data ever leaves my machine.

#### Acceptance Criteria

1. WHEN the memory system initializes THEN the system SHALL create all storage (Obsidian vault, Neo4j data, data lake events) under `/mnt/NOBILITY_VAULT/.miranda/`.
2. WHEN any memory data is written to disk THEN the system SHALL encrypt it at rest using locally-generated keys.
3. WHEN the memory system operates THEN the system SHALL NOT transmit conversation data to any external network endpoint.
4. WHEN a user requests export or deletion of memory data THEN the system SHALL provide a script that performs the operation entirely locally.

### Requirement 2: Knowledge Graph Storage (Neo4j)

**User Story:** As Miranda, I want a graph database of entities and relationships, so that I can reason about associative context rather than flat chat history.

#### Acceptance Criteria

1. WHEN a conversation turn completes THEN the system SHALL create a Conversation node in Neo4j with timestamp and mood_state attributes.
2. WHEN entities are extracted from a turn THEN the system SHALL create or update Entity nodes and link them to the Conversation node via a MENTIONS relationship.
3. WHEN a mood is classified for a turn THEN the system SHALL link the Conversation node to a MoodState node via a HAS_MOOD relationship.
4. WHEN a graph query is issued for related conversations THEN the system SHALL return results in under 100ms for graphs up to 1 million edges.
5. IF the Neo4j container is unavailable THEN the system SHALL queue writes and retry with exponential backoff rather than dropping data.

### Requirement 3: Obsidian Vault Storage

**User Story:** As the user, I want conversation history stored as human-readable, linkable markdown notes, so that I can browse and search my history outside of the running application.

#### Acceptance Criteria

1. WHEN a conversation turn completes THEN the system SHALL append an entry to a daily markdown note under `/mnt/NOBILITY_VAULT/.miranda/obsidian/`.
2. WHEN entities are mentioned in a turn THEN the system SHALL generate bidirectional markdown links (`[[entity-name]]`) for first mentions.
3. WHEN a note is written THEN the system SHALL tag it with the mood state (e.g., `#mood/research`).
4. WHEN the vault grows to 10,000+ notes THEN full-text search SHALL remain functional through native Obsidian search.

### Requirement 4: Data Lake Event Log

**User Story:** As the user, I want an immutable, queryable log of every memory operation, so that I can audit, replay, or analyze Miranda's memory over time.

#### Acceptance Criteria

1. WHEN any conversation turn is processed THEN the system SHALL write an immutable JSONL event to `/mnt/NOBILITY_VAULT/.miranda/datalake/events/{date}.jsonl`.
2. WHEN an event is written THEN it SHALL include timestamp, user_message, miranda_response, entities_extracted, mood_state, mood_color, and retrieval_context_used.
3. WHEN analytics queries are needed THEN the system SHALL expose event data through a DuckDB SQL interface indexed on timestamp, mood_state, and entities.
4. WHEN a query filters by mood state over a date range THEN the system SHALL return results in under 100ms.

### Requirement 5: Bi-Directional Context Flow

**User Story:** As the user, I want Miranda's past context to actively inform her current responses, so that conversations feel continuous rather than starting fresh each time.

#### Acceptance Criteria

1. WHEN a new user message arrives THEN the system SHALL query the knowledge graph and data lake for related past contexts before LLM inference.
2. WHEN related contexts are retrieved THEN the system SHALL rank them by entity overlap, temporal recency, and mood continuity.
3. WHEN the top-K contexts are selected THEN the system SHALL format them as a natural-language system prompt injection.
4. WHEN retrieval and injection complete THEN the total added latency SHALL be under 200ms before the LLM call.
5. WHEN no relevant past context exists THEN the system SHALL proceed with LLM inference without injection rather than blocking.

### Requirement 6: Unrestricted Conversation Storage

**User Story:** As the user, I want the memory system itself to impose no content restrictions, so that Miranda's conversational freedom is preserved end-to-end.

#### Acceptance Criteria

1. WHEN a conversation turn is stored THEN the system SHALL NOT apply any content moderation or filtering to the stored text.
2. WHEN retrieval occurs THEN the system SHALL NOT exclude past conversations based on content classification.
3. WHEN the user requests a full conversation history replay at a given timestamp THEN the system SHALL reconstruct and return it without redaction.

### Requirement 7: Mood-Color Integration

**User Story:** As the user, I want every stored conversation tagged with a mood and corresponding color, so that Miranda's emotional/conversational state is visually and structurally traceable.

#### Acceptance Criteria

1. WHEN a conversation turn is processed THEN the system SHALL classify it into one of the defined mood states and attach an RGB/HSL color value.
2. WHEN mood state is stored THEN it SHALL be queryable independently in both Neo4j and DuckDB.
3. WHEN retrieval considers mood continuity THEN the system SHALL be able to bias results toward conversations sharing the current mood state.

### Requirement 8: Mahogany Hall Readiness

**User Story:** As the user, I want the memory architecture to support future intimacy/relationship tracking, so that the companionship app can reuse this system without redesign.

#### Acceptance Criteria

1. WHEN the schema is designed THEN it SHALL support an intimacy-level attribute on Conversation nodes without requiring schema migration.
2. WHEN mood states are extended for companionship contexts THEN new mood states SHALL be addable without altering existing stored data.

