# WO-1: Tasks

## Before you start — mandatory

1. **Read `requirements.md` and `design.md` in this directory first.** They contain the full context, the pre-flight skill references, and the struct designs you will implement. Do not skip them.

2. **Run the CAT-5 scanner now:**
   ```
   node scripts/cat-router-check.mjs
   ```
   from the repo root. Confirm this Work Order's tasks appear in the output. Paste the output as a reply before beginning.

3. **Check your current model against the first task's CAT tag.** This Work Order starts on CAT 1 (Qwen3 Coder Next). If you are running on a different model, switch before proceeding. See `.kiro/steering/model-routing-protocol.md` for the exact switch notice wording.

4. **Every task below requires real command output as evidence.** A description of what the output "should" say is not evidence. Paste the actual terminal output.

---

## The CAT-5 routing summary for this Work Order

| Tasks | CAT tier | Model |
|---|---|---|
| T1, T2, T7 | CAT 1 | Qwen3 Coder Next |
| T6 | CAT 2 | GLM-5 |
| T3 | CAT 3 | DeepSeek 3.2 |
| T5 | CAT 4 | Claude Sonnet 5 (escalate to Opus 5 after 2 real failed verifications) |
| T4 | **CAT 5** | **Claude Opus 5 — mandatory, no exceptions** |

---

## Tasks

- [ ] [CAT 1] **T1 — Verify the workspace builds clean**

  From the repo root, run:
  ```
  cargo build
  ```
  This must exit 0 with zero errors. If it does not, stop and diagnose before doing anything else — the scaffold was clean on the initial commit; something in your environment is different.

  Paste the real output. Mark this task done with the build output pasted verbatim.

  *If you get a "linker not found" or "error[E0]: can't find crate" error: check that the Rust toolchain is installed (`rustup show`) and that all six crate directories exist under the repo root (`ls -d miranda-*/`).*

---

- [ ] [CAT 1] **T2 — Add dependencies to `miranda-ipc/Cargo.toml`**

  Open `miranda-ipc/Cargo.toml`. Add the following to the `[dependencies]` table:
  ```toml
  miranda-core = { path = "../miranda-core" }
  memmap2 = "0.9"
  bytemuck = { version = "1", features = ["derive"] }
  ```

  Notes:
  - `memmap2` is the maintained fork of the original `memmap` crate. It provides `MmapMut` for the shared-memory region. Do not use the original `memmap` crate — it is unmaintained.
  - `bytemuck` with the `derive` feature enables `#[derive(Pod, Zeroable)]` on the payload structs. These derives are compile-time proofs that a struct has no padding, no uninit bytes, and can be safely cast to/from `&[u8]`.
  - You do not need `crossbeam` or any external atomic crate — `std::sync::atomic::AtomicUsize` with explicit orderings is sufficient for this ring buffer.

  After editing the file, run `cargo build` again to confirm the new deps resolve and compile:
  ```
  cargo build
  ```
  Paste the output. If `memmap2` or `bytemuck` fail to download (offline or registry issue), diagnose — do not skip.

---

- [ ] [CAT 3] **T3 — Define the three payload structs in `miranda-core`**

  Open `miranda-core/src/lib.rs`. Add the following, exactly as specified in `design.md`. Do not invent different field names or sizes — these are fixed by the pipeline science.

  **Constants (add at the top of the file):**
  ```rust
  pub const AUDIO_SAMPLE_RATE_HZ: u32 = 16_000;
  pub const AUDIO_CHUNK_FRAMES: usize = 160;   // 10 ms at 16 kHz
  pub const BLENDSHAPE_COUNT: usize = 52;       // ARKit 52 standard blend shapes
  pub const SH_COEFF_COUNT: usize = 9;          // L2 spherical harmonic coefficients
  ```

  **Why these specific numbers:**
  - `AUDIO_CHUNK_FRAMES = 160`: 10 ms of audio at 16 kHz. This is the standard VAD/ASR chunk size for streaming speech recognition (Parakeet, Whisper, all use 10 ms frames).
  - `BLENDSHAPE_COUNT = 52`: the ARKit face blendshape standard (Eyes, brows, mouth, jaw, cheek — full facial animation rig). WO-3 will implement the SIMD kinematic solver that produces these 52 values.
  - `SH_COEFF_COUNT = 9`: L2 spherical harmonics — 9 coefficients per color channel encoding low-frequency ambient light. Used by the WebGPU Gaussian-splat renderer (WO-5) to relight EVE dynamically.

  **Structs (add after the constants):**
  ```rust
  #[repr(C)]
  #[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
  pub struct AudioChunk {
      pub timestamp_us: u64,
      pub sample_rate: u32,
      pub frame_count: u32,
      pub samples: [f32; AUDIO_CHUNK_FRAMES],
  }

  #[repr(C)]
  #[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
  pub struct BlendshapeFrame {
      pub timestamp_us: u64,
      pub weights: [f32; BLENDSHAPE_COUNT],
  }

  #[repr(C)]
  #[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
  pub struct SphericalHarmonics {
      pub timestamp_us: u64,
      pub coefficients: [f32; SH_COEFF_COUNT],
  }
  ```

  After adding, run:
  ```
  cargo build -p miranda-core
  ```
  The `bytemuck::Pod` and `bytemuck::Zeroable` derives will fail to compile if the struct has any padding, non-Copy fields, or uninit bytes — that is the compile-time safety proof working as intended. If you get a compile error from the derives, it means the struct layout is not C-ABI safe and you must fix it before proceeding.

  Paste the real build output.

  *Size check (informational — cargo does not assert this automatically):*
  - `AudioChunk`: 8 + 4 + 4 + (160 × 4) = 656 bytes
  - `BlendshapeFrame`: 8 + (52 × 4) = 216 bytes
  - `SphericalHarmonics`: 8 + (9 × 4) = 44 bytes

  You can verify with `std::mem::size_of::<AudioChunk>()` in a test if you want compile-time confirmation.

