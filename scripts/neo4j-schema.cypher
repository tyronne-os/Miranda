// neo4j-schema.cypher — Miranda memory knowledge-graph schema.
// Per .kiro/specs/wo-memory-data-lake/design.md.
//
// Node labels: Conversation, Entity, MoodState, ResearchThread
// Relationships: MENTIONS, HAS_MOOD, RELATES_TO, FOLLOWS, INCLUDES
//
// Idempotent: uses IF NOT EXISTS everywhere so this can be re-run safely
// against a live database without erroring or duplicating constraints.

// --- Uniqueness constraints (each implicitly creates a backing index) ---

CREATE CONSTRAINT conversation_id_unique IF NOT EXISTS
FOR (c:Conversation) REQUIRE c.conversation_id IS UNIQUE;

// NODE KEY constraints require Neo4j Enterprise Edition; this deployment
// runs Community Edition (verified via `CALL dbms.components()`), so
// composite uniqueness is enforced differently: a single-property
// uniqueness constraint on a computed `entity_key` field
// (`entity_name + '::' + entity_type`), maintained by the Neo4j writer at
// write time, plus a composite index below for query performance.
CREATE CONSTRAINT entity_key_unique IF NOT EXISTS
FOR (e:Entity) REQUIRE e.entity_key IS UNIQUE;

CREATE INDEX entity_name_type_idx IF NOT EXISTS
FOR (e:Entity) ON (e.entity_name, e.entity_type);

CREATE CONSTRAINT mood_state_name_unique IF NOT EXISTS
FOR (m:MoodState) REQUIRE m.name IS UNIQUE;

CREATE CONSTRAINT research_thread_id_unique IF NOT EXISTS
FOR (r:ResearchThread) REQUIRE r.thread_id IS UNIQUE;

// --- Additional indexes required by design.md / requirements.md ---
// (Requirement 2.4: sub-100ms related-conversation queries up to 1M edges;
//  Requirement 7.2: mood_state must be independently queryable.)

CREATE INDEX conversation_timestamp_idx IF NOT EXISTS
FOR (c:Conversation) ON (c.timestamp);

CREATE INDEX conversation_mood_state_idx IF NOT EXISTS
FOR (c:Conversation) ON (c.mood_state);

CREATE INDEX entity_name_idx IF NOT EXISTS
FOR (e:Entity) ON (e.entity_name);

CREATE INDEX research_thread_timestamp_idx IF NOT EXISTS
FOR (r:ResearchThread) ON (r.created_at);

// --- Seed the 8 canonical MoodState nodes (7 + Unknown) so HAS_MOOD ---
// --- relationships always have a target node to attach to. ---
// Requirement 8.2: new mood states must be addable later without
// altering existing stored data — this MERGE-based seed is safe to
// extend with more mood nodes without migrating anything.

MERGE (m:MoodState {name: 'research'})       ON CREATE SET m.color_hex = '#3B82C4';
MERGE (m:MoodState {name: 'curiosity'})      ON CREATE SET m.color_hex = '#F5A623';
MERGE (m:MoodState {name: 'disappointment'}) ON CREATE SET m.color_hex = '#6B7280';
MERGE (m:MoodState {name: 'casual'})         ON CREATE SET m.color_hex = '#8FD694';
MERGE (m:MoodState {name: 'intimate'})       ON CREATE SET m.color_hex = '#D46A9F';
MERGE (m:MoodState {name: 'frustrated'})     ON CREATE SET m.color_hex = '#D64545';
MERGE (m:MoodState {name: 'excited'})        ON CREATE SET m.color_hex = '#F5D547';
MERGE (m:MoodState {name: 'unknown'})        ON CREATE SET m.color_hex = '#444444';

// --- Node/relationship shape reference (documentation only, not DDL) ---
//
// (:Conversation {
//   conversation_id: STRING (uuid),
//   timestamp: DATETIME,
//   mood_state: STRING,           // denormalized for fast filtering
//   intimacy_level: FLOAT,        // Requirement 8.1 — Mahogany Hall readiness,
//                                  // present from day one, nullable, no migration needed
//   user_message: STRING,
//   miranda_response: STRING
// })
//
// (:Entity {
//   entity_name: STRING,
//   entity_type: STRING,          // PERSON | ORG | LOC | TECH | MISC
//   first_mention: DATETIME,
//   last_mention: DATETIME,
//   mention_count: INTEGER
// })
//
// (:MoodState { name: STRING, color_hex: STRING })
//
// (:ResearchThread {
//   thread_id: STRING (uuid),
//   title: STRING,
//   created_at: DATETIME
// })
//
// (:Conversation)-[:MENTIONS]->(:Entity)
// (:Conversation)-[:HAS_MOOD]->(:MoodState)
// (:Entity)-[:RELATES_TO]->(:Entity)
// (:Conversation)-[:FOLLOWS]->(:Conversation)     // temporal chain, prev -> next
// (:ResearchThread)-[:INCLUDES]->(:Conversation)
