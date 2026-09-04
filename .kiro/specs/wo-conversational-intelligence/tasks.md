# Implementation Plan: Conversational Intelligence for Miranda

## Overview

Builds Miranda's adaptive conversation layer: continuous mood tracking, hierarchical state machine, anticipatory move generation, interest/curiosity modeling, real-time knowledge updates, role fluidity, response tuning, the autonomy calibration interview, partnership investment tracking, and full system prompt integration.

## Task Dependency Graph

```json
{
  "waves": [
    {"wave": 1, "tasks": [1]},
    {"wave": 2, "tasks": [2, 4]},
    {"wave": 3, "tasks": [3, 5, 6, 7, 8]},
    {"wave": 4, "tasks": [9]},
    {"wave": 5, "tasks": [10]},
    {"wave": 6, "tasks": [11]}
  ]
}
```

## Tasks

- [ ] 1. Real-time mood stream processor [CAT 3]
  - Implement `miranda-nodes/src/conversation/mood_stream.rs` processing input in ~5-token chunks
  - Output continuous `MoodVector` with EMA smoothing, <50ms latency per chunk
  - Reuse mood classifier from WO-Memory rather than duplicating
  - Unit test against 15 labeled input streams verifying mood trajectory
  - _Requirements: 1.1, 1.2, 1.3, 1.4_

- [ ] 2. Conversation state machine [CAT 3]
  - Implement `miranda-nodes/src/conversation/state_machine.rs` with states Opening/DeepWork/Debugging/Reflection/Casual and micro-states
  - Implement transition logic driven by mood vector, entity signals, explicit cues
  - Publish state to `conversation_state_bus` IPC channel
  - Unit test 10 conversation sequences for correct transitions
  - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5_

- [ ] 3. Anticipatory move generator [CAT 4]
  - Implement `miranda-nodes/src/conversation/anticipation.rs` predicting next likely user action from state + mood + last 5 turns
  - Generate 2-3 scored candidate moves; suppress below 0.7 confidence
  - Implement dismissal feedback loop lowering future scores for similar moves
  - Calibrate confidence scoring against 20 labeled conversation snippets; verify via real (not mocked) end-to-end test
  - _Requirements: 3.1, 3.2, 3.3, 3.4_

- [ ] 4. Interest model & curiosity engine [CAT 3]
  - Implement `miranda-nodes/src/conversation/interest_model.rs` tracking topic frequency, sentiment, last-mentioned
  - Implement curiosity question generation gated at ≤1/hour, with dismissal-based deprioritization
  - Integrate with WO-Memory for historical topic data
  - Unit test question generation against sample interest histories
  - _Requirements: 4.1, 4.2, 4.3, 4.4_

- [ ] 5. Knowledge update pipeline [CAT 3]
  - Implement `miranda-nodes/src/conversation/knowledge_updater.rs`: correction detection, framework/tool extraction, code style profiling
  - Store corrected facts with confidence + "user-corrected" source attribution
  - Integration test: verify a correction applies to the very next response in the same session
  - _Requirements: 5.1, 5.2, 5.3, 5.4_

- [ ] 6. Role/persona injection system [CAT 2]
  - Implement `miranda-nodes/src/conversation/persona_injection.rs` with 5 roles and detection heuristics
  - Write role-specific prompt templates preserving Miranda's core identity across switches
  - Unit test 15 sample inputs for correct role detection; manual tone review per role
  - _Requirements: 6.1, 6.2, 6.3_

- [ ] 7. Response latency & depth tuning [CAT 2]
  - Implement `miranda-nodes/src/conversation/response_tuning.rs` computing target latency/depth from mood vector and state
  - Add streaming support for partial responses under high frustration
  - Unit test target computation for 10 mood/state combinations
  - _Requirements: 2.3, 2.4_

- [ ] 8. Autonomy calibration interview [CAT 3]
  - Implement `miranda-nodes/src/conversation/autonomy_calibration.rs` running a structured interview covering file operations, spending/GPU provisioning, version control, and install/config changes
  - Store per-category thresholds (`Autonomous`/`FastPathConfirm`/`ExplicitConfirm`) in the memory system
  - Enforce fixed floor: destructive-at-scale, production-impacting, and high-blast-radius categories always resolve to `ExplicitConfirm`
  - Implement periodic re-check prompt based on accumulated action track record
  - Unit test: verify no interview input combination can produce `Autonomous` for floor categories
  - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5, 7.6_

- [ ] 9. Partnership investment tracker [CAT 2]
  - Implement `miranda-nodes/src/conversation/partnership_tracker.rs`: goal extraction, progress linkage across sessions, win detection
  - Implement banned-pattern filter rejecting dependency/guilt language before any acknowledgment is surfaced
  - Unit test 10 goal-setting conversations for extraction + later callback; run banned-pattern corpus through filter, verify 100% rejection
  - _Requirements: 8.1, 8.2, 8.3, 8.4_

- [ ] 10. System prompt builder (integration) [CAT 3]
  - Implement `miranda-nodes/src/conversation/prompt_builder.rs` combining role, state, mood, memory injection, anticipatory moves, and partnership acknowledgment
  - Enforce token budget (default 2000) with defined truncation priority order
  - Integration test: 10 sample conversations through full pipeline, manual review of assembled prompts
  - _Requirements: 3.3, 6.1, 8.2, 9.1, 9.3_

- [ ] 11. Integration tests & performance benchmarks [CAT 3]
  - Write `miranda-nodes/tests/conversation_integration_tests.rs` covering mood→state transitions, confidence gating, curiosity rate limiting, in-session correction application, role detection, and autonomy floor invariants
  - Write `scripts/conversation-intelligence-benchmarks.sh` measuring latency targets (mood <50ms, state transition <10ms, anticipation <150ms, full prompt assembly <300ms)
  - Write `CONVERSATIONAL_INTELLIGENCE.md` and performance report with real measured numbers
  - _Requirements: 1.3, 3.1, 4.3, 7.4, 8.3_

## Notes

CAT tags follow the CAT-5 Model Routing Protocol. Task 3 (Anticipatory Move Generator) is CAT 4 — real correctness risk since an overconfident wrong prediction produces an awkward or presumptuous interjection; starts on Claude Sonnet 5, escalates to Opus 5 only after two failed real-verification attempts per the protocol's evidence-based escalation trigger. All other tasks are CAT 2-3. Task 8's autonomy-floor invariant test is a hard gate — it must pass before this spec can be considered complete, per the user's explicit requirement that destructive/irreversible actions never bypass confirmation regardless of calibration.

