# WO-1: Tasks

## Clarifications — read before doing anything else

These answer the three questions you asked before starting, plus a correction on a commonly misunderstood part of the project. They are embedded here so this spec is self-contained and you do not have to ask again.

---

### On Hermes — it is not integrated and not running

Nothing in this repo calls a Hermes API, spawns a Hermes process, or depends on Hermes being installed. The `nobility-posh-framework` Kiro skill references "5 Work Orders as Hermes Execution Prompts" — that phrase means the Work Orders were *written in the format* of Hermes execution prompts (a structured prompt style used in multi-agent orchestration). It describes the format, not a running system.

**In this Kiro workspace, Kiro is the agent.** The spec files are what Kiro reads and executes. The CAT-5 model switching is controlled by the human changing the model dropdown in Kiro's UI and re-issuing the task. There is no external orchestrator. There is nothing to install or configure for "Hermes." The IPC bus you are building in this Work Order *is* Miranda-Engine's equivalent of what Hermes would coordinate — but the bus is what WO-1 builds from scratch in Rust.

---

### On the Pipeline 1 change — the node labels are role labels, not product deployments

Since this spec was written, the example first pipeline has been updated to a **fully AWS-native, batteries-included implementation**. The node labels you see in THE VANITY's left pane (Riva ASR / Nemotron Agent / Hive TTS / Audio2Face-3D / AnimGraph / Omniverse Stream) are **role labels** — they describe the class of work at each position, not literal product deployments.

For Pipeline 1, each role is filled by an AWS managed service Kiro already has direct access to:

| Node role label | Pipeline 1 AWS service |
|---|---|
| Riva ASR | Amazon Transcribe Streaming |
| Nemotron Agent | Amazon Bedrock (Nova Pro or Claude Haiku via Converse API) |
| Hive TTS | Amazon Polly Neural TTS |
| Audio2Face-3D | Amazon Polly Speech Marks (viseme output) |
| AnimGraph / Omniverse | Amazon Sumerian Hosts SDK (Three.js, `aws-samples/amazon-sumerian-hosts`) |

**The IPC bus you are building in WO-1 does not change based on which pipeline fills it.** `BlendshapeFrame.weights` (52 channels) will be populated by Polly viseme-to-blendshape adapter logic in Pipeline 1, and by the SIMD kinematics solver in Pipeline 2. The bus is agnostic — build it once. The full AWS node mapping, Polly viseme→BlendshapeFrame adapter table, and Sumerian Hosts setup are in `.kiro/steering/pipeline-1-aws-native.md`. Read that after WO-1 is done.

---

### On the build target — local x86-64 first, ARM64 second

Build and verify locally on the **Celeron N4500 (x86-64, native, no cross-compilation)**. The ≤50 μs target in the requirements is stated against this machine.

The AWS plan uses a **t4g.small (Graviton/ARM64)** — a different ISA. When you move to EC2:
- Option A (recommended): SSH into the t4g.small and run `cargo build` + `cargo test` natively there — no cross-compilation toolchain needed.
- Option B: cross-compile locally: `rustup target add aarch64-unknown-linux-gnu`, then `cargo build --target aarch64-unknown-linux-gnu`.

**The ≤50 μs latency target must be re-benchmarked separately on each platform.** ARM64 has weaker memory ordering than x86 (x86 is TSO — total store order — so atomic `AcqRel`/`Acquire`/`Release` is nearly free there but costs real memory fences on ARM). A benchmark that passes 50 μs on the Celeron does not guarantee passing on Graviton. In practice, SHM round-trips on Graviton are typically *faster* than on the Celeron — but verify rather than assume.

**Sequence:** verify correctness locally (T1–T7), then SSH into the t4g.small, re-run `cargo test`, and re-run the latency benchmark from T5 natively. If the benchmark fails on ARM64, the fix is `#[repr(align(64))]` on the atomic control structs to eliminate false sharing between the head/tail atomics and the slot data.

---

### On the session model — multi-session with real model switches

Use **multi-session with explicit handoff blocks at each CAT transition**, not a single-session planning exercise. The CAT-5 protocol is a real cost-control tool:

- T4 (the lock-free ring buffer) is a genuine CAT 5 problem. Wrong-but-compiling lock-free code produces silent data corruption at 60 FPS. It benefits from Opus 5's reasoning depth — not from "acknowledging the routing exists" while Sonnet executes it.
- T1 (verify `cargo build`) costs the same tokens whether it runs on Qwen3 or Opus 5. Sending it through Opus 5 is direct cash spent with zero quality return.

