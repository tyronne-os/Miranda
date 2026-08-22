# WO-5: Tasks

Per `model-routing-protocol.md`: this Work Order contains the third and final of the project's three CAT 5 tasks — arguably the hardest single task across the whole build (least precedent to lean on).

- [ ] [CAT 1] Audit `client-apps/web/src/data/aceTopology.ts` against the real Miranda-Engine crate set; update node definitions to match (currently reflects the old eve-ecc node set: mic/presence/syncer/riva-asr/nemotron/riva-tts/a2f/animgraph/omniverse).
- [ ] [CAT 3] Rewire `src/lib/ace/controllerClient.ts` and `spatialSyncer.ts` to speak to the new WO-4 Rust WebSocket telemetry server instead of the old Node.js `ace-controller`.
- [ ] [CAT 3] Verify `src/lib/stageMachine/` L0/L1/L2 logic still applies cleanly to the new backend, or adapt it.
- [ ] [CAT 5] Build the new WebGPU viewport component (WGSL shaders, DataChannel frame ingestion, Gaussian-splat deformation). **Opus 5 only** — genuinely novel graphics-shader engineering, the least-precedented task in the entire project.
- [ ] [CAT 2] Source or stub a placeholder Gaussian-splat asset to develop the viewport against while the real GaussianAvatars/FLAME/TetGS pipeline (`live-avatar-expert` skill) is built separately.
- [ ] [CAT 3] Verify the No Loop Video Protocol against actually-rendered frames (screen-capture or a frame-diff harness), not code review alone — this is exactly the failure mode `ORCHESTRATION-PIVOT.md` documents from the first attempt.
- [ ] [CAT 1] Measure real glass-to-glass latency end-to-end.