---

- [ ] [CAT 5] **T4 — Implement the ring buffer in `miranda-ipc` — Claude Opus 5 ONLY**

  ⚠️ **MANDATORY MODEL SWITCH BEFORE THIS TASK**

  If you are not currently running as Claude Opus 5, STOP. Do not attempt this task on any other model. Emit the following and wait for the model to be switched:

  ```
  CAT 5 MODEL SWITCH REQUIRED — T4 (ring buffer implementation) requires Claude Opus 5.
  Switch the chat model dropdown to Claude Opus 5, then re-issue this task alone.
  ```

  **Handoff block to paste into the Opus 5 session:**
  ```
  === CAT 5 HANDOFF — switching to Claude Opus 5 ===
  Task: T4 — Implement the lock-free ring buffer in miranda-ipc
  Status: Ready to implement — all prerequisites complete (T1 build verified, T2 deps added, T3 structs defined)
  State: miranda-core now exports AudioChunk (656B), BlendshapeFrame (216B), SphericalHarmonics (44B) — all #[repr(C)] with bytemuck::Pod. miranda-ipc has memmap2 and bytemuck as deps.
  Incoming needs to know:
  - Three separate ring buffers (not one polymorphic), one per struct type
  - POSIX shared memory at /dev/shm/miranda_bus (tmpfs, RAM-backed — required for ≤50 μs target)
  - AtomicUsize head/tail, AcqRel/Acquire/Release ordering — NO mutex, NO SeqCst
  - Power-of-2 slot counts: audio=64, blendshape=128, sh=128
  - AtomicUsize cannot be trivially placed in mmap — requires raw pointer cast with SAFETY comment
  - Every unsafe block requires a // SAFETY: comment explaining the invariant
  - bytemuck::bytes_of and bytemuck::from_bytes are the safe cast path for writing/reading slots
  === END HANDOFF ===
  ```

  **What Opus 5 must implement in `miranda-ipc/src/lib.rs`:**

  1. A `MirandaBus` struct that opens or creates `/dev/shm/miranda_bus` using `memmap2::MmapMut`, with the three ring buffers laid out at fixed byte offsets within the mapping.

  2. Push methods for each payload type: `push_audio`, `push_blendshape`, `push_sh` — each returning `Result<(), BackpressureError>`.

  3. Pop methods for each payload type: `pop_audio`, `pop_blendshape`, `pop_sh` — each returning `Option<T>` where T is the payload type.

  4. The `AtomicUsize` head and tail for each buffer must be placed at the *start* of the mmap at known, alignment-safe byte offsets. The slot data follows.

  5. Every `unsafe` block — pointer cast, raw memory read/write — must have a `// SAFETY:` comment.

  6. The implementation must use `bytemuck::bytes_of()` to write a struct as bytes into a slot, and `bytemuck::from_bytes()` to read it back — this is the safe, proven path for `#[repr(C)] + Pod` types.

  **What Opus 5 must NOT do:**
  - Do not use a `Mutex` or `RwLock` anywhere in the hot path
  - Do not use `Ordering::SeqCst`
  - Do not use `unsafe` transmute without a `SAFETY` comment
  - Do not hardcode the struct sizes as magic numbers — use `std::mem::size_of::<T>()`
  - Do not make the capacity non-power-of-2

  After implementation, run:
  ```
  cargo build -p miranda-ipc
  ```
  Paste the real output. If it does not compile cleanly, fix it — do not proceed to T5 until T4 builds.

---

