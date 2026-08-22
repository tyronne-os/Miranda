# WO-1: Cargo Workspace & Lock-Free IPC Backbone — Requirements

**Role**: Principal Systems Engineer — you are building the foundation every other Work Order depends on.  
**Depends on**: nothing — this is the root. Nothing can be built until WO-1 ships.  
**Target**: ≤50 μs for a full tensor write→read round-trip on the shared-memory bus, measured on the local build machine (Celeron N4500 or equivalent x86-64).  
**CAT-5 routing**: Run `node scripts/cat-router-check.mjs` from the repo root before touching any task. This Work Order contains **one CAT 5 task** (the ring buffer implementation) — you will need Claude Opus 5 for that one task. See `.kiro/steering/model-routing-protocol.md` for the full switching protocol.

---

## Read these first — mandatory pre-flight before any code

You are working inside a Kiro workspace that has been fully pre-loaded with architecture, science, and operating rules. **Do not skip this section.** Reading these now costs two minutes; skipping them costs a day of wrong assumptions.

### 1. The project overview

Read `PROJECT_OVERVIEW.md` at the repo root right now. It explains:
- What Miranda-Engine *is* (a harness, not a pipeline — this distinction is load-bearing)
- The quad-test design (N parallel pipelines, N independent EVE renders, scored head-to-head)
- The full CAT-5 model routing table with all five providers
- What has already been built in this workspace (the 6-crate Rust skeleton, the prior client scaffold, the 5 Work Order specs)

### 2. The master steering rules

Read `.kiro/steering/build-standards.md`. These are non-negotiable operating rules that apply to every task in this Work Order:
- No claimed progress without real command-output evidence
- Nothing marked done without a real `cargo build` / `cargo test` pass — no "should work," no described output
- No simulated inference — if something isn't wired up yet, say so plainly
- Reuse before rebuild — check what's already in the crate stubs before writing new code

### 3. The CAT-5 model routing protocol

Read `.kiro/steering/model-routing-protocol.md` in full. The rules in it govern *which model handles which task* in this Work Order. You cannot skip the protocol on a CAT 5 task because the implementation "looks straightforward" — lock-free ring buffers are exactly the category where wrong-but-compiling code causes silent data corruption in production at 60 FPS.

### 4. The global Kiro skills loaded into this workspace

These four skills are globally available (at `~/.kiro/skills/`). They contain the pre-built architecture knowledge you need. Reference them by name in your session:

- **`nobility-posh-framework`** — master reference for the NOBILITY POSH FRAMEWORK: EVE, THE VANITY dual-pane design, the Instant Presence Standard, the 25 Vanguard Innovations, the 5 Work Orders as Hermes Execution Prompts, the Sequence Matrix performance targets. **Read this to understand what you're building toward.**
- **`live-avatar-expert`** — the 5-stage universal live-avatar pattern (ingress / understanding / expression / rendering / transport), research papers, rendering science chain (GaussianAvatars + FLAME + TetGS). The IPC backbone you're building in WO-1 is the substrate that the ingress and expression stages (WO-2 and WO-3) will write into.
- **`aws-pipeline-architect`** — AWS deployment, Podman container strategy, CPU/GPU instance tiering, CloudWatch kill-switch discipline. WO-1's IPC bus is **bare-metal only** — it uses POSIX shared memory at `/dev/shm/miranda_bus` and cannot run inside a container because the SHM region must be visible to multiple processes on the same host. This rule is documented in the skill.
- **`llamacpp-huggingface-expert`** — GGUF quantization, llama-server endpoint details, ISA mismatch diagnosis (exit 132 = missing AVX2 on Celeron N4500 = build from source with `-march=native`). This matters most in WO-2, but file it now so you know it exists.

### 5. What EVE is and what THE VANITY is

