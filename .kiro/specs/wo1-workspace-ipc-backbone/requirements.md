# WO-1: Cargo Workspace & Lock-Free IPC Backbone — Requirements

**Role**: Principal Systems Engineer. **Depends on**: nothing (foundation). **Target**: <50μs tensor transfer.

## Requirements (EARS notation)

1. WHEN the workspace is built with `cargo build` THE SYSTEM SHALL compile all six crates (`miranda-core`, `miranda-ipc`, `miranda-audio`, `miranda-nodes`, `miranda-supervisor`, `miranda-transport`) with zero errors.
2. WHEN `miranda-ipc` initializes THE SYSTEM SHALL map a POSIX shared-memory region at `/dev/shm/miranda_bus` using `memmap2`.
3. WHEN two threads/processes write and read concurrently on the ring buffer THE SYSTEM SHALL use lock-free atomic head/tail pointers (no mutex) to coordinate access.
4. WHEN a raw audio chunk, a 52-channel ARKit blendshape frame, or a 9-coefficient spherical-harmonic vector is written to the bus THE SYSTEM SHALL use a fixed-size, `#[repr(C)]`-aligned struct matching C-ABI layout rules.
5. WHEN a payload is written and then read back from the ring buffer THE SYSTEM SHALL return byte-for-byte identical data (round-trip integrity).
6. IF the ring buffer is full WHEN a writer attempts to write THE SYSTEM SHALL either block with a bounded wait or return a clear backpressure signal — never silently drop or corrupt data.

## Acceptance criteria

- `cargo build` succeeds workspace-wide.
- `cargo test` in `miranda-ipc` includes a real round-trip test: write a payload from one thread, read it from another, assert equality.
- No `unsafe` block lacks a comment explaining the invariant it upholds (this is where memory-alignment bugs hide).
