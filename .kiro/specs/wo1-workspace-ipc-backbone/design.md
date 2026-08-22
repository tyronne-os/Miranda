# WO-1: Design

This document is the technical blueprint for Work Order 1. Read `requirements.md` in this directory first — it contains the pre-flight checklist, the skill references, and the context about EVE, THE VANITY, and what this IPC backbone is actually for. This doc assumes you have read that.

---

## Where this fits in the full system

Miranda-Engine is a **workflow-agnostic testing harness**. The IPC backbone you're building in WO-1 is the central nervous system — the shared-memory bus that all other Work Orders write into and read from:

```
[WO-2: Audio/VAD/ASR] ──writes AudioChunk──────────────┐
[WO-3: Kinematics]    ──writes BlendshapeFrame──────────┼──▶  /dev/shm/miranda_bus  ──▶  [WO-5: Renderer]
[WO-3: Lighting]      ──writes SphericalHarmonics───────┘                           ──▶  [WO-4: WebRTC transport]
```

THE VANITY's left pane (the node graph in `client-apps/web/`) visualizes this topology. The node names (Riva ASR → Nemotron → Hive TTS → Audio2Face-3D → Omniverse) are **role labels** — they show what class of processing happens at each position. When Miranda-Engine is running, real Rust nodes write their outputs to this bus; the renderer reads from it to update EVE's live expression. WO-1 is the bus. Nothing else can start until it exists.

---

## Crate dependency graph

The dependency direction is already scaffolded in the workspace `Cargo.toml` files. **Do not change this graph** — inverting a dependency (e.g., making `miranda-core` depend on `miranda-ipc`) would create circular dependencies that `cargo` will refuse to build.

```
miranda-core          ← no dependencies on other workspace crates (foundation)
    ↑
miranda-ipc           ← depends only on miranda-core
    ↑
miranda-audio         ← depends on miranda-ipc (writes AudioChunks to bus)   [WO-2]
miranda-nodes         ← depends on miranda-ipc (node trait reads/writes bus)  [WO-3]
miranda-supervisor    ← depends on miranda-ipc (manages bus lifecycle)
miranda-transport     ← depends on miranda-ipc (reads bus for WebRTC frames)  [WO-4]
```

**What lives in `miranda-core`**: shared constants (BLENDSHAPE_COUNT = 52, SH_COEFF_COUNT = 9), the three fixed-size payload structs (`AudioChunk`, `BlendshapeFrame`, `SphericalHarmonics`), and the error types. Everything that multiple crates need to agree on.

**What lives in `miranda-ipc`**: the ring buffer implementation — the mmap, the atomic head/tail, and the push/pop methods. Nothing else.

---

## The three ring buffers

Do not use a single polymorphic ring buffer. Use three separate, fixed-size ring buffers — one per payload type:

| Buffer | Payload type | Fixed slot size | Write rate |
|---|---|---|---|
| `audio_bus` | `AudioChunk` | see below | ~100 Hz (10 ms chunks) |
| `blendshape_bus` | `BlendshapeFrame` | 52 × 4 bytes + 8 bytes timestamp = 216 bytes | 60 Hz (60 FPS) |
| `sh_bus` | `SphericalHarmonics` | 9 × 4 bytes + 8 bytes timestamp = 44 bytes | 30–60 Hz |

**Why three separate buffers instead of one tagged union?**  
A tagged union over these three types would have a C-ABI footprint as large as the largest member. It would also require every reader to branch on the tag, which adds complexity on the hot path. More critically: WO-2's parakeet.cpp FFI binding needs to write `AudioChunk`s directly into the bus from C++ — a single polymorphic buffer makes that harder to get right. Three simple buffers with known, fixed layouts are safer at the FFI boundary.

---

## `AudioChunk` struct design

A raw audio chunk from the microphone or from a VAD pre-buffer. The sample rate, channel count, and format come from the pipeline configuration (mono 16 kHz f32 is the target for Parakeet/ASR; do not hardcode these as magic numbers in the struct — use constants from `miranda-core`).

```rust
// In miranda-core/src/lib.rs
pub const AUDIO_SAMPLE_RATE_HZ: u32 = 16_000;
pub const AUDIO_CHUNK_FRAMES: usize = 160;  // 10 ms at 16 kHz

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct AudioChunk {
    pub timestamp_us: u64,              // microseconds since harness start
    pub sample_rate: u32,               // always AUDIO_SAMPLE_RATE_HZ in v1
    pub frame_count: u32,               // number of valid samples in `samples`
    pub samples: [f32; AUDIO_CHUNK_FRAMES],  // 160 × 4 = 640 bytes
}
```

Total size: 8 + 4 + 4 + 640 = **656 bytes per slot**.

**`bytemuck::Pod` and `bytemuck::Zeroable`**: these derive macros from the `bytemuck` crate verify at compile time that the struct has no padding, no uninit bytes, and no invalid bit patterns — which is required for safe casting to/from `&[u8]` when writing to and reading from the mmap. Do not skip these derives; they are the compile-time proof of the safety invariant.

---

## `BlendshapeFrame` struct design

