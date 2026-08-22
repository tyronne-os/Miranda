# WO-3: Tasks

- [ ] Add `glam` (vector/matrix math), `simba`/`wide` (SIMD) to `miranda-nodes/Cargo.toml`.
- [ ] Implement the SIMD blendshape regression from audio energy → 52 ARKit weights.
- [ ] Implement the Perlin-noise micro-saccade generator on its own thread.
- [ ] Implement the asymmetric eye-blink state machine on its own thread.
- [ ] Implement the sine-wave respiratory modulator (clavicle/jaw priors) on its own thread.
- [ ] Combine oscillator outputs + speech-driven blendshapes into one frame, with velocity clamping applied last.
- [ ] Write the combined frame to the WO-1 shared memory bus at 60 FPS.
- [ ] Add a test/benchmark proving sustained 60 FPS under load, and a test proving no-frame-is-fully-static (No Loop Video Protocol compliance).