**How the session boundaries work in Kiro:**

1. **Qwen3 Coder Next** — paste MASTER_PROMPT.md, execute T1 and T2
2. **DeepSeek 3.2** — execute T3 (struct definitions)
3. **Claude Opus 5** — paste the T4 handoff block below, execute T4 only
4. **Claude Sonnet 5** (or stay on Opus 5 — acceptable superset) — execute T5
5. **GLM-5** — execute T6 (SAFETY comments)
6. **Qwen3 Coder Next** — execute T7 (final verification + commit)

Each switch is a 10-second dropdown change in Kiro. The handoff block in T4 is the paste that brings the new model up to speed.

---

## Before you start — mandatory

1. **Read `requirements.md` and `design.md` in this directory first.** They contain the full context, the pre-flight skill references, the struct designs, and the atomic ordering rationale. Do not skip them.

2. **Run the CAT-5 scanner now:**
   ```
   node scripts/cat-router-check.mjs
   ```
   from the repo root. Confirm this Work Order's tasks appear in the output. Paste the output as a reply before beginning.

3. **Check your current model against the first task's CAT tag.** This Work Order starts on CAT 1 (Qwen3 Coder Next). If you are running on a different model, switch before proceeding.

4. **Every task requires real command output as evidence.** A description of what the output "should" say is not evidence. Paste the actual terminal output.

---

## CAT-5 routing summary for this Work Order

| Task | CAT tier | Model |
|---|---|---|
| T1, T2, T7 | CAT 1 | Qwen3 Coder Next |
| T6 | CAT 2 | GLM-5 |
| T3 | CAT 3 | DeepSeek 3.2 |
| T5 | CAT 4 | Claude Sonnet 5 (escalate to Opus 5 after 2 real failed verifications) |
| T4 | **CAT 5** | **Claude Opus 5 — mandatory, no exceptions** |

---

## Tasks

- [x] [CAT 1] **T1 — Verify the workspace builds clean**

  From the repo root, run:
  ```
  cargo build
  ```
  This must exit 0 with zero errors. If it does not, stop and diagnose before doing anything else — the scaffold was clean on the initial commit; something in your environment is different.

  Paste the real output. Mark this task done with the build output pasted verbatim.

  *If you get a "linker not found" or "error[E0]: can't find crate" error: check that the Rust toolchain is installed (`rustup show`) and that all six crate directories exist under the repo root (`ls -d miranda-*/`).*

  *Note on build targets: this verification runs locally (x86-64, native). The t4g.small (ARM64) verification happens separately after T7 — see the build target clarification at the top of this file.*

---

- [x] [CAT 1] **T2 — Add dependencies to `miranda-ipc/Cargo.toml`**

  Open `miranda-ipc/Cargo.toml`. Add the following to the `[dependencies]` table:
  ```toml
  miranda-core = { path = "../miranda-core" }
  memmap2 = "0.9"
  bytemuck = { version = "1", features = ["derive"] }
  ```

  Notes:
  - `memmap2` is the maintained fork of the original `memmap` crate. Do not use the original `memmap` — it is unmaintained.
  - `bytemuck` with the `derive` feature enables `#[derive(Pod, Zeroable)]` on the payload structs — compile-time proof that the struct has no padding, no uninit bytes, and can be safely cast to/from `&[u8]`.
  - You do not need `crossbeam` — `std::sync::atomic::AtomicUsize` with explicit orderings is sufficient.

  After editing, run `cargo build` again and paste the output. If `memmap2` or `bytemuck` fail to resolve, diagnose — do not skip.

---

