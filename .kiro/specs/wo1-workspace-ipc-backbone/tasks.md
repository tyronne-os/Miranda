# WO-1: Tasks

- [ ] Confirm workspace skeleton builds clean (`cargo build` from repo root) — already true as of the initial commit; re-verify before starting.
- [ ] Add `memmap2`, `crossbeam` (or plain `std::sync::atomic`), and `bytemuck` (for safe `#[repr(C)]` casting) to `miranda-ipc/Cargo.toml`.
- [ ] Define `AudioChunk`, `BlendshapeFrame` (52 f32 weights + timestamp), `SphericalHarmonics` (9 f32 coefficients + timestamp) as `#[repr(C)]` structs in `miranda-core`.
- [ ] Implement the ring buffer struct in `miranda-ipc` (open-or-create the `/dev/shm/miranda_bus` mapping, atomic head/tail, push/pop methods per data type).
- [ ] Write a `#[cfg(test)]` round-trip test: spawn a writer thread and a reader thread, assert every written payload is read back byte-identical, in order.
- [ ] Document the safety invariant on every `unsafe` block (why the cast/pointer arithmetic is sound).
- [ ] Run `cargo test` and paste the real output into the PR/commit — no "should work."
