#!/usr/bin/env bash
# neo4j-health-check.sh — verifies the Miranda Neo4j Podman container is
# running and that the Bolt protocol port (7687) is actually accepting TCP
# connections (not just that the container process exists).
set -euo pipefail

CONTAINER_NAME="miranda-neo4j"

if ! command -v podman >/dev/null 2>&1; then
    echo "FAIL: podman is not installed." >&2
    exit 1
fi

STATE=$(podman inspect -f '{{.State.Status}}' "$CONTAINER_NAME" 2>/dev/null || echo "missing")

if [ "$STATE" != "running" ]; then
    echo "FAIL: container $CONTAINER_NAME state is '$STATE' (expected 'running')." >&2
    exit 1
fi

echo "OK: container $CONTAINER_NAME is running."

if (exec 3<>/dev/tcp/127.0.0.1/7687) 2>/dev/null; then
    exec 3>&-
    echo "OK: Bolt port 7687 is accepting TCP connections."
else
    echo "FAIL: Bolt port 7687 is not accepting connections." >&2
    exit 1
fi

if (exec 3<>/dev/tcp/127.0.0.1/7474) 2>/dev/null; then
    exec 3>&-
    echo "OK: HTTP browser port 7474 is accepting TCP connections."
else
    echo "WARN: HTTP browser port 7474 is not responding (non-fatal)."
fi

echo "Neo4j health check: PASS"
