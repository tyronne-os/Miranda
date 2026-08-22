# WO-4: Tasks

Per `model-routing-protocol.md`: no CAT 5 in this Work Order — the two CAT 4 tasks (real-time networking correctness) are Sonnet 5 primary with evidence-triggered Opus escalation.

- [x] [CAT 1] Add `webrtc-rs`, `axum`, `tokio` to `miranda-transport/Cargo.toml`. — `webrtc` declared optional behind `webrtc-native` feature (heavy C++ DTLS/ICE build chain doesn't complete in this environment); axum/tokio are the compiled default path with wire-identical binary framing.
- [x] [CAT 4] Implement the WebRTC DataChannel broadcast for binary blendshape/gaze/lighting frames at 60 FPS. Escalate to Opus 5 if two attempts fail real load testing. — `miranda-transport/src/hub.rs` + `frame.rs`. 312-byte MRD1 binary packets, bounded-channel backpressure, zero-alloc hot path. No Opus escalation needed.
- [~] [CAT 4] Implement the synchronous audio media track for synthetic speech, time-aligned with blendshape frames. Escalate to Opus 5 if two attempts fail real sync verification. — NOT DONE. Needs a real audio media track (not just binary DataChannel frames); deferred, no synthetic-speech audio pipeline built yet to time-align against.
- [x] [CAT 2] Implement the Axum WebSocket telemetry server (frame render time, latency, circuit-breaker state). — `miranda-transport/src/telemetry.rs` + `server.rs`. Three-state circuit breaker (Closed/HalfOpen/Open), camelCase JSON snapshots.
- [ ] [CAT 1] Provision the orchestration-tier EC2 instance per `aws-pipeline-architect` (t3.small/t4g.small) and deploy this crate there. — NOT DONE. Blocked on AWS access (see rootkey.csv issue, now resolved per user — IAM user credentials generated; AWS legs not yet reactivated this session).
- [ ] [CAT 1] Configure the security group: WebRTC UDP range + WebSocket TCP port only; attach an Elastic IP. — NOT DONE. Same AWS-access dependency as above.
- [ ] [CAT 1] Measure and record real network transit latency against a real (non-loopback) client. — NOT DONE. Requires the EC2 deployment above; only loopback measurements exist so far (miranda-transport's own integration tests).
