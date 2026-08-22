# Project Overview — read this first, every session

This document exists to formally introduce this workspace to any model operating in it — chat or autonomous builder — so the mission, the architecture, and the operating rules are established before any task begins.

## The mission

**NOBILITY POSH FRAMEWORK** — building exceptional 2D-to-4D humanized companions & colleagues.

**Miranda-Engine is the harness — the lab, not a pipeline.** This is the single most important architectural fact about this project and it must never get flattened into "Miranda is a fixed ASR→LLM→TTS→render chain." She isn't. Miranda is **workflow-agnostic**: the substrate (shared-memory IPC bus, node contracts, telemetry, ephemeral session isolation) that any number of *candidate* pipeline configurations run inside, each independently measured against the same 2D control image ("EVE"). A **quad-test** — four parallel pipeline variants (different ASR models, different TTS engines, different rendering approaches), each in its own isolated harness session, each producing its own EVE render, scored head-to-head against the same Instant Presence Standard — is a first-class capability this harness must support, not a stretch goal. Design every piece of Miranda-Engine with "this needs to run N times in parallel, independently, comparably" in mind.

**Miranda's larger claim:** she can take the description of *any* pipeline — from an AWS reference architecture to a theoretical research paper — and engineer it into a running, measured, scored implementation without rebuilding the harness. Pipelines come and go. The harness is permanent.

---

## Pipeline 1: AWS-native (batteries-included — first live test)

**Full spec: `.kiro/steering/pipeline-1-aws-native.md`** — read that file for the node mapping, SDK names, Polly viseme→BlendshapeFrame adapter spec, and credential access instructions.

The node labels in THE VANITY's left pane (Riva ASR / Nemotron Agent / Hive TTS / Audio2Face-3D / AnimGraph / Omniverse Stream) are **role labels** — they describe the *class of work* at each node position, not literal product deployments. For Pipeline 1, each role maps to an AWS-managed service Kiro already has direct access to via the project's AWS Bedrock credits:

| Node role label | Pipeline 1 AWS service | What it does |
|---|---|---|
| Riva ASR | **Amazon Transcribe Streaming** | Real-time WebSocket STT, managed, no GPU |
| Nemotron Agent | **Amazon Bedrock** (Nova Pro or Claude Haiku) | Converse API — model-agnostic routing and response generation |
| Hive TTS | **Amazon Polly Neural TTS** | 60+ neural voices, billed per character, zero standing cost |
| Audio2Face-3D | **Amazon Polly Speech Marks (visemes)** | 22 viseme types time-aligned to audio — populates BlendshapeFrame on the IPC bus |
| AnimGraph | **Amazon Sumerian Hosts SDK** (`aws-samples/amazon-sumerian-hosts`) | Pre-built character rig + Polly lip-sync, Three.js, works in browser today with zero GPU |
| Omniverse Stream | **Amazon Kinesis Video Streams WebRTC** | Managed WebRTC signaling, no signaling server to build |

**Why Pipeline 1 before anything else:** it proves the IPC bus, node contracts, and THE VANITY GUI work end-to-end with real signals from real AWS services — before we introduce research-grade components (parakeet.cpp, SIMD kinematics, Gaussian-splat rendering) in later pipelines. Pipeline 1's latency numbers are the baseline. Pipeline 2 must beat them in a head-to-head quad-test.

The node labels in THE VANITY do NOT need to be renamed — they describe the role permanently. The AWS service is just what fills that role in Pipeline 1. When Pipeline 2 uses parakeet.cpp instead of Transcribe, the "Riva ASR" node slot still exists; a different implementation is plugged into it.

---

## What's already built in this workspace

