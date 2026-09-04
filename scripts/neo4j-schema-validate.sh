#!/usr/bin/env bash
# neo4j-schema-validate.sh — verifies the Miranda graph schema (Task 5)
# was actually applied to the live miranda-neo4j container: constraints,
# indexes, and the 8 seeded MoodState nodes.
set -euo pipefail

CONTAINER_NAME="miranda-neo4j"
NEO4J_USER="neo4j"
NEO4J_PASS="mirandamemory"

run_cypher() {
    podman exec "$CONTAINER_NAME" cypher-shell -u "$NEO4J_USER" -p "$NEO4J_PASS" "$1"
}

echo "== Constraints =="
CONSTRAINTS=$(run_cypher "SHOW CONSTRAINTS")
echo "$CONSTRAINTS"

for name in conversation_id_unique entity_key_unique mood_state_name_unique research_thread_id_unique; do
    if echo "$CONSTRAINTS" | grep -q "$name"; then
        echo "OK: constraint $name present"
    else
        echo "FAIL: constraint $name missing" >&2
        exit 1
    fi
done

echo "== Indexes =="
INDEXES=$(run_cypher "SHOW INDEXES")
echo "$INDEXES"

for name in conversation_timestamp_idx conversation_mood_state_idx entity_name_idx entity_name_type_idx research_thread_timestamp_idx; do
    if echo "$INDEXES" | grep -q "$name"; then
        echo "OK: index $name present"
    else
        echo "FAIL: index $name missing" >&2
        exit 1
    fi
done

echo "== MoodState seed nodes =="
COUNT=$(run_cypher "MATCH (m:MoodState) RETURN count(m) AS c" | tail -n1 | tr -d '"')
echo "MoodState node count: $COUNT"
if [ "$COUNT" -ge 8 ]; then
    echo "OK: 8 MoodState nodes present"
else
    echo "FAIL: expected >=8 MoodState nodes, got $COUNT" >&2
    exit 1
fi

echo "Neo4j schema validation: PASS"