- [x] [CAT 3] **T3 — Define the three payload structs in `miranda-core`**

  Open `miranda-core/src/lib.rs`. Add the constants and structs exactly as specified in `design.md`. Do not invent different field names or sizes — these are fixed by the pipeline science.

  **Constants:**
  ```rust
  pub const AUDIO_SAMPLE_RATE_HZ: u32 = 16_000;
  pub const AUDIO_CHUNK_FRAMES: usize = 160;   // 10 ms at 16 kHz
  pub const BLENDSHAPE_COUNT: usize = 52;       // ARKit 52 standard blend shapes
  pub const SH_COEFF_COUNT: usize = 9;          // L2 spherical harmonic coefficients
  ```

  **Why these numbers:**
  - `160 frames`: 10 ms of audio at 16 kHz — the standard VAD/ASR chunk size for streaming speech recognition. Pipeline 1 uses Amazon Transcribe Streaming (sends 10 ms chunks); later pipelines use parakeet.cpp. Same constant, same bus, different implementation.
  - `52 blendshapes`: ARKit face blend shape standard — the full facial animation rig. Pipeline 1 populates ~22 of these channels from Amazon Polly Speech Marks (visemes); Pipeline 2 will populate all 52 from the SIMD kinematics solver (WO-3). The bus struct does not change between pipelines.
  - `9 SH coefficients`: L2 spherical harmonics encoding low-frequency ambient lighting — used by the Gaussian-splat renderer (WO-5, Pipeline 2) to relight EVE. Pipeline 1's Sumerian Hosts renderer does not use them yet; they default to 0.0.

  **Structs:**
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

  The `bytemuck::Pod` and `bytemuck::Zeroable` derives will fail to compile if the struct has any padding, non-Copy fields, or uninit bytes — that is the compile-time safety proof working as intended. A compile error from the derives means the struct layout is not C-ABI safe and must be fixed.

  Run:
  ```
  cargo build -p miranda-core
  ```
  Paste the real build output.

---

- [x] [CAT 5] **T4 — Implement the ring buffer in `miranda-ipc` — Claude Opus 5 ONLY**

  ⚠️ **MANDATORY MODEL SWITCH BEFORE THIS TASK**

  If you are not currently running as Claude Opus 5, STOP. Emit:
  ```
  CAT 5 MODEL SWITCH REQUIRED — T4 (ring buffer implementation) requires Claude Opus 5.
  Switch the chat model dropdown to Claude Opus 5, then re-issue this task alone.
  ```

  **Handoff block — paste this as the first message in the Opus 5 session:**
  ```
  === CAT 5 HANDOFF — switching to Claude Opus 5 ===
  Task: T4 — Implement the lock-free ring buffer in miranda-ipc
  Status: Ready — T1 build verified clean, T2 deps added, T3 structs defined and compiling
  State: miranda-core exports AudioChunk (656B), BlendshapeFrame (216B), SphericalHarmonics (44B)
         All #[repr(C)] + bytemuck::Pod. miranda-ipc has memmap2 and bytemuck as deps.
  Incoming needs to know:
  - Three separate ring buffers (not one polymorphic), one per struct type
  - POSIX shared memory at /dev/shm/miranda_bus (tmpfs, RAM-backed — required for ≤50 μs target)
  - AtomicUsize head/tail, AcqRel/Acquire/Release ordering — NO mutex, NO SeqCst
  - Power-of-2 slot counts: audio=64, blendshape=128, sh=128
  - AtomicUsize cannot be trivially placed in mmap — requires raw pointer cast with // SAFETY: comment
  - Every unsafe block requires a // SAFETY: comment explaining the invariant
  - bytemuck::bytes_of and bytemuck::from_bytes are the safe cast path for slots
  - The context: this bus serves Pipeline 1 (Polly visemes → BlendshapeFrame) and all future
    pipelines. The bus is pipeline-agnostic. Build it once.
  === END HANDOFF ===
  ```

  **What Opus 5 must implement in `miranda-ipc/src/lib.rs`:**

  1. `MirandaBus` struct — opens or creates `/dev/shm/miranda_bus` with `memmap2::MmapMut`, three ring buffers at fixed byte offsets.
  2. Push methods: `push_audio`, `push_blendshape`, `push_sh` → `Result<(), BackpressureError>`
  3. Pop methods: `pop_audio`, `pop_blendshape`, `pop_sh` → `Option<T>`
  4. `AtomicUsize` head/tail at alignment-safe byte offsets at the start of the mmap; slot data follows.
  5. Every `unsafe` block has a `// SAFETY:` comment.
  6. Uses `bytemuck::bytes_of()` to write a struct as bytes into a slot, `bytemuck::from_bytes()` to read back.

  **Must NOT do:**
  - No `Mutex` or `RwLock` in the hot path
  - No `Ordering::SeqCst`
  - No `unsafe` transmute without a `// SAFETY:` comment
  - No hardcoded struct sizes as magic numbers — use `std::mem::size_of::<T>()`
  - No non-power-of-2 capacities

  **Step 1 — build:**
  ```
  cargo build -p miranda-ipc
  ```
  Paste the output. Do not proceed to Step 2 if this does not exit 0.

  **Step 2 — MIRI undefined behavior check (added requirement):**
  ```
  cargo +nightly miri test -p miranda-ipc
  ```
  MIRI is Rust's interpreter-based undefined behavior checker. It catches invalid pointer casts, misaligned memory accesses, and incorrect atomic memory ordering that `cargo test` on x86 cannot detect (x86's TSO model hides many ordering errors that would crash on ARM or be caught by MIRI). If MIRI is not installed: `rustup component add miri --toolchain nightly`. If MIRI reports any error, the implementation has undefined behavior and must be fixed before T5 starts — do not proceed past a MIRI failure.

  Paste the MIRI output.