The 52 ARKit blend shapes that drive facial animation on EVE. These are normalized weights in [0.0, 1.0]. WO-3 (ARKit-52 SIMD kinematics) will be the primary writer; the renderer (WO-5) will be the primary reader.

```rust
// In miranda-core/src/lib.rs
pub const BLENDSHAPE_COUNT: usize = 52;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BlendshapeFrame {
    pub timestamp_us: u64,
    pub weights: [f32; BLENDSHAPE_COUNT],  // 52 × 4 = 208 bytes
}
```

Total size: 8 + 208 = **216 bytes per slot**.

---

## `SphericalHarmonics` struct design

L2 spherical harmonic lighting coefficients — 9 floats that encode the low-frequency ambient lighting environment. These are used by the Gaussian-splat renderer (WO-5) to relights EVE's render dynamically.

```rust
// In miranda-core/src/lib.rs
pub const SH_COEFF_COUNT: usize = 9;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SphericalHarmonics {
    pub timestamp_us: u64,
    pub coefficients: [f32; SH_COEFF_COUNT],  // 9 × 4 = 36 bytes
}
```

Total size: 8 + 36 = **44 bytes per slot**.

---

## Ring buffer implementation — structure

The ring buffer is implemented in `miranda-ipc/src/lib.rs`. The design uses a **power-of-2 capacity** for each buffer so that the head/tail modulo operation can be replaced with a bitmask (`index & (capacity - 1)`), avoiding an integer division on the hot path.

```rust
// Recommended capacity (power of 2, tunable later):
const AUDIO_RING_SLOTS: usize = 64;       // ~640 ms of audio at 100 Hz writes
const BLENDSHAPE_RING_SLOTS: usize = 128; // ~2 s at 60 Hz writes
const SH_RING_SLOTS: usize = 128;

// The bus layout in /dev/shm/miranda_bus:
// [AtomicUsize head_audio][AtomicUsize tail_audio][AudioChunk × 64]
// [AtomicUsize head_blend][AtomicUsize tail_blend][BlendshapeFrame × 128]
// [AtomicUsize head_sh]   [AtomicUsize tail_sh]   [SphericalHarmonics × 128]
// (all at fixed offsets within the mmap — do not use pointer arithmetic to
//  find them; use explicit byte offsets computed from the sizes above)
```

**Atomic ordering — the critical part:**

- Writer: `head` is read with `Ordering::Relaxed`, `tail` is written with `Ordering::Release` after the slot is written. This ensures that any reader doing an `Acquire` load of `tail` sees the fully-written slot.
- Reader: `tail` is loaded with `Ordering::Acquire`, `head` is written with `Ordering::Release` after consuming the slot.
- Do not use `Ordering::SeqCst` — it adds an unnecessary full memory fence on x86 and is a performance regression on ARM. `AcqRel`/`Acquire`/`Release` is the correct pattern for a SPSC ring buffer.

**The `AtomicUsize` in shared memory — the unsafe invariant:**

`AtomicUsize` cannot be trivially placed in an mmap region because the mmap returns a `*mut u8` and `AtomicUsize` requires proper alignment and initialization. The correct approach:
1. Cast the mmap pointer to a `*mut AtomicUsize` at the known offset.
2. Use `AtomicUsize::from_mut` or raw pointer operations with `// SAFETY:` comments explaining that: (a) the alignment is correct (usize-aligned offset), (b) the memory is valid for the lifetime of the mmap, and (c) the initial value was written as `0u8.repeat(size_of::<usize>())` before the cast.

This is the CAT 5 task. Do not implement it on anything other than Opus 5.

---

## Cross-references — where to find the science behind the design decisions

- **ARKit 52 blend shapes**: `~/.kiro/skills/nobility-posh-framework/SKILL.md` → section on WO-3, the Hermes Execution Prompt for the kinematics work order, and the blend shape channel list.
- **L2 spherical harmonics**: `~/.kiro/skills/live-avatar-expert/SKILL.md` → rendering science section (GaussianAvatars + FLAME + TetGS) and the reference papers on real-time relighting.
- **Performance targets (≤50 μs)**: the Sequence Matrix in `~/.kiro/skills/nobility-posh-framework/SKILL.md` — the 50 μs target is for the IPC round-trip alone, as one segment in the full glass-to-glass latency budget.
- **Podman placement rule (WO-1 is bare-metal)**: `~/.kiro/skills/aws-pipeline-architect/SKILL.md` → Podman hybrid placement section. WO-1's SHM bus cannot be inside a container.
- **`bytemuck` and `#[repr(C)]` correctness**: `~/.kiro/skills/llamacpp-huggingface-expert/SKILL.md` → the FFI boundary section. The same C-ABI layout principles that make parakeet.cpp safe to call from Rust apply here.
- **The Instant Presence Standard**: `eve-ecc-docs/INSTANT-PRESENCE-STANDARD.md` in this repo — this is what every downstream Work Order is ultimately serving. WO-1's latency target (≤50 μs) is derived from the requirement that EVE must respond to audio input within 150 ms glass-to-glass, of which the IPC bus is one segment.
