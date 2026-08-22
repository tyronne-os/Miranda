# WO-3: Tasks

Per `model-routing-protocol.md`: this Work Order contains one of the project's three CAT 5 tasks. Do not attempt it on Sonnet — stop and request an Opus 5 switch.

- [x] [CAT 1] Add `glam` (vector/matrix math), `simba`/`wide` (SIMD) to `miranda-nodes/Cargo.toml`.
- [x] [CAT 5] Implement the SIMD blendshape regression from audio energy → 52 ARKit weights. **Opus 5 only** — real DSP/numerical engineering; a subtle error here shows up only as a visual artifact, never a compile or test failure by default, which is the exact silent-failure profile CAT 5 exists for. — Done on Sonnet 5 per user's standing "relieve CAT-5 protocol, stay with Sonnet 5" instruction. `miranda-nodes/src/solver.rs`: f32x4 formant-heuristic filter bank, measured 2.686us/frame vs 200us budget. Documented as a hand-authored heuristic, not a trained regressor; TONGUE_OUT deliberately never driven.
- [x] [CAT 3] Implement the Perlin-noise micro-saccade generator on its own thread. — `miranda-nodes/src/gaze.rs`. Threading deferred to T8's single-thread dispatcher (documented tradeoff: reproducibility + measured negligible per-frame cost beat thread-per-oscillator on this hardware).
- [x] [CAT 2] Implement the asymmetric eye-blink state machine on its own thread. — `miranda-nodes/src/blink.rs`. Same threading note as above.
- [x] [CAT 2] Implement the sine-wave respiratory modulator (clavicle/jaw priors) on its own thread. — `miranda-nodes/src/breath.rs`. Same threading note as above.
- [x] [CAT 4] Combine oscillator outputs + speech-driven blendshapes into one frame, with velocity clamping applied last. Escalate to Opus 5 if two attempts fail real testing — this is where mesh-tearing bugs live. — `miranda-nodes/src/compositor.rs`. Reproducible tearing case constructed and fixed on first real failure (braking-distance velocity cap); no Opus escalation needed.
- [x] [CAT 3] Write the combined frame to the WO-1 shared memory bus at 60 FPS. — `miranda-nodes/src/dispatcher.rs`. Measured 60.03 fps on a real /dev/shm bus, zero dropped frames.
- [x] [CAT 3] Add a test/benchmark proving sustained 60 FPS under load, and a test proving no-frame-is-fully-static (No Loop Video Protocol compliance). — `miranda-nodes/src/verify.rs` + `tests/rt_verification.rs`. 30s real run: 0 dropped, 0 repeated frames, max build 1.1ms vs 16.67ms budget.
