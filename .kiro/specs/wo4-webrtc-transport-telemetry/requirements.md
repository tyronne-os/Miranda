# WO-4: Full-Duplex WebRTC Transport & Telemetry Hub — Requirements

**Role**: Network Protocol Architect. **Depends on**: WO-3 (frames to stream). **Target**: <15ms network transit.

## Requirements (EARS notation)

1. WHEN a blendshape/gaze/lighting frame is available THE SYSTEM SHALL broadcast it over a dedicated `webrtc-rs` DataChannel to all connected browsers at 60 FPS.
2. WHEN synthetic speech audio is generated THE SYSTEM SHALL stream it as a synchronous WebRTC audio media track, time-aligned with the corresponding blendshape frames.
3. WHEN a node's operational state changes (frame render time, latency, circuit-breaker trip) THE SYSTEM SHALL broadcast that metric over an Axum-based WebSocket server to connected management tools in real time.
4. IF a client's connection degrades (dropped frames, high RTT) THE SYSTEM SHALL surface that degradation as a telemetry event rather than silently dropping data with no signal.
5. WHEN deployed on AWS per the `aws-pipeline-architect` skill THE SYSTEM SHALL run on the low-cost orchestration tier (`t3.small`/`t4g.small`), not a GPU instance — this Work Order has no GPU dependency.

## Acceptance criteria

- Real measured network transit latency under a real client connection, not a loopback-only test.
- Security group restricted to exactly the required WebRTC UDP range + the WebSocket TCP port (per `aws-pipeline-architect`'s network traversal rule) — no broad open ranges.
- Elastic IP assigned for stable NAT traversal, per the same skill.
