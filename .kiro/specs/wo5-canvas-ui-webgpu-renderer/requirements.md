# WO-5: Reactive Canvas UI & WebGPU Edge Splat Renderer — Requirements

**Role**: Lead Frontend and Graphics Engineer. **Depends on**: WO-3 (kinematics output) + WO-4 (transport). **Target**: sub-150ms glass-to-glass loop.

**Important — this Work Order is NOT a from-scratch build.** `client-apps/web` in this repo already has a working `@xyflow/react` node graph (the "ACE Cortex" topology) matching much of this spec, carried forward from the first attempt (eve-ecc). Extend it; do not rewrite it.

## Requirements (EARS notation)

1. WHEN the client loads THE SYSTEM SHALL render the node graph matching the Miranda-Engine topology (the 6 crates + their real signal-path connections) with visual status indicators and dynamic latency meters. (Largely already implemented in `client-apps/web/src/components/pipeline/` — verify against the current WO-1 through WO-4 crate names, which have changed since the original eve-ecc topology was drawn.)
2. WHEN a WO-4 telemetry WebSocket message arrives THE SYSTEM SHALL update the corresponding node's visual metrics in real time.
3. WHEN the WebGPU canvas viewport is active THE SYSTEM SHALL ingest spatial blendshape frames over the WO-4 WebRTC DataChannel and deform the 3D Gaussian Splatting representation of EVE using native WGSL shaders.
4. THE SYSTEM SHALL render locally at screen resolution without server-side video compression (no video codec round-trip — raw frame data drives the local GPU renderer directly).
5. THE SYSTEM SHALL satisfy the Instant Presence Standard: waist-up mid-frame composition, instant aliveness from first frame, waving as the opening gesture, choreographed micromovement — see `eve-ecc-docs/INSTANT-PRESENCE-STANDARD.md`.

## Acceptance criteria

- Measured glass-to-glass latency (blendshape generated → pixel rendered) under sub-150ms, real measurement not estimate.
- No canned idle-loop video anywhere in the render path — verify by observing painted pixels over time, not just code intent (per the ORCHESTRATION-PIVOT.md lesson on measuring the wrong metric).
- The actual hard part: there is no trained/rigged Gaussian-splat avatar to render yet. This Work Order's WebGPU/WGSL viewport work can and should proceed against a placeholder/test splat while the GaussianAvatars+FLAME+TetGS pipeline (a separate, research-heavy effort — see `live-avatar-expert` skill) is built out. Don't block the UI/transport work on the rendering research being finished first.