**EVE** is the 2D control reference: a single uncropped image at native resolution, permanent on the right pane of THE VANITY (Miranda's dual-pane GUI). She is the ground truth that every pipeline variant gets scored against. She does not move, does not loop, and is never replaced with a video or an AI render during development — she is the target.

**THE VANITY** is the name of Miranda's dual-pane interface:
- **Left pane**: the live node graph / harness — the React Flow topology from the `eve-ecc` repo scaffold (the "Cerebral Project" UI you can see with L0/L1/L2 tabs, node latency readouts, warm-path status, circuit breaker indicators).
- **Right pane**: EVE's image, static, uncropped, native resolution, always visible.

The model names you see in the node graph topology (Riva ASR, Nemotron Agent, Hive TTS, Audio2Face-3D, Omniverse) are **architectural role labels** — they describe the *class of work* that happens at each position in the pipeline. They are not literal product deployments. Miranda is the lab where we engineer our own science to fill those roles. WO-1's IPC bus is the plumbing that connects all of those roles to each other.

### 6. The existing repo structure — what has already been built

The Rust workspace skeleton already exists and builds clean. Before writing any new code, verify this yourself with `cargo build` from the repo root. The crates that exist:

```
miranda-engine/
├── Cargo.toml                     ← workspace root, resolver="2", 6 members
├── miranda-core/src/lib.rs        ← shared constants + error types (stub)
├── miranda-ipc/src/lib.rs         ← ring buffer home (stub — your main target)
├── miranda-audio/src/lib.rs       ← audio capture + VAD plumbing (stub, WO-2)
├── miranda-nodes/src/lib.rs       ← node trait + executor (stub, WO-3)
├── miranda-supervisor/src/lib.rs  ← session lifecycle + quad-test runner (stub)
├── miranda-transport/src/lib.rs   ← WebRTC + telemetry (stub, WO-4)
├── scripts/cat-router-check.mjs  ← CAT-5 backlog scanner — run this first
├── PROJECT_OVERVIEW.md            ← start here every session
├── MASTER_PROMPT.md               ← paste into Kiro chat to start a new session
└── .kiro/
    ├── steering/build-standards.md
    ├── steering/model-routing-protocol.md
    └── specs/wo1-workspace-ipc-backbone/   ← you are here
        ├── requirements.md  (this file)
        ├── design.md
        └── tasks.md
```

The prior-attempt client scaffold is also in the repo under `client-apps/web/` — React Flow + Vite + TypeScript, already showing a working topology UI. **Do not rebuild it; do not delete it.** WO-5 will wire it to the Rust harness once WO-1 through WO-4 are done.

---

## Requirements (EARS notation)

EARS = Easy Approach to Requirements Syntax. Every requirement below is either WHEN/SHALL (event-driven) or IF/WHEN/SHALL (conditional). Acceptance criteria at the bottom define what "done" looks like.

**REQ-1** — WHEN the workspace is built with `cargo build` from the repo root  
THE SYSTEM SHALL compile all six crates (`miranda-core`, `miranda-ipc`, `miranda-audio`, `miranda-nodes`, `miranda-supervisor`, `miranda-transport`) with zero errors and zero warnings treated as errors.

> *Baseline: this already passes on the initial commit. Re-verify before starting any other task — if it doesn't pass, stop and diagnose before writing new code.*

**REQ-2** — WHEN `miranda-ipc` initializes  
THE SYSTEM SHALL create or open a POSIX shared-memory region at `/dev/shm/miranda_bus` using the `memmap2` crate, mapping it into the process address space as a mutable byte slice.

> *Why /dev/shm: it's a tmpfs mount (RAM-backed), which is what makes the ≤50 μs target achievable. A regular file on disk cannot meet this target. The path `/dev/shm/miranda_bus` is fixed — do not make it configurable at this stage.*

**REQ-3** — WHEN two threads or processes write and read concurrently on the same ring buffer  
THE SYSTEM SHALL coordinate access using lock-free atomic head and tail pointers — specifically `AtomicUsize` with `Ordering::AcqRel` on writes and `Ordering::Acquire` on reads — with no mutex, no `RwLock`, and no spinloop that busy-waits indefinitely.

> *Why lock-free: the ring buffer is on the critical path of the 60 FPS render loop. A mutex held by a stalled audio thread would cause the renderer to miss its frame deadline. This is not a preference — it is a latency constraint.*

**REQ-4** — WHEN a raw audio chunk, a 52-channel ARKit blendshape frame, or a 9-coefficient spherical-harmonic lighting vector is written to the bus  
THE SYSTEM SHALL use a fixed-size `#[repr(C)]` struct with explicit field alignment matching the C ABI, so the memory layout is identical whether read from Rust, from the future C FFI bindings in WO-2 (parakeet.cpp), or from any other language that may sit on this bus.

> *The 52 blendshape channels are the ARKit 52 standard face blend shapes. The 9 SH coefficients are the L2 spherical harmonic lighting representation. These numbers are fixed by the pipeline science — do not parameterize them as generic constants that can be changed at runtime.*

**REQ-5** — WHEN a payload is written to the ring buffer and then read back  
THE SYSTEM SHALL return byte-for-byte identical data (round-trip integrity), verified by a real concurrent test that exercises actual memory-ordering — not a single-threaded sequential test that cannot catch race conditions.

**REQ-6** — IF the ring buffer is full WHEN a writer attempts to push a new payload  
THE SYSTEM SHALL either block with a bounded wait (≤1 ms timeout) or return a typed `BackpressureError` — never silently drop the payload and never corrupt the existing contents of the buffer.

**REQ-7** — WHEN any `unsafe` block appears in `miranda-ipc` or `miranda-core`  
THE SYSTEM SHALL have a `// SAFETY:` comment directly above or inside it explaining in one or two sentences exactly why the unsafe operation is sound — specifically: what invariant the caller upholds, and what would happen if the invariant were violated.

> *This is not a style preference. The lock-free ring buffer is the single most failure-prone component in the entire project. The safety comment is how a future reader (including you at 2am during a regression) understands why a pointer cast or atomic fence is correct.*

---

## Acceptance criteria — what "WO-1 done" looks like

WO-1 is complete when **all six** of the following are true, each verified with real command output:

1. `cargo build` from repo root exits 0 with zero warnings on all six crates.
2. `cargo test -p miranda-ipc` exits 0 and the output shows a real concurrency test passing — with thread names or timestamps in the output proving both threads actually ran, not a degenerate single-threaded execution path.
3. The test includes a write from one thread and a read from another, with the read value asserted equal to the written value using `assert_eq!` — not just "no panic."
4. Every `unsafe` block in `miranda-ipc` and `miranda-core` has a `// SAFETY:` comment.
5. The test runs under `cargo test` without `--test-threads=1` — it must be safe to run in the default parallel test harness.
6. The commit message includes the real output of `cargo test -p miranda-ipc` pasted verbatim — not a description of the output.
