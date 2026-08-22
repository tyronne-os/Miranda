Paste the block below into Kiro's chat, in this workspace, on Sonnet 5, to formally start the build.

---

Read `PROJECT_OVERVIEW.md` in this workspace root before doing anything else.

You are now working on the **NOBILITY POSH FRAMEWORK**, building exceptional 2D-to-4D humanized companions & colleagues. **Miranda-Engine is the harness, not a pipeline** — she is workflow-agnostic, built to run N parallel candidate pipelines side-by-side (a quad-test: 4 independent variants, 4 independent EVE renders, scored head-to-head), not one fixed chain. Hold that distinction through everything you do here.

Load `.kiro/steering/build-standards.md` and `.kiro/steering/model-routing-protocol.md` — these are mandatory, global, no-deviation operating rules for this entire workspace, not suggestions.

The CAT-5 Model Routing Protocol governs which model handles which task. You are currently running as Sonnet 5 — the default for this workspace. Every task in every Work Order under `.kiro/specs/` is tagged `[CAT 1]` through `[CAT 5]`. Before starting any task, check its tag:

- CAT 1-3: proceed normally.
- CAT 4: proceed, but if you fail real verification (a real test/build/benchmark, not uncertainty) twice on the same task, stop and output a CAT 5 escalation notice instead of a third attempt.
- CAT 5: do not attempt it. Stop immediately and output: `CAT 5 ESCALATION REQUIRED — <task name> requires Opus 5. Switch the chat model dropdown to Opus 5, then re-issue this task alone.`

Run `node scripts/cat-router-check.mjs` now to see the current task backlog broken out by CAT tier before we begin.

Then load `.kiro/specs/wo1-workspace-ipc-backbone/requirements.md`, `design.md`, and `tasks.md`, and begin executing Work Order 1's CAT 1-4 tasks in order. Stop at the CAT 5 ring-buffer task and hand it back per the protocol above. Confirm every task with real command output — `cargo build`/`cargo test` results, not a description of what should happen.
