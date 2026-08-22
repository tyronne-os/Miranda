Paste the block below into Kiro's chat, in this workspace, to formally start the build.

Before pasting, run `node scripts/cat-router-check.mjs` yourself and set your chat model to whatever CAT 1 requires (**Qwen3 Coder Next**) if you're starting from the top of a Work Order — the protocol below will tell you exactly when to switch from there.

---

Read `PROJECT_OVERVIEW.md` in this workspace root before doing anything else.

You are now working on the **NOBILITY POSH FRAMEWORK**, building exceptional 2D-to-4D humanized companions & colleagues. **Miranda-Engine is the harness, not a pipeline** — she is workflow-agnostic, built to run N parallel candidate pipelines side-by-side (a quad-test: 4 independent variants, 4 independent EVE renders, scored head-to-head), not one fixed chain. Hold that distinction through everything you do here.

Load `.kiro/steering/build-standards.md` and `.kiro/steering/model-routing-protocol.md` — these are mandatory, global, no-deviation operating rules for this entire workspace, not suggestions.

The CAT-5 Model Routing Protocol governs which model handles which task, and it's deliberately mixed-provider — Claude is reserved for CAT 4-5 only:

- **CAT 1** → Qwen3 Coder Next
- **CAT 2** → GLM-5
- **CAT 3** → DeepSeek 3.2
- **CAT 4** → Claude Sonnet 5 (escalate to Opus 5 after 2 real failed verifications)
- **CAT 5** → Claude Opus 5 only

Every task in every Work Order under `.kiro/specs/` is tagged `[CAT 1]` through `[CAT 5]`. Before starting any task, check its tag against the model you're currently running as:

- If they match: proceed normally.
- If they don't match: stop. Do not attempt the task on the wrong model. Output: `CAT <n> MODEL SWITCH REQUIRED — <task name> requires <model>. Switch the chat model dropdown to <model>, then re-issue this task alone.`
- CAT 4 specifically: if you fail real verification (a real test/build/benchmark, not uncertainty) twice on the same task, escalate to Opus 5 instead of a third attempt.

Then load `.kiro/specs/wo1-workspace-ipc-backbone/requirements.md`, `design.md`, and `tasks.md`, and begin executing Work Order 1's tasks in tag order, switching models exactly when the protocol requires it. Confirm every task with real command output — `cargo build`/`cargo test` results, not a description of what should happen.
