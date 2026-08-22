# WO-1: Tasks

Per the `model-routing-protocol.md` steering rule: check the `[CAT n]` tag before starting each task. CAT 5 requires Opus 5 — stop and request a model switch rather than attempting it on a lower tier.

- [ ] [CAT 1] Confirm workspace skeleton builds clean (`cargo build` from repo root) — already true as of the initial commit; re-verify before starting.
- [ ] [CAT 1] Add `memmap2`, `crossbeam` (or plain `std::sync::atomic`), and `bytemuck` (for safe `#[repr(C)]` casting) to `miranda-ipc/Cargo.toml`.
- [ ] [CAT 3] Define `AudioChunk`, `BlendshapeFrame` (52 f32 weights + timestamp), `SphericalHarmonics` (9 f32 coefficients + timestamp) as `#[repr(C)]` structs in `miranda-core`.
- [ ] [CAT 5] Implement the ring buffer struct in `miranda-ipc` (open-or-create the `/dev/shm/miranda_bus` mapping, atomic head/tail, push/pop methods per data type). **Opus 5 only** — lock-free concurrency correctness, silent-corruption risk if wrong.
- [ ] [CAT 4] Write a `#[cfg(test)]` round-trip test: spawn a writer thread and a reader thread, assert every written payload is read back byte-identical, in order. Escalate to Opus 5 if two attempts fail to actually exercise the race condition (a test that only ever passes isn't proof of anything).
- [ ] [CAT 2] Document the safety invariant on every `unsafe` block (why the cast/pointer arithmetic is sound).
- [ ] [CAT 1] Run `cargo test` and paste the real output into the PR/commit — no "should work."