---

- [x] [CAT 4] **T5 — Write the concurrent round-trip test + latency benchmark — Claude Sonnet 5**

  ⚠️ **Model check:** Claude Sonnet 5. If on Opus 5 from T4, you may stay — Opus 5 is an acceptable superset. If on a lower tier, switch to Sonnet 5 first.

  **Part A — concurrent round-trip test:**

  Write a `#[cfg(test)]` test named `test_blendshape_round_trip_concurrent` that:

  1. Opens a `MirandaBus` instance.
  2. Spawns a writer thread pushing 1,000 `BlendshapeFrame` payloads with distinct, incrementing `timestamp_us` values (0, 1, 2, ... 999) and weights filled with the frame index cast to f32.
  3. Spawns a reader thread popping 1,000 `BlendshapeFrame` payloads and asserting each is byte-identical to the written payload — same `timestamp_us`, same `weights` — using `assert_eq!`.
  4. Both threads run **concurrently** — no `thread::sleep` to serialize, no channel to artificially sequence. This must exercise actual concurrent read-write access.
  5. Prints (via `println!` or `eprintln!`) a message from each thread with thread name and timestamp — this is the proof both threads actually ran.
  6. Passes under `cargo test` without `--test-threads=1`.

  **Part B — latency benchmark (added requirement):**

  Write a second `#[cfg(test)]` test named `test_round_trip_latency` that measures real round-trip performance:
  ```rust
  #[test]
  fn test_round_trip_latency() {
      let bus = MirandaBus::open_or_create().unwrap();
      let frame = BlendshapeFrame { timestamp_us: 0, weights: [0.0; BLENDSHAPE_COUNT] };
      let start = std::time::Instant::now();
      for _ in 0..10_000 {
          bus.push_blendshape(frame).unwrap();
          let _ = bus.pop_blendshape();
      }
      let elapsed_us = start.elapsed().as_micros() as u64 / 10_000;
      println!("Mean round-trip latency: {} μs", elapsed_us);
      assert!(
          elapsed_us <= 50,
          "Round-trip latency {} μs exceeds ≤50 μs target — check for false sharing on head/tail atomics",
          elapsed_us
      );
  }
  ```

  **If the latency benchmark fails (elapsed_us > 50 on the local Celeron N4500):** the most likely cause is false sharing — the head/tail `AtomicUsize` values and the slot data are on the same 64-byte cache line, causing the CPU to invalidate the entire line on every write. Fix: add `#[repr(align(64))]` to the control struct that holds the atomics so they occupy their own cache line. Re-run after the fix.

  Run:
  ```
  cargo test -p miranda-ipc -- --nocapture
  ```
  Paste the complete output including `println!`/`eprintln!` from both tests.

  **Escalation rule:** If the concurrent round-trip test fails real concurrent verification twice (data corruption assertion, deadlock, race), emit:
  ```
  CAT 5 ESCALATION — T5 has failed real concurrent verification twice. Switch to Claude Opus 5 before the third attempt.
  ```

---

- [x] [CAT 2] **T6 — Document every `unsafe` block — GLM-5**

  ⚠️ **Model check:** Switch to GLM-5.

  Scan `miranda-ipc/src/lib.rs` for every `unsafe` block or `unsafe` expression. For each one, add or verify a `// SAFETY:` comment explaining:
  - What the unsafe operation is
  - What invariant the caller upholds that makes it sound
  - What would happen if the invariant were violated

  Every `unsafe` block without a `// SAFETY:` comment is a defect. After adding comments, run:
  ```
  cargo build -p miranda-ipc
  ```
  and paste the output to confirm no syntax errors were introduced.

