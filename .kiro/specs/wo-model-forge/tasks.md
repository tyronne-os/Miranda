# Implementation Plan: Model Forge

## Overview

Builds conversational/voice-triggered LLM customization: LoRA fine-tuning, model merging via mergekit, automatic naming, GPU cost discipline, and composer/menu integration.

## Task Dependency Graph

```json
{
  "waves": [
    {"wave": 1, "tasks": [1, 2]},
    {"wave": 2, "tasks": [3, 4, 5]},
    {"wave": 3, "tasks": [6, 7]},
    {"wave": 4, "tasks": [8, 9]},
    {"wave": 5, "tasks": [10]}
  ]
}
```

## Tasks

- [ ] 1. Job parser & conversational intent routing [CAT 3]
  - Implement `miranda-nodes/src/forge/job_parser.rs` parsing fine-tune/merge intents from text into `JobSpec`
  - Wire routing hook into WO-Conversational-Intelligence's persona/intent detection so Forge intents bypass normal chat handling
  - Unit test against 15 labeled sample commands (fine-tune, merge, non-Forge chat)
  - _Requirements: 1.1, 4.1_

- [ ] 2. Model library registry [CAT 2]
  - Implement `miranda-nodes/src/forge/model_registry.rs`: register/list local models with metadata
  - Feed composer/menu UI hover-overlay data (role, source, specs)
  - Unit test registration + uniqueness collision handling
  - _Requirements: 3.3_

- [ ] 3. Naming engine [CAT 2]
  - Implement `miranda-nodes/src/forge/naming.rs` generating `<Female Name> <Family>-<Size> <Descriptor>` names
  - Enforce uniqueness against Model Library Registry with disambiguating suffix on collision
  - Unit test name generation and collision handling with 10 sample cases
  - _Requirements: 3.1, 3.2_

- [ ] 4. GPU provisioner with cost discipline [CAT 3]
  - Implement `miranda-nodes/src/forge/gpu_provisioner.rs`: provision, 15-minute idle auto-stop, teardown on completion/failure/cancellation
  - Enforce spending-threshold confirmation gate using WO-Conversational-Intelligence's autonomy calibration thresholds
  - Integration test: simulate idle timeout, verify teardown fires; simulate over-threshold cost, verify confirmation block
  - _Requirements: 4.2, 5.1, 5.2, 5.3_

- [ ] 5. Compatibility validator [CAT 2]
  - Implement `miranda-nodes/src/forge/compatibility.rs` checking architecture family and tokenizer match across candidate merge models
  - Unit test against matching and mismatched fixture pairs
  - _Requirements: 2.1, 2.3_

- [ ] 6. LoRA fine-tune pipeline [CAT 4]
  - Implement `miranda-nodes/src/forge/finetune_pipeline.rs` wrapping a LoRA training stack (HF `peft` or `axolotl`)
  - Emit real training metrics (loss curve, duration, GPU-hours); abort early on sustained loss divergence
  - Real end-to-end integration test: fine-tune a small local base model, verify adapted model output differs meaningfully from base
  - _Requirements: 1.2, 1.3, 1.4_

- [ ] 7. Model merge pipeline (mergekit integration) [CAT 4]
  - Implement `miranda-nodes/src/forge/merge_pipeline.rs` invoking mergekit (SLERP/TIES/DARE) after compatibility validation
  - Implement coherence smoke test (sample generation, repetition/garbage heuristic) gating successful registration
  - Real end-to-end integration test: merge two small compatible local models, verify smoke test and registration behavior
  - _Requirements: 2.2, 2.4_

- [ ] 8. Conversational deployment & progress reporting [CAT 2]
  - Wire Job Parser → confirmation dialogue → pipeline dispatch → completion announcement, all through the existing conversation turn flow
  - Implement progress query handling ("how's the fine-tune going?") against running job state
  - Integration test: full conversational flow from trigger phrase to completion announcement using a real (small) job
  - _Requirements: 4.2, 4.3, 4.4_

- [ ] 9. Scope-boundary handling for pretraining requests [CAT 1]
  - Implement intent classification branch that detects from-scratch pretraining requests and responds with the fine-tune/merge alternative explanation
  - Unit test against 5 sample pretraining-style requests
  - _Requirements: 6.1_

- [ ] 10. Integration tests & documentation [CAT 3]
  - Write `miranda-nodes/tests/model_forge_integration_tests.rs` covering the full trigger→confirm→run→name→register→announce flow for both fine-tune and merge paths
  - Verify all four Correctness Properties from design.md hold under test (cost gate, broken-merge rejection, idle teardown, naming uniqueness)
  - Write `MODEL_FORGE.md` documentation with real measured job durations/costs from test runs
  - _Requirements: 2.3, 2.4, 3.2, 5.1, 5.2, 5.3_

## Notes

CAT tags follow the CAT-5 Model Routing Protocol. Tasks 6 and 7 (LoRA fine-tune and merge pipelines) are CAT 4 — real correctness risk since a subtly broken fine-tune or merge compiles/runs but produces a degraded or incoherent model; both start on Claude Sonnet 5 with escalation to Opus 5 only after two failed real-verification attempts. This spec explicitly excludes from-scratch pretraining (Requirement 6) — Task 9 exists specifically to set that expectation conversationally rather than attempt it.

