Paste the block below into Kiro's chat, in this workspace, to formally start the build.

Before pasting, run `node scripts/cat-router-check.mjs` yourself and set your chat model to whatever CAT 1 requires (**Qwen3 Coder Next**) if you're starting from the top of a Work Order — the protocol below will tell you exactly when to switch from there.

---

Read `PROJECT_OVERVIEW.md` in this workspace root before doing anything else.

You are now working on the **NOBILITY POSH FRAMEWORK**, building exceptional 2D-to-4D humanized companions & colleagues. **Miranda-Engine is the harness, not a pipeline** — she is workflow-agnostic, built to run N parallel candidate pipelines side-by-side (a quad-test: 4 independent variants, 4 independent EVE renders, scored head-to-head), not one fixed chain. Hold that distinction through everything you do here.

Load `.kiro/steering/build-standards.md` and `.kiro/steering/model-routing-protocol.md` — these are mandatory, global, no-deviation operating rules for this entire workspace, not suggestions.

---

## CAT-5 Model Routing Protocol (mandatory — full spec at `.kiro/steering/model-routing-protocol.md`)

Every task in every Work Order under `.kiro/specs/` is tagged `[CAT 1]` through `[CAT 5]`. This controls which model handles it — **deliberately mixed-provider, Claude reserved for CAT 4-5 only:**

| CAT | Model | Why |
|---|---|---|
| **1** | Qwen3 Coder Next | Mechanical — boilerplate, scaffolding, adding deps, confirming builds. Zero reasoning required. |
| **2** | GLM-5 | Light reasoning — well-known patterns, simple data structures, basic tests. |
| **3** | DeepSeek 3.2 | Workhorse — non-trivial logic, third-party integrations, behavioral tests. Most tasks land here. |
| **4** | Claude Sonnet 5 | Real risk — networking correctness, SIMD math, security-sensitive code. Escalate to Opus 5 after 2 real failed verifications. |
| **5** | Claude Opus 5 only — no exceptions | Silent-failure risk — lock-free concurrency, unsafe memory/ABI, real-time SIMD, novel shader math. Wrong code compiles and fails in production. |

**28 of 36 total tasks (CAT 1-3) never touch Claude at all.** Claude runs on exactly 8 tasks — the 5 CAT 4 and 3 CAT 5 tasks — roughly 22% of the build, spent on the highest-value engineering.

### Before starting any task

1. Read the task's `[CAT n]` tag from its `tasks.md`.
2. Check whether your active model matches the required tier.
3. **Match → proceed.** No match → **STOP. Emit:**

   `CAT <n> MODEL SWITCH REQUIRED — <task name> requires <model>. Switch the chat model dropdown to <model>, then re-issue this task alone.`

### CAT 4 escalation rule

If a CAT 4 task fails **real verification** (a real `cargo build` error, `cargo test` failure, or benchmark miss — not uncertainty) twice, emit:

`CAT 5 ESCALATION — <task name> has failed real verification twice on Claude Sonnet 5. Switch to Claude Opus 5 before the third attempt.`

### Handoff on model switch

Before switching tiers, emit:

```
=== CAT <n> HANDOFF — switching to <model> ===
Task: <task name>
Status: <completed | failed N times>
State: <one paragraph — what exists, what was tested, what passed>
Incoming needs to know: <any constraint or partial state>
=== END HANDOFF ===
```

---

## Session start checklist

Run `node scripts/cat-router-check.mjs` now. The output tells you:
- How many tasks at each tier are still pending
- Whether you will need Opus 5 this session (CAT 5 count > 0)
- Which Work Order to start from

Then load `.kiro/specs/wo1-workspace-ipc-backbone/requirements.md`, `design.md`, and `tasks.md` and begin executing Work Order 1 in CAT tag order.

**Confirm every task with real command output** — `cargo build`/`cargo test` results, not a description of what should happen. Nothing is marked done without a real pass.

---

## Reference

- Full CAT-5 protocol explanation and adoption guide: https://github.com/tyronne-os/beryl-cat5-protocol
- Project overview and architecture: `PROJECT_OVERVIEW.md`
- Build standards: `.kiro/steering/build-standards.md`
- Model routing rules: `.kiro/steering/model-routing-protocol.md`
- **Pipeline 1 (AWS-native): `.kiro/steering/pipeline-1-aws-native.md`** — node-by-node mapping of AWS managed services (Transcribe / Bedrock / Polly / Sumerian Hosts / KVS WebRTC), Polly viseme→BlendshapeFrame adapter spec, and credential access. Read this after WO-1 to understand what each Work Order's Pipeline 1 implementation actually is.
- All 5 Work Order specs: `.kiro/specs/wo1-*` through `wo5-*`
- Instant Presence Standard: `eve-ecc-docs/INSTANT-PRESENCE-STANDARD.md`

## About the node labels in THE VANITY

The node graph in THE VANITY's left pane shows labels like "Riva ASR," "Nemotron Agent," "Hive TTS," "Audio2Face-3D," "Omniverse Stream." These are **role labels** — they describe the class of work at each node position, not literal product deployments. For Pipeline 1, each role is filled by an AWS managed service (see the pipeline-1 steering doc). The labels do not change between pipelines; the implementations plugged into them do. Miranda's job is to engineer the harness that makes any implementation pluggable into any role slot — including implementations derived from a theoretical research paper.
