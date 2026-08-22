# Project Overview — read this first, every session

This document exists to formally introduce this workspace to any model operating in it — chat or autonomous builder — so the mission, the architecture, and the operating rules are established before any task begins.

## The mission

**NOBILITY POSH FRAMEWORK** — building exceptional 2D-to-4D humanized companions & colleagues.

**Miranda-Engine is the harness — the lab, not a pipeline.** This is the single most important architectural fact about this project and it must never get flattened into "Miranda is a fixed ASR→LLM→TTS→render chain." She isn't. Miranda is **workflow-agnostic**: the substrate (shared-memory IPC bus, node contracts, telemetry, ephemeral session isolation) that any number of *candidate* pipeline configurations run inside, each independently measured against the same 2D control image ("EVE"). A **quad-test** — four parallel pipeline variants (different ASR models, different TTS engines, different rendering approaches), each in its own isolated harness session, each producing its own EVE render, scored head-to-head against the same Instant Presence Standard — is a first-class capability this harness must support, not a stretch goal. Design every piece of Miranda-Engine with "this needs to run N times in parallel, independently, comparably" in mind.

## What's already built in this workspace

- **The Rust workspace skeleton** — 6 crates (`miranda-core/ipc/audio/nodes/supervisor/transport`), `cargo build` verified clean.
- **The prior-attempt client** (`client-apps/web`, `client-services/ace-controller`) — real, working Vite+React+TS code carried forward from the first attempt (eve-ecc), not a from-scratch rebuild.
- **The Instant Presence Standard** (`eve-ecc-docs/INSTANT-PRESENCE-STANDARD.md`) — the No Loop Video Protocol: instantly alive from frame one, waist-up mid-frame composition, waving as the opening gesture, choreographed micromovement, zero loop-video anywhere.
- **5 Work Order specs** (`.kiro/specs/wo1-*` through `wo5-*`) — each with EARS-notation requirements, a design doc, and a CAT-tagged task list.
- **The CAT-5 Model Routing Protocol** (`.kiro/steering/model-routing-protocol.md`) — see below.
- **4 global Kiro skills** — `nobility-posh-framework` (this project's master science/architecture reference), `live-avatar-expert` (real-time avatar generation research), `aws-pipeline-architect` (AWS deployment + Podman container strategy), `llamacpp-huggingface-expert` (local GGUF/llama.cpp tooling).

## The CAT-5 Model Routing Protocol — mandatory, global, no deviation

Every task in every Work Order is tagged `[CAT 1]` through `[CAT 5]`. This governs which model handles it — **deliberately mixed-provider, Claude reserved for CAT 4-5 only**:

| CAT | Model | Why |
|---|---|---|
| 1 | **Qwen3 Coder Next** | Coding-specialized, cheapest tier — mechanical, zero reasoning required |
| 2 | **GLM-5** | Light reasoning, well-known patterns |
| 3 | **DeepSeek 3.2** | The workhorse — most tasks land here, strong reasoning-per-dollar |
| 4 | **Claude Sonnet 5**, escalate to Opus 5 after 2 real failed verification attempts | Real risk, bounded scope — first tier where Claude is required |
| 5 | **Claude Opus 5 only, no exceptions** | Silent-failure-risk engineering: lock-free concurrency, unsafe memory/ABI correctness, real-time SIMD math, novel shader algorithms |

**Before starting any task, check its CAT tag and switch to the required model if the active session doesn't match.** See the full protocol doc for the exact escalation-notice wording. Run `node scripts/cat-router-check.mjs` at the start of any session to see the current backlog by tier.

**28 of 36 total tasks (CAT 1-3) never touch Claude at all.** Claude is confined to the 5 CAT 4 tasks and 3 CAT 5 tasks — roughly 22% of the build, spent on the highest-value engineering. This is a deliberate cost/capability allocation, not a default — don't reach for Claude out of convenience on a CAT 1-3 task.

## Non-negotiable operating rules (full detail in `.kiro/steering/build-standards.md`)

1. No claimed progress without real command-output evidence.
2. No simulated inference, ever — say plainly when something isn't wired up yet.
3. Reuse before rebuild — check `client-apps/web` and the existing crate stubs first.
4. GPU cost discipline — never leave a GPU instance idle.
5. Podman hybrid placement — containerize WO-2/4/5, keep WO-1's IPC bus and GPU rendering bare-metal.
6. Ephemeral session isolation — every workflow test runs in a fresh, disposable Podman container.

## Where to start

Work Order 1 (`.kiro/specs/wo1-workspace-ipc-backbone/`) is the foundation everything else depends on. Start there.
