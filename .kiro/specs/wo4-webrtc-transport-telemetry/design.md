# WO-4: Design

## Architecture

`miranda-transport` depends only on `miranda-core` — it should be able to stream *any* well-formed frame payload without knowing how WO-3 computed it, keeping the transport layer swappable independent of the kinematics engine.

## Two logical servers, one crate

1. **Data-plane WebRTC hub**: DataChannels for binary frames (blendshapes/gaze/lighting) + a synchronous audio media track. This is the latency-critical path (<15ms target) — keep serialization minimal (raw binary, not JSON) here.
2. **Control-plane telemetry WebSocket** (Axum): JSON is fine here since it's metrics/observability, not the hot path. This is what `aws-pipeline-architect`'s CloudWatch guardrail and any external dashboard would consume.

## AWS placement

Per `aws-pipeline-architect`: this Work Order is CPU/network-bound only, runs on the cheap orchestration-tier EC2 instance alongside WO-1/2/3, never the GPU box. Security group: WebRTC UDP range + WebSocket TCP port only, Elastic IP attached.

## Cross-reference

Full Hermes Execution Prompt: `nobility-posh-framework` skill. AWS deployment rules: `aws-pipeline-architect` skill.
