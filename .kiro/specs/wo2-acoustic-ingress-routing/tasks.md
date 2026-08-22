# WO-2: Tasks

- [ ] Add `cpal` to `miranda-audio/Cargo.toml`; implement non-blocking mic capture writing into the WO-1 audio ring.
- [ ] Integrate Silero VAD (find a Rust binding or ONNX-runtime path — verify latency claim, don't assume it).
- [ ] Locate this project's existing built `parakeet.cpp` binary/library (see `llamacpp-huggingface-expert` skill for its exact path/build flags) and write the `cxx` FFI binding against it.
- [ ] Wire PCM frames from the ring buffer directly into the FFI call — no intermediate file/socket hop.
- [ ] Implement the Nemotron-Flash routing interface in `miranda-supervisor`: intent parsing, turn-taking state, dispatch signal to TTS/motion.
- [ ] Add a rolling lookahead audio buffer for coarticulation shaping.
- [ ] Measure real end-to-end latency (mic → transcript) and record the actual number, not the target.
