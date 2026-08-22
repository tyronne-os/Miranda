# The CAT-5 Model Routing Protocol

**Status: GLOBAL, MANDATORY, NO DEVIATION.** This protocol applies to every task in every Work Order, in this repo, without exception. Its purpose is to control agentic orchestration cost while guaranteeing the hardest engineering gets the strongest available reasoning.

## Honest constraint this protocol is built around

Kiro has no documented API for a running agent session to automatically switch its own model mid-task. Model selection is a **human action** via the chat model dropdown, and it applies to all subsequent messages in that session. This protocol is therefore not an invisible auto-router — it's a **decision system + escalation procedure** that tells whoever is acting (human or agent) exactly when a model switch is required, and exactly when to switch back down. Anything claiming to "automatically" swap models inside Kiro without that human action would be describing a capability that doesn't exist here.

## The CAT scale

| CAT | Meaning | Model | Criteria |
|---|---|---|---|
| **1** | Easy button. Zero reasoning required. | **Haiku 4.5** | Mechanical: dependency additions, config, boilerplate, running a command and reporting output. |
| **2** | Light reasoning, well-known patterns. | **Sonnet 5** | Wiring a documented library API, straightforward struct/state definitions, low risk of subtle bugs. |
| **3** | Moderate engineering. Real decisions, established patterns. | **Sonnet 5** | The default workhorse tier — most tasks in this project land here. |
| **4** | Hard. Real correctness risk, non-trivial reasoning, but bounded scope. | **Sonnet 5 primary → escalate to Opus 5 after 2 failed verification attempts** | FFI boundaries, real-time networking correctness, timing-sensitive sync. Escalation is evidence-triggered, not assumed up front. |
| **5** | Cat-5-hurricane difficulty. Silent-failure risk. Top-tier reasoning mandatory. | **Opus 5 only. No exceptions.** | Lock-free concurrency, unsafe memory/ABI correctness, real-time SIMD/DSP math, novel shader/graphics algorithms — domains where a wrong answer compiles clean, passes a shallow test, and corrupts data or renders garbage in production. |

## The recruit-and-relieve procedure (CAT 5)

1. Before starting any task, the acting model checks its CAT tag (every task in every `tasks.md` in this repo is tagged — see below).
2. **If CAT 5 and the active session is not running Opus 5**: stop immediately. Do not attempt the task. Output exactly this notice, naming the specific task:

   > **CAT 5 ESCALATION REQUIRED** — `<task name>` requires Opus 5. Switch the chat model dropdown to Opus 5, then re-issue this task alone.

3. The human switches the model dropdown to Opus 5.
4. Opus 5 handles **only the tagged CAT 5 task** — not adjacent CAT 1-3 tasks bundled into the same turn. Get in, get out.
5. **Opus 5 is explicitly granted the turns/time it needs to run its own real verification** (tests, benchmarks, a reproducible failure case fixed and re-tested) before declaring the task done. Cost discipline governs *which* tasks reach Opus, never *how thoroughly* Opus verifies one it's actually working — cutting verification short on a CAT 5 task to save credits is the single most expensive false economy available here, since CAT 5 is exactly where a wrong answer is silent and costly.
6. Once real, shown verification output confirms the task is done, the human switches the dropdown back down (Sonnet 5 for the next CAT 2-4 task, Haiku 4.5 for CAT 1) before continuing.

## CAT 4 escalation trigger (not automatic — evidence-based)

CAT 4 tasks start on Sonnet 5. If Sonnet fails real verification twice on the same task (not "seems uncertain" — actually fails a test/build/benchmark twice), that's the escalation trigger: treat it as CAT 5 for the remainder of that specific task and follow the recruit-and-relieve procedure. This prevents both premature Opus spend (assuming difficulty before evidence) and silent Sonnet thrashing (burning credits on repeated failed attempts that should have escalated).

## Per-Work-Order CAT breakdown

Full detail lives inline in each Work Order's `tasks.md` (every checkbox is tagged `[CAT n]`). Summary:

- **Work Order 1** (IPC backbone), 7 tasks: CAT 1, 1, 3, 5, 4, 2, 1 — the lock-free ring buffer implementation is the one CAT 5 in this Work Order; everything else is mechanical-to-moderate.
- **Work Order 2** (audio ingress/routing), 7 tasks: CAT 1, 2, 2, 3, 3, 3, 4 — no CAT 5 here; the parakeet.cpp FFI binding is the hardest single task (CAT 4, cross-language ABI risk).
- **Work Order 3** (kinematics/face physics), 8 tasks: CAT 1, 2, 2, 3, 3, 3, 4, 5 — the SIMD blendshape regression (audio energy → 52 ARKit weights) is CAT 5: real DSP math where a subtle error shows up only as a visual artifact, never a compile or test failure by default. The oscillator-combine + velocity-clamping step is CAT 4 (adjacent, one tier down).
- **Work Order 4** (WebRTC transport/telemetry), 7 tasks: CAT 1, 1, 1, 1, 2, 4, 4 — no CAT 5; the DataChannel broadcast and audio-track sync are the two CAT 4s (real-time networking correctness).
- **Work Order 5** (canvas UI/WebGPU renderer), 7 tasks: CAT 1, 1, 2, 3, 3, 3, 5 — the WebGPU/WGSL Gaussian-splat viewport is CAT 5, arguably the hardest single task across all five Work Orders: genuinely novel graphics-shader engineering with the least precedent to lean on.

**Total CAT 5 tasks across the whole project: 3** — WO-1's ring buffer, WO-3's SIMD blendshape regression, WO-5's WebGPU viewport. At 2,000 monthly credits, three surgical Opus 5 engagements plus a handful of evidence-triggered CAT 4 escalations should be a small minority of total spend if this protocol is followed — Sonnet 5 and Haiku 4.5 carry the bulk of the build.

## The quad-test cost multiplier

Per the workflow-agnostic harness design: running N parallel pipeline variants (a quad-test = N=4) multiplies routine task cost by N, but a CAT 5 task that lives in shared harness infrastructure (the WO-1 IPC bus, for example) should be solved **once**, correctly, at CAT 5 rigor — not re-solved per variant. Variant-specific work (swapping which ASR model a given harness instance uses, for example) is typically CAT 2-3, not CAT 5, since it's configuration within an already-proven substrate.

## Helper script

`scripts/cat-router-check.mjs` scans every `tasks.md` under `.kiro/specs/` and prints the current CAT 5 (and pending-escalation CAT 4) task backlog — run it before starting a session to know up front whether you'll need to touch the Opus 5 dropdown at all.
