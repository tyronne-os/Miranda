#!/usr/bin/env bash
# neo4j-start.sh — rootless Podman startup for the local Neo4j knowledge
# graph backend used by Miranda's memory system.
#
# Mounts the vault's neo4j-data directory for persistence so graph state
# survives container restarts and lives under the encrypted vault root
# alongside the rest of Miranda's memory (datalake, obsidian).
set -euo pipefail

PREFERRED_ROOT="/mnt/NOBILITY_VAULT/.miranda"
FALLBACK_ROOT="${HOME}/NOBILITY_VAULT/.miranda"

if [ -d "$PREFERRED_ROOT" ]; then
    VAULT_ROOT="$PREFERRED_ROOT"
else
    VAULT_ROOT="$FALLBACK_ROOT"
fi

DATA_DIR="$VAULT_ROOT/neo4j-data"
CONTAINER_NAME="miranda-neo4j"
NEO4J_IMAGE="docker.io/library/neo4j:5.24"

mkdir -p "$DATA_DIR/data" "$DATA_DIR/logs"

if ! command -v podman >/dev/null 2>&1; then
    echo "FAIL: podman is not installed. Install it first (e.g. sudo apt-get install -y podman) and re-run this script." >&2
    exit 1
fi

if podman ps -a --format '{{.Names}}' | grep -qx "$CONTAINER_NAME"; then
    echo "Container $CONTAINER_NAME already exists — starting it."
    podman start "$CONTAINER_NAME"
else
    echo "Creating and starting rootless container $CONTAINER_NAME from $NEO4J_IMAGE"
    podman run -d \
        --name "$CONTAINER_NAME" \
        --userns=keep-id \
        -p 7474:7474 \
        -p 7687:7687 \
        -v "$DATA_DIR/data:/data:Z" \
        -v "$DATA_DIR/logs:/logs:Z" \
        -e NEO4J_AUTH=neo4j/mirandamemory \
        -e NEO4J_ACCEPT_LICENSE_AGREEMENT=yes \
        "$NEO4J_IMAGE"
fi

echo "Waiting for Neo4j to accept Bolt connections on port 7687..."
for i in $(seq 1 30); do
    if (exec 3<>/dev/tcp/127.0.0.1/7687) 2>/dev/null; then
        exec 3>&-
        echo "Neo4j is accepting connections on port 7687 (waited ~${i}s)."
        exit 0
    fi
    sleep 1
done

echo "WARNING: Neo4j did not open port 7687 within 30s. Check 'podman logs $CONTAINER_NAME'." >&2
exit 1
