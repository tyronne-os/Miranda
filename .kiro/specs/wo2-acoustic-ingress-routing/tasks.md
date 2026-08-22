# WO-2: Tasks

Per `model-routing-protocol.md`: no CAT 5 in this Work Order — the hardest single task (parakeet.cpp FFI) is CAT 4, Sonnet 5 primary with evidence-triggered Opus escalation.

- [ ] [CAT 3] Add `cpal` to `miranda-audio/Cargo.toml`; implement non-blocking mic capture writing into the WO-1 audio ring.
- [ ] [CAT 2] Integrate Silero VAD (find a Rust binding or ONNX-runtime path — verify latency claim, don't assume it).
- [ ] [CAT 4] Locate this project's existing built `parakeet.cpp` binary/library (see `llamacpp-huggingface-expert` skill for its exact path/build flags) and write the `cxx` FFI binding against it. Escalate to Opus 5 if two attempts fail — cross-language ABI/memory-ownership bugs are exactly the CAT 4 profile.
- [ ] [CAT 3] Wire PCM frames from the ring buffer directly into the FFI call — no intermediate file/socket hop.
- [ ] [CAT 3] Implement the Nemotron-Flash routing interface in `miranda-supervisor`: intent parsing, turn-taking state, dispatch signal to TTS/motion.
- [ ] [CAT 2] Add a rolling lookahead audio buffer for coarticulation shaping.
- [ ] [CAT 1] Measure real end-to-end latency (mic → transcript) and record the actual number, not the target.
