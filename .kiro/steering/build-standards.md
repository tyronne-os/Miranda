# Miranda-Engine build standards

These rules apply to every Work Order in `.kiro/specs/` and any other work in this repo.

## No claimed progress without evidence

A Work Order is not "done" because the code was written — it's done when there's real command output proving it: `cargo build`/`cargo test` passing, a real measured latency number, a real screen-capture/frame-diff showing the No Loop Video Protocol is satisfied. Never report a task complete on code-review confidence alone.

## No simulated inference, ever

If a component doesn't have a real backend wired up yet (an unimplemented Node Warden, an unrigged Gaussian-splat asset), say so plainly rather than faking output. This project's own prior work (`eve-ecc-docs/ORCHESTRATION-PIVOT.md`) documents exactly this failure once already — a placeholder scored 96/100 because the measurement read code intent, not painted pixels. Don't repeat it.

## Reuse before rebuild

Before writing new code, check `client-apps/web`, `client-services/ace-controller`, and the existing Rust crate stubs for something already close to what's needed. The first attempt (eve-ecc) has real, working pieces — extend them.

## GPU cost discipline

Per the `aws-pipeline-architect` skill: never leave a GPU instance running idle. WO-1 through WO-4 need zero GPU. Only WO-5's actual rendering/rigging work touches a GPU instance, and only for the duration of the specific test.

## Cross-reference the Kiro skills, don't re-derive their content

`nobility-posh-framework`, `live-avatar-expert`, `aws-pipeline-architect`, and `llamacpp-huggingface-expert` are all active global skills with the science, architecture rationale, and deployment rules already written out. Reference them; don't duplicate or re-research what they already contain.