---

- [x] [CAT 1] **T7 — Final verification pass and commit — Qwen3 Coder Next** (x86-64 local verification + commit done; ARM64/t4g.small cross-check NOT performed — no AWS credentials/EC2 access in this environment, see commit message for detail)

  ⚠️ **Model check:** Switch back to Qwen3 Coder Next.

  Run the full workspace test suite:
  ```
  cargo test
  ```
  then the IPC-specific suite with output:
  ```
  cargo test -p miranda-ipc -- --nocapture
  ```

  Both must exit 0. The second command must show:
  - The concurrent round-trip test output with thread-level detail proving both threads ran
  - The latency benchmark output showing the mean round-trip μs and confirming it is ≤50

  Paste both outputs verbatim.

  Run the CAT-5 scanner:
  ```
  node scripts/cat-router-check.mjs
  ```
  All seven WO-1 tasks should show as complete (no unchecked boxes for this Work Order). Paste the scanner output.

  Commit:
  ```
  git add -A
  git commit -m "WO-1 complete: lock-free IPC backbone, 3 ring buffers, MIRI clean, concurrent test, ≤50μs latency verified"
  ```

  **After committing — ARM64 verification (automated):**

  The ARM64 cross-check does not block WO-2. It runs in parallel once AWS credentials are configured. Three scripts in `scripts/` handle the entire flow — no manual AWS console interaction required:

  | Script | What it does |
  |---|---|
  | `scripts/aws-setup.sh` | One-time credential setup: silent-read prompts for AWS key ID + secret → writes `~/.aws/credentials`, verifies with `aws sts get-caller-identity`, accepts PEM key via stdin → `~/.ssh/beryl-aws-key.pem`, auto-discovers the EC2 instance IP |
  | `scripts/provision-ec2.sh` | If no t4g.small exists yet: launches one, installs Rust, clones the repo via user-data. Run once, wait ~3 minutes. |
  | `scripts/arm64-verify.sh` | SSHes into the instance, syncs the repo via `git pull`, runs `cargo build`, `cargo test -p miranda-ipc -- --nocapture`, and the MIRI check — pastes real ARM64 output. |

  **To close the ARM64 gap:**
  1. Run `bash scripts/aws-setup.sh` once from your local terminal (adds AWS credentials + PEM key — keys go directly to disk, never through chat)
  2. If no EC2 instance exists: run `bash scripts/provision-ec2.sh` once (launches the t4g.small automatically, wait 3 minutes)
  3. Run `bash scripts/arm64-verify.sh` — it handles SSH, repo sync, and all tests, and pastes the results back here

  If the latency benchmark fails on ARM64 (> 50 μs): add `#[repr(align(64))]` to the `AtomicUsize` control structs and re-run. The ARM64 fix does not require a model switch — it's a CAT 1 change (adding a repr attribute).

  **WO-1 is done and WO-2 can begin.** The ARM64 verification runs in parallel. Read `.kiro/steering/pipeline-1-aws-native.md` now — it defines exactly what WO-2 through WO-5 look like for Pipeline 1 (AWS managed services, no custom Rust for the first pass). WO-2 (acoustic ingress) starts next.

---

## When WO-1 is done — what it unblocks

- **WO-2 (Pipeline 1)**: Amazon Transcribe Streaming client in `miranda-audio` writes `AudioChunk`s to `audio_bus`. No parakeet.cpp yet — Transcribe is the batteries-included ASR for Pipeline 1.
- **WO-3 (Pipeline 1)**: Polly Speech Marks viseme-to-BlendshapeFrame adapter in `miranda-nodes` writes `BlendshapeFrame`s to `blendshape_bus`. The SIMD solver (full WO-3) is Pipeline 2.
- **WO-4 / WO-5 (Pipeline 1)**: KVS WebRTC (JS SDK) and Amazon Sumerian Hosts (Three.js) in `client-apps/web/` handle transport and rendering without touching the Rust transport crate or WebGPU — those are Pipeline 2 targets.
- **MASTER_PROMPT.md** at the repo root is the paste-ready prompt for opening any subsequent Kiro session. It loads all steering docs and references the Pipeline 1 spec automatically.
