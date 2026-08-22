# WO-5: Tasks

- [ ] Audit `client-apps/web/src/data/aceTopology.ts` against the real Miranda-Engine crate set; update node definitions to match (currently reflects the old eve-ecc node set: mic/presence/syncer/riva-asr/nemotron/riva-tts/a2f/animgraph/omniverse).
- [ ] Rewire `src/lib/ace/controllerClient.ts` and `spatialSyncer.ts` to speak to the new WO-4 Rust WebSocket telemetry server instead of the old Node.js `ace-controller`.
- [ ] Verify `src/lib/stageMachine/` L0/L1/L2 logic still applies cleanly to the new backend, or adapt it.
- [ ] Build the new WebGPU viewport component (WGSL shaders, DataChannel frame ingestion, Gaussian-splat deformation) — this is genuinely new work, not a port.
- [ ] Source or stub a placeholder Gaussian-splat asset to develop the viewport against while the real GaussianAvatars/FLAME/TetGS pipeline (`live-avatar-expert` skill) is built separately.
- [ ] Verify the No Loop Video Protocol against actually-rendered frames (screen-capture or a frame-diff harness), not code review alone — this is exactly the failure mode `ORCHESTRATION-PIVOT.md` documents from the first attempt.
- [ ] Measure real glass-to-glass latency end-to-end.
