# WO-4: Tasks

Per `model-routing-protocol.md`: no CAT 5 in this Work Order — the two CAT 4 tasks (real-time networking correctness) are Sonnet 5 primary with evidence-triggered Opus escalation.

- [ ] [CAT 1] Add `webrtc-rs`, `axum`, `tokio` to `miranda-transport/Cargo.toml`.
- [ ] [CAT 4] Implement the WebRTC DataChannel broadcast for binary blendshape/gaze/lighting frames at 60 FPS. Escalate to Opus 5 if two attempts fail real load testing.
- [ ] [CAT 4] Implement the synchronous audio media track for synthetic speech, time-aligned with blendshape frames. Escalate to Opus 5 if two attempts fail real sync verification.
- [ ] [CAT 2] Implement the Axum WebSocket telemetry server (frame render time, latency, circuit-breaker state).
- [ ] [CAT 1] Provision the orchestration-tier EC2 instance per `aws-pipeline-architect` (t3.small/t4g.small) and deploy this crate there.
- [ ] [CAT 1] Configure the security group: WebRTC UDP range + WebSocket TCP port only; attach an Elastic IP.
- [ ] [CAT 1] Measure and record real network transit latency against a real (non-loopback) client.
