# WO-3: Tasks

Per `model-routing-protocol.md`: this Work Order contains one of the project's three CAT 5 tasks. Do not attempt it on Sonnet — stop and request an Opus 5 switch.

- [ ] [CAT 1] Add `glam` (vector/matrix math), `simba`/`wide` (SIMD) to `miranda-nodes/Cargo.toml`.
- [ ] [CAT 5] Implement the SIMD blendshape regression from audio energy → 52 ARKit weights. **Opus 5 only** — real DSP/numerical engineering; a subtle error here shows up only as a visual artifact, never a compile or test failure by default, which is the exact silent-failure profile CAT 5 exists for.
- [ ] [CAT 3] Implement the Perlin-noise micro-saccade generator on its own thread.
- [ ] [CAT 2] Implement the asymmetric eye-blink state machine on its own thread.
- [ ] [CAT 2] Implement the sine-wave respiratory modulator (clavicle/jaw priors) on its own thread.
- [ ] [CAT 4] Combine oscillator outputs + speech-driven blendshapes into one frame, with velocity clamping applied last. Escalate to Opus 5 if two attempts fail real testing — this is where mesh-tearing bugs live.
- [ ] [CAT 3] Write the combined frame to the WO-1 shared memory bus at 60 FPS.
- [ ] [CAT 3] Add a test/benchmark proving sustained 60 FPS under load, and a test proving no-frame-is-fully-static (No Loop Video Protocol compliance).