- [ ] [CAT 4] **T5 — Write the concurrent round-trip test — Claude Sonnet 5**

  ⚠️ **Model check:** This task requires Claude Sonnet 5. If you are currently on Opus 5 (from T4), you may switch back to Sonnet 5 for this task, or remain on Opus 5 if you prefer — Opus 5 is an acceptable superset for CAT 4 work. If you are on a lower tier, switch to Sonnet 5 first.

  Write a `#[cfg(test)]` module in `miranda-ipc/src/lib.rs` with a test named `test_blendshape_round_trip_concurrent`. This test must:

  1. Open a `MirandaBus` instance (or two, one for write and one for read, if the bus supports multiple openers).

  2. Spawn a writer thread that pushes 1,000 `BlendshapeFrame` payloads with distinct, incrementing `timestamp_us` values (0, 1, 2, ... 999) and weights filled with the frame index cast to f32.

  3. Spawn a reader thread that pops 1,000 `BlendshapeFrame` payloads and asserts each one is byte-identical to the corresponding payload the writer pushed — same `timestamp_us`, same `weights`.

  4. Both threads run **concurrently** — do not use `thread::sleep` to serialize them, and do not use a channel to artificially sequence them. The test must exercise actual concurrent read-write access.

  5. The test must print (using `println!` or `eprintln!`) a message that identifies which thread wrote and which read, with timestamps — this is the evidence that proves both threads actually ran concurrently, not sequentially.

  6. The test must pass under `cargo test` without `--test-threads=1`.

  **Escalation rule:** If this test fails real concurrent verification twice (a real race condition, a data corruption assertion, a deadlock), emit:
  ```
  CAT 5 ESCALATION — T5 has failed real concurrent verification twice. Switch to Claude Opus 5 before the third attempt.
  ```

  Run:
  ```
  cargo test -p miranda-ipc -- --nocapture
  ```
  Paste the complete output, including any `println!`/`eprintln!` output from the test threads.

---

- [ ] [CAT 2] **T6 — Document every `unsafe` block — GLM-5**

  ⚠️ **Model check:** Switch to GLM-5 for this task.

  Open `miranda-ipc/src/lib.rs` and scan for every `unsafe` block or `unsafe` expression. For each one, add or verify a `// SAFETY:` comment directly above or inside the block explaining:
  - What the unsafe operation is (e.g., "casting *mut u8 at offset N to *mut AtomicUsize")
  - What invariant the caller upholds that makes this sound (e.g., "offset N is 8-byte aligned because it was computed as size_of::<AtomicUsize>() * n from the mmap base, and memmap2 guarantees page alignment")
  - What would happen if the invariant were violated (e.g., "undefined behavior: misaligned atomic operations on x86 can tear")

  Every `unsafe` block without a `// SAFETY:` comment is a defect in WO-1 — the ring buffer is the single most failure-prone component, and these comments are the written record of why it is correct.

  No build output required for this task (it's a documentation addition), but run:
  ```
  cargo build -p miranda-ipc
  ```
  to confirm no accidental syntax errors were introduced. Paste the output.

---

- [ ] [CAT 1] **T7 — Final verification pass and commit — Qwen3 Coder Next**

  ⚠️ **Model check:** Switch back to Qwen3 Coder Next for this task.

  Run the full test suite from the repo root:
  ```
  cargo test
  ```
  and then:
  ```
  cargo test -p miranda-ipc -- --nocapture
  ```

  The first command verifies that adding WO-1's code has not broken any other crate. The second command shows the full concurrent test output with thread-level detail.

  Both must exit 0. Paste both outputs verbatim.

  Then run the CAT-5 scanner to update the pending backlog:
  ```
  node scripts/cat-router-check.mjs
  ```
  All seven WO-1 tasks should now show as done (no unchecked boxes for this Work Order). Paste the scanner output.

  Commit everything with a message that includes the `cargo test` output:
  ```
  git add -A
  git commit -m "WO-1 complete: lock-free IPC backbone, 3 ring buffers, concurrent round-trip test"
  ```

  WO-1 is done. The foundation is live. WO-2 (VAD/ASR routing) can begin.

---

## When WO-1 is done — what changes

Once all seven tasks above are checked off with real evidence:

- **WO-2** can begin: `miranda-audio` can now write `AudioChunk`s to the bus for VAD and parakeet.cpp ASR processing.
- **WO-3** can begin: `miranda-nodes` can now write `BlendshapeFrame`s and `SphericalHarmonics` to the bus from the SIMD kinematics solver.
- **WO-4** (transport) and **WO-5** (renderer) both depend on WO-2 and WO-3 being further along — they can begin design work but not integration work until WO-2 and WO-3 are writing real data to the bus.

The MASTER_PROMPT.md at the repo root is the paste-ready prompt for starting WO-2's Kiro session. It includes the updated CAT table with all five models and references both steering docs automatically.
