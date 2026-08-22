# The CAT-5 Model Routing Protocol

**Status: GLOBAL, MANDATORY, NO DEVIATION.** This protocol applies to every task in every Work Order, in this repo, without exception. Its purpose is to control agentic orchestration cost while guaranteeing the hardest engineering gets the strongest available reasoning.

## Honest constraint this protocol is built around

Kiro has no documented API for a running agent session to automatically switch its own model mid-task. Model selection is a **human action** via the chat model dropdown, and it applies to all subsequent messages in that session. This protocol is therefore not an invisible auto-router — it's a **decision system + escalation procedure** that tells whoever is acting (human or agent) exactly when a model switch is required, and exactly when to switch back down. Anything claiming to "automatically" swap models inside Kiro without that human action would be describing a capability that doesn't exist here.

## The CAT scale — deliberately mixed-provider, not all-Claude

Claude is reserved for CAT 4-5 only. CAT 1-3 run on cheaper, non-Claude models — a real cost decision, not a capability compromise: Kiro's own model roster includes strong options in each tier.

| CAT | Meaning | Model | Criteria |
|---|---|---|---|
| **1** | Easy button. Zero reasoning required. | **Qwen3 Coder Next** | Coding-specialized, cheapest tier. Mechanical: dependency additions, config, boilerplate, running a command and reporting output. |
| **2** | Light reasoning, well-known patterns. | **Amazon Nova Lite** | Bedrock-native, full tool-calling support in Kiro's agent flow. Wiring a documented library API, straightforward struct/state definitions, low risk of subtle bugs. |
| **3** | Moderate engineering. Real decisions, established patterns. | **Amazon Nova Pro** | Bedrock-native, full tool-calling support, strong reasoning-per-dollar. The workhorse tier — most tasks in this project land here. |
| **4** | Hard. Real correctness risk, non-trivial reasoning, but bounded scope. | **Claude Sonnet 5 primary → escalate to Claude Opus 5 after 2 failed verification attempts** | FFI boundaries, real-time networking correctness, timing-sensitive sync. First tier where Claude is required. Escalation is evidence-triggered, not assumed up front. |
| **5** | Cat-5-hurricane difficulty. Silent-failure risk. Top-tier reasoning mandatory. | **Claude Opus 5 only. No exceptions.** | Lock-free concurrency, unsafe memory/ABI correctness, real-time SIMD/DSP math, novel shader/graphics algorithms — domains where a wrong answer compiles clean, passes a shallow test, and corrupts data or renders garbage in production. |

## The recruit-and-relieve procedure (any tier transition)

Because every tier now names a specific model, moving between tasks of different CAT levels may require a chat-dropdown switch, not just the climb to Opus. The procedure generalizes:

1. Before starting any task, the acting model checks its CAT tag (every task in every `tasks.md` in this repo is tagged — see below).
2. **If the task's tier doesn't match the currently active model**: stop. Do not attempt the task on the wrong model. Output exactly this notice, naming the specific task and the required model:

   > **CAT `<n>` MODEL SWITCH REQUIRED** — `<task name>` requires `<model>`. Switch the chat model dropdown to `<model>`, then re-issue this task alone.

3. The human switches the model dropdown accordingly.
4. The recruited model handles **only the tagged task** — not adjacent tasks from a different tier bundled into the same turn. Get in, get out. This applies at every tier, not just CAT 5, but matters most there.
5. **For CAT 4-5 specifically, the recruited Claude model is explicitly granted the turns/time it needs to run its own real verification** (tests, benchmarks, a reproducible failure case fixed and re-tested) before declaring the task done. Cost discipline governs *which* tasks reach Claude, never *how thoroughly* it verifies one it's actually working — cutting verification short to save credits is the single most expensive false economy available here.
6. Once real, shown verification output confirms the task is done, the human switches the dropdown to whatever the next task's tier requires before continuing.

