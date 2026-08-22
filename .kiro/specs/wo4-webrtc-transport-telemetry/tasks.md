# WO-4: Tasks

- [ ] Add `webrtc-rs`, `axum`, `tokio` to `miranda-transport/Cargo.toml`.
- [ ] Implement the WebRTC DataChannel broadcast for binary blendshape/gaze/lighting frames at 60 FPS.
- [ ] Implement the synchronous audio media track for synthetic speech.
- [ ] Implement the Axum WebSocket telemetry server (frame render time, latency, circuit-breaker state).
- [ ] Provision the orchestration-tier EC2 instance per `aws-pipeline-architect` (t3.small/t4g.small) and deploy this crate there.
- [ ] Configure the security group: WebRTC UDP range + WebSocket TCP port only; attach an Elastic IP.
- [ ] Measure and record real network transit latency against a real (non-loopback) client.
