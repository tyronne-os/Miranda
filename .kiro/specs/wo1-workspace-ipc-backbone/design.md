# WO-1: Design

## Architecture

`miranda-core` holds shared constants (52 blendshapes, 9 SH coefficients) and error types — every other crate depends on it. `miranda-ipc` owns the ring buffer itself and depends only on `miranda-core`. This dependency direction is already scaffolded in the workspace's `Cargo.toml` files — do not invert it (nothing should depend on `miranda-ipc` transitively pulling in unrelated node logic).

## The ring buffer

- Backing store: `memmap2::MmapMut` over a file at `/dev/shm/miranda_bus`, sized to hold N fixed-size slots (choose N as a power of 2 for cheap modulo via bitmask).
- Head/tail: `AtomicUsize` (or `AtomicU64` if wraparound at `usize::MAX` slots is a real concern — it isn't at this scale), `Ordering::AcqRel` on the write side, `Ordering::Acquire` on the read side.
- Slot layout: a tagged union isn't C-ABI-safe across languages the same way a fixed struct-per-type is — prefer three separate fixed-size ring buffers (audio / blendshape / spherical-harmonic) over one polymorphic ring, since the FFI boundary (WO-2's parakeet.cpp binding) needs a stable, simple layout.

## Cross-reference

See the `nobility-posh-framework` Kiro skill for the full Hermes Execution Prompt and the Sequence Matrix performance targets.