**Practical note**: a Work Order's task list can span all five tiers in sequence (e.g. WO-1 is CAT 1,1,3,5,4,2,1 — Qwen→Qwen→Nova Pro→Opus→Sonnet→Nova Lite→Qwen). That's real switching overhead. Where task order is flexible, batching same-tier tasks together within one sitting reduces dropdown switches without violating the protocol — the tier assignment is what's mandatory, not the original task ordering.

## CAT 4 escalation trigger (not automatic — evidence-based)

CAT 4 tasks start on Claude Sonnet 5. If Sonnet fails real verification twice on the same task (not "seems uncertain" — actually fails a test/build/benchmark twice), that's the escalation trigger: treat it as CAT 5 for the remainder of that specific task and follow the recruit-and-relieve procedure, switching to Opus 5. This prevents both premature Opus spend (assuming difficulty before evidence) and silent Sonnet thrashing (burning credits on repeated failed attempts that should have escalated).

## Per-Work-Order CAT breakdown

Full detail lives inline in each Work Order's `tasks.md` (every checkbox is tagged `[CAT n]`). Summary:

- **Work Order 1** (IPC backbone), 7 tasks: CAT 1, 1, 3, 5, 4, 2, 1 — the lock-free ring buffer implementation is the one CAT 5 in this Work Order; everything else is mechanical-to-moderate.
- **Work Order 2** (audio ingress/routing), 7 tasks: CAT 1, 2, 2, 3, 3, 3, 4 — no CAT 5 here; the parakeet.cpp FFI binding is the hardest single task (CAT 4, cross-language ABI risk).
- **Work Order 3** (kinematics/face physics), 8 tasks: CAT 1, 2, 2, 3, 3, 3, 4, 5 — the SIMD blendshape regression (audio energy → 52 ARKit weights) is CAT 5: real DSP math where a subtle error shows up only as a visual artifact, never a compile or test failure by default. The oscillator-combine + velocity-clamping step is CAT 4 (adjacent, one tier down).
- **Work Order 4** (WebRTC transport/telemetry), 7 tasks: CAT 1, 1, 1, 1, 2, 4, 4 — no CAT 5; the DataChannel broadcast and audio-track sync are the two CAT 4s (real-time networking correctness).
- **Work Order 5** (canvas UI/WebGPU renderer), 7 tasks: CAT 1, 1, 2, 3, 3, 3, 5 — the WebGPU/WGSL Gaussian-splat viewport is CAT 5, arguably the hardest single task across all five Work Orders: genuinely novel graphics-shader engineering with the least precedent to lean on.

**Total CAT 5 tasks across the whole project: 3** — WO-1's ring buffer, WO-3's SIMD blendshape regression, WO-5's WebGPU viewport. **28 of the 36 total tasks are CAT 1-3 and never touch Claude at all** — Qwen3 Coder Next, Amazon Nova Lite, and Amazon Nova Pro carry the bulk of the build. Claude (Sonnet 5 then Opus 5) is confined to the 5 CAT 4 tasks plus the 3 CAT 5 tasks — 8 of 36, roughly 22% of the total task count, and typically the highest-value 22% to spend real reasoning on.

## The quad-test cost multiplier

Per the workflow-agnostic harness design: running N parallel pipeline variants (a quad-test = N=4) multiplies routine task cost by N, but a CAT 5 task that lives in shared harness infrastructure (the WO-1 IPC bus, for example) should be solved **once**, correctly, at CAT 5 rigor — not re-solved per variant. Variant-specific work (swapping which ASR model a given harness instance uses, for example) is typically CAT 2-3, not CAT 5, since it's configuration within an already-proven substrate.

## Helper script

`scripts/cat-router-check.mjs` scans every `tasks.md` under `.kiro/specs/` and prints the current CAT 5 (and pending-escalation CAT 4) task backlog — run it before starting a session to know up front whether you'll need to touch the Opus 5 dropdown at all.