- **The Rust workspace skeleton** — 6 crates (`miranda-core/ipc/audio/nodes/supervisor/transport`), `cargo build` verified clean.
- **The prior-attempt client** (`client-apps/web`, `client-services/ace-controller`) — real, working Vite+React+TS code carried forward from the first attempt (eve-ecc), not a from-scratch rebuild. This is THE VANITY's frontend — the React Flow node graph (left pane) and EVE's static 2D reference image (right pane).
- **The Instant Presence Standard** (`eve-ecc-docs/INSTANT-PRESENCE-STANDARD.md`) — the No Loop Video Protocol: instantly alive from frame one, waist-up mid-frame composition, waving as the opening gesture, choreographed micromovement, zero loop-video anywhere.
- **5 Work Order specs** (`.kiro/specs/wo1-*` through `wo5-*`) — each with EARS-notation requirements, a design doc, and a CAT-tagged task list.
- **The CAT-5 Model Routing Protocol** (`.kiro/steering/model-routing-protocol.md`) — see below.
- **Pipeline 1 steering doc** (`.kiro/steering/pipeline-1-aws-native.md`) — the AWS-native first pipeline: full node mapping, Polly viseme adapter, Sumerian Hosts setup, credential access.
- **4 global Kiro skills** — `nobility-posh-framework` (this project's master science/architecture reference), `live-avatar-expert` (real-time avatar generation research), `aws-pipeline-architect` (AWS deployment + Podman container strategy), `llamacpp-huggingface-expert` (local GGUF/llama.cpp tooling).

---

## The CAT-5 Model Routing Protocol — mandatory, global, no deviation

Every task in every Work Order is tagged `[CAT 1]` through `[CAT 5]`. This governs which model handles it — **deliberately mixed-provider, Claude reserved for CAT 4-5 only**:

| CAT | Model | Why |
|---|---|---|
| 1 | **Qwen3 Coder Next** | Coding-specialized, cheapest tier — mechanical, zero reasoning required |
| 2 | **Amazon Nova Lite** | Light reasoning, well-known patterns |
| 3 | **Amazon Nova Pro** | The workhorse — most tasks land here, strong reasoning-per-dollar |
| 4 | **Claude Sonnet 5**, escalate to Opus 5 after 2 real failed verification attempts | Real risk, bounded scope — first tier where Claude is required |
| 5 | **Claude Opus 5 only, no exceptions** | Silent-failure-risk engineering: lock-free concurrency, unsafe memory/ABI correctness, real-time SIMD math, novel shader algorithms |

**Before starting any task, check its CAT tag and switch to the required model if the active session doesn't match.** See the full protocol doc for the exact escalation-notice wording. Run `node scripts/cat-router-check.mjs` at the start of any session to see the current backlog by tier.

**28 of 36 total tasks (CAT 1-3) never touch Claude at all.** Claude is confined to the 5 CAT 4 tasks and 3 CAT 5 tasks — roughly 22% of the build, spent on the highest-value engineering. This is a deliberate cost/capability allocation, not a default — don't reach for Claude out of convenience on a CAT 1-3 task.

---

## Non-negotiable operating rules (full detail in `.kiro/steering/build-standards.md`)

1. No claimed progress without real command-output evidence.
2. No simulated inference, ever — say plainly when something isn't wired up yet.
3. Reuse before rebuild — check `client-apps/web` and the existing crate stubs first.
4. GPU cost discipline — never leave a GPU instance idle.
5. Podman hybrid placement — containerize WO-2/4/5, keep WO-1's IPC bus and GPU rendering bare-metal.
6. Ephemeral session isolation — every workflow test runs in a fresh, disposable Podman container.

---

## Where to start

Work Order 1 (`.kiro/specs/wo1-workspace-ipc-backbone/`) is the foundation everything else depends on. Start there. WO-1's IPC bus is pipeline-agnostic — it serves Pipeline 1 and all future pipelines without change. Build it once.

After WO-1: read `.kiro/steering/pipeline-1-aws-native.md` to understand which node implementations Pipeline 1 uses for WO-2 through WO-5 — they are all simpler than the full Work Order specs (AWS managed services instead of custom Rust/C++) and can run in parallel with the deeper WO-2/WO-3/WO-4/WO-5 builds.
